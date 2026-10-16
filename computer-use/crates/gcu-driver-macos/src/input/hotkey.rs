//! Stop ownership stays with the trusted Host. Drivers never clear its latch.

use super::{
    activity::{ActivityClock, OWN_EVENT_TAG},
    clock::{runtime_clock, Clock},
};
use gcu_core::driver::{DriverError, Result};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::sync::Notify;

/// Native registration outcome, without leaking platform error payloads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Registration {
    /// This process owns the configured chord.
    Registered,
    /// Another process already owns it.
    Conflict,
    /// Unsupported configuration, missing API or other registration failure.
    Unavailable,
}

/// Thread-local hotkey registration boundary. Production keeps the owned
/// Carbon reference on the event thread; tests inject registration failures.
pub trait HotkeyRegistrar {
    /// Reserve a chord once. Must not steal an existing registration.
    fn register(&mut self, config: &HotkeyConfig) -> Registration;
}

/// Apply a registration result to the exact health source used by routing.
pub fn register_hotkey(
    api: &mut dyn HotkeyRegistrar,
    monitor: &super::activity::ActivityMonitor,
    config: &HotkeyConfig,
) -> Registration {
    let result = api.register(config);
    monitor.hotkey_registered(result == Registration::Registered);
    result
}

/// Host-owned durable Stop state. `request_stop` must latch synchronously and
/// arrange persistence independently of the action/approval queue (ADR 0003).
/// Resume is deliberately absent; only the trusted Host may expose it.
pub trait StopLatch: Send + Sync {
    /// Non-blocking level, preserved across turns and reconnections.
    fn is_stopped(&self) -> bool;
    /// Latch immediately. No blocking disk I/O in this method.
    fn request_stop(&self);
}

/// In-memory latch for tests. Production injects a persistent Host latch.
pub struct SharedStopLatch(AtomicBool);
impl SharedStopLatch {
    /// Restore the Host's latched state. No automatic clearing API.
    pub fn new(stopped: bool) -> Self {
        Self(AtomicBool::new(stopped))
    }
}
impl StopLatch for SharedStopLatch {
    fn is_stopped(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
    fn request_stop(&self) {
        self.0.store(true, Ordering::Release);
    }
}

/// Configurable virtual key and CoreGraphics modifier flags.
#[derive(Clone, Copy, Debug)]
pub struct HotkeyConfig {
    /// macOS virtual keycode (default: period).
    pub keycode: u16,
    /// Control + Option + Command by default. Caps Lock is ignored.
    pub modifiers: u64,
}
impl Default for HotkeyConfig {
    fn default() -> Self {
        Self {
            keycode: 47,
            modifiers: 262144 | 524288 | 1048576,
        }
    }
}

/// Safety sources for event routing and the independent release worker.
pub struct Guards {
    /// Injectable monotonic clock used for every guard deadline.
    pub clock: Arc<dyn Clock>,
    activity: Arc<dyn ActivityClock>,
    latch: Arc<dyn StopLatch>,
    idle_ms: u64,
    changed: Notify,
}

impl Guards {
    /// Bind to Host-owned Stop state; construction never clears it.
    pub fn new(
        clock: Arc<dyn Clock>,
        activity: Arc<dyn ActivityClock>,
        latch: Arc<dyn StopLatch>,
        idle_ms: u64,
    ) -> Self {
        Self {
            clock,
            activity,
            latch,
            idle_ms,
            changed: Notify::new(),
        }
    }
    pub(crate) fn fixture() -> Arc<Self> {
        Arc::new(Self::new(
            runtime_clock(),
            Arc::new(super::activity::IdleFixture),
            Arc::new(SharedStopLatch::new(false)),
            3000,
        ))
    }
    /// Read the same Host latch used by the global hotkey.
    pub fn is_stopped(&self) -> bool {
        self.latch.is_stopped()
    }
    /// Latch without waiting on actions or approvals, and wake release workers.
    pub fn request_stop(&self) {
        self.latch.request_stop();
        self.changed.notify_waiters();
    }
    /// Input age suitable for the Host's `UserActivity` adapter. Core returns
    /// `user_active` for recent input; driver gates independently fail closed.
    pub fn ms_since_physical_input(&self) -> Option<u64> {
        self.activity.ms_since_physical_input()
    }
    /// Whether reliable global Stop is currently available.
    pub fn stop_available(&self) -> bool {
        self.activity.stop_available()
    }
    /// Foreground requires a healthy Stop listener and positively known idle time.
    pub fn foreground_ready(&self) -> bool {
        !self.is_stopped()
            && self.stop_available()
            && self
                .ms_since_physical_input()
                .is_some_and(|ms| ms >= self.idle_ms)
    }
    /// Gate before activation and before each post. Never waits for user idle.
    pub fn check(&self, foreground: bool) -> Result<()> {
        if self.is_stopped() {
            return Err(DriverError::Cancelled {
                maybe_executed: false,
            });
        }
        if foreground && !self.foreground_ready() {
            return Err(DriverError::CapabilityUnavailable);
        }
        Ok(())
    }
    /// Process a tap key-down. Extra Shift/Fn modifiers do not match.
    pub fn key_down(&self, config: &HotkeyConfig, keycode: u16, flags: u64, marker: i64) -> bool {
        const MODIFIERS: u64 = 131072 | 262144 | 524288 | 1048576 | 8388608;
        if marker == OWN_EVENT_TAG
            || !self.stop_available()
            || keycode != config.keycode
            || flags & MODIFIERS != config.modifiers
        {
            return false;
        }
        self.request_stop();
        true
    }
    /// Wait for Stop from either this listener or the external Host. Polling
    /// uses the injected clock; cancellation is dropping the future.
    pub async fn stopped(&self) {
        loop {
            let changed = self.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if self.is_stopped() {
                return;
            }
            tokio::select! {
                _ = changed => {},
                _ = self.clock.wait_until(self.clock.now() + Duration::from_millis(10)) => {},
            }
        }
    }
}

impl std::fmt::Debug for Guards {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Guards")
            .field("stopped", &self.is_stopped())
            .finish_non_exhaustive()
    }
}
