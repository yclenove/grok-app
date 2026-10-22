//! Shared live/background process-exit mutation; effects run after pool guards.
use super::human_gate_invalidation::PendingGateInvalidation;
use super::*;
use tauri::AppHandle;

pub(super) struct ProcessExitUpdate {
    pub(super) invalidations: Vec<PendingGateInvalidation>,
    pub(super) emits: Vec<StreamEmitPayload>,
    pub(super) persists: Vec<PendingSessionPersist>,
    continuation_failure: Option<(String, String)>,
}

impl ProcessExitUpdate {
    pub(super) fn publish(self, app: &AppHandle) {
        SessionManager::emit_stream_payloads(app, self.emits);
        SessionManager::emit_gates_invalidated(app, "agent_exit", self.invalidations);
        SessionManager::commit_session_persists(Some(app), self.persists);
        if let Some((sid, turn)) = self.continuation_failure {
            crate::session_migration::continuation::notify_turn_failure(&sid, &turn);
        }
    }
}

impl SessionManager {
    pub(super) fn prepare_process_exit_locked(
        s: &mut LiveSession,
        code: Option<i32>,
    ) -> ProcessExitUpdate {
        let mut update = ProcessExitUpdate {
            invalidations: Vec::new(),
            emits: Vec::new(),
            persists: Vec::new(),
            continuation_failure: None,
        };
        // Preserve the last coalesced batch without signalling a fake Ready.
        if let Some(payload) = Self::take_pending_stream_emit(s) {
            update.emits.push(payload);
        }
        let state = s.fsm.state();
        let was_busy = Self::live_session_is_busy(s)
            || matches!(
                state,
                SessionState::Streaming | SessionState::AwaitingPermission
            );
        if was_busy {
            update.continuation_failure = s
                .active_turn_id
                .as_ref()
                .map(|turn| (s.app_session_id.clone(), turn.clone()));
            if let Some(boundary) = Self::prepare_journal_turn_cancelled(s, "agent_exit") {
                update
                    .persists
                    .push(PendingSessionPersist::TurnBoundary(Box::new(boundary)));
            }
        } else if let Some(flush) = Self::prepare_stream_journal_flush(s, true, false) {
            update
                .persists
                .push(PendingSessionPersist::StreamJournal(Box::new(flush)));
        }
        if let Some(pending) = Self::take_pending_gate_invalidation(s) {
            update.invalidations.push(pending);
        }
        if process_exit_should_fail_fsm(state, s.fsm.last_error().is_some()) {
            let detail = match code {
                Some(status) => format!("Agent process exited (code {status})"),
                None => "Agent process exited (EOF/unknown status)".into(),
            };
            let _ = s.fsm.crash(detail);
        }
        s.acp = None;
        s.open_tool_ids.clear();
        s.terminal_tool_ids.clear();
        s.open_tool_seen_at.clear();
        s.deferred_prompt_complete = None;
        s.streaming_message_id = None;
        s.active_turn_id = None;
        s.stream_message_id_locked = false;
        s.prompt_in_flight = false;
        update
    }
}
