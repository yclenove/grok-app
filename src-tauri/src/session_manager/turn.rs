//! User turn: send, interject, stop.

use std::sync::Arc;
use std::time::Duration;

use tauri::{AppHandle, Emitter};
use uuid::Uuid;

use crate::acp_client::{AcpClient, PermissionOutcome};
use crate::error::{AgentError, AgentErrorCode};
use crate::mock_acp::{self, StreamChunk};
use crate::session_fsm::SessionState;
use crate::store::{self, ChatMessageStored, MessageAttachmentStored};

use super::*;

impl SessionManager {
    /// A prompt waiter can outlive Stop/retry. Never let its completion mutate
    /// the newer turn's runtime flags, whether the chat is live or background.
    pub(super) fn with_turn_mut<R>(
        &self,
        sid: &str,
        turn_id: &str,
        f: impl FnOnce(&mut LiveSession) -> R,
    ) -> Option<R> {
        self.with_session_mut(sid, |session| {
            (session.active_turn_id.as_deref() == Some(turn_id)).then(|| f(session))
        })
        .flatten()
    }

    /// Connect-lock gate `send_message` takes before ACP. Same wait and error.
    pub(crate) async fn acquire_send_connect_lock(
        &self,
    ) -> Result<tokio::sync::MutexGuard<'_, ()>, String> {
        self.try_lock_connect(Duration::from_secs(CONNECT_WALL_CLOCK_SECS))
            .await
            .ok_or_else(|| {
                format!(
                    "{}: {}",
                    AgentErrorCode::ConnectFailed.as_str(),
                    connect_gave_up_reason(false)
                )
            })
    }

    pub async fn send_message(
        self: &Arc<Self>,
        app: AppHandle,
        text: String,
        display_text: Option<String>,
        attachments: Option<Vec<MessageAttachmentStored>>,
        session_id: Option<String>,
    ) -> Result<SessionSnapshot, String> {
        self.send_prompt(app, text.into(), display_text, attachments, session_id)
            .await
    }

    pub(crate) async fn send_injected_message(
        self: &Arc<Self>,
        app: AppHandle,
        text: String,
        session_id: String,
        origin: &'static str,
    ) -> Result<SessionSnapshot, String> {
        self.send_prompt(
            app,
            crate::injected_prompts::SendPrompt {
                text,
                origin: Some(origin),
            },
            None,
            None,
            Some(session_id),
        )
        .await
    }

    async fn send_prompt(
        self: &Arc<Self>,
        app: AppHandle,
        prompt: crate::injected_prompts::SendPrompt,
        display_text: Option<String>,
        attachments: Option<Vec<MessageAttachmentStored>>,
        session_id: Option<String>,
    ) -> Result<SessionSnapshot, String> {
        let crate::injected_prompts::SendPrompt { text, origin } = prompt;
        let text = text.trim().to_string();
        // Journal stores UI form when provided (skill / attached-chat tokens).
        // Do not wholesale-trim display: that drops leading/trailing blank lines the user
        // typed. Empty-check uses trim; payload keeps internal + intentional edge blanks.
        let mut journal_content = display_text
            .and_then(|s| {
                let s = s.replace("\r\n", "\n").replace('\r', "\n");
                if s.trim().is_empty() {
                    None
                } else {
                    Some(s)
                }
            })
            .unwrap_or_else(|| text.clone());
        let attached = crate::session_attach::extract_attached_chats(&journal_content);
        let stripped_text = crate::session_attach::strip_chat_tokens(&text);
        if stripped_text.is_empty() && attached.is_empty() {
            return Err("empty message".into());
        }
        // CLI slash commands run only at byte 0. Detach this send's `/goal`
        // before attach/history wrappers are built, then restore it immediately
        // before dispatch. Historical `/goal` lines must never be promoted.
        let (goal_cmd, request_text) = detach_current_goal_command(&stripped_text);
        // User file/image cards — structured field is primary for history cards.
        // Also dual-write `@/abs/path` sole-lines into content so reload can recover
        // cards even if an older reader ignores the attachments field (FE strips
        // those lines via parseAttachmentsFromContent for the bubble body).
        let journal_attachments = attachments.filter(|items| !items.is_empty());
        if let Some(ref atts) = journal_attachments {
            grant_journal_attachment_paths(atts);
            journal_content = append_journal_attachment_refs(journal_content, atts);
        }
        // Note: image @path stripping + Host vision runs on the *final*
        // agent_prompt after history bootstrap (see below). Do not rewrite
        // here only — bootstrap can reintroduce @image paths from the journal.

        // Serialize against connect for the whole focus + turn-open window, so
        // the slot cannot move between the target check and `begin_stream`.
        let _focus_guard = self.acquire_send_connect_lock().await?;
        // Open the turn on the target wherever it sits (live **or** background).
        // Multi-window: a secondary send must not require stealing the live focus
        // from a main-window mid-turn when the target already has a warm agent.
        let target_sid = session_id
            .clone()
            .or_else(|| self.inner.lock().as_ref().map(|s| s.app_session_id.clone()));
        let Some(app_sid) = target_sid else {
            return Err("no active session".into());
        };
        if !AcpClient::use_mock() {
            let meta = workspace_context::session(&app_sid)?;
            let prefs = store::resolve_composer_prefs(meta.project_id.as_deref(), Some(&app_sid));
            self.queue_main_route_rebind(&meta, &prefs).await?;
        }
        // The alignment/flags request belongs to this chat, not the focus slot.
        // Drop an idle stale process before promptability; the frontend's one
        // reconnect/retry then starts with the saved settings. Busy turns defer.
        self.flush_pending_soft_respawn(&app, &app_sid).await;
        match self.ensure_promptable_session(&app, &app_sid) {
            Ok(true) => {}
            Ok(false) => {
                return Err(format!(
                    "{}: chat {app_sid} has no live agent process — reconnect and retry",
                    AgentErrorCode::ConnectFailed.as_str()
                ));
            }
            Err(e) => return Err(format!("{}: {}", e.code.as_str(), e.message)),
        }

        let mut opened = match self
            .open_user_turn_with_origin(
                &app_sid,
                &request_text,
                &journal_content,
                journal_attachments,
                origin,
            )
            .await
        {
            Ok(open) => open,
            Err(error) => {
                self.emit_for_session(&app, &app_sid);
                return Err(error);
            }
        };
        let continuation_claimed =
            match crate::session_migration::continuation::apply_continuation_first_send(
                &opened.app_sid,
                &opened.agent_prompt,
                opened.policy,
            ) {
                Ok(crate::session_migration::continuation::ContinuationFirstSend::Handoff {
                    prompt,
                    handoff_text_hash,
                }) => {
                    opened.agent_prompt = prompt;
                    if let Err(e) = crate::session_migration::continuation::mark_dispatching_checked(
                        &opened.app_sid,
                        &opened.agent_prompt,
                        opened.agent_sid.as_deref(),
                        &handoff_text_hash,
                        opened.policy,
                        &opened.turn_id,
                    ) {
                        let cause = e.into_command_err();
                        let error = self
                            .rollback_opened_turn_before_dispatch(&opened, &cause)
                            .await;
                        self.emit_for_session(&app, &opened.app_sid);
                        return Err(error);
                    }
                    tracing::info!(
                        "import handoff attached ({} chars) for session {}",
                        opened.agent_prompt.len(),
                        opened.app_sid
                    );
                    true
                }
                Ok(crate::session_migration::continuation::ContinuationFirstSend::Passthrough) => {
                    if opened.needs_history_bootstrap {
                        if let Some(ctx) = build_history_bootstrap(&opened.app_sid) {
                            opened.agent_prompt = format!("{ctx}\n{}", opened.agent_prompt);
                            tracing::info!(
                                "history bootstrap attached ({} chars) for session {}",
                                ctx.len(),
                                opened.app_sid
                            );
                        }
                    }
                    false
                }
                Err(e) => {
                    let cause = e.into_command_err();
                    let error = self
                        .rollback_opened_turn_before_dispatch(&opened, &cause)
                        .await;
                    self.emit_for_session(&app, &opened.app_sid);
                    return Err(error);
                }
            };
        let turn_open::OpenedTurn {
            backend,
            app_sid,
            acp,
            agent_sid,
            mut agent_prompt,
            message_id,
            turn_id,
            user_row,
            ..
        } = opened;
        // P2: steer session-by-UUID lookups to App/agent-home roots (avoid home-wide find).
        if let Some(hint) = session_lookup_host_hint(&request_text) {
            agent_prompt = format!("{hint}\n{agent_prompt}");
        }
        // The session slot is now in `Streaming` and owns its ACP client. Do
        // not hold the global connect/park lock while host vision or another
        // side-channel performs network/CLI work (official vision can take
        // minutes). Keeping the guard here used to block every other chat's
        // connect/send path until that preparation completed.
        drop(_focus_guard);
        // Host side-channels before main model (vision first, then X). Emit tool
        // chips immediately so the UI shows waiting state instead of freezing.
        // Copy is non-technical (no `grok -p` / command lines in chip detail).
        let locale = store::load_settings().locale;
        let zh = locale.starts_with("zh");

        // Push streaming state before long host side-channels so the pill stays
        // "进行中" (not "就绪") while recognizing.
        self.emit_for_session(&app, &app_sid);
        // Mirror / other windows did not run local optimistic UI — publish the
        // journal user row (+ stream shell id) so every client can paint it (#1001).
        crate::mirror::fanout_event(
            &app,
            "session://user_message",
            serde_json::json!({
                "sessionId": app_sid,
                "message": user_row,
                "streamMessageId": message_id,
            }),
        );
        crate::mirror::notify_sessions_changed(Some(&app), "turn", &app_sid);

        // ── Host vision (custom text-only main + @image only) ──────────────
        // Official Grok route: never Host-describe (native multimodal).
        // X/web: tools-first via official-aux MCP — no Host keyword pre-search.
        let vision_context = acp
            .as_ref()
            .and_then(|client| client.main_vision_context(agent_sid.as_deref()))
            .unwrap_or_else(crate::models_aux::HostVisionContext::for_current_main);
        let host_vision = vision_context.will_run(&agent_prompt);
        let host_tool_id = if host_vision {
            let id = format!("host-vision-{}", Uuid::new_v4());
            let (title, detail_run) = if zh {
                ("识别图片内容", "正在识别，请耐心等待…")
            } else {
                ("Recognizing image", "Working… please wait")
            };
            let _ = app.emit(
                "session://tool",
                serde_json::json!({
                    "sessionId": app_sid,
                    "toolCallId": id,
                    "title": title,
                    "kind": "vision",
                    "status": "in_progress",
                    "path": null,
                    "detail": detail_run,
                }),
            );
            self.with_session_mut(&app_sid, |s| {
                SessionManager::touch_stream_progress_locked(s);
            });
            Some((id, title.to_string()))
        } else {
            None
        };
        // Stream progress from official ACP into the *same* host-vision tool row
        // (native tool_step upsert by toolCallId — no second chip).
        let vision_progress: Option<crate::official_aux::OfficialProgressCb> =
            host_tool_id.as_ref().map(|(id, title)| {
                let app_p = app.clone();
                let sid_p = app_sid.clone();
                let tool_id = id.clone();
                let title_p = title.clone();
                let mgr = Arc::clone(self);
                std::sync::Arc::new(move |p: crate::official_aux::OfficialAcpProgress| {
                    let detail = if p.detail.trim().is_empty() {
                        if zh {
                            "正在识别…".to_string()
                        } else {
                            "Working…".to_string()
                        }
                    } else {
                        p.detail
                    };
                    // Always keep Host title; stream lives in detail only.
                    let _ = app_p.emit(
                        "session://tool",
                        serde_json::json!({
                            "sessionId": sid_p,
                            "toolCallId": tool_id,
                            "title": title_p,
                            "kind": "vision",
                            "status": "in_progress",
                            "path": null,
                            "detail": detail,
                        }),
                    );
                    mgr.with_session_mut(&sid_p, |s| {
                        SessionManager::touch_stream_progress_locked(s);
                    });
                }) as crate::official_aux::OfficialProgressCb
            });

        let prep = crate::models_aux::prepare_agent_prompt_for_context(
            &agent_prompt,
            vision_progress,
            &vision_context,
        )
        .await;
        // CLI slash commands run only at byte 0. Put this send's `/goal`
        // back in front of the host wrappers. Do not search those wrappers
        // for another `/goal` line.
        let agent_prompt = match goal_cmd {
            Some(cmd) => prefix_goal_command(&cmd, &prep.prompt),
            None => prep.prompt,
        };
        if let Some((id, title)) = host_tool_id {
            let status = if prep.ok { "completed" } else { "failed" };
            // Keep full description in detail for expand / journal (not "识别完成").
            let detail = if !prep.description.trim().is_empty() {
                prep.description.clone()
            } else if prep.ok {
                if zh {
                    "识别完成".to_string()
                } else {
                    "Done".to_string()
                }
            } else if zh {
                "识别失败".to_string()
            } else {
                "Failed".to_string()
            };
            let _ = app.emit(
                "session://tool",
                serde_json::json!({
                    "sessionId": app_sid,
                    "toolCallId": id,
                    "title": title,
                    "kind": "vision",
                    "status": status,
                    "path": null,
                    "detail": detail,
                }),
            );
            journal_host_tool_step(&app_sid, &id, status, "vision", &title, &detail);
            self.with_session_mut(&app_sid, |s| {
                SessionManager::touch_stream_progress_locked(s);
            });
        }
        // Emit runtime for background targets; state for live focus.
        self.emit_for_session(&app, &app_sid);

        // Host vision / prepare can take a long time. If the user hit Stop (or
        // stall/force-end cleared this turn) while we were away, do **not**
        // spawn session/prompt — otherwise the agent runs with streams dropped
        // as load-replay and the next send may see task_already_running.
        let still_this_turn = self
            .with_session_mut(&app_sid, |s| {
                s.prompt_in_flight
                    && s.active_turn_id.as_deref() == Some(turn_id.as_str())
                    && s.streaming_message_id.as_deref() == Some(message_id.as_str())
            })
            .unwrap_or(false);
        if !still_this_turn {
            tracing::info!(
                session = %app_sid,
                turn = %turn_id,
                message = %message_id,
                "send_message: turn no longer active after prepare; skip prompt_for"
            );
            crate::session_migration::continuation::notify_failed_before_dispatch(
                &app_sid, &turn_id,
            );
            self.emit_for_session(&app, &app_sid);
            // Must not return Ok — automations treat Ok as "fired" and
            // advance next_run_at / disable once tasks (P0-2). Frontend
            // maps TURN_CANCELLED to a silent cancel (user Stop / stall).
            return Err(format!(
                "TURN_CANCELLED: turn {turn_id} no longer active after prepare; prompt not dispatched"
            ));
        }

        // Host-side prompt rewriting is complete. Persist the exact payload
        // before ACP so recovery can compare the wire prompt, not just the
        // handoff fragment captured during preflight.
        if continuation_claimed {
            if let Err(e) = crate::session_migration::continuation::finalize_prompt_hash_for_turn(
                &app_sid,
                &turn_id,
                &agent_prompt,
            ) {
                self.with_turn_mut(&app_sid, &turn_id, |s| {
                    let _ = s.fsm.end_stream();
                    s.prompt_in_flight = false;
                    crate::turn_lease::clear_lease(&s.app_session_id);
                    s.sent_prompt_this_visit = false;
                    s.active_turn_id = None;
                    s.streaming_message_id = None;
                });
                crate::session_migration::continuation::notify_failed_before_dispatch(
                    &app_sid, &turn_id,
                );
                self.emit_for_session(&app, &app_sid);
                return Err(e.into_command_err());
            }
        }

        if backend == "mock_acp" || AcpClient::use_mock() {
            let mgr = Arc::clone(self);
            let app_done = app.clone();
            let turn_sid = app_sid.clone();
            let mock_turn_id = turn_id.clone();
            let handle = mock_acp::spawn_fake_stream(
                app_sid.clone(),
                message_id,
                agent_prompt,
                Duration::from_millis(25),
                move |chunk: StreamChunk| {
                    let Some(update) =
                        mgr.prepare_mock_stream_chunk(&turn_sid, &mock_turn_id, &chunk)
                    else {
                        return;
                    };
                    SessionManager::emit_stream_payloads(&app_done, update.emits);
                    if let Some(journal) = update.journal {
                        SessionManager::commit_stream_journal_flush(journal);
                    }
                    if let Some(schedule) = update.schedule {
                        mgr.schedule_mock_stream_emit_flush(app_done.clone(), schedule);
                    }
                    if chunk.done {
                        crate::session_migration::continuation::notify_turn_success(
                            &turn_sid,
                            &mock_turn_id,
                        );
                        mgr.emit_for_session(&app_done, &turn_sid);
                    }
                },
            );
            let mut handle = Some(handle);
            self.with_turn_mut(&app_sid, &turn_id, |s| {
                s.mock_stream = handle.take();
            });
            if let Some(handle) = handle {
                handle.request_stop();
            }
            // Return the target's snapshot when possible (background path).
            if self.is_live_session(&app_sid) {
                return Ok(self.snapshot());
            }
            if let Some(snap) = self
                .background
                .lock()
                .get(&app_sid)
                .map(Self::snapshot_from_live)
            {
                return Ok(snap);
            }
            return Ok(self.snapshot());
        }

        // Bail *after* the turn was opened → roll it back, or the chat is stuck
        // forever: `prompt_in_flight` blocks both parking and the next send.
        let Some(acp) = acp else {
            self.with_session_mut(&app_sid, |s| {
                s.prompt_in_flight = false;
                s.streaming_message_id = None;
                s.active_turn_id = None;
                s.stream_message_id_locked = false;
                if s.fsm.state() == SessionState::Streaming {
                    let _ = s.fsm.end_stream();
                }
            });
            crate::session_migration::continuation::notify_failed_before_dispatch(
                &app_sid, &turn_id,
            );
            self.emit_for_session(&app, &app_sid);
            return Err("ACP client missing".into());
        };
        let mgr = Arc::clone(self);
        let app2 = app.clone();
        let turn_sid = app_sid.clone();
        tokio::spawn(async move {
            // Explicit session id: on a shared process the CLI's “recently
            // bound” id may belong to another App session — prompts must
            // target this chat's own agent session.
            let outcome = match agent_sid {
                Some(sid) => {
                    let ready = if continuation_claimed {
                        crate::session_migration::continuation::mark_running_for_turn(
                            &turn_sid, &turn_id,
                        )
                    } else {
                        Ok(())
                    };
                    match ready {
                        Ok(()) => acp.prompt_for(&sid, &agent_prompt).await,
                        Err(error) => Err(AgentError::new(
                            AgentErrorCode::AgentCrashed,
                            error.into_command_err(),
                        )),
                    }
                }
                None => {
                    crate::session_migration::continuation::notify_failed_before_dispatch(
                        &turn_sid, &turn_id,
                    );
                    Err(AgentError::new(
                        AgentErrorCode::AgentCrashed,
                        "chat has no agent session id (reconnect)",
                    ))
                }
            };
            match outcome {
                Err(e) => {
                    // Route by session id: this chat may have been demoted to
                    // background while the prompt ran, and the live slot now holds
                    // someone else's turn — recording the error there would blame
                    // the wrong chat.
                    let mut record_error = false;
                    let mut pending_emits = Vec::new();
                    let mut pending_persists = Vec::new();
                    mgr.with_turn_mut(&turn_sid, &turn_id, |s| {
                        // The RPC failed, so no authoritative PromptComplete will
                        // arrive. Release the turn or the chat stays un-parkable
                        // and refuses further sends.
                        s.prompt_in_flight = false;
                        // Stall heal / user stop already force-ended (Ready) with
                        // journal kept — do not clobber with fail_with when
                        // cancel/abort unblocks this waiter.
                        if !matches!(
                            s.fsm.state(),
                            SessionState::Streaming | SessionState::AwaitingPermission
                        ) {
                            return;
                        }
                        // Skip if host already recorded a retry-exhausted error this turn.
                        if !s.provider_retry_aborted {
                            pending_persists.push(PendingSessionPersist::TurnBoundary(Box::new(
                                SessionManager::prepare_turn_error(s, &e, &mut pending_emits),
                            )));
                            let _ = s.fsm.fail_with(e);
                            record_error = true;
                        }
                    });
                    SessionManager::emit_stream_payloads(&app2, pending_emits);
                    SessionManager::commit_session_persists(Some(&app2), pending_persists);
                    if record_error {
                        mgr.emit_for_session(&app2, &turn_sid);
                    }
                    crate::session_migration::continuation::notify_turn_failure(
                        &turn_sid, &turn_id,
                    );
                    // A policy/effort change queued during this turn must be
                    // applied once the prompt RPC is terminal; otherwise the
                    // next ready fast-path promotes the stale ACP forever.
                    mgr.flush_pending_soft_respawn(&app2, &turn_sid).await;
                }
                Ok(()) => {
                    // #522: even if PromptComplete events were dropped/raced,
                    // a successful session/prompt must release the busy gate.
                    // Also heal sticky Streaming when `prompt_in_flight` was
                    // already cleared (dropped PromptComplete / partial finish)
                    // but FSM never left Streaming — UI shows "thinking" forever
                    // while the agent turn already ended (journal may hold body).
                    let mut need_emit = false;
                    let mut pending_emits = Vec::new();
                    let mut pending_persists = Vec::new();
                    mgr.with_turn_mut(&turn_sid, &turn_id, |s| {
                        // Only heal sticky *Streaming* here — leave
                        // AwaitingPermission alone (user gate still live).
                        let sticky_streaming = s.fsm.state() == SessionState::Streaming;
                        if s.prompt_in_flight {
                            tracing::warn!(
                                target: "session",
                                session = %turn_sid,
                                "prompt RPC Ok but prompt_in_flight still true — force-clear (#522)"
                            );
                            s.prompt_in_flight = false;
                            if s.deferred_prompt_complete.is_none() {
                                s.deferred_prompt_complete = Some("end_turn".into());
                            }
                            need_emit = SessionManager::try_finish_deferred_prompt_complete(
                            s,
                            Some(&mut pending_emits),
                            Some(&mut pending_persists),
                        )
                            .is_some();
                        } else if sticky_streaming {
                            tracing::warn!(
                                target: "session",
                                session = %turn_sid,
                                "prompt RPC Ok but FSM still Streaming with prompt_in_flight=false — force-finish sticky stream"
                            );
                            if s.deferred_prompt_complete.is_none() {
                                s.deferred_prompt_complete = Some("end_turn".into());
                            }
                            need_emit = SessionManager::try_finish_deferred_prompt_complete(
                            s,
                            Some(&mut pending_emits),
                            Some(&mut pending_persists),
                        )
                            .is_some();
                            // If gates still block finish, at least drop busy so
                            // reconnect/send are not wedged forever.
                            if !need_emit
                                && s.open_tool_ids.is_empty()
                                && s.pending_plan_rpc_id.is_none()
                                && s.pending_ask_user_rpc_id.is_none()
                            {
                                // Best-effort take so partial stream_buf is not lost
                                // when we force-end without try_finish.
                                if let Some(p) =
                                    SessionManager::take_pending_stream_emit_done(s)
                                {
                                    pending_emits.push(p);
                                }
                                SessionManager::maybe_flush_stream_journal(s, true, false);
                                s.stream_buf.clear();
                                s.stream_thought.clear();
                                s.stream_last_was_assistant = false;
                                let _ = s.fsm.end_stream();
                                s.streaming_message_id = None;
                                s.active_turn_id = None;
                                s.stream_message_id_locked = false;
                                s.deferred_prompt_complete = None;
                                need_emit = true;
                            }
                        }
                    });
                    SessionManager::emit_stream_payloads(&app2, pending_emits);
                    SessionManager::commit_session_persists(Some(&app2), pending_persists);
                    if need_emit {
                        mgr.emit_for_session(&app2, &turn_sid);
                    }
                    crate::session_migration::continuation::notify_turn_success(
                        &turn_sid, &turn_id,
                    );
                    // Always best-effort pull missing assistant/tool rows from
                    // agent chat_history after the prompt RPC completes. The
                    // CLI's final disk flush can trail the RPC response, so use
                    // a short bounded retry window off the async worker thread.
                    let changed =
                        post_turn_reconcile::reconcile_linked_session(&mgr, &turn_sid, &turn_id)
                            .await;
                    if changed > 0 {
                        let _ = app2.emit(
                            "session://journal_reconciled",
                            serde_json::json!({
                                "sessionId": turn_sid,
                                "changed": changed,
                            }),
                        );
                    }
                    // Apply deferred process-level settings only after the
                    // final journal reconcile has had a chance to read the
                    // completed turn from the agent's disk log.
                    mgr.flush_pending_soft_respawn(&app2, &turn_sid).await;
                }
            }
        });

        if self.is_live_session(&app_sid) {
            return Ok(self.snapshot());
        }
        if let Some(snap) = self
            .background
            .lock()
            .get(&app_sid)
            .map(Self::snapshot_from_live)
        {
            return Ok(snap);
        }
        Ok(self.snapshot())
    }

    /// Stop the turn on `session_id` (defaults to the live focus slot).
    ///
    /// Targets background turns too: the user can watch a demoted chat and hit
    /// Stop there, which previously cancelled whichever chat held focus.
    ///
    /// Inject guidance into the currently streaming turn without cancelling it.
    ///
    /// `session_id` names the chat (live or background). Omitting it uses the
    /// focused live slot. Does **not** rewrite the follow-up send queue.
    pub async fn interject_message<R: tauri::Runtime>(
        &self,
        app: AppHandle<R>,
        text: String,
        display_text: Option<String>,
        attachments: Option<Vec<MessageAttachmentStored>>,
        session_id: Option<String>,
    ) -> Result<SessionSnapshot, String> {
        let text = text.trim().to_string();
        if text.is_empty() {
            return Err("empty interjection".into());
        }
        // Same display blank-line policy as send_message (no wholesale trim).
        let mut journal_content = display_text
            .and_then(|s| {
                let s = s.replace("\r\n", "\n").replace('\r', "\n");
                if s.trim().is_empty() {
                    None
                } else {
                    Some(s)
                }
            })
            .unwrap_or_else(|| text.clone());
        let attachments = attachments.filter(|items| !items.is_empty());
        if let Some(ref atts) = attachments {
            grant_journal_attachment_paths(atts);
            journal_content = append_journal_attachment_refs(journal_content, atts);
        }
        let target = session_id.as_deref();

        // Take any pending ask-user / plan reverse-RPC so steer is not blocked
        // behind an unanswered questionnaire (diag: interject OK + ask_user hang).
        let (backend, app_sid, turn_id, agent_sid, acp, pending_ask, pending_plan, invalidation) = {
            if let Some(t) = target {
                let mut guard = self.inner.lock();
                if let Some(s) = guard.as_mut().filter(|s| s.app_session_id == t) {
                    let picked = Self::pick_interjection_target(s)?;
                    let (pending_ask, pending_plan, invalidation) =
                        Self::take_human_gates_for_interject(s);
                    (
                        picked.0,
                        picked.1,
                        picked.2,
                        picked.3,
                        picked.4,
                        pending_ask,
                        pending_plan,
                        invalidation,
                    )
                } else {
                    drop(guard);
                    let mut background = self.background.lock();
                    let s = background
                        .get_mut(t)
                        .ok_or_else(|| format!("interjection: chat {t} is not active"))?;
                    let picked = Self::pick_interjection_target(s)?;
                    let (pending_ask, pending_plan, invalidation) =
                        Self::take_human_gates_for_interject(s);
                    (
                        picked.0,
                        picked.1,
                        picked.2,
                        picked.3,
                        picked.4,
                        pending_ask,
                        pending_plan,
                        invalidation,
                    )
                }
            } else {
                let mut guard = self.inner.lock();
                let s = guard.as_mut().ok_or("no active session")?;
                let picked = Self::pick_interjection_target(s)?;
                let (pending_ask, pending_plan, invalidation) =
                    Self::take_human_gates_for_interject(s);
                (
                    picked.0,
                    picked.1,
                    picked.2,
                    picked.3,
                    picked.4,
                    pending_ask,
                    pending_plan,
                    invalidation,
                )
            }
        };

        if let Some(invalidation) = invalidation {
            invalidation.emit(&app, "interject");
        }
        if backend != "mock_acp" && !AcpClient::use_mock() {
            let client = acp.ok_or("ACP client missing")?;
            // Unblock reverse-RPCs first — otherwise interject is accepted but
            // the agent stays wedged on ask_user_question / exit_plan_mode.
            if let Some(id) = pending_ask {
                if let Err(e) = id.respond(&client).await {
                    tracing::warn!(rpc = id.rpc, "interject: auto-cancel ask_user failed: {e}");
                }
            }
            if let Some(id) = pending_plan {
                if let Err(e) = id.respond(&client).await {
                    tracing::warn!(rpc = id.rpc, "interject: auto-cancel plan failed: {e}");
                }
            }
            match agent_sid {
                Some(sid) => {
                    tracing::info!(
                        "interject: app_session={app_sid} agent_session={sid} turn={turn_id}"
                    );
                    client.interject_for(&sid, &text).await.map_err(|e| {
                        tracing::warn!("interject: ACP failed app={app_sid} agent={sid}: {e}");
                        e
                    })?;
                }
                None => {
                    return Err("interjection: chat has no agent session id (reconnect)".into());
                }
            }
        }

        let created_at = chrono::Utc::now();
        let message = ChatMessageStored {
            id: Uuid::new_v4().to_string(),
            role: "user".into(),
            content: journal_content,
            thought: None,
            created_at,
            is_error: false,
            attachments,
            marker: Some("interjection".into()),
        };

        // Session may move between live/background while the ACP RPC is in flight.
        // ACP already accepted the inject — always try to surface UI + journal.
        //
        // IMPORTANT: while holding `inner` / `background`, return
        // `snapshot_from_live` — never `self.snapshot()`, which re-locks `inner`
        // (parking_lot is non-reentrant → permanent deadlock → FE stuck on
        // 「正在引导…」 until the 55s UI timeout).
        {
            let mut guard = self.inner.lock();
            if let Some(s) = guard.as_mut() {
                if s.app_session_id == app_sid {
                    Self::commit_interjection_boundary(s, &app, &message, &app_sid, &turn_id)?;
                    return Ok(Self::snapshot_from_live(s));
                }
            }
        }
        {
            let mut background = self.background.lock();
            if let Some(s) = background.get_mut(&app_sid) {
                Self::commit_interjection_boundary(s, &app, &message, &app_sid, &turn_id)?;
                return Ok(Self::snapshot_from_live(s));
            }
        }

        // Live/background slot gone after ACP ok: still journal so history is honest
        // and FE can drop the queue item (agent already received the steer).
        tracing::warn!("interject: chat {app_sid} left live/background after ACP ok; journal-only");
        if let Err(e) = store::append_message(&app_sid, message.clone()) {
            tracing::error!("interjection journal-only append failed: {e}");
        }
        let _ = app.emit(
            "session://interjection",
            serde_json::json!({
                "sessionId": app_sid,
                "message": message,
            }),
        );
        Ok(self.snapshot())
    }

    pub async fn stop(
        self: &Arc<Self>,
        app: AppHandle,
        session_id: Option<String>,
    ) -> Result<SessionSnapshot, String> {
        Ok(self.stop_report(app, session_id).await?.snapshot)
    }

    /// Same as [`Self::stop`], plus whether `session/cancel` reached the agent.
    ///
    /// Ready is published before cancel is awaited. A cancel error still
    /// returns `Ok` with that snapshot and `cancel_delivered: false`.
    /// No ACP client, and a handshake abort, count as delivered.
    pub async fn stop_report(
        self: &Arc<Self>,
        app: AppHandle,
        session_id: Option<String>,
    ) -> Result<StopReport, String> {
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
            s.turn_open_epoch = s.turn_open_epoch.wrapping_add(1);
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
            return Ok(StopReport {
                snapshot: self.snapshot(),
                cancel_delivered: true,
            });
        }
        // Also release ask_user / plan reverse-RPCs. Leaving them set kept
        // `live_session_is_busy` true after stop, so Send/park paths stayed
        // wedged until process kill (user diag 5bda6b52).
        let mut pending_persists = Vec::new();
        let (acp, agent_sid, pending_ask, pending_plan, pending_perm, invalidation) = self
            .with_session_mut(&target, |s| {
                if let Some(h) = s.mock_stream.take() {
                    h.request_stop();
                }
                let (pending_ask, pending_plan, invalidation) =
                    Self::take_human_gates_for_cancel(s);
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
                    if let Some(boundary) = Self::prepare_journal_turn_cancelled(s, "user_stop") {
                        pending_persists
                            .push(PendingSessionPersist::TurnBoundary(Box::new(boundary)));
                    }
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
                    invalidation,
                )
            })
            .ok_or("no active session")?;
        if let Some(invalidation) = invalidation {
            invalidation.emit(&app, "user_stop");
        }
        Self::commit_session_persists(Some(&app), pending_persists);
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
        // No ACP client means there is nothing to deliver.
        let mut cancel_delivered = true;
        if let Some(acp) = acp {
            // Reply to reverse-RPCs before session/cancel so the agent does not
            // sit forever on an unanswered ask_user_question after Host "stop".
            if let Some(id) = pending_ask {
                if let Err(e) = id.respond(&acp).await {
                    tracing::warn!(rpc = id.rpc, "stop: cancel ask_user failed: {e}");
                }
            }
            if let Some(id) = pending_plan {
                if let Err(e) = id.respond(&acp).await {
                    tracing::warn!(rpc = id.rpc, "stop: cancel plan failed: {e}");
                }
            }
            if let Some(id) = pending_perm {
                if let Err(e) = acp
                    .respond_permission(id, PermissionOutcome::Cancelled)
                    .await
                {
                    tracing::warn!("stop: cancel permission id={id} failed: {e}");
                }
            }
            // Target the session explicitly (shared process safety).
            if let Err(e) = match agent_sid {
                Some(ref sid) => acp.cancel_for(sid).await,
                None => acp.cancel().await,
            } {
                cancel_delivered = false;
                tracing::warn!(
                    target: "session",
                    session = %target,
                    "stop: session/cancel after Ready emit failed (UI already settled): {e}"
                );
            }
        }
        self.flush_pending_soft_respawn(&app, &target).await;
        Ok(StopReport {
            snapshot: stopped_snap,
            cancel_delivered,
        })
    }
}

