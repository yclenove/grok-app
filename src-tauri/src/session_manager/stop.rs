//! User-requested session cancellation.

use std::sync::Arc;

use tauri::{AppHandle, Emitter};

use crate::acp_client::{AskUserOutcome, PermissionOutcome};
use crate::session_fsm::SessionState;

use super::*;

impl SessionManager {
    pub async fn stop(
        self: &Arc<Self>,
        app: AppHandle,
        session_id: Option<String>,
    ) -> Result<SessionSnapshot, String> {
        let target = match session_id {
            Some(sid) => sid,
            None => self
                .inner
                .lock()
                .as_ref()
                .map(|s| s.app_session_id.clone())
                .ok_or("no active session")?,
        };
        // Handshake is not a turn. Stop / Retry must tear it down so the
        // user is not stuck on 连接中 until restart.
        let abort_handshake = self.with_session_mut(&target, |s| {
            if !stop_should_abort_handshake(s.fsm.state()) {
                return (false, None);
            }
            let _ = s.fsm.connect_failed(crate::error::AgentError::new(
                crate::error::AgentErrorCode::ConnectFailed,
                "connect cancelled",
            ));
            (true, s.acp.take())
        });
        if let Some((true, acp)) = abort_handshake {
            if let Some(acp) = acp {
                Self::kill_acp_bounded(&acp).await;
            }
            self.emit_for_session(&app, &target);
            return Ok(self.snapshot());
        }
        let app_for_marker = app.clone();
        // Also release ask_user / plan reverse-RPCs. Leaving them set kept
        // `live_session_is_busy` true after stop, so Send/park paths stayed
        // wedged until process kill (user diag 5bda6b52).
        let (acp, agent_sid, pending_ask, pending_plan, pending_perm) = self
            .with_session_mut(&target, move |s| {
                let app = app_for_marker;
                if let Some(h) = s.mock_stream.take() {
                    h.request_stop();
                }
                let pending_ask = s.pending_ask_user_rpc_id.take();
                let pending_plan = s.pending_plan_rpc_id.take();
                let pending_perm = s.pending_permission_rpc_id.take();
                s.pending_permission_options = None;
                s.pending_permission_tool_name = None;
                s.pending_permission_ui = None;
                let was_busy = s.fsm.state() == SessionState::Streaming
                    || s.fsm.state() == SessionState::AwaitingPermission
                    || s.streaming_message_id.is_some()
                    || !s.open_tool_ids.is_empty()
                    || s.prompt_in_flight
                    || pending_ask.is_some()
                    || pending_plan.is_some()
                    || pending_perm.is_some();
                // Journal a cancel marker so UI history is not left as user-only silence.
                if was_busy {
                    // Shared helper: durable chip + live emit (history matches live).
                    Self::journal_turn_cancelled(s, Some(&app), "user_stop");
                    if s.fsm.state() == SessionState::Streaming
                        || s.fsm.state() == SessionState::AwaitingPermission
                    {
                        let _ = s.fsm.end_stream();
                    }
                }
                s.streaming_message_id = None;
                s.active_turn_id = None;
                s.stream_message_id_locked = false;
                s.stream_buf.clear();
                s.stream_thought.clear();
                s.stream_last_was_assistant = false;
                s.stream_attachments.clear();
                s.open_tool_ids.clear();
                s.terminal_tool_ids.clear();
                s.open_tool_seen_at.clear();
                s.deferred_prompt_complete = None;
                // Cancelled: the prompt RPC resolves as cancelled, so release the
                // turn here too — otherwise the chat can never be parked again.
                s.prompt_in_flight = false;
                s.journal_throttle.reset();
                s.last_stall_emit = None;
                (
                    s.acp.clone(),
                    s.meta.agent_session_id.clone(),
                    pending_ask,
                    pending_plan,
                    pending_perm,
                )
            })
            .ok_or("no active session")?;
        let had_pending_ask = pending_ask.is_some();
        if had_pending_ask {
            let _ = app.emit(
                "session://ask_user_cleared",
                serde_json::json!({
                    "sessionId": target,
                    "reason": "user_stop",
                }),
            );
        }
        // Publish Ready *before* agent cancel. `session/cancel` goes through
        // stdin write (up to STDIN_WRITE_TIMEOUT) — if the agent is wedged or
        // the turn already ended with sticky Streaming, awaiting cancel first
        // left the UI/Host consumers blocked on Stop for the whole timeout
        // (user report: Stop does nothing while thinking is stuck).
        self.promote_background_ready_to_parked(&target);
        self.emit_for_session(&app, &target);
        // Prefer the stopped chat's snapshot (not the live focus slot).
        let stopped_snap = if self.is_live_session(&target) {
            self.snapshot()
        } else if let Some(snap) = self
            .background
            .lock()
            .get(&target)
            .map(Self::snapshot_from_live)
        {
            snap
        } else {
            // Parked after promote, or already idle — return live focus snap.
            self.snapshot()
        };

        // Best-effort agent cancel after UI is already unblocked.
        if let Some(acp) = acp {
            // Reply to reverse-RPCs before session/cancel so the agent does not
            // sit forever on an unanswered ask_user_question after Host "stop".
            if let Some(id) = pending_ask {
                if let Err(e) = acp
                    .respond_ask_user_question(id, AskUserOutcome::Cancelled)
                    .await
                {
                    tracing::warn!("stop: cancel ask_user id={id} failed: {e}");
                }
            }
            if let Some(id) = pending_plan {
                if let Err(e) = acp.respond_exit_plan_mode(id, "cancelled", None).await {
                    tracing::warn!("stop: cancel plan id={id} failed: {e}");
                }
            }
            if let Some(id) = pending_perm {
                if let Err(e) = acp
                    .respond_permission(id, PermissionOutcome::Cancelled)
                    .await
                {
                    tracing::warn!("stop: cancel permission id={id} failed: {e}");
                }
                let _ = app.emit(
                    "session://permissions_invalidated",
                    serde_json::json!({
                        "reason": "user_stop",
                        "gates": [{
                            "sessionId": target,
                            "rpcId": id,
                        }],
                    }),
                );
            }
            // Target the session explicitly (shared process safety).
            if let Err(e) = match agent_sid {
                Some(ref sid) => acp.cancel_for(sid).await,
                None => acp.cancel().await,
            } {
                tracing::warn!(
                    target: "session",
                    session = %target,
                    "stop: session/cancel after Ready emit failed (UI already settled): {e}"
                );
            }
        }
        self.flush_pending_soft_respawn(&app, &target).await;
        Ok(stopped_snap)
    }
}
