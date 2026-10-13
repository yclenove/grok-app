//! Completion timestamps are owned by the original grant, not a new owner.
use super::types::{Clock, GRANT_IDLE_TTL};
use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

pub(crate) struct PhysicalActivity {
    clock: Clock,
    deadline: Mutex<Duration>,
    idle_confirmed: AtomicBool,
}

impl PhysicalActivity {
    pub(super) fn new(clock: Clock) -> Self {
        let deadline = clock.now().saturating_add(GRANT_IDLE_TTL);
        Self {
            clock,
            deadline: Mutex::new(deadline),
            idle_confirmed: AtomicBool::new(false),
        }
    }

    pub(crate) fn finish(&self) {
        let next = self.clock.now().saturating_add(GRANT_IDLE_TTL);
        let mut deadline = self.deadline.lock();
        *deadline = (*deadline).max(next);
        self.idle_confirmed.store(true, Ordering::SeqCst);
    }

    pub(crate) fn begin(&self) {
        self.idle_confirmed.store(false, Ordering::SeqCst);
    }

    pub(crate) fn idle_confirmed(&self) -> bool {
        self.idle_confirmed.load(Ordering::SeqCst)
    }

    pub(super) fn deadline(&self) -> Duration {
        *self.deadline.lock()
    }
}
