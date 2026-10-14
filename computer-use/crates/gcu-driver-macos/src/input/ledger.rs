//! Press-before-send ledger. Cancel, timeout, and Stop post the stored ups.
//!
//! The up event is recorded before [`crate::input::events::EventApi::post`].
//! A proven not-sent press is popped without an extra up. Anything that may
//! have reached the process is released in reverse order, once.

use super::events::SyntheticEvent;
use std::sync::Mutex;

/// Held buttons and keys. The stored event is the up that releases them.
#[derive(Debug, Default)]
pub struct InputLedger {
    held: Mutex<Vec<SyntheticEvent>>,
}

impl InputLedger {
    /// Empty ledger.
    pub fn new() -> Self {
        Self::default()
    }

    /// Remember `up` before the matching down is posted.
    pub fn record_press(&self, up: SyntheticEvent) {
        lock(&self.held).push(up);
    }

    /// Drop a press that the backend proved it did not send.
    pub fn pop_unsent(&self) {
        lock(&self.held).pop();
    }

    /// Forget one input after its up was posted successfully.
    pub fn record_release(&self, up: &SyntheticEvent) {
        let mut held = lock(&self.held);
        if let Some(index) = held.iter().rposition(|event| same_input(event, up)) {
            held.remove(index);
        }
    }

    /// True when no synthetic button or key is held.
    pub fn is_empty(&self) -> bool {
        lock(&self.held).is_empty()
    }

    /// Take every pending up, newest first.
    pub fn take_ups(&self) -> Vec<SyntheticEvent> {
        let mut held = lock(&self.held);
        let mut ups = std::mem::take(&mut *held);
        ups.reverse();
        ups
    }
}

fn lock(held: &Mutex<Vec<SyntheticEvent>>) -> std::sync::MutexGuard<'_, Vec<SyntheticEvent>> {
    held.lock().unwrap_or_else(|poison| poison.into_inner())
}

fn same_input(left: &SyntheticEvent, right: &SyntheticEvent) -> bool {
    left.pid == right.pid
        && left.window_id == right.window_id
        && left.button == right.button
        && left.keycode == right.keycode
}
