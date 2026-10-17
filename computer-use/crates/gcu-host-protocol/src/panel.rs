//! Per-session Computer Use timeline for the side panel.
//!
//! Each session admits at most ten `host.event.emit` records in any sliding
//! one-second window. Further records increment a drop counter and publish at
//! most one extra notice until the window moves. The timeline keeps the newest
//! 200 steps. Stop and resume are explicit user actions; a later tool outcome
//! does not clear the stop latch.

use crate::events::{safe_key, thumbnail_id};
use crate::messages::Event;
use gcu_protocol::result::Outcome;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;

/// Width of the admission window.
pub const WINDOW_MS: u64 = 1000;
/// Admitted events kept inside one [`WINDOW_MS`] window.
pub const MAX_PER_WINDOW: usize = 10;
/// Newest steps retained per session. This is not the throttle.
pub const MAX_STEPS: usize = 200;
const NOTICE_CAP: usize = 64;
const TARGET_CHARS: usize = 280;

/// Why [`Panel::push`] did not change the visible timeline the same way.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Admit {
    /// The step is in the timeline.
    Kept,
    /// The step was counted as dropped. At most one notice is sent per window.
    Throttled,
    /// The session id is not a safe key. Nothing was stored.
    Rejected,
}

/// Status the panel shows before the chat approval and handoff desks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    /// No admitted step, and no global latch or banner.
    Idle,
    /// The newest step completed.
    Running,
    /// The newest step was refused.
    Refused,
    /// The newest step failed.
    Failed,
    /// The newest step may not have finished.
    Unknown,
    /// The user pressed Stop. Later outcomes do not clear this.
    Stopped,
    /// The engine process was replaced. The next admitted step clears it.
    Restarted,
    /// The supervisor breaker is open. It stays until the feature is reset.
    Tripped,
}

/// One admitted tool step. The shot id matches the gallery thumbnail key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Step {
    /// Tool name.
    pub tool: String,
    /// Short target label. Longer engine text is clipped for display.
    pub target: String,
    /// Wire outcome of this step.
    pub outcome: Outcome,
    /// Engine-reported duration.
    pub duration_ms: u64,
    /// Gallery key when the event included a thumbnail.
    pub shot_id: Option<String>,
    /// Caller clock when the step was admitted.
    pub at_ms: u64,
}

/// Timeline plus global flags for one session.
///
/// `revision` increases on every published change. A slower snapshot must not
/// overwrite a newer notice.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct View {
    /// Session the steps belong to.
    pub session_id: String,
    /// Display phase. Stopped overrides tripped, which overrides restarted.
    pub phase: Phase,
    /// User stop latch. Global across sessions.
    pub stopped: bool,
    /// Supervisor breaker. Global, and sticky across later steps.
    pub tripped: bool,
    /// Engine restart banner. Global until the next admitted step.
    pub restarted: bool,
    /// Newest steps, oldest first.
    pub steps: Vec<Step>,
    /// Events dropped by the throttle since this session was opened.
    pub dropped: u64,
    /// Monotonic publication counter.
    pub revision: u64,
}

/// Panel publication. Subscribers replace state; they do not throttle again.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum Notice {
    /// Whole view of one session after an admitted change.
    Snapshot {
        /// Session timeline.
        view: View,
    },
    /// Global flags changed and no single session step was added.
    Engine {
        /// User stop latch.
        stopped: bool,
        /// Breaker latch.
        tripped: bool,
        /// Restart banner.
        restarted: bool,
        /// Publication counter.
        revision: u64,
    },
    /// Drop one session timeline. Global flags stay.
    Forgotten {
        /// Session that was dropped.
        #[serde(rename = "sessionId")]
        session_id: String,
        /// Publication counter.
        revision: u64,
    },
}

#[derive(Default)]
struct Slot {
    steps: VecDeque<Step>,
    admitted_at: VecDeque<u64>,
    drop_noted: bool,
    dropped: u64,
}

struct Inner {
    revision: u64,
    stopped: bool,
    tripped: bool,
    restarted: bool,
    sessions: HashMap<String, Slot>,
}

enum Decision {
    Keep { cleared_restart: bool },
    NoteDrop,
    SilentDrop,
}