struct OpenFence {
    ch: char,
    len: usize,
}

/// CommonMark fence opener. Closing lines are handled separately.
fn opening_fence(line: &str) -> Option<OpenFence> {
    let indent = line.chars().take_while(|c| *c == ' ').count();
    if indent > 3 {
        return None;
    }
    let rest = &line[indent..];
    let ch = rest.chars().next()?;
    if ch != '`' && ch != '~' {
        return None;
    }
    let len = rest.chars().take_while(|c| *c == ch).count();
    if len < 3 {
        return None;
    }
    let info = rest[len..].trim();
    if ch == '`' && info.contains('`') {
        return None;
    }
    Some(OpenFence { ch, len })
}

fn closes_fence(line: &str, open: &OpenFence) -> bool {
    let trimmed = line.trim_end();
    let indent = trimmed.chars().take_while(|c| *c == ' ').count();
    if indent > 3 {
        return false;
    }
    let marks = &trimmed[indent..];
    !marks.is_empty() && marks.chars().all(|c| c == open.ch) && marks.chars().count() >= open.len
}

/// `/goal`, or `/goal` plus one of clear/status/pause/resume.
/// Any other same-line text is the objective and must stay in the body so
/// host vision still sees image paths.
fn quote_opener(line: &str) -> Option<usize> {
    let trimmed = line.trim();
    if trimmed.chars().count() >= 3 && trimmed.chars().all(|c| c == '"') {
        Some(trimmed.chars().count())
    } else {
        None
    }
}

