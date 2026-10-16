//! Injectable monotonic time. Fake waits never use wall-clock sleeps.

use async_trait::async_trait;
use std::{
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::sync::Notify;

/// Monotonic deadlines shared by activity, listener health and native calls.
#[async_trait]
pub trait Clock: Send + Sync {
    /// Elapsed time from an arbitrary fixed epoch. Non-blocking.
    fn now(&self) -> Duration;
    /// Wait until the deadline, or cancel by dropping this future.
    async fn wait_until(&self, deadline: Duration);
}

/// Production monotonic clock; Tokio's test clock can also drive it.
pub struct RuntimeClock(tokio::time::Instant);

impl Default for RuntimeClock {
    fn default() -> Self {
        Self(tokio::time::Instant::now())
    }
}

#[async_trait]
impl Clock for RuntimeClock {
    fn now(&self) -> Duration {
        self.0.elapsed()
    }
    async fn wait_until(&self, deadline: Duration) {
        tokio::time::sleep_until(self.0 + deadline).await;
    }
}

/// Manually advanced clock. Advancing wakes all pending deadlines.
#[derive(Default)]
pub struct FakeClock {
    millis: AtomicU64,
    changed: Notify,
}

impl FakeClock {
    /// Clock starting at zero.
    pub fn new() -> Self {
        Self::default()
    }
    /// Advance monotonically, saturating at the largest millisecond value.
    pub fn advance(&self, by: Duration) {
        let delta = u64::try_from(by.as_millis()).unwrap_or(u64::MAX);
        let _ = self
            .millis
            .try_update(Ordering::AcqRel, Ordering::Acquire, |old| {
                Some(old.saturating_add(delta))
            });
        self.changed.notify_waiters();
    }
}

#[async_trait]
impl Clock for FakeClock {
    fn now(&self) -> Duration {
        Duration::from_millis(self.millis.load(Ordering::Acquire))
    }
    async fn wait_until(&self, deadline: Duration) {
        loop {
            let changed = self.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if self.now() >= deadline {
                return;
            }
            changed.await;
        }
    }
}

pub(crate) fn runtime_clock() -> Arc<dyn Clock> {
    Arc::new(RuntimeClock::default())
}
