//! Mock chunks follow the same turn ownership and completion rules as ACP.

use crate::acp_client::StreamKind;
use crate::journal_throttle::is_paragraph_break;
use crate::mock_acp::StreamChunk;
use crate::session_fsm::SessionState;
use crate::stream_emit::DEFAULT_STREAM_EMIT_MS;
use std::sync::Arc;
use std::time::Duration;
use tauri::AppHandle;

use super::{PendingStreamJournalFlush, SessionManager, StreamEmitPayload};

pub(super) struct MockStreamSchedule {
    session_id: String,
    turn_id: String,
    generation: u64,
}

pub(super) struct MockStreamUpdate {
    pub(super) schedule: Option<MockStreamSchedule>,
    pub(super) journal: Option<PendingStreamJournalFlush>,
    pub(super) emits: Vec<StreamEmitPayload>,
}

impl SessionManager {
    /// Rejected callbacks must not emit stream events, completion
    /// notifications, or mutate a replacement turn. Accepted mock chunks use
    /// the same Host-side IPC coalescer as native ACP chunks, so a long mock
    /// answer cannot bypass the product's stream backpressure.
    pub(super) fn prepare_mock_stream_chunk(
        &self,
        session_id: &str,
        turn_id: &str,
        chunk: &StreamChunk,
    ) -> Option<MockStreamUpdate> {
        self.with_turn_mut(session_id, turn_id, |s| {
            if s.backend != "mock_acp"
                || chunk.session_id != s.app_session_id
                || s.streaming_message_id.as_deref() != Some(chunk.message_id.as_str())
                || s.fsm.state() != SessionState::Streaming
                || !s.prompt_in_flight
            {
                return None;
            }
            Self::touch_stream_progress_locked(s);
            s.stream_buf.push_str(&chunk.text);
            if !chunk.text.trim().is_empty() {
                s.stream_last_was_assistant = true;
                s.saw_model_output = true;
            }
            // Keep journal writes and IPC outside the session-map lock.
            let journal =
                Self::prepare_stream_journal_flush(s, chunk.done, is_paragraph_break(&chunk.text));
            let message_id = s.streaming_message_id.clone().unwrap_or_default();
            let (needs_schedule, mut emits) = Self::queue_stream_emit(
                s,
                StreamKind::Assistant,
                message_id,
                chunk.text.clone(),
                "none",
                chunk.done,
            );
            let schedule = needs_schedule.then(|| {
                s.stream_emit_flush_gen = s.stream_emit_flush_gen.wrapping_add(1);
                MockStreamSchedule {
                    session_id: s.app_session_id.clone(),
                    turn_id: turn_id.to_string(),
                    generation: s.stream_emit_flush_gen,
                }
            });
            if chunk.done {
                if let Some(terminal) = emits.iter_mut().rfind(|payload| payload.done) {
                    terminal.host_finalized = true;
                }
                // Match real ACP completion. Keeping this lease active makes
                // startup recovery append a false host_exit marker. Ownership
                // is still held here, so a late callback cannot clear turn N+1.
                crate::turn_lease::clear_lease(&s.app_session_id);
                s.stream_buf.clear();
                s.journal_throttle.reset();
                s.last_stall_emit = None;
                s.prompt_in_flight = false;
                let _ = s.fsm.end_stream();
                s.streaming_message_id = None;
                s.active_turn_id = None;
                s.stream_message_id_locked = false;
            }
            Some(MockStreamUpdate {
                schedule,
                journal,
                emits,
            })
        })
        .flatten()
    }

    fn prepare_mock_stream_flush(
        &self,
        schedule: &MockStreamSchedule,
    ) -> Option<StreamEmitPayload> {
        self.with_turn_mut(&schedule.session_id, &schedule.turn_id, |session| {
            if session.stream_emit_flush_gen != schedule.generation {
                return None;
            }
            Self::take_due_stream_emit(session)
        })
        .flatten()
    }

    pub(super) fn schedule_mock_stream_emit_flush(
        self: &Arc<Self>,
        app: AppHandle,
        schedule: MockStreamSchedule,
    ) {
        let manager = Arc::clone(self);
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(Duration::from_millis(DEFAULT_STREAM_EMIT_MS)).await;
            if let Some(payload) = manager.prepare_mock_stream_flush(&schedule) {
                Self::emit_stream_payload(&app, payload);
            }
        });
    }
}