fn quote_closer(line: &str, open_len: usize) -> bool {
    quote_opener(line).is_some_and(|len| len >= open_len)
}

fn split_goal_command(line: &str) -> Option<(String, Option<String>)> {
    let rest = line.strip_prefix("/goal")?;
    if !rest.is_empty() && !rest.chars().next().is_some_and(|c| c.is_whitespace()) {
        return None;
    }
    let objective = rest.trim();
    if objective.is_empty() {
        return Some(("/goal".to_string(), None));
    }
    let mut words = objective.split_whitespace();
    let first = words.next().unwrap_or("");
    let single = words.next().is_none();
    if single && matches!(first, "clear" | "status" | "pause" | "resume") {
        return Some((format!("/goal {first}"), None));
    }
    Some(("/goal".to_string(), Some(objective.to_string())))
}

/// Take this send's `/goal` command out of the current request.
///
/// History bootstrap and attach transcripts are added later and may contain
/// the same line. Only a column-0 command outside fences and quote blocks
/// counts. A same-line objective stays in the request so image references
/// still reach host vision. When there is no command, the text is unchanged.
fn detach_current_goal_command(user_text: &str) -> (Option<String>, String) {
    let normalized = user_text.replace("\r\n", "\n").replace('\r', "\n");
    let parts: Vec<&str> = normalized.split('\n').collect();
    let mut fence: Option<OpenFence> = None;
    let mut quote_len: Option<usize> = None;
    let mut prev = "";
    let mut found = None;
    for (i, line) in parts.iter().enumerate() {
        if let Some(open) = fence.as_ref() {
            if closes_fence(line, open) {
                fence = None;
            }
            prev = line;
            continue;
        }
        // A quote can contain an unmatched fence. Do not let that fence
        // outlive the quote and swallow the real command.
        if let Some(len) = quote_len {
            if quote_closer(line, len) {
                quote_len = None;
            }
            prev = line;
            continue;
        }
        if let Some(open) = opening_fence(line) {
            fence = Some(open);
            prev = line;
            continue;
        }
        // Only the composer quote block opens this region, and its fence is
        // longer than any quote-only line inside the excerpt.
        if prev.trim() == "Quoted excerpt:" {
            if let Some(len) = quote_opener(line) {
                quote_len = Some(len);
                prev = line;
                continue;
            }
        }
        if split_goal_command(line).is_none() {
            prev = line;
            continue;
        }
        found = Some(i);
        break;
    }
    let Some(idx) = found else {
        return (None, user_text.to_string());
    };
    let (cmd, same_line) = split_goal_command(parts[idx]).expect("found a command");
    let mut kept = Vec::with_capacity(parts.len());
    for (i, line) in parts.iter().enumerate() {
        if i == idx {
            if let Some(objective) = same_line.clone() {
                kept.push(objective);
            }
            continue;
        }
        kept.push((*line).to_string());
    }
    (Some(cmd), kept.join("\n"))
}

