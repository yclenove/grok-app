//! Post-open setting alignment shared by cold, warm reuse and unpark.
use super::*;
use crate::{acp_client::AcpClient, store};

pub(super) struct ConnectedOwner {
    pub(super) client: Arc<AcpClient>,
    pub(super) agent_id: String,
    process_id: String,
    binding: workspace_context::Binding,
    context: Option<workspace_context::RuntimeWorkspaceContext>,
}

impl ConnectedOwner {
    pub(super) fn capture(session: &LiveSession) -> Result<Self, String> {
        let client = session
            .acp
            .clone()
            .ok_or("connected transport is missing")?;
        let agent_id = session
            .meta
            .agent_session_id
            .clone()
            .filter(|id| !id.trim().is_empty())
            .ok_or("connected agent identity is missing")?;
        Ok(Self {
            client,
            agent_id,
            process_id: session.process_id.clone(),
            binding: workspace_context::Binding::from(&session.meta),
            context: session.workspace_context.clone(),
        })
    }

    fn matches(&self, session: &LiveSession) -> bool {
        session.process_id == self.process_id
            && session.meta.agent_session_id.as_deref() == Some(&self.agent_id)
            && session
                .acp
                .as_ref()
                .is_some_and(|c| Arc::ptr_eq(c, &self.client))
            && workspace_context::Binding::from(&session.meta) == self.binding
            && session.workspace_context == self.context
    }

    pub(super) fn check(&self, manager: &SessionManager, id: &str) -> Result<(), String> {
        if !self.client.is_alive()
            || !manager
                .with_session_mut(id, |s| self.matches(s))
                .unwrap_or(false)
        {
            return Err("connected session owner changed".into());
        }
        let current = workspace_context::session(id)?;
        if workspace_context::Binding::from(&current) != self.binding {
            return Err(workspace_context::changed(
                "connection directory binding changed",
            ));
        }
        if let Some(context) = &self.context {
            context.validate(id).map_err(workspace_context::changed)?;
        }
        Ok(())
    }

    pub(super) fn prefs(
        &self,
        manager: &SessionManager,
        id: &str,
    ) -> Result<store::ComposerPrefs, String> {
        self.check(manager, id)?;
        let meta = workspace_context::session(id)?;
        let prefs = store::resolve_composer_prefs(meta.project_id.as_deref(), Some(id));
        if !main_route::matches_client(&meta, &prefs, &self.client)? {
            return Err("main channel or pinned model changed; reconnect is required".into());
        }
        // The resolver reads multiple stores. A deletion/rebind is never allowed
        // to turn that read into draft/global intent for a still-running child.
        self.check(manager, id)?;
        Ok(prefs)
    }
}

impl SessionManager {
    /// Caller already holds connect_lock. Only the explicitly opened owner may
    /// receive these RPCs; never lock connect again or use the recent agent id.
    pub(super) async fn align_connected_composer(
        &self,
        session_id: &str,
        starting: &store::ComposerPrefs,
        skip_mode: bool,
    ) -> Result<(), String> {
        self.align_opened_composer(session_id, starting, skip_mode, false)
            .await
    }

    pub(super) async fn align_opened_composer(
        &self,
        session_id: &str,
        starting: &store::ComposerPrefs,
        skip_mode: bool,
        new_empty_session: bool,
    ) -> Result<(), String> {
        let owner = self
            .with_session_mut(session_id, |s| ConnectedOwner::capture(s))
            .ok_or("connected session owner is missing")??;
        let result = self
            .align_connected_owner(session_id, &owner, starting, skip_mode, new_empty_session)
            .await;
        if result.is_err()
            && self
                .with_session_mut(session_id, |s| owner.matches(s))
                .unwrap_or(false)
        {
            // Never leave a failed/invalid Ready shell eligible for a prompt.
            self.pending_soft_respawn
                .lock()
                .insert(session_id.into(), "composer_alignment".into());
        }
        result
    }

    async fn align_connected_owner(
        &self,
        id: &str,
        owner: &ConnectedOwner,
        starting: &store::ComposerPrefs,
        skip_mode: bool,
        new_empty_session: bool,
    ) -> Result<(), String> {
        let mut applied_mode = skip_mode.then(|| starting.mode.clone());
        let mut applied_model = None;
        // Normal alignment is one pass. Concurrent edits can converge without
        // an unbounded connection wait; exhaustion requires a fresh connect.
        for _ in 0..3 {
            let latest = owner.prefs(self, id)?;
            let mode_result = if applied_mode.as_deref() == Some(latest.mode.as_str()) {
                Ok(())
            } else {
                Self::with_soft_rpc_budget(owner.client.set_mode_for(&owner.agent_id, &latest.mode))
                    .await
                    .map(|_| ())
            };
            owner.check(self, id)?;
            if mode_result.is_ok() {
                applied_mode = Some(latest.mode);
            }
            // Mode RPC can yield while the user chooses a different model.
            let latest = owner.prefs(self, id)?;
            let model_result = if applied_model.as_deref() == Some(latest.model_id.as_str()) {
                Ok(())
            } else {
                let model = owner.client.resolve_agent_model(&latest.model_id);
                self.align_model_context(id, owner, &model, new_empty_session)
                    .await
            };
            owner.check(self, id)?;
            if model_result.is_ok() {
                applied_model = Some(latest.model_id);
            }
            // A rejected mode must not prevent model alignment, but neither
            // rejection may silently certify this process for its next turn.
            model_result.and(mode_result)?;
            let latest = owner.prefs(self, id)?;
            if applied_model.as_deref() != Some(latest.model_id.as_str())
                || applied_mode.as_deref() != Some(latest.mode.as_str())
            {
                continue;
            }
            self.with_session_mut(id, |s| -> Result<(), String> {
                if !owner.matches(s) {
                    return Err("connected session owner changed".into());
                }
                s.model_id = Some(latest.model_id.clone());
                s.meta.model_id = Some(latest.model_id);
                s.product_mode = Some(latest.mode.clone());
                s.meta.mode = Some(latest.mode);
                Ok(())
            })
            .ok_or("connected session owner is missing")??;
            let effort_changed = latest.effort != starting.effort;
            if effort_changed || latest.permission_policy != starting.permission_policy {
                self.pending_soft_respawn
                    .lock()
                    .insert(id.into(), "composer_spawn_flags".into());
                if effort_changed {
                    // session/load would restore the previous effort. Only
                    // clear runtime identity; preserve desired prefs and history.
                    store::update_sessions_index_strict(|rows| {
                        let row = rows
                            .iter_mut()
                            .find(|row| row.id == id)
                            .ok_or("session no longer exists")?;
                        if workspace_context::Binding::from(&*row) != owner.binding {
                            return Err(workspace_context::changed("connection binding changed"));
                        }
                        row.agent_session_id = None;
                        row.agent_execution = None;
                        Ok(())
                    })?;
                    self.with_session_mut(id, |s| {
                        if owner.matches(s) {
                            s.meta.agent_session_id = None;
                            s.meta.agent_execution = None;
                            s.needs_history_bootstrap = true;
                        }
                    });
                }
            }
            // Effort and permission in memory remain the actual spawn values.
            return Ok(());
        }
        Err("composer intent changed repeatedly during connection; reconnect".into())
    }
}