/// In-memory timelines for every Computer Use session.
///
/// Clone shares the timelines and the notice sender. A lagged subscriber skips
/// notices and should read [`Panel::view`]. Blocking: none. Timeout: none.
/// Cancellation: dropping the receiver ends that subscription only.
#[derive(Clone)]
pub struct Panel {
    inner: Arc<Mutex<Inner>>,
    notes: broadcast::Sender<Notice>,
}

impl Panel {
    /// Empty timelines, not stopped.
    ///
    /// Blocking: none. Timeout: none. Cancellation: none.
    #[must_use]
    pub fn open() -> Self {
        let (notes, _) = broadcast::channel(NOTICE_CAP);
        Self {
            inner: Arc::new(Mutex::new(Inner {
                revision: 0,
                stopped: false,
                tripped: false,
                restarted: false,
                sessions: HashMap::new(),
            })),
            notes,
        }
    }

    /// Receiver for later notices. The slot that exists before this call is dropped.
    ///
    /// Lagged receivers must call [`Panel::view`]. Blocking: none. Timeout: none.
    /// Cancellation: drop the receiver.
    #[must_use]
    pub fn subscribe(&self) -> broadcast::Receiver<Notice> {
        self.notes.subscribe()
    }

    /// Current view. `None` when `session` is not a safe key.
    ///
    /// An unknown session is an empty timeline with the global flags.
    /// Blocking: none. Timeout: none. Cancellation: none.
    #[must_use]
    pub fn view(&self, session: &str) -> Option<View> {
        if !safe_key(session) {
            return None;
        }
        let state = lock(&self.inner);
        Some(view_of(&state, session))
    }

    /// Admit one engine event at `now_ms`.
    ///
    /// `now_ms` is the caller's millisecond clock. A jump backward does not
    /// expire stamps that are still ahead of it. Blocking: none. Timeout: none.
    /// Cancellation: none.
    pub fn push(&self, event: &Event, now_ms: u64) -> Admit {
        if !safe_key(&event.session_id) {
            return Admit::Rejected;
        }
        let mut state = lock(&self.inner);
        let decision = admit(&mut state, event, now_ms);
        let snap = matches!(decision, Decision::Keep { .. } | Decision::NoteDrop).then(|| {
            Notice::Snapshot {
                view: view_of(&state, &event.session_id),
            }
        });
        let engine = matches!(
            decision,
            Decision::Keep {
                cleared_restart: true
            }
        )
        .then(|| engine_notice(&state));
        drop(state);
        if let Some(notice) = snap {
            let _ = self.notes.send(notice);
        }
        if let Some(notice) = engine {
            let _ = self.notes.send(notice);
        }
        match decision {
            Decision::Keep { .. } => Admit::Kept,
            Decision::NoteDrop | Decision::SilentDrop => Admit::Throttled,
            // `admit` is only called with a safe key.
        }
    }

    /// Latch Stop for every session. Already stopped is a no-op success.
    ///
    /// Blocking: none. Timeout: none. Cancellation: none.
    pub fn user_stop(&self) -> bool {
        self.set_flag(Flag::Stopped)
    }

    /// Clear Stop. Returns false, and emits nothing, when Stop is not latched.
    ///
    /// Blocking: none. Timeout: none. Cancellation: none.
    pub fn user_resume(&self) -> bool {
        let mut state = lock(&self.inner);
        if !state.stopped {
            return false;
        }
        state.stopped = false;
        state.revision = state.revision.saturating_add(1);
        let notice = engine_notice(&state);
        drop(state);
        let _ = self.notes.send(notice);
        true
    }

    /// Show the engine-restart banner until the next admitted step.
    ///
    /// Blocking: none. Timeout: none. Cancellation: none.
    pub fn note_restart(&self) {
        let _ = self.set_flag(Flag::Restarted);
    }

    /// Open the breaker banner. Later steps do not clear it.
    ///
    /// Blocking: none. Timeout: none. Cancellation: none.
    pub fn note_tripped(&self) {
        let _ = self.set_flag(Flag::Tripped);
    }

    /// Forget one session timeline. An unsafe id does not emit.
    ///
    /// A safe key always publishes, even when this panel stored no step, so
    /// the UI can drop screenshots for that session. Blocking: none.
    /// Timeout: none. Cancellation: none.
    pub fn drop_session(&self, session: &str) {
        if !safe_key(session) {
            return;
        }
        let mut state = lock(&self.inner);
        state.sessions.remove(session);
        state.revision = state.revision.saturating_add(1);
        let notice = Notice::Forgotten {
            session_id: session.to_owned(),
            revision: state.revision,
        };
        drop(state);
        let _ = self.notes.send(notice);
    }