/// Put a command captured from the current send at byte 0.
fn prefix_goal_command(cmd: &str, body: &str) -> String {
    if body.trim().is_empty() {
        cmd.to_string()
    } else {
        format!("{cmd}\n{body}")
    }
}

#[cfg(test)]
mod tests {
    use super::{detach_current_goal_command, prefix_goal_command};

    fn place(current: &str, wrapped_body: &str) -> String {
        match detach_current_goal_command(current) {
            (Some(cmd), _) => prefix_goal_command(&cmd, wrapped_body),
            (None, _) => wrapped_body.to_string(),
        }
    }

    #[test]
    fn detaches_a_leading_goal_and_prefixes_it_again() {
        let (cmd, rest) = detach_current_goal_command("/goal\nfix the bug");
        assert_eq!(cmd.as_deref(), Some("/goal"));
        assert_eq!(rest, "fix the bug");
        assert_eq!(
            prefix_goal_command(cmd.as_deref().unwrap(), &rest),
            "/goal\nfix the bug"
        );
    }

    #[test]
    fn bare_goal_has_no_extra_newline() {
        let (cmd, rest) = detach_current_goal_command("/goal\n");
        assert_eq!(cmd.as_deref(), Some("/goal"));
        assert!(rest.trim().is_empty());
        assert_eq!(prefix_goal_command("/goal", &rest), "/goal");
        assert_eq!(prefix_goal_command("/goal clear", "\n"), "/goal clear");
    }

