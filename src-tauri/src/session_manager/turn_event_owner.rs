//! Synchronous turn-event ownership shared by native Stream and tool consumers.
use super::*;

#[derive(Clone)]
pub(super) struct TurnEventOwner {
    pub(super) id: String,
    pub(super) background: bool,
    process: String,
    agent: Option<String>,
    client: Option<Arc<crate::acp_client::AcpClient>>,
    epoch: u64,
    turn: Option<String>,
    prompt_in_flight: bool,
}

impl TurnEventOwner {
    pub(super) fn capture(session: &LiveSession, background: bool) -> Self {
        Self {
            id: session.app_session_id.clone(),
            background,
            process: session.process_id.clone(),
            agent: session.meta.agent_session_id.clone(),
            client: session.acp.clone(),
            epoch: session.turn_open_epoch,
            turn: session.active_turn_id.clone(),
            prompt_in_flight: session.prompt_in_flight,
        }
    }

    fn hint(&self) -> SessionRouteHint {
        SessionRouteHint {
            app_session_id: self.id.clone(),
            process_id: self.process.clone(),
            agent_session_id: self.agent.clone(),
            prompt_in_flight: self.prompt_in_flight,
        }
    }

    pub(super) fn matches(&self, session: &LiveSession) -> bool {
        self.id == session.app_session_id
            && self.process == session.process_id
            && self.agent == session.meta.agent_session_id
            && self.epoch == session.turn_open_epoch
            && self.turn == session.active_turn_id
            && match (&self.client, &session.acp) {
                (Some(expected), Some(actual)) => Arc::ptr_eq(expected, actual),
                (None, None) => true,
                _ => false,
            }
    }
    pub(super) fn matches_parked(&self, session: &ParkedAgent) -> bool {
        self.turn.is_none()
            && self.id == session.app_session_id
            && self.process == session.process_id
            && self.agent == session.meta.agent_session_id
            && self.epoch == session.turn_open_epoch
            && self
                .client
                .as_ref()
                .is_some_and(|client| Arc::ptr_eq(client, &session.acp))
    }
}

impl SessionManager {
    pub(super) fn capture_turn_event_owner(
        &self,
        process: &str,
        agent: Option<&str>,
    ) -> Option<TurnEventOwner> {
        if process.trim().is_empty() || agent.is_some_and(|agent| agent.trim().is_empty()) {
            return None;
        }
        // Capture routing and owner identity in one synchronous handoff. Each
        // map guard drops before the next; no store/IPC/RPC work runs here.
        let _transfer = self.runtime_transfer.lock();
        let live = self
            .inner
            .lock()
            .as_ref()
            .map(|session| TurnEventOwner::capture(session, false));
        let background: Vec<_> = self
            .background
            .lock()
            .values()
            .map(|session| TurnEventOwner::capture(session, true))
            .collect();
        let parked: Vec<_> = self
            .parked
            .lock()
            .values()
            .map(|session| SessionRouteHint {
                app_session_id: session.app_session_id.clone(),
                process_id: session.process_id.clone(),
                agent_session_id: session.meta.agent_session_id.clone(),
                prompt_in_flight: false,
            })
            .collect();
        let live_hint = live.as_ref().map(TurnEventOwner::hint);
        let background_hints: Vec<_> = background.iter().map(TurnEventOwner::hint).collect();
        match resolve_turn_event_route(
            process,
            agent,
            live_hint.as_ref(),
            &background_hints,
            &parked,
        ) {
            TurnEventRoute::Live => live,
            TurnEventRoute::Background(id) => background.into_iter().find(|owner| owner.id == id),
            TurnEventRoute::Drop => None,
        }
    }

    pub(super) fn with_turn_event_owner_mut<R>(
        &self,
        owner: &TurnEventOwner,
        work: impl FnOnce(&mut LiveSession, bool) -> R,
    ) -> Option<R> {
        let transfer = self.runtime_transfer.lock();
        {
            let mut live = self.inner.lock();
            if let Some(session) = live.as_mut().filter(|s| s.app_session_id == owner.id) {
                drop(transfer);
                return owner.matches(session).then(|| work(session, false));
            }
        }
        let mut background = self.background.lock();
        drop(transfer);
        let session = background.get_mut(&owner.id)?;
        owner.matches(session).then(|| work(session, true))
    }
}