    fn set_flag(&self, flag: Flag) -> bool {
        let mut state = lock(&self.inner);
        let already = match flag {
            Flag::Stopped => state.stopped,
            Flag::Restarted => state.restarted,
            Flag::Tripped => state.tripped,
        };
        if already {
            return true;
        }
        match flag {
            Flag::Stopped => state.stopped = true,
            Flag::Restarted => state.restarted = true,
            Flag::Tripped => state.tripped = true,
        }
        state.revision = state.revision.saturating_add(1);
        let notice = engine_notice(&state);
        drop(state);
        let _ = self.notes.send(notice);
        true
    }
}

enum Flag {
    Stopped,
    Restarted,
    Tripped,
}

fn admit(state: &mut Inner, event: &Event, now_ms: u64) -> Decision {
    let session = event.session_id.clone();
    let mut slot = state.sessions.remove(&session).unwrap_or_default();
    expire(&mut slot, now_ms);
    if slot.admitted_at.len() >= MAX_PER_WINDOW {
        slot.dropped = slot.dropped.saturating_add(1);
        let noted = !slot.drop_noted;
        slot.drop_noted = true;
        state.sessions.insert(session, slot);
        state.revision = state.revision.saturating_add(1);
        return if noted {
            Decision::NoteDrop
        } else {
            Decision::SilentDrop
        };
    }
    slot.drop_noted = false;
    slot.admitted_at.push_back(now_ms);
    slot.steps.push_back(step_from(event, now_ms));
    while slot.steps.len() > MAX_STEPS {
        slot.steps.pop_front();
    }
    state.sessions.insert(session, slot);
    let cleared = state.restarted;
    state.restarted = false;
    state.revision = state.revision.saturating_add(1);
    Decision::Keep {
        cleared_restart: cleared,
    }
}

fn expire(slot: &mut Slot, now_ms: u64) {
    while slot
        .admitted_at
        .front()
        .is_some_and(|stamp| now_ms.saturating_sub(*stamp) >= WINDOW_MS)
    {
        slot.admitted_at.pop_front();
    }
}

fn step_from(event: &Event, now_ms: u64) -> Step {
    Step {
        tool: event.tool.as_str().to_owned(),
        target: clip(&event.target),
        outcome: event.outcome,
        duration_ms: event.duration_ms,
        shot_id: event.thumbnail.as_ref().map(|_| thumbnail_id(event)),
        at_ms: now_ms,
    }
}

fn view_of(state: &Inner, session: &str) -> View {
    let slot = state.sessions.get(session);
    View {
        session_id: session.to_owned(),
        phase: displayed(state, slot),
        stopped: state.stopped,
        tripped: state.tripped,
        restarted: state.restarted,
        steps: slot
            .map(|item| item.steps.iter().cloned().collect())
            .unwrap_or_default(),
        dropped: slot.map(|item| item.dropped).unwrap_or(0),
        revision: state.revision,
    }
}

fn displayed(state: &Inner, slot: Option<&Slot>) -> Phase {
    if state.stopped {
        return Phase::Stopped;
    }
    if state.tripped {
        return Phase::Tripped;
    }
    if state.restarted {
        return Phase::Restarted;
    }
    match slot.and_then(|item| item.steps.back()) {
        None => Phase::Idle,
        Some(step) => match step.outcome {
            Outcome::Done => Phase::Running,
            Outcome::Refused => Phase::Refused,
            Outcome::Failed => Phase::Failed,
            Outcome::Unknown => Phase::Unknown,
        },
    }
}

fn engine_notice(state: &Inner) -> Notice {
    Notice::Engine {
        stopped: state.stopped,
        tripped: state.tripped,
        restarted: state.restarted,
        revision: state.revision,
    }
}

fn clip(value: &str) -> String {
    if value.chars().count() <= TARGET_CHARS {
        return value.to_owned();
    }
    value.chars().take(TARGET_CHARS).collect()
}

fn lock(mutex: &Mutex<Inner>) -> std::sync::MutexGuard<'_, Inner> {
    mutex.lock().unwrap_or_else(|err| err.into_inner())
}