    #[test]
    fn hoists_the_current_goal_above_history_without_taking_history_command() {
        let current = "/goal\nfix the bug";
        let (_, rest) = detach_current_goal_command(current);
        let wrapped = format!("[Prior conversation context]\nUser: /goal clear\nold\n{rest}");
        let out = place(current, &wrapped);
        let body = out.strip_prefix("/goal\n").expect("current command leads");
        assert!(body.contains("User: /goal clear\nold"));
        assert!(
            body.find("[Prior conversation context]").unwrap() < body.find("fix the bug").unwrap()
        );
        assert!(!body.starts_with("/goal"));
    }

    #[test]
    fn leaves_a_normal_send_alone_when_history_contains_goal() {
        let current = "what does this mean?";
        let (cmd, rest) = detach_current_goal_command(current);
        assert!(cmd.is_none());
        assert_eq!(rest, current);
        let wrapped = "[Prior]\n/goal\nold objective\nwhat does this mean?";
        assert_eq!(place(current, wrapped), wrapped);
    }

    #[test]
    fn hoists_goal_buried_under_quotes_or_schema_in_the_current_send() {
        let current = "\
[Structured output]
Respond as JSON.

Quoted excerpt:
\"\"\"
> quoted line
\"\"\"

/goal
ship it";
        let (cmd, rest) = detach_current_goal_command(current);
        assert_eq!(cmd.as_deref(), Some("/goal"));
        assert!(rest.contains("[Structured output]"));
        assert!(rest.contains("> quoted line"));
        assert!(rest.contains("ship it"));
        assert!(!rest.lines().any(|line| line == "/goal"));
        let out = prefix_goal_command("/goal", &format!("[Prior]\n{rest}"));
        assert!(out.starts_with("/goal\n[Prior]\n"));
    }

