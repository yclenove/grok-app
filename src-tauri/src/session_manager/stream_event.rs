//! Shared stream mutation, with journal/IPC effects committed after map guards.
use super::turn_event_owner::TurnEventOwner;
use super::*;
use crate::acp_client::StreamKind;
use crate::journal_throttle::is_paragraph_break;
use crate::stream_emit::{
    should_flush_stream_emit, DEFAULT_STREAM_EMIT_MAX_CHARS, DEFAULT_STREAM_EMIT_MS,
};
use std::time::{Duration, Instant};
use tauri::AppHandle;

pub(super) struct StreamChunk {
    pub kind: StreamKind,
    pub text: String,
    pub message_id: Option<String>,
    pub done: bool,
}

pub(super) struct StreamUpdate {
    pub schedule: Option<StreamSchedule>,
    pub journal: Option<PendingStreamJournalFlush>,
    pub emits: Vec<StreamEmitPayload>,
}

pub(super) struct StreamSchedule {
    owner: TurnEventOwner,
    pub generation: u64,
}

impl SessionManager {
    pub(super) fn handle_stream_chunk(
        self: &Arc<Self>,
        app: &AppHandle,
        process: &str,
        agent: Option<&str>,
        chunk: StreamChunk,
    ) {
        let Some(update) = self.prepare_stream_chunk(process, agent, chunk) else {
            return;
        };
        // No runtime-map guard remains while emitting, writing, or scheduling.
        Self::emit_stream_payloads(app, update.emits);
        if let Some(journal) = update.journal {
            Self::commit_stream_journal_flush(journal);
        }
        if let Some(schedule) = update.schedule {
            self.schedule_stream_emit_flush(app.clone(), schedule);
        }
    }

    /// Actual native Stream consumer, returning only effects that need no map lock.
    pub(super) fn prepare_stream_chunk(
        &self,
        process: &str,
        agent: Option<&str>,
        chunk: StreamChunk,
    ) -> Option<StreamUpdate> {
        let owner = self.capture_turn_event_owner(process, agent)?;
        #[cfg(test)]
        super::command_spawn_options_tests::stream_owner_tests::pause_after_route(&owner.id);
        // Pin the captured App owner wherever it moved. A reused alias or flush
        // counter cannot authorize a replacement ACP or a different turn.
        self.with_session_mut(&owner.id, |session| {
            if !owner.matches(session) {
                tracing::debug!(session = %owner.id, process = %process, "stream receipt rejected: owner changed");
                return None;
            }
            Self::update_stream_locked(session, chunk, owner.background)
        })
        .flatten()
    }

    fn update_stream_locked(
        session: &mut LiveSession,
        chunk: StreamChunk,
        background: bool,
    ) -> Option<StreamUpdate> {
        let StreamChunk {
            kind,
            text,
            message_id,
            done,
        } = chunk;
        // Load replay remains separate from an active answer, including when
        // early prompt_complete has already readied the FSM.
        if Self::is_session_load_replay(session) {
            if background {
                tracing::warn!(session = %session.app_session_id, "background stream chunk dropped after turn close");
            } else {
                tracing::debug!(session = %session.app_session_id, "acp stream dropped: load replay");
            }
            return None;
        }
        if session.fsm.state() == SessionState::Ready && session.fsm.begin_stream().is_ok() {
            tracing::debug!(session = %session.app_session_id, "acp turn re-opened after early completion");
        }
        Self::touch_stream_progress_locked(session);
        Self::ensure_stream_message_id(session, kind, message_id);
        let thought_phase = match kind {
            StreamKind::Thought => {
                let phase = if session.stream_last_was_assistant {
                    if !session.stream_thought.is_empty() {
                        session.stream_thought.push_str("\n\n⟪phase⟫\n\n");
                    }
                    session.stream_last_was_assistant = false;
                    "new"
                } else if session.stream_thought.is_empty() {
                    "open"
                } else {
                    "continue"
                };
                session.stream_thought.push_str(&text);
                phase
            }
            StreamKind::Assistant => {
                session.stream_buf.push_str(&text);
                if !text.trim().is_empty() {
                    session.stream_last_was_assistant = true;
                    session.saw_model_output = true;
                }
                "none"
            }
        };
        let emit_done = done
            && crate::turn_complete::should_emit_stream_done(
                session.prompt_in_flight,
                session.fsm.state() == SessionState::AwaitingPermission,
                session.pending_plan_rpc_id.is_some(),
                session.pending_ask_user_rpc_id.is_some(),
                session.open_tool_ids.len(),
            );
        let journal =
            Self::prepare_stream_journal_flush(session, emit_done, is_paragraph_break(&text));
        let message = session.streaming_message_id.clone().unwrap_or_default();
        let (need, emits) =
            Self::queue_stream_emit(session, kind, message, text, thought_phase, emit_done);
        let schedule = need.then(|| {
            session.stream_emit_flush_gen = session.stream_emit_flush_gen.wrapping_add(1);
            StreamSchedule {
                owner: TurnEventOwner::capture(session, background),
                generation: session.stream_emit_flush_gen,
            }
        });
        Some(StreamUpdate {
            schedule,
            journal,
            emits,
        })
    }

    /// Native timer's actual take step; UI emission follows after all guards drop.
    pub(super) fn prepare_stream_flush(
        &self,
        schedule: &StreamSchedule,
    ) -> Option<StreamEmitPayload> {
        self.with_session_mut(&schedule.owner.id, |session| {
            if !schedule.owner.matches(session)
                || session.stream_emit_flush_gen != schedule.generation
            {
                return None;
            }
            Self::take_due_stream_emit(session)
        })
        .flatten()
    }

    pub(super) fn take_due_stream_emit(session: &mut LiveSession) -> Option<StreamEmitPayload> {
        let due = session.pending_stream_emit.as_ref().is_some_and(|pending| {
            should_flush_stream_emit(
                pending.first_at,
                pending.text.len(),
                Instant::now(),
                false,
                DEFAULT_STREAM_EMIT_MAX_CHARS,
                Duration::from_millis(DEFAULT_STREAM_EMIT_MS),
            )
        });
        if due {
            Self::take_pending_stream_emit(session)
        } else {
            None
        }
    }
}
