//! Injectable monotonic time; every settle timer uses the same clock.
use async_trait::async_trait;
use std::time::Duration;

/// Local monotonic clock. Implementations must never move backwards.
#[async_trait]
pub trait Clock: Send + Sync {
    /// Elapsed time from a fixed epoch. Nonblocking; no IO or cancellation.
    fn now(&self) -> Duration;
    /// Wait for an absolute deadline. Dropping the future cancels the wait.
    /// Must yield while the deadline is in the future, including in test clocks.
    async fn wait_until(&self, deadline: Duration);
}

/// Production clock, also driven by Tokio's paused test clock.
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
