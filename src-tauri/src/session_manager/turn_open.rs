//! Production turn-open boundary, independently testable without a WebView or model.
use super::*;
use crate::acp_client::AcpClient;
use crate::error::AgentErrorCode;
use crate::store::{self, ChatMessageStored, MessageAttachmentStored};
use std::sync::Arc;
use uuid::Uuid;

pub(super) struct OpenedTurn {
    pub backend: String,
    pub app_sid: String,
    pub acp: Option<Arc<AcpClient>>,
    pub agent_sid: Option<String>,
    pub agent_prompt: String,
    pub message_id: String,
    pub turn_id: String,
    pub user_row: ChatMessageStored,
    pub policy: crate::session_migration::mcp_stdio::SessionMcpPolicy,
    pub needs_history_bootstrap: bool,
    pub previous_sent_prompt_this_visit: bool,
    pub previous_updated_at: chrono::DateTime<chrono::Utc>,
    pub activity_updated_at: chrono::DateTime<chrono::Utc>,
    pub activity_persisted: bool,
}

impl SessionManager {
    fn reset_opened_turn_runtime(
        session: &mut LiveSession,
        opened: &OpenedTurn,
    ) -> Result<(), String> {
        if session.active_turn_id.as_deref() != Some(opened.turn_id.as_str()) {
            return Err("opened turn no longer owns the runtime".into());
        }
        session
            .fsm
            .end_stream()
            .map_err(|error| error.to_string())?;
        session.prompt_in_flight = false;
        session.sent_prompt_this_visit = opened.previous_sent_prompt_this_visit;
        session.active_turn_id = None;
        session.streaming_message_id = None;
        session.stream_message_id_locked = false;
        session.stream_buf.clear();
        session.stream_thought.clear();
        session.stream_last_was_assistant = false;
        session.stream_attachments.clear();
        session.journal_throttle.reset();
        session.last_stall_emit = None;
        session.stall_soft_emits = 0;
        session.open_tool_ids.clear();
        session.open_tool_seen_at.clear();
        session.terminal_tool_ids.clear();
        session.deferred_prompt_complete = None;
        session.saw_model_output = false;
        session.provider_retry_attempt = 0;
        session.provider_retry_aborted = false;
        session.tools_this_turn = 0;
        session.needs_history_bootstrap = opened.needs_history_bootstrap;
        session.meta.updated_at = opened.previous_updated_at;
        crate::turn_lease::clear_lease(&session.app_session_id);
        Ok(())
    }

    /// Roll back a continuation refusal before any model dispatch. The caller
    /// may advertise the turn as unsent only when every durable/runtime part is
    /// proven restored; otherwise the safe disposition is DISPATCH_UNKNOWN.
    pub(super) async fn rollback_opened_turn_before_dispatch(
        &self,
        opened: &OpenedTurn,
        cause: &str,
    ) -> String {
        let journal_lock = self.post_turn_journal_lock(&opened.app_sid);
        let _journal_guard = journal_lock.lock().await;
        let journal = store::rollback_tail_message(&opened.app_sid, &opened.user_row);
        let runtime = self
            .with_session_mut(&opened.app_sid, |session| {
                Self::reset_opened_turn_runtime(session, opened)
            })
            .ok_or_else(|| "opened turn runtime is no longer present".to_string())
            .and_then(|result| result);
        let activity = if opened.activity_persisted {
            store::rollback_session_activity_timestamp(
                &opened.app_sid,
                opened.activity_updated_at,
                opened.previous_updated_at,
            )
        } else {
            Ok(())
        };
        if journal.is_ok() && runtime.is_ok() && activity.is_ok() {
            return format!("TURN_NOT_DISPATCHED_ROLLED_BACK: {cause}");
        }
        tracing::error!(
            session = %opened.app_sid,
            journal_error = ?journal.err(),
            runtime_error = ?runtime.err(),
            activity_error = ?activity.err(),
            "pre-dispatch turn rollback could not be verified"
        );
        "DISPATCH_UNKNOWN: local pre-dispatch rollback could not be verified".into()
    }

    fn require_applied_composer_for_turn(&self, id: &str) -> Result<(), String> {
        if self.pending_soft_respawn.lock().contains_key(id) {
            return Err(format!(
                "{}: chat {id} requires reconnect to apply composer settings",
                AgentErrorCode::ConnectFailed.as_str()
            ));
        }
        Ok(())
    }

    #[cfg(test)]
    pub(super) async fn open_user_turn(
        &self,
        app_sid: &str,
        stripped_text: &str,
        journal_content: &str,
        journal_attachments: Option<Vec<MessageAttachmentStored>>,
    ) -> Result<OpenedTurn, String> {
        self.open_user_turn_with_origin(
            app_sid,
            stripped_text,
            journal_content,
            journal_attachments,
            None,
        )
        .await
    }

