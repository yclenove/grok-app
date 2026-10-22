//! Refresh runtime permission tiers without turning inheritance into overrides.
use super::*;
use crate::permission::{effective_permission_policy, PermissionPolicy};
use crate::store;
use std::collections::BTreeSet;
use tauri::AppHandle;

impl SessionManager {
    pub async fn refresh_permission_policies(&self, app: &AppHandle) -> Result<(), String> {
        self.refresh_permission_policies_core(Some(app)).await
    }

    /// Command-only entry: callers never already hold connect_lock. Read saved
    /// intent after acquiring it so an in-flight connect or older command cannot
    /// publish a process using a stale permission argument. No map guard crosses
    /// an await and no runtime value is persisted as a new user override.
    pub(crate) async fn refresh_permission_policies_core(
        &self,
        app: Option<&AppHandle>,
    ) -> Result<(), String> {
        let _connect_guard = self.connect_lock.lock().await;
        let sessions: HashMap<_, _> = store::load_sessions_snapshot()?
            .into_iter()
            .map(|session| (session.id.clone(), session))
            .collect();
        let projects: HashMap<_, _> = store::load_projects_snapshot()?
            .into_iter()
            .map(|project| (project.id.clone(), project))
            .collect();
        let settings = store::load_settings();
        // The App agent-home profile carries only the global default. A project's or
        // chat's override reaches its own process through connect spawn flags.
        if let Err(error) = crate::agent_prefs::sync_permission_to_agent_profile(
            &settings.session_data_mode,
            &settings.permission_policy,
        ) {
            tracing::warn!("refresh permission profile: {error}");
        }
        let resolve = |owner: &str| -> Option<(PermissionPolicy, Option<String>)> {
            let session = sessions.get(owner)?;
            let project = session.project_id.as_ref().and_then(|id| projects.get(id));
            Some((
                effective_permission_policy(
                    &settings.permission_policy,
                    project.map(|project| project.trusted),
                    project.and_then(|project| project.permission_policy.as_deref()),
                    session.permission_policy.as_deref(),
                ),
                session.permission_policy.clone(),
            ))
        };
        let mut retire = BTreeSet::new();
        {
            let _transfer = self.runtime_transfer.lock();
            {
                let mut live = self.inner.lock();
                if let Some(session) = live.as_mut() {
                    if let Some((policy, intent)) = resolve(&session.app_session_id) {
                        if session.policy != policy && session.acp.is_some() {
                            retire.insert(session.app_session_id.clone());
                        }
                        session.policy = policy;
                        session.meta.permission_policy = intent;
                    }
                }
            }
            {
                let mut background = self.background.lock();
                for session in background.values_mut() {
                    if let Some((policy, intent)) = resolve(&session.app_session_id) {
                        if session.policy != policy && session.acp.is_some() {
                            retire.insert(session.app_session_id.clone());
                        }
                        session.policy = policy;
                        session.meta.permission_policy = intent;
                    }
                }
            }
            {
                let mut parked = self.parked.lock();
                for session in parked.values_mut() {
                    if let Some((policy, intent)) = resolve(&session.app_session_id) {
                        if session.policy != policy {
                            retire.insert(session.app_session_id.clone());
                        }
                        // Keep the parked process's spawn policy until it detaches.
                        session.meta.permission_policy = intent;
                    }
                }
            }
        }
        // No transfer or map guard survives process shutdown.
        // Each child has a bounded kill. Do not multiply that bound by the
        // number of idle chats while blocking all connects behind this guard.
        futures_util::future::join_all(retire.into_iter().map(|owner| async move {
            self.soft_respawn_for_session_core(app, &owner, "permission_policy")
                .await;
        }))
        .await;
        Ok(())
    }
}
