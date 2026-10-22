//! Live ACP event pump handler.

use std::sync::Arc;

use tauri::{AppHandle, Emitter};
use uuid::Uuid;

use crate::acp_client::{
    provider_retry_abort_error, provider_retry_abort_rpc_message, should_abort_provider_retry_ex,
    AcpEvent, HOST_PROVIDER_MAX_RETRIES,
};
use crate::session_fsm::SessionState;
use crate::store::{self, ChatMessageStored};

use super::stream_event::StreamChunk;
use super::*;

impl SessionManager {
    pub(super) async fn handle_acp_event(
        self: &Arc<Self>,
        app: &AppHandle,
        process_id: &str,
        session_id: Option<&str>,
        ev: AcpEvent,
    ) {
        if matches!(ev, AcpEvent::PermissionRequest { .. }) {
            if let Some(event) = super::permission_request::PermissionRequestEvent::from_acp(ev) {
                self.handle_permission_request(app, process_id, session_id, event)
                    .await;
            }
            return;
        }
        if matches!(ev, AcpEvent::Plan { .. } | AcpEvent::AskUserQuestion { .. }) {
            if let Some(event) = super::human_gate_event::HumanGateEvent::from_acp(ev) {
                self.handle_human_gate_event(app, process_id, session_id, event);
            }
            return;
        }
        if matches!(
            ev,
            AcpEvent::ToolCall { .. } | AcpEvent::ToolOpenReleased { .. }
        ) {
            if let Some(event) = super::tool_event::ToolEvent::from_acp(ev) {
                self.handle_tool_event(app, process_id, session_id, event);
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
                session_id,
                StreamChunk {
                    kind,
                    text,
                    message_id,
                    done,
                },
            );
            return;
        }
        // State has its own explicit owner and never uses unstamped turn/focus
        // fallback. Handle before advisory live/background routing.
        if self.handle_state_event(app, process_id, session_id, &ev) {
            return;
        }
        // Process death is process-scoped: always scrub co-tenants first, then
        // deliver to live if this process is the focused shell.
        if let AcpEvent::ProcessExited { code } = &ev {
            let exit_code = *code;
            {
                let mut parked = self.parked.lock();
                parked.retain(|_, p| p.process_id != process_id);
            }
            // Background mid-turn shells need their own cancel path.
            let bg_ids: Vec<String> = self
                .background
                .lock()
                .iter()
                .filter(|(_, s)| s.process_id == process_id)
                .map(|(id, _)| id.clone())
                .collect();
            for bg_id in bg_ids {
                self.pending_soft_respawn.lock().remove(&bg_id);
                self.handle_acp_event_on_background(
                    app,
                    &bg_id,
                    process_id,
                    None,
                    AcpEvent::ProcessExited { code: exit_code },
                )
                .await;
            }
            let live_sid = self
                .inner
                .lock()
                .as_ref()
                .filter(|s| s.process_id == process_id)
                .map(|s| s.app_session_id.clone());
            if live_sid.is_none() {
                return;
            }
            if let Some(sid) = live_sid {
                self.pending_soft_respawn.lock().remove(&sid);
            }
            // Fall through to live ProcessExited handling below.
        } else {
            // Multi-session safety (P0): route by agent sessionId when present;
            // never rescue a parked co-tenant into a fake mid-turn (that wrote
            // foreign session/load replay into the wrong App journal).
            let live_hint = self.inner.lock().as_ref().map(|s| SessionRouteHint {
                app_session_id: s.app_session_id.clone(),
                process_id: s.process_id.clone(),
                agent_session_id: s.meta.agent_session_id.clone(),
                prompt_in_flight: s.prompt_in_flight,
            });
            let bg_hints: Vec<SessionRouteHint> = self
                .background
                .lock()
                .values()
                .map(|s| SessionRouteHint {
                    app_session_id: s.app_session_id.clone(),
                    process_id: s.process_id.clone(),
                    agent_session_id: s.meta.agent_session_id.clone(),
                    prompt_in_flight: s.prompt_in_flight,
                })
                .collect();
            let parked_hints: Vec<SessionRouteHint> = self
                .parked
                .lock()
                .values()
                .map(|p| SessionRouteHint {
                    app_session_id: p.app_session_id.clone(),
                    process_id: p.process_id.clone(),
                    agent_session_id: p.meta.agent_session_id.clone(),
                    prompt_in_flight: false,
                })
                .collect();
            match resolve_turn_event_route(
                process_id,
                session_id,
                live_hint.as_ref(),
                &bg_hints,
                &parked_hints,
            ) {
                TurnEventRoute::Background(bg_sid) => {
                    self.handle_acp_event_on_background(app, &bg_sid, process_id, session_id, ev)
                        .await;
                    return;
                }
                TurnEventRoute::Drop => {
                    if Self::event_carries_turn_output(&ev) {
                        tracing::debug!(
                            "acp event dropped: no apply target process={process_id} agent_sid={session_id:?} ev={}",
                            Self::event_kind_name(&ev)
                        );
                    }
                    return;
                }
                TurnEventRoute::Live => {
                    // Fall through to live match below.
                }
            }
        }

        // The route snapshot above is advisory. Re-check the live slot before
        // applying the event so a concurrent focus/park transition cannot
        // deliver a late process event to the next chat occupying the slot.
        let live_route_matches = self.inner.lock().as_ref().is_some_and(|s| {
            s.process_id == process_id
                && session_id.is_none_or(|sid| {
                    // Once the wire event carries a session id, require an
                    // exact owner match. Treating an unbound live hint as a
                    // wildcard lets late events from a co-tenant land in the
                    // newly focused session during connect/park races.
                    s.meta.agent_session_id.as_deref() == Some(sid)
                })
        });
        if !live_route_matches {
            if Self::event_carries_turn_output(&ev) {
                tracing::debug!(
                    process = %process_id,
                    agent = ?session_id,
                    ev = Self::event_kind_name(&ev),
                    "live ACP event dropped after route changed"
                );
            }
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
                session_id,
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
                let empty_run = {
                    let mut guard = self.inner.lock();
                    if let Some(s) = guard.as_mut() {
                        // Take any buffered stream before turn-end signals.
                        if let Some(p) = Self::take_pending_stream_emit(s) {
                            pending_emits.push(p);
                        }
                        Self::touch_stream_progress_locked(s);
                        // Only the RPC result ends the turn. It is ordered after
                        // every chunk, so clearing here cannot truncate output.
                        if authoritative {
                            s.prompt_in_flight = false;
                        }
                        if !Self::should_rearm_deferred_prompt_complete(s) {
                            None
                        } else {
                            s.deferred_prompt_complete = Some(stop_reason.clone());
                            // #52: do not Ready the UI while tools / permission / ask_user / plan
                            // are still open — agent often fires prompt_complete early.
                            match Self::try_finish_deferred_prompt_complete(
                                s,
                                Some(&mut pending_emits),
                                Some(&mut pending_persists),
                            ) {
                                None => {
                                    tracing::info!(
                                        "acp prompt_complete deferred stop={stop_reason} tools={} perm={} plan={} ask={}",
                                        s.open_tool_ids.len(),
                                        s.fsm.state() == SessionState::AwaitingPermission,
                                        s.pending_plan_rpc_id.is_some(),
                                        s.pending_ask_user_rpc_id.is_some(),
                                    );
                                    None
                                }
                                Some(empty) => empty,
                            }
                        }
                    } else {
                        None
                    }
                };
                Self::emit_stream_payloads(app, pending_emits);
                Self::commit_session_persists(Some(app), pending_persists);
                Self::emit_state(app, &self.snapshot());
                Self::emit_empty_run_if_any(app, empty_run);
            }
            AcpEvent::PermissionRequest { .. } => {}
            event @ (AcpEvent::ToolCall { .. } | AcpEvent::ToolOpenReleased { .. }) => {
                if let Some(event) = super::tool_event::ToolEvent::from_acp(event) {
                    self.handle_tool_event(app, process_id, session_id, event);
                }
            }
            AcpEvent::Plan { .. } | AcpEvent::AskUserQuestion { .. } => {}
            AcpEvent::Error { error } => {
                let mut pending_emits = Vec::new();
                let mut pending_persists = Vec::new();
                let mut continuation_failure: Option<(String, String)> = None;
                {
                    let mut guard = self.inner.lock();
                    if let Some(s) = guard.as_mut() {
                        continuation_failure = s
                            .active_turn_id
                            .as_ref()
                            .map(|turn| (s.app_session_id.clone(), turn.clone()));
                        if !s.provider_retry_aborted {
                            pending_persists.push(PendingSessionPersist::TurnBoundary(Box::new(
                                Self::prepare_turn_error(s, &error, &mut pending_emits),
                            )));
                        } else {
                            // Retry path already recorded the error; still drop busy
                            // markers so reconnect is not stuck Disconnected+busy.
                            if let Some(invalidation) =
                                Self::release_failed_turn_markers(s, Some(&mut pending_emits))
                            {
                                pending_persists.push(PendingSessionPersist::GateInvalidation(
                                    Box::new(invalidation),
                                ));
                            }
                        }
                        let _ = s.fsm.fail_with(error);
                    }
                }
                Self::emit_stream_payloads(app, pending_emits);
                Self::commit_session_persists(Some(app), pending_persists);
                if let Some((sid, turn)) = continuation_failure {
                    crate::session_migration::continuation::notify_turn_failure(&sid, &turn);
                }
                Self::emit_state(app, &self.snapshot());
            }
            AcpEvent::ProcessExited { code } => {
                let update = {
                    let mut guard = self.inner.lock();
                    guard
                        .as_mut()
                        .filter(|s| s.process_id == process_id)
                        .map(|s| Self::prepare_process_exit_locked(s, code))
                };
                if let Some(update) = update {
                    update.publish(app);
                }
                // Also drop any parked entry with this process id (defensive).
                self.parked.lock().retain(|_, p| p.process_id != process_id);
                Self::emit_state(app, &self.snapshot());
            }
            AcpEvent::State { .. } => {} // Consumed by the explicit receipt path above.
            AcpEvent::Stderr { line } => {
                // Always land agent stderr in the diagnostic log (post-mortem).
                tracing::warn!(target: "acp_stderr", "{line}");
                // CLI MCP worker fatals with AuthRequired when OAuth is missing/
                // expired — suppress re-inject of that host on the next session open.
                crate::extensions::note_mcp_auth_required_from_stderr(&line);
                let _ = app.emit("session://stderr", serde_json::json!({ "line": line }));
            }
            AcpEvent::HookActivity {
                kind,
                event_name,
                tool_name,
                ok,
                detail,
                raw,
            } => {
                let app_sid = {
                    let guard = self.inner.lock();
                    guard
                        .as_ref()
                        .map(|s| s.app_session_id.clone())
                        .unwrap_or_default()
                };
                let _ = app.emit(
                    "session://hook",
                    serde_json::json!({
                        "sessionId": app_sid,
                        "kind": kind,
                        "eventName": event_name,
                        "toolName": tool_name,
                        "ok": ok,
                        "detail": detail,
                        "update": raw,
                    }),
                );
            }
            AcpEvent::GoalUpdated {
                goal_id,
                role,
                current_deliverable_title,
                completed_deliverables,
                total_deliverables,
                verifying_completion,
                last_classifier_verdict,
                raw,
            } => {
                let app_sid = {
                    let guard = self.inner.lock();
                    guard
                        .as_ref()
                        .map(|s| s.app_session_id.clone())
                        .unwrap_or_default()
                };
                let _ = app.emit(
                    "session://goal",
                    serde_json::json!({
                        "sessionId": app_sid,
                        "goalId": goal_id,
                        "currentSubagentRole": role,
                        "currentDeliverableTitle": current_deliverable_title,
                        "completedDeliverables": completed_deliverables,
                        "totalDeliverables": total_deliverables,
                        "verifyingCompletion": verifying_completion,
                        "lastClassifierVerdict": last_classifier_verdict,
                        "update": raw,
                    }),
                );
            }
            AcpEvent::Subagent(sub) => {
                let app_sid = {
                    let guard = self.inner.lock();
                    guard
                        .as_ref()
                        .map(|s| s.app_session_id.clone())
                        .unwrap_or_default()
                };
                if !app_sid.is_empty() {
                    let _ = app.emit("session://subagent", sub.to_payload(&app_sid));
                }
            }
            AcpEvent::RetryState {
                attempt,
                max_retries,
                reason,
                status,
            } => {
                let cap = max_retries.clamp(1, HOST_PROVIDER_MAX_RETRIES);
                let abort = {
                    let mut guard = self.inner.lock();
                    if let Some(s) = guard.as_mut() {
                        Self::note_provider_retry(s, attempt, &reason);
                        // Reconnect / session/load residual retries (and shared-
                        // process noise while idle) must not fail the shell or
                        // append NETWORK_PROVIDER rows without a host-owned turn.
                        if !Self::should_apply_provider_retry_abort(s) {
                            tracing::debug!(
                                "acp retry_state ignored: no host turn (fsm={:?} pif={} attempt={attempt})",
                                s.fsm.state(),
                                s.prompt_in_flight
                            );
                            false
                        } else if s.provider_retry_aborted {
                            false
                        } else {
                            should_abort_provider_retry_ex(attempt, max_retries, &status, &reason)
                        }
                    } else {
                        false
                    }
                };

                // Match background emits: include sessionId so the UI can
                // ignore retries for a non-viewed chat.
                let live_sid = self.inner.lock().as_ref().map(|s| s.app_session_id.clone());
                let _ = app.emit(
                    "session://retry",
                    serde_json::json!({
                        "sessionId": live_sid,
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
                        let mut guard = self.inner.lock();
                        if let Some(s) = guard.as_mut() {
                            if s.provider_retry_aborted {
                                (None, None)
                            } else {
                                s.provider_retry_aborted = true;
                                // `attempt` is how many tries the host saw; `cap`
                                // is the agent/host budget — do not claim we ran
                                // the full budget when hard-transport fail-fast
                                // aborts early (e.g. attempt 3 of 12).
                                // Terminal quota uses QuotaExceeded + the CLI sentence.
                                let err = provider_retry_abort_error(attempt, cap, &reason);
                                // Chat-visible error row (must happen before clearing stream ids)
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
                        // Target the session explicitly (shared process safety).
                        let _ = match agent_sid {
                            Some(sid) => acp.cancel_for(&sid).await,
                            None => acp.cancel().await,
                        };
                    }
                    Self::emit_state(app, &self.snapshot());
                }
            }
            AcpEvent::ContextCompact {
                trigger,
                tokens_before,
                tokens_after,
                summary_preview,
                note,
            } => {
                let (app_sid, content) = {
                    let guard = self.inner.lock();
                    let Some(s) = guard.as_ref() else {
                        return;
                    };
                    // Compact markers during load/replay would spam the journal.
                    if Self::is_session_load_replay(s) {
                        tracing::debug!(
                            "acp context_compact dropped: no prompt in flight (replay)"
                        );
                        return;
                    }
                    let mut parts = Vec::new();
                    if trigger == "manual" {
                        parts.push("manual".to_string());
                    } else {
                        parts.push("auto".to_string());
                    }
                    if let (Some(b), Some(a)) = (tokens_before, tokens_after) {
                        parts.push(format!("tokens:{b}->{a}"));
                    } else if let Some(b) = tokens_before {
                        parts.push(format!("tokens_before:{b}"));
                    } else if let Some(a) = tokens_after {
                        parts.push(format!("tokens_after:{a}"));
                    }
                    if let Some(n) = note.as_ref().filter(|s| !s.is_empty()) {
                        parts.push(format!("note:{n}"));
                    }
                    // Machine-readable line for UI; human copy is i18n on frontend.
                    let mut content = format!("context_compact|{}", parts.join("|"));
                    if let Some(sum) = summary_preview
                        .as_ref()
                        .map(|s| s.trim())
                        .filter(|s| !s.is_empty())
                    {
                        content.push('\n');
                        content.push_str(sum);
                    }
                    (s.app_session_id.clone(), content)
                };
                let mid = Uuid::new_v4().to_string();
                if let Err(e) = store::append_message(
                    &app_sid,
                    ChatMessageStored {
                        id: mid.clone(),
                        role: "tool".into(),
                        content: content.clone(),
                        thought: None,
                        created_at: chrono::Utc::now(),
                        is_error: false,
                        attachments: None,
                        marker: Some("context_compact".into()),
                    },
                ) {
                    tracing::error!(session = %app_sid, "context compact journal append failed: {e}");
                }
                let _ = app.emit(
                    "session://context_compact",
                    serde_json::json!({
                        "sessionId": app_sid,
                        "messageId": mid,
                        "trigger": trigger,
                        "tokensBefore": tokens_before,
                        "tokensAfter": tokens_after,
                        "summaryPreview": summary_preview,
                        "note": note,
                        "content": content,
                    }),
                );
            }
            AcpEvent::UsageReported {
                total_tokens,
                input_tokens,
                output_tokens,
                system_tokens,
                tools_tokens,
                history_tokens,
                cached_read_tokens,
                cache_creation_tokens,
                reasoning_tokens,
                cost_usd_ticks,
                model_calls,
                api_duration_ms,
                cost_is_partial,
                usage_is_incomplete,
                context_window,
                percentage,
                source,
            } => {
                let app_sid = {
                    let guard = self.inner.lock();
                    let Some(s) = guard.as_ref() else {
                        return;
                    };
                    if Self::is_session_load_replay(s) {
                        return;
                    }
                    s.app_session_id.clone()
                };
                let _ = app.emit(
                    "session://usage",
                    serde_json::json!({
                        "sessionId": app_sid,
                        "totalTokens": total_tokens,
                        "inputTokens": input_tokens,
                        "outputTokens": output_tokens,
                        "systemTokens": system_tokens,
                        "toolsTokens": tools_tokens,
                        "historyTokens": history_tokens,
                        "cachedReadTokens": cached_read_tokens,
                        "cacheCreationTokens": cache_creation_tokens,
                        "reasoningTokens": reasoning_tokens,
                        "costUsdTicks": cost_usd_ticks,
                        "modelCalls": model_calls,
                        "apiDurationMs": api_duration_ms,
                        "costIsPartial": cost_is_partial,
                        "usageIsIncomplete": usage_is_incomplete,
                        "contextWindow": context_window,
                        "percentage": percentage,
                        "source": source,
                    }),
                );
            }
        }
    }
}