    /// Caller holds connect_lock. Origin is bound to this exact turn, not its text
    /// or a shared session slot. Reject before journal, lease, continuation or ACP.
    pub(super) async fn open_user_turn_with_origin(
        &self,
        app_sid: &str,
        stripped_text: &str,
        journal_content: &str,
        journal_attachments: Option<Vec<MessageAttachmentStored>>,
        origin: Option<&str>,
    ) -> Result<OpenedTurn, String> {
        self.require_applied_composer_for_turn(app_sid)?;
        let attached = crate::session_attach::extract_attached_chats(journal_content);
        let workspace_guard = self.snapshot_send_workspace(app_sid)?;
        let journal_lock = self.post_turn_journal_lock(app_sid);
        let _journal_guard = journal_lock.lock().await;
        workspace_guard.validate(app_sid).await?;
        // A settings/event callback can request respawn while journal/context
        // validation yields, even though this caller owns connect_lock.
        self.require_applied_composer_for_turn(app_sid)?;
        let open = self.with_session_mut(app_sid, |s| {
            workspace_guard.matches(s)?;
            // One prompt per chat at a time. The FSM alone is not enough: an
            // early prompt_complete Readies it while the agent is still working,
            // and a second `session/prompt` would then be dispatched into a busy
            // agent (the CLI rejects it as `task_already_running`).
            if s.prompt_in_flight {
                return Err(format!(
                    "{}: chat {} is still running its previous turn",
                    AgentErrorCode::ConnectFailed.as_str(),
                    s.app_session_id
                ));
            }

            let agent_prompt = crate::session_attach::agent_prompt_after_attach(
                stripped_text,
                &attached,
                &s.app_session_id,
                &crate::session_attach::StoreAttachJournal,
            )
            .map_err(|e| e.to_string())?;
            if !attached.is_empty() && agent_prompt.len() > stripped_text.len() {
                tracing::info!(
                    "attached chat context ({} chars, {} id(s)) for session {}",
                    agent_prompt.len().saturating_sub(stripped_text.len()),
                    attached.len(),
                    s.app_session_id
                );
            }

            let previous_sent_prompt_this_visit = s.sent_prompt_this_visit;
            let previous_updated_at = s.meta.updated_at;
            if let Some(ref atts) = journal_attachments {
                grant_journal_attachment_paths(atts);
            }
            s.fsm.begin_stream().map_err(|e| e.to_string())?;
            s.prompt_in_flight = true;
            s.sent_prompt_this_visit = true;
            Self::touch_stream_progress_locked(s);
            let turn_id = Uuid::new_v4().to_string();
            s.active_turn_id = Some(turn_id.clone());
            crate::turn_lease::begin_active(
                &s.app_session_id,
                s.meta.agent_session_id.as_deref(),
                Some(turn_id.as_str()),
            );
            s.stream_message_id_locked = false;
            let mid = Uuid::new_v4().to_string();
            s.streaming_message_id = Some(mid.clone());
            s.stream_buf.clear();
            s.stream_thought.clear();
            s.stream_last_was_assistant = false;
            s.stream_attachments.clear();
            s.journal_throttle.reset();
            s.last_stall_emit = None;
            s.open_tool_ids.clear();
            s.open_tool_seen_at.clear();
            s.terminal_tool_ids.clear();
            s.deferred_prompt_complete = None;
            s.stall_soft_emits = 0;
            s.saw_model_output = false;
            s.provider_retry_attempt = 0;
            s.provider_retry_aborted = false;
            s.tools_this_turn = 0;

            // Continuation sidecar I/O and history bootstrap run after this
            // lock drops. Capture only the immutable inputs here.
            let policy = s
                .acp
                .as_ref()
                .map(|c| c.session_mcp_policy())
                .unwrap_or_default();
            let needs_history_bootstrap = s.needs_history_bootstrap;
            s.needs_history_bootstrap = false;

            // Persist the user-facing turn before dispatching the prompt. A
            // failed journal write must not leave the runtime in Streaming
            // with a prompt that cannot be reconstructed after reload.
            let user_row = ChatMessageStored {
                id: Uuid::new_v4().to_string(),
                role: "user".into(),
                content: journal_content.to_string(),
                thought: None,
                created_at: chrono::Utc::now(),
                is_error: false,
                attachments: journal_attachments.clone(),
                marker: crate::injected_prompts::marker(origin),
            };
            if let Err(e) = store::append_message(&s.app_session_id, user_row.clone()) {
                let _ = s.fsm.end_stream();
                s.prompt_in_flight = false;
                crate::turn_lease::clear_lease(&s.app_session_id);
                s.sent_prompt_this_visit = previous_sent_prompt_this_visit;
                s.active_turn_id = None;
                s.streaming_message_id = None;
                s.stream_buf.clear();
                s.stream_thought.clear();
                s.stream_last_was_assistant = false;
                s.stream_attachments.clear();
                s.journal_throttle.reset();
                s.open_tool_ids.clear();
                s.open_tool_seen_at.clear();
                s.terminal_tool_ids.clear();
                s.deferred_prompt_complete = None;
                s.saw_model_output = false;
                s.needs_history_bootstrap = needs_history_bootstrap;
                return Err(format!("JOURNAL_WRITE_FAILED: {e}"));
            }
            // Sidebar recency: last activity, not last click. Pin stays organizational.
            s.meta.updated_at = user_row.created_at.max(previous_updated_at);
            let activity_updated_at = s.meta.updated_at;
            let activity_persisted = store::update_runtime_session_meta(&s.meta)
                .map(|_| true)
                .unwrap_or_else(|e| {
                    tracing::warn!(
                        session = %s.app_session_id,
                        "turn-start session metadata update failed after user journal: {e}"
                    );
                    false
                });
            Ok(OpenedTurn {
                backend: s.backend.clone(),
                app_sid: s.app_session_id.clone(),
                acp: s.acp.clone(),
                agent_sid: s.meta.agent_session_id.clone(),
                agent_prompt,
                message_id: mid,
                turn_id,
                user_row,
                policy,
                needs_history_bootstrap,
                previous_sent_prompt_this_visit,
                previous_updated_at,
                activity_updated_at,
                activity_persisted,
            })
        });
        open.unwrap_or_else(|| {
            Err(format!(
                "{}: chat {app_sid} has no live agent process — reconnect and retry",
                AgentErrorCode::ConnectFailed.as_str()
            ))
        })
    }
}
