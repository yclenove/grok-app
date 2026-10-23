//! Producing authority for one response that is still being encoded.
//!
//! A missing stamp is not proof. The check is synchronous and does not await.

use gcu_core::action::ActionState;
use gcu_core::scope::{ScopeLease, SCOPE_DENIED_MESSAGE};
use gcu_protocol::{ErrorCode, ToolError};
use serde_json::Value;

const FIXED_NOTES: &[&str] = &[
    "Execution is uncertain; do not automatically retry.",
    "Launch is uncertain; do not automatically retry.",
    "No input was delivered.",
    "Input release reached the native timeout.",
    "No visible change after delivery; the action may not have taken effect.",
    "Delivered without foreground activation.",
    "Expectation met.",
    "Expectation was not met.",
];

/// Epochs that must still match when the response is handed out.
///
/// An empty list means this response has no producing observation. Memory only.
/// [`Self::blocks`] does not wait, time out, or cancel.
#[derive(Clone, Debug)]
pub(crate) struct LiveStamp {
    epochs: Vec<(String, u64)>,
}

impl LiveStamp {
    /// No observation epoch. Memory only. No blocking, timeout, or cancellation.
    pub(crate) fn off() -> Self {
        Self { epochs: Vec::new() }
    }

    /// One producing run and generation. Memory only. No blocking or cancellation.
    pub(crate) fn epoch(run: impl Into<String>, generation: u64) -> Self {
        Self {
            epochs: vec![(run.into(), generation)],
        }
    }

    /// Whether any carried epoch is missing from the live lease.
    ///
    /// No lease never blocks. Memory only. The lease read drops its lock before
    /// return. No timeout or cancellation.
    pub(crate) fn blocks(&self, lease: Option<&ScopeLease>) -> bool {
        let Some(lease) = lease else {
            return false;
        };
        self.epochs
            .iter()
            .any(|(run, generation)| !lease.same_epoch(run, *generation))
    }
}

/// Decision before the first await of an encode.
pub(crate) struct Guard {
    /// Drop observation content and keep only action status and error identity.
    pub block: bool,
    /// Epochs to check again after every later await.
    pub stamp: LiveStamp,
}

/// Decide whether this response may carry scope-derived fields.
///
/// Memory only. No await, timeout, or cancellation. A hard scope denial blocks
/// even when the lease is already gone. An unstamped observation blocks unless
/// it is the fixed refusal sentence and carries no window, target, or image.
pub(crate) fn initial_block(
    lease: Option<&ScopeLease>,
    state: Option<&ActionState>,
    error: Option<&ToolError>,
    data: &Value,
    produced: &LiveStamp,
) -> Guard {
    if hard_denial(error) {
        return blocked();
    }
    let Some(lease) = lease else {
        return Guard {
            block: false,
            stamp: LiveStamp::off(),
        };
    };
    if produced.blocks(Some(lease)) {
        return blocked();
    }
    match state {
        Some(state) => match (&state.scope_run, state.scope_generation) {
            (Some(run), Some(generation)) => {
                if !lease.same_epoch(run, generation) {
                    return blocked();
                }
                let mut stamp = produced.clone();
                stamp.epochs.push((run.clone(), generation));
                Guard {
                    block: false,
                    stamp,
                }
            }
            (None, None) if fixed_unstamped(state, data) => Guard {
                block: false,
                stamp: LiveStamp::off(),
            },
            _ => blocked(),
        },
        None if residue(data) => {
            if produced.epochs.is_empty() {
                blocked()
            } else {
                Guard {
                    block: false,
                    stamp: produced.clone(),
                }
            }
        }
        None if lease.current().is_none() && error.is_some_and(|err| err.state.is_some()) => {
            blocked()
        }
        None => Guard {
            block: false,
            stamp: produced.clone(),
        },
    }
}

/// Remove scope-derived fields. Notes that are not fixed status text go too.
///
/// Memory only. No blocking, timeout, or cancellation.
pub(crate) fn scrub(data: &mut Value) {
    let Some(map) = data.as_object_mut() else {
        return;
    };
    map.remove("windows");
    map.remove("state");
    if let Some(error) = map.get_mut("error").and_then(Value::as_object_mut) {
        error.remove("state");
    }
    if let Some(action) = map.get_mut("action").and_then(Value::as_object_mut) {
        action.remove("target");
        if let Some(notes) = action.get_mut("notes").and_then(Value::as_array_mut) {
            notes.retain(|note| note.as_str().is_some_and(fixed_note));
        }
    }
}

fn blocked() -> Guard {
    Guard {
        block: true,
        stamp: LiveStamp::off(),
    }
}

fn hard_denial(error: Option<&ToolError>) -> bool {
    error.is_some_and(|err| err.code == ErrorCode::AppDenied && err.message == SCOPE_DENIED_MESSAGE)
}

fn residue(data: &Value) -> bool {
    data.get("windows").is_some()
        || data
            .pointer("/action/target")
            .is_some_and(|target| !target.is_null())
}

fn fixed_unstamped(state: &ActionState, data: &Value) -> bool {
    state.image.is_none()
        && state.overflow_text.is_none()
        && state.text == SCOPE_DENIED_MESSAGE
        && !residue(data)
}

fn fixed_note(note: &str) -> bool {
    FIXED_NOTES.contains(&note) || note.starts_with("Batch: ")
}
