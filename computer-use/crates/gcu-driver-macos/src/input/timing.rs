//! Injectable monotonic time and cancellation shared by text and clipboard.

use gcu_core::driver::{Cancel, DriverError, Result};
use std::{
    future::Future,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

/// Monotonic clock in the runtime timer's domain. Tests may pause/advance it.
pub trait InputClock: Send + Sync {
    /// Current runtime instant; never wall-clock time.
    fn now(&self) -> tokio::time::Instant;
}

/// Runtime clock, virtualized by Tokio's test clock.
pub struct RuntimeClock;
impl InputClock for RuntimeClock {
    fn now(&self) -> tokio::time::Instant {
        tokio::time::Instant::now()
    }
}

pub(crate) struct Operation<'a> {
    pub cancel: &'a Cancel,
    pub sent: &'a AtomicBool,
    pub clock: &'a dyn InputClock,
    pub deadline: tokio::time::Instant,
}

impl<'a> Operation<'a> {
    pub fn new(
        cancel: &'a Cancel,
        sent: &'a AtomicBool,
        clock: &'a dyn InputClock,
        budget: Duration,
    ) -> Self {
        Self {
            cancel,
            sent,
            clock,
            deadline: clock.now() + budget,
        }
    }
    pub fn check(&self) -> Result<()> {
        let maybe_executed = self.sent.load(Ordering::Acquire);
        if self.cancel.is_cancelled() {
            return Err(DriverError::Cancelled { maybe_executed });
        }
        if self.clock.now() >= self.deadline {
            return Err(DriverError::Timeout { maybe_executed });
        }
        Ok(())
    }
    pub async fn guard<T>(&self, future: impl Future<Output = Result<T>>) -> Result<T> {
        self.check()?;
        let result = tokio::select! {
            biased;
            _ = self.cancel.cancelled() => Err(DriverError::Cancelled { maybe_executed: self.sent.load(Ordering::Acquire) }),
            result = tokio::time::timeout_at(self.deadline, future) => result.unwrap_or_else(|_| Err(DriverError::Timeout { maybe_executed: self.sent.load(Ordering::Acquire) })),
        };
        if result.is_ok() {
            self.check()?;
        }
        result
    }
}
