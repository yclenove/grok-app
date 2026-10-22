//! Background ACP event pump handler.

use std::sync::Arc;

use tauri::{AppHandle, Emitter};

use crate::acp_client::{
    provider_retry_abort_error, provider_retry_abort_rpc_message, should_abort_provider_retry_ex,
    AcpEvent, HOST_PROVIDER_MAX_RETRIES,
};
use crate::session_fsm::SessionState;

use super::stream_event::StreamChunk;
use super::*;

impl SessionManager {
    pub(super) async fn handle_acp_event_on_background(
        self: &Arc<Self>,
        app: &AppHandle,
        app_session_id: &str,
        process_id: &str,
        agent_session_id: Option<&str>,
        ev: AcpEvent,
    ) {
        if matches!(ev, AcpEvent::PermissionRequest { .. }) {
            if let Some(event) = super::permission_request::PermissionRequestEvent::from_acp(ev) {
                self.handle_permission_request(app, process_id, agent_session_id, event)
                    .await;
            }
            return;
        }
        if matches!(ev, AcpEvent::Plan { .. } | AcpEvent::AskUserQuestion { .. }) {
            if let Some(event) = super::human_gate_event::HumanGateEvent::from_acp(ev) {
                self.handle_human_gate_event(app, process_id, agent_session_id, event);
            }
            return;
        }
        if matches!(
            ev,
            AcpEvent::ToolCall { .. } | AcpEvent::ToolOpenReleased { .. }
        ) {
            if let Some(event) = super::tool_event::ToolEvent::from_acp(ev) {
                self.handle_tool_event(app, process_id, agent_session_id, event);
            }
            return;
        }
        if let AcpEvent::Stream {
            kind,
            text,
            message_id,
            done,
        } = ev
        {
            self.handle_stream_chunk(
                app,
                process_id,
                agent_session_id,
                StreamChunk {
                    kind,
                    text,
                    message_id,
                    done,
                },
            );
            return;
        }
        if self.handle_state_event(app, process_id, agent_session_id, &ev) {
            return;
        }
        // Routing is decided from a snapshot, but focus/park transitions can
        // move the map before this async handler is polled. Re-check the
        // process and (when stamped) ACP session identity at the application
        // boundary so a late event cannot write into a newly reused slot.
        let route_matches = self.background.lock().get(app_session_id).is_some_and(|s| {
            s.process_id == process_id
                && agent_session_id.is_none_or(|sid| {
                    // Stamped events must name this background session
                    // exactly; an unset owner is not a safe wildcard while a
                    // focus/park transition is moving the same process.
                    s.meta.agent_session_id.as_deref() == Some(sid)
                })
        });
        if !route_matches {
            tracing::debug!(
                session = %app_session_id,
                process = %process_id,
                agent = ?agent_session_id,
                "background ACP event dropped after route changed"
            );
            return;
        }
        match ev {
            AcpEvent::Stream {
                kind,
                text,
                message_id,
                done,
            } => self.handle_stream_chunk(
                app,
                process_id,
                agent_session_id,
                StreamChunk {
                    kind,
                    text,
                    message_id,
                    done,
                },
            ),
            AcpEvent::PromptComplete {
                stop_reason,
                authoritative,
            } => {
                let mut pending_emits = Vec::new();
                let mut pending_persists = Vec::new();
                let finished = {
                    let mut bg = self.background.lock();
                    if let Some(s) = bg.get_mut(app_session_id) {
                        if let Some(p) = Self::take_pending_stream_emit(s) {
                            pending_emits.push(p);
                        }
                        Self::touch_stream_progress_locked(s);
                        if authoritative {
                            s.prompt_in_flight = false;
                        }
                        if !Self::should_rearm_deferred_prompt_complete(s) {
                            false
                        } else {
                            s.deferred_prompt_complete = Some(stop_reason.clone());
                            // Keep turn open while tools still running (long find / subagent).
                            match Self::try_finish_deferred_prompt_complete(
                                s,
                                Some(&mut pending_emits),
                                Some(&mut pending_persists),
                            ) {
                                None => {
                                    tracing::info!(
                                        "background prompt_complete deferred sid={} tools={}",
                                        app_session_id,
                                        s.open_tool_ids.len()
                                    );
                                    false
                                }
                                Some(_) => true,
                            }
                        }
                    } else {
                        false
                    }
                };
                Self::emit_stream_payloads(app, pending_emits);
                Self::commit_session_persists(Some(app), pending_persists);
                if finished {
                    self.promote_background_ready_to_parked(app_session_id);
                    Self::emit_runtime(
                        app,
                        &SessionSnapshot {
                            plan_chrome_revision: None,
                            session_id: Some(app_session_id.to_string()),
                            agent_session_id: None,
                            state: SessionState::Ready,
                            last_error: None,
                            streaming_message_id: None,
                            backend: Self::backend_name(),
                            model_id: None,
                            project_path: None,
                            title: String::new(),
                        },
                    );
                } else {
                    // Still busy in background — keep liveMap streaming.
                    Self::emit_runtime(
                        app,
                        &SessionSnapshot {
                            plan_chrome_revision: None,
                            session_id: Some(app_session_id.to_string()),
                            agent_session_id: None,
                            state: SessionState::Streaming,
                            last_error: None,
                            streaming_message_id: None,
                            backend: Self::backend_name(),
                            model_id: None,
                            project_path: None,
                            title: String::new(),
                        },
                    );
                }
                Self::emit_state(app, &self.snapshot());
            }
            AcpEvent::PermissionRequest { .. } => {}
            event @ (AcpEvent::ToolCall { .. } | AcpEvent::ToolOpenReleased { .. }) => {
                if let Some(event) = super::tool_event::ToolEvent::from_acp(event) {
                    self.handle_tool_event(app, process_id, agent_session_id, event);
                }
            }
            AcpEvent::Subagent(sub) => {
                let owned = self.background.lock().get(app_session_id).is_some_and(|s| {
                    s.process_id == process_id
                        && agent_session_id
                            .is_none_or(|id| s.meta.agent_session_id.as_deref() == Some(id))
                        && !Self::is_session_load_replay(s)
                });
                if owned {
                    let _ = app.emit("session://subagent", sub.to_payload(app_session_id));
                }
            }
            AcpEvent::Plan { .. } | AcpEvent::AskUserQuestion { .. } => {}
            AcpEvent::ProcessExited { code } => {
                let update = {
                    let mut background = self.background.lock();
                    background
                        .get_mut(app_session_id)
                        .filter(|s| s.process_id == process_id)
                        .map(|s| Self::prepare_process_exit_locked(s, code))
                };
                if let Some(update) = update {
                    update.publish(app);
                }
                self.emit_for_session(app, app_session_id);
            }
            AcpEvent::RetryState {
                attempt,
                max_retries,
                reason,
                status,
            } => {
                let cap = max_retries.clamp(1, HOST_PROVIDER_MAX_RETRIES);
                let abort = {
                    let mut bg = self.background.lock();
                    if let Some(s) = bg.get_mut(app_session_id) {
                        Self::note_provider_retry(s, attempt, &reason);
                        if !Self::should_apply_provider_retry_abort(s) || s.provider_retry_aborted {
                            false
                        } else {
                            should_abort_provider_retry_ex(attempt, max_retries, &status, &reason)
                        }
                    } else {
                        false
                    }
                };
                let _ = app.emit(
                    "session://retry",
                    serde_json::json!({
                        "sessionId": app_session_id,
                        "attempt": attempt,
                        "maxRetries": cap,
                        "reason": reason,
                        "status": status,
                        "aborting": abort,
                    }),
                );
                if abort {
                    let mut pending_emits = Vec::new();
                    let mut pending_persists = Vec::new();
                    let (acp, agent_sid) = {
                        let mut bg = self.background.lock();
                        if let Some(s) = bg.get_mut(app_session_id) {
                            if s.provider_retry_aborted {
                                (None, None)
                            } else {
                                s.provider_retry_aborted = true;
                                let err = provider_retry_abort_error(attempt, cap, &reason);
                                pending_persists.push(PendingSessionPersist::TurnBoundary(
                                    Box::new(Self::prepare_turn_error(s, &err, &mut pending_emits)),
                                ));
                                let _ = s.fsm.fail_with(err);
                                (s.acp.clone(), s.meta.agent_session_id.clone())
                            }
                        } else {
                            (None, None)
                        }
                    };
                    Self::emit_stream_payloads(app, pending_emits);
                    Self::commit_session_persists(Some(app), pending_persists);
                    if let Some(acp) = acp {
                        let abort_msg = provider_retry_abort_rpc_message(&reason);
                        acp.abort_pending_prompts(&abort_msg);
                        let _ = match agent_sid {
                            Some(sid) => acp.cancel_for(&sid).await,
                            None => acp.cancel().await,
                        };
                    }
                    self.promote_background_ready_to_parked(app_session_id);
                    Self::emit_state(app, &self.snapshot());
                }
            }
            _ => {
                // stderr / other variants — log only
                tracing::debug!("background acp event ignored variant for sid={app_session_id}");
            }
        }
    }
}
