//! ACP state receipt, separate from turn output and composer intent.
use super::*;
use crate::{acp_client::AcpClient, store};

pub(super) struct StateOwner {
    app_id: String,
    agent_id: String,
    process_id: String,
    client: Arc<AcpClient>,
    binding: workspace_context::Binding,
    context: Option<workspace_context::RuntimeWorkspaceContext>,
}

impl StateOwner {
    fn matches(&self, session: &LiveSession) -> bool {
        session.app_session_id == self.app_id
            && session.process_id == self.process_id
            && session.meta.agent_session_id.as_deref() == Some(&self.agent_id)
            && session
                .acp
                .as_ref()
                .is_some_and(|c| Arc::ptr_eq(c, &self.client))
            && workspace_context::Binding::from(&session.meta) == self.binding
            && session.workspace_context == self.context
            && self.client.is_alive()
    }

    fn persisted_owner(&self) -> bool {
        let Some(row) = saved_agent_owner(&self.agent_id) else {
            return false;
        };
        row.id == self.app_id && workspace_context::Binding::from(&row) == self.binding
    }
}

fn saved_agent_owner(agent: &str) -> Option<store::SessionMeta> {
    // Strict read only: a torn index is not an empty store to recover/rewrite.
    let rows = store::load_sessions_snapshot()
        .map_err(|error| {
            tracing::warn!("ACP State rejected: strict owner read failed: {error}");
        })
        .ok()?;
    let mut owners = rows
        .into_iter()
        .filter(|r| r.agent_session_id.as_deref() == Some(agent));
    let owner = owners.next()?;
    // No recent/focus fallback or arbitrary first-match choice for duplicates.
    if owners.next().is_some() {
        return None;
    }
    Some(owner)
}

impl SessionManager {
    /// State telemetry never establishes a connection or changes focus. Runtime
    /// is session-scoped even when its owner was live at the mutation boundary.
    pub(super) fn handle_state_event(
        &self,
        app: &tauri::AppHandle,
        process_id: &str,
        envelope_agent: Option<&str>,
        event: &crate::acp_client::AcpEvent,
    ) -> bool {
        let crate::acp_client::AcpEvent::State {
            backend,
            agent_session_id,
            model_id,
        } = event
        else {
            return false;
        };
        if let Some(snapshot) = self.apply_state_event(
            process_id,
            envelope_agent,
            agent_session_id.as_deref(),
            backend,
            model_id.as_deref(),
        ) {
            // Never emit_for_session/snapshot(): that can recapture foreign focus.
            Self::emit_runtime(app, &snapshot);
        }
        true
    }

    pub(super) fn apply_state_event(
        &self,
        process_id: &str,
        envelope_agent: Option<&str>,
        agent_id: Option<&str>,
        backend: &str,
        _model_id: Option<&str>,
    ) -> Option<SessionSnapshot> {
        let owner = self.capture_state_owner(process_id, envelope_agent, agent_id)?;
        self.apply_state_owner(&owner, backend)
    }

    pub(super) fn capture_state_owner(
        &self,
        process_id: &str,
        envelope_agent: Option<&str>,
        agent_id: Option<&str>,
    ) -> Option<StateOwner> {
        let agent = agent_id.filter(|id| !id.trim().is_empty())?;
        if envelope_agent.is_some_and(|id| id != agent) {
            return None;
        }
        let row = saved_agent_owner(agent)?;
        // One runtime-map mutex at a time. Resolve the App id from the strict
        // owner row, not by scanning live/background in an ABBA lock order.
        self.with_session_mut(&row.id, |session| {
            let owner = StateOwner {
                app_id: row.id.clone(),
                agent_id: agent.into(),
                process_id: process_id.into(),
                client: session.acp.clone()?,
                binding: workspace_context::Binding::from(&row),
                context: session.workspace_context.clone(),
            };
            owner.matches(session).then_some(owner)
        })
        .flatten()
    }

    pub(super) fn apply_state_owner(
        &self,
        owner: &StateOwner,
        backend: &str,
    ) -> Option<SessionSnapshot> {
        if let Some(context) = &owner.context {
            // Validation can take store locks; never run it while a runtime-map
            // mutex is held. Final mutation rechecks the exact runtime owner.
            if context.validate(&owner.app_id).is_err() {
                return None;
            }
        }
        self.with_session_mut(&owner.app_id, |session| {
            if !owner.matches(session) || !owner.persisted_owner() {
                return None;
            }
            session.backend = backend.into();
            // Connection code owns agent identity; store/manager commands own
            // Composer intent. Wire alias/echo/noecho is not a new user choice.
            Some(Self::snapshot_from_live(session))
        })
        .flatten()
    }
}
