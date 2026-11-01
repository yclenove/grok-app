//! Test pause around one session-file write. Production never constructs it.

use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

/// One-shot pause around the first session-file write in a test.
///
/// Memory only. The armed write waits until [`Self::release`]. Later writes do
/// not wait. Panics after 15 seconds. No cancellation.
pub(crate) struct FilePause {
    state: Mutex<PauseState>,
    cv: Condvar,
}

struct PauseState {
    armed: bool,
    used: bool,
    parked: bool,
    released: bool,
    finished: bool,
    /// Writes to ignore before the one that parks. Zero parks the next write.
    skip: u32,
}

impl FilePause {
    pub(super) fn new() -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(PauseState {
                armed: false,
                used: false,
                parked: false,
                released: false,
                finished: false,
                skip: 0,
            }),
            cv: Condvar::new(),
        })
    }

    pub(super) fn arm_skip(&self, skip: u32) {
        let mut guard = self.state.lock().unwrap_or_else(|err| err.into_inner());
        guard.armed = true;
        guard.released = false;
        guard.parked = false;
        guard.finished = false;
        guard.used = false;
        guard.skip = skip;
    }

    pub(super) fn pause_once(&self) -> bool {
        let mut guard = self.state.lock().unwrap_or_else(|err| err.into_inner());
        if !guard.armed || guard.used {
            return false;
        }
        if guard.skip > 0 {
            guard.skip -= 1;
            return false;
        }
        guard.used = true;
        guard.parked = true;
        self.cv.notify_all();
        let (mut guard, timeout) = self
            .cv
            .wait_timeout_while(guard, Duration::from_secs(15), |state| !state.released)
            .unwrap_or_else(|err| err.into_inner());
        let expired = timeout.timed_out() && !guard.released;
        guard.parked = false;
        guard.armed = false;
        drop(guard);
        assert!(
            !expired,
            "file test gate was not released within 15 seconds"
        );
        true
    }

    /// Unblock the write parked in this gate.
    ///
    /// Memory only. No timeout or cancellation.
    pub(crate) fn release(&self) {
        let mut guard = self.state.lock().unwrap_or_else(|err| err.into_inner());
        guard.released = true;
        guard.armed = false;
        self.cv.notify_all();
    }

    /// Whether the first write is waiting. Memory only. No timeout or cancellation.
    pub(crate) fn is_parked(&self) -> bool {
        self.state
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .parked
    }

    /// The staged file from this gate was dropped or committed.
    ///
    /// Memory only. No timeout or cancellation.
    pub(crate) fn has_finished(&self) -> bool {
        self.state
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .finished
    }

    pub(super) fn note_finished(&self) {
        let mut guard = self.state.lock().unwrap_or_else(|err| err.into_inner());
        guard.finished = true;
        self.cv.notify_all();
    }
}
