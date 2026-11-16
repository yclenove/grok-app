//! One-shot cleanup lock. The try returns on this call.
//!
//! A held lock is [`LockError::Busy`]. This module does not sleep, spin, or
//! wait for the deadline. The fake drops its guard before `try_acquire`
//! returns, so a Tart or guest call cannot run while the guard is held.

use std::sync::Mutex;
use std::time::Duration;

use super::deadline::{FakeClock, HostClock, MonoInstant};

/// Why the lock was not acquired.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum LockError {
    /// `now` was already at or past the deadline. The lock was not taken.
    #[error("lock deadline already expired")]
    Expired,
    /// Someone else holds it. This call did not wait.
    #[error("cleanup lock is busy")]
    Busy,
    /// Part of a scripted acquire cost was charged. The lock was not taken.
    ///
    /// The caller polls again. This is not [`Self::Busy`] and not expiry.
    #[error("lock acquire is still pending")]
    Pending,
}

/// Try-lock used around one state decision.
///
/// Implementations must return on this call. They must not wait until
/// `deadline`, and they must not still hold the guard when this returns.
pub trait CleanupLock {
    /// Acquire once, or fail.
    ///
    /// Does not block. An already-due `deadline` is [`LockError::Expired`]
    /// and does not take the lock. A held lock is [`LockError::Busy`].
    fn try_acquire(&mut self, deadline: MonoInstant) -> Result<(), LockError>;
}

/// In-memory lock for the fake machine.
///
/// [`Self::hold_forever`] keeps the mutex locked for the rest of the process
/// so a later blocking wait would hang. [`Self::try_acquire`] does not block
/// on that hold.
#[derive(Debug)]
pub struct FakeCleanupLock {
    mutex: Mutex<()>,
    clock: FakeClock,
    /// Consumed by the next `try_acquire` that is still inside its deadline.
    acquire_advance: Duration,
}

impl FakeCleanupLock {
    /// Unlocked lock that reads `clock`.
    ///
    /// Does not spawn and does not wait.
    #[must_use]
    pub fn new(clock: FakeClock) -> Self {
        Self {
            mutex: Mutex::new(()),
            clock,
            acquire_advance: Duration::ZERO,
        }
    }

    /// On the next in-time `try_acquire`, move the shared clock forward by `step`.
    ///
    /// Does not sleep. The step is consumed once. Zero clears a previously
    /// armed step. A step that lands on or past the supplied deadline makes
    /// that acquire [`LockError::Expired`].
    pub fn advance_on_acquire(&mut self, step: Duration) {
        self.acquire_advance = step;
    }

    /// Lock and never unlock.
    ///
    /// Uses one non-blocking try. On success the guard is forgotten, so
    /// `Drop` does not unlock it. A second call returns [`LockError::Busy`].
    /// Does not sleep. This is the injection for a lock that is never released.
    pub fn hold_forever(&mut self) -> Result<(), LockError> {
        match self.mutex.try_lock() {
            Ok(guard) => {
                std::mem::forget(guard);
                Ok(())
            }
            Err(_) => Err(LockError::Busy),
        }
    }
}

impl CleanupLock for FakeCleanupLock {
    fn try_acquire(&mut self, deadline: MonoInstant) -> Result<(), LockError> {
        if self.clock.now() >= deadline {
            return Err(LockError::Expired);
        }
        if !self.acquire_advance.is_zero() {
            let step = self.acquire_advance;
            // No slice room keeps the old one-shot step, including expiry when
            // that step lands on the deadline. A room charges at most the room
            // and leaves the rest for the next poll.
            let quantum = match self.clock.charge_room() {
                Some(room) => step.min(room),
                None => step,
            };
            if quantum.is_zero() {
                return Err(LockError::Pending);
            }
            if self.clock.advance(quantum).is_err() || self.clock.now() >= deadline {
                self.acquire_advance = Duration::ZERO;
                return Err(LockError::Expired);
            }
            let left = step.saturating_sub(quantum);
            self.acquire_advance = left;
            if !left.is_zero() {
                return Err(LockError::Pending);
            }
        }
        if self.clock.now() >= deadline {
            return Err(LockError::Expired);
        }
        match self.mutex.try_lock() {
            Ok(guard) => {
                // Drop before return. Callers cannot do IO under this guard.
                drop(guard);
                Ok(())
            }
            Err(_) => Err(LockError::Busy),
        }
    }
}
