//! Native call timeout watchdog and listener heartbeat monitor.
//!
//! Enforces the driver-internal deadline [`gcu_core::driver::DRIVER_DEADLINE`] (4,750ms),
//! which is shorter than the router-level [`gcu_core::driver::NATIVE_TIMEOUT`] (5,000ms).
//! This guarantees that the driver times out first and can decisively report
//! `maybe_executed: true` without outer race conditions.
//!
//! Also monitors heartbeats from event listener threads so that connection drops
//! or thread panics immediately mark the physical activity guard and hotkey as unavailable.

use crate::x11::activity::SharedActivityClock;
use crate::x11::hotkey::HotkeyManager;
use gcu_core::driver::{DriverError, DRIVER_DEADLINE};
use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Default maximum allowable staleness for listener heartbeats before declaring failure.
pub const DEFAULT_HEARTBEAT_TIMEOUT: Duration = Duration::from_millis(1000);

/// Run an asynchronous native operation wrapped with the driver watchdog deadline.
///
/// If `work` does not finish within [`DRIVER_DEADLINE`] (4,750ms), the operation is abandoned
/// and [`DriverError::Timeout`] with `maybe_executed: true` is returned.
pub async fn run_with_watchdog<F, T>(work: F) -> Result<T, DriverError>
where
    F: Future<Output = Result<T, DriverError>>,
{
    run_with_timeout(DRIVER_DEADLINE, work).await
}

/// Run an asynchronous native operation with an explicit timeout duration.
pub async fn run_with_timeout<F, T>(timeout: Duration, work: F) -> Result<T, DriverError>
where
    F: Future<Output = Result<T, DriverError>>,
{
    match tokio::time::timeout(timeout, work).await {
        Ok(res) => res,
        Err(_) => Err(DriverError::Timeout {
            maybe_executed: true,
        }),
    }
}

/// Heartbeat monitor tracking the health of background X11 event listener threads.
#[derive(Debug, Clone)]
pub struct HeartbeatWatchdog {
    last_heartbeat: Arc<AtomicU64>,
    epoch: Instant,
}

impl Default for HeartbeatWatchdog {
    fn default() -> Self {
        Self::new()
    }
}

impl HeartbeatWatchdog {
    /// Create a new [`HeartbeatWatchdog`].
    pub fn new() -> Self {
        let epoch = Instant::now();
        Self {
            last_heartbeat: Arc::new(AtomicU64::new(0)),
            epoch,
        }
    }

    /// Record a heartbeat from the monitored thread at the current instant.
    pub fn record_heartbeat(&self) {
        let elapsed = self.epoch.elapsed().as_millis() as u64;
        self.last_heartbeat.store(elapsed + 1, Ordering::Release);
    }

    /// Check if a heartbeat was recorded within `max_staleness`.
    pub fn is_alive(&self, max_staleness: Duration) -> bool {
        let last = self.last_heartbeat.load(Ordering::Acquire);
        if last == 0 {
            // No heartbeat has ever been received
            return false;
        }
        let last_elapsed = last - 1;
        let current = self.epoch.elapsed().as_millis() as u64;
        let diff = current.saturating_sub(last_elapsed);
        diff <= max_staleness.as_millis() as u64
    }

    /// Inspect the heartbeat and, if stale, mark both activity guard and hotkey as unavailable.
    pub fn check_liveness(
        &self,
        clock: &SharedActivityClock,
        hotkey: &HotkeyManager,
        max_staleness: Duration,
    ) -> bool {
        let alive = self.is_alive(max_staleness);
        if !alive {
            clock.set_available(false);
            hotkey.set_available(false);
        }
        alive
    }
}
