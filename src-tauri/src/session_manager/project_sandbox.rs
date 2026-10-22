//! Apply a project's sandbox change to its owned processes across all slots.
use super::*;
use crate::store;
use std::collections::BTreeSet;
use tauri::AppHandle;

impl SessionManager {
    /// Command-only entry: read the latest store and process generation after
    /// connect completes. Connect's held-lock paths must not call this method.
    pub(crate) async fn refresh_project_sandbox_core(
        &self,
        app: Option<&AppHandle>,
        project_id: &str,
    ) -> Result<(), String> {
        let _connect = self.connect_lock.lock().await;
        let settings = store::load_settings();
        let (sessions, project) = {
            let _stores = workspace_context::store_locks()?;
            let sessions: HashMap<_, _> = store::load_sessions_snapshot()?
                .into_iter()
                .filter(|session| session.project_id.as_deref() == Some(project_id))
                .map(|session| (session.id.clone(), session))
                .collect();
            let project = store::load_projects_snapshot()?
                .into_iter()
                .find(|project| project.id == project_id)
                .ok_or("project no longer exists")?;
            (sessions, project)
        };
        let desired = store::resolve_sandbox_profile(
            &settings.sandbox_profile,
            project.sandbox_profile.as_deref(),
        );
        let needs_refresh =
            |owner: &str, context: Option<&RuntimeWorkspaceContext>, sandbox: Option<String>| {
                sessions.get(owner).is_some_and(|meta| {
                    !context.map_or_else(
                        || sandbox.as_deref().unwrap_or("off") == desired,
                        |context| context.matches_project_sandbox(meta, &project, &settings),
                    )
                })
            };
        let mut retire = BTreeSet::new();
        {
            let _transfer = self.runtime_transfer.lock();
            {
                let live = self.inner.lock();
                if let Some(session) = live.as_ref() {
                    if let Some(acp) = session.acp.as_ref() {
                        if needs_refresh(
                            &session.app_session_id,
                            session.workspace_context.as_ref(),
                            acp.sandbox_profile(),
                        ) {
                            retire.insert(session.app_session_id.clone());
                        }
                    }
                }
            }
            {
                let background = self.background.lock();
                for session in background.values() {
                    if let Some(acp) = session.acp.as_ref() {
                        if needs_refresh(
                            &session.app_session_id,
                            session.workspace_context.as_ref(),
                            acp.sandbox_profile(),
                        ) {
                            retire.insert(session.app_session_id.clone());
                        }
                    }
                }
            }
            {
                let parked = self.parked.lock();
                for session in parked.values() {
                    if needs_refresh(
                        &session.app_session_id,
                        session.workspace_context.as_ref(),
                        session.sandbox_profile.clone(),
                    ) {
                        retire.insert(session.app_session_id.clone());
                    }
                }
            }
        }
        // No transfer or map guard survives process shutdown.
        // Preserve busy answers and process co-tenants through the shared
        // owner retirement core. Do not multiply each bounded kill's wait.
        futures_util::future::join_all(retire.into_iter().map(|owner| async move {
            self.soft_respawn_for_session_core(app, &owner, "project_sandbox")
                .await;
        }))
        .await;
        Ok(())
    }
}
