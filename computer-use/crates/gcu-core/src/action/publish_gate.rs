//! Test pause at the publication boundary. Production leaves it disarmed.
//!
//! The gate is memory only. It does not take the scope lock, talk to a driver,
//! or apply a timeout. A disarmed `enter` is one atomic load.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};

struct GateState {
    armed: bool,
    parked: bool,
    released: bool,
}

/// One-shot pause inside observation publication.
///
/// Memory only. No timeout or cancellation. Production never calls [`Self::arm`],
/// so [`Self::enter`] does not wait and does not hold any scope lock.
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
    /// Memory only: no driver call, no scope lock, no timeout, and no cancellation.
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
        while guard.armed && !guard.released {
            guard = self.cv.wait(guard).unwrap_or_else(|err| err.into_inner());
        }
        guard.parked = false;
        guard.armed = false;
        guard.released = false;
        self.armed.store(false, Ordering::Release);
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
