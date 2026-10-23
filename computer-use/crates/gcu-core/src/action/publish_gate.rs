//! Test pause at the publication boundary. Production leaves it disarmed.
//!
//! The gate is memory only. It does not take the scope lock or access a driver.
//! Armed test pauses fail after 15 seconds; a disarmed
//! `enter` is one atomic load.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

#[derive(Debug)]
struct GateState {
    armed: bool,
    parked: bool,
    released: bool,
}

/// One-shot pause inside observation publication.
///
/// Memory only. Armed pauses panic after 15 seconds. No cancellation.
/// Production never calls [`Self::arm`],
/// so [`Self::enter`] does not wait and does not hold any scope lock.
#[derive(Debug)]
pub struct PublishGate {
    armed: AtomicBool,
    state: Mutex<GateState>,
    cv: Condvar,
}

impl PublishGate {
    /// Disarmed gate. Memory only. No blocking, timeout, or cancellation.
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            armed: AtomicBool::new(false),
            state: Mutex::new(GateState {
                armed: false,
                parked: false,
                released: false,
            }),
            cv: Condvar::new(),
        })
    }

    /// Pause when [`Self::arm`] ran and [`Self::release`] has not.
    ///
    /// A disarmed gate returns immediately. An armed gate waits on a condvar.
    /// Memory only: no driver call or cancellation. An armed test pause panics
    /// after 15 seconds, releasing its mutex before unwinding.
    /// Poisoned mutexes are recovered.
    pub fn enter(&self) {
        if !self.armed.load(Ordering::Acquire) {
            return;
        }
        let mut guard = self.state.lock().unwrap_or_else(|err| err.into_inner());
        if !guard.armed {
            return;
        }
        guard.parked = true;
        self.cv.notify_all();
        let (mut guard, timeout) = self
            .cv
            .wait_timeout_while(guard, Duration::from_secs(15), |state| {
                state.armed && !state.released
            })
            .unwrap_or_else(|err| err.into_inner());
        let expired = timeout.timed_out() && guard.armed && !guard.released;
        guard.parked = false;
        guard.armed = false;
        guard.released = false;
        self.armed.store(false, Ordering::Release);
        drop(guard);
        assert!(
            !expired,
            "publication test gate was not released within 15 seconds"
        );
    }

    /// Arm the next [`Self::enter`]. Memory only. No timeout or cancellation.
    pub fn arm(&self) {
        let mut guard = self.state.lock().unwrap_or_else(|err| err.into_inner());
        guard.armed = true;
        guard.released = false;
        guard.parked = false;
        self.armed.store(true, Ordering::Release);
    }

    /// Unblock a publisher parked in [`Self::enter`].
    ///
    /// Memory only. No timeout or cancellation. Poisoned mutexes are recovered.
    pub fn release(&self) {
        let mut guard = self.state.lock().unwrap_or_else(|err| err.into_inner());
        guard.released = true;
        guard.armed = false;
        self.armed.store(false, Ordering::Release);
        self.cv.notify_all();
    }

    /// Whether a publisher is inside [`Self::enter`].
    ///
    /// Memory only. May wait on the gate mutex. No timeout or cancellation.
    pub fn is_parked(&self) -> bool {
        self.state
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .parked
    }
}