    #[test]
    fn does_not_take_goal_from_a_quote_block() {
        let current = "\
Quoted excerpt:
\"\"\"
/goal
quoted objective
\"\"\"

please explain";
        let (cmd, rest) = detach_current_goal_command(current);
        assert!(cmd.is_none());
        assert_eq!(rest, current);
    }

    #[test]
    fn quote_with_an_unmatched_fence_does_not_hide_the_real_command() {
        let current = "\
Quoted excerpt:
\"\"\"
```
partial fence
\"\"\"

/goal
ship it
```
/goal clear
```
";
        let (cmd, rest) = detach_current_goal_command(current);
        assert_eq!(cmd.as_deref(), Some("/goal"));
        assert!(rest.contains("ship it"));
        assert!(rest.contains("/goal clear"));
        assert!(!rest.lines().any(|line| line == "/goal"));
    }

    #[test]
    fn does_not_take_goal_from_a_docstring_inside_a_quote() {
        let current = "\
Quoted excerpt:
\"\"\"\"
def f():
    \"\"\"
/goal clear
    \"\"\"
\"\"\"\"

explain";
        let (cmd, rest) = detach_current_goal_command(current);
        assert!(cmd.is_none(), "{rest}");
        assert_eq!(rest, current);
    }

    #[test]
    fn keeps_a_same_line_objective_for_host_vision() {
        let (cmd, rest) = detach_current_goal_command("/goal describe @/tmp/image.png");
        assert_eq!(cmd.as_deref(), Some("/goal"));
        assert_eq!(rest, "describe @/tmp/image.png");
        assert_eq!(
            prefix_goal_command("/goal", &rest),
            "/goal\ndescribe @/tmp/image.png"
        );
    }

    #[test]
    fn detaches_goal_clear_and_keeps_a_later_status_line() {
        let (cmd, rest) = detach_current_goal_command("note\n/goal clear\n");
        assert_eq!(cmd.as_deref(), Some("/goal clear"));
        assert_eq!(rest, "note\n");
        let (cmd, rest) = detach_current_goal_command("/goal\nfix it\n/goal status\n");
        assert_eq!(cmd.as_deref(), Some("/goal"));
        assert_eq!(rest, "fix it\n/goal status\n");
    }

    #[test]
    fn does_not_treat_samples_or_lookalikes_as_commands() {
        for sample in [
            "check /goals before shipping",
            "the /goalkeeper file\n/goalkeeper",
            "see /goal/readme\n",
            "please run /goal now\r\n",
            "before\n    /goal\nafter",
            "```\n/goal\ndo it\n```\nfix the bug",
            "~~~\n/goal clear\n~~~\nexplain",
            "````\n```\n/goal clear\n```\n````\nexplain this sample",
        ] {
            let (cmd, rest) = detach_current_goal_command(sample);
            assert!(cmd.is_none(), "{sample}");
            assert_eq!(rest, sample);
        }
    }
}
