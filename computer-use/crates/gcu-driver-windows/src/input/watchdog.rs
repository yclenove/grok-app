//! Parent watchdog contains no Win32/COM calls and cannot wait behind FFI.
use super::hotkey::GuardEvents;
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

/// Polling interval for independent health supervision and the native pump.
pub const POLL_INTERVAL: Duration = Duration::from_millis(20);
/// Maximum age of confirmed hook/pump heartbeats before disabling input.
pub const HEARTBEAT_TIMEOUT: Duration = Duration::from_millis(100);
/// Replace low-level hooks even when Windows removed them silently.
pub const REINSTALL_INTERVAL: Duration = Duration::from_secs(1);
/// Shared time source: production uses the OS monotonic clock; fake listeners
/// advance explicitly so executor/thread scheduling cannot revoke their health.
pub(crate) trait MonotonicTime: Send + Sync {
    fn now(&self) -> Instant;
}
pub(crate) struct SystemTime;
impl MonotonicTime for SystemTime {
    fn now(&self) -> Instant {
        Instant::now()
    }
}
/// Short, bounded read of the registration/message-pump lease; no native IO.
pub struct Heartbeat {
    last: Mutex<Option<Instant>>,
    time: Arc<dyn MonotonicTime>,
}
impl Default for Heartbeat {
    fn default() -> Self {
        Self::with_time(Arc::new(SystemTime))
    }
}
impl Heartbeat {
    pub(crate) fn with_time(time: Arc<dyn MonotonicTime>) -> Self {
        Self {
            last: Mutex::default(),
            time,
        }
    }
    /// Refresh after a successful poll, never before a blocking FFI call.
    pub fn beat(&self) {
        *self.last.lock().unwrap_or_else(|p| p.into_inner()) = Some(self.time.now());
    }
    /// Listener death/hang invalidates health even without an explicit failure.
    pub fn alive(&self) -> bool {
        self.last
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .is_some_and(|at| self.time.now().saturating_duration_since(at) <= HEARTBEAT_TIMEOUT)
    }
    /// Permanently discard the previous heartbeat on failure.
    pub fn invalidate(&self) {
        *self.last.lock().unwrap_or_else(|p| p.into_inner()) = None;
    }
}
pub(super) fn spawn(events: Arc<GuardEvents>, close: Arc<AtomicBool>) -> std::io::Result<()> {
    std::thread::Builder::new()
        .name("gcu-stop-watch".into())
        .spawn(move || {
            while !close.load(Ordering::Acquire) {
                events.check_health();
                std::thread::sleep(POLL_INTERVAL);
            }
        })
        .map(|_| ())
}
pub(crate) fn capabilities(base: &gcu_protocol::Capabilities) -> [gcu_protocol::Capabilities; 8] {
    std::array::from_fn(|i| {
        let mut caps = base.clone();
        caps.user_activity_guard = i & 1 != 0;
        if i & 2 == 0 {
            caps.delivery.foreground =
                gcu_protocol::capabilities::ForegroundCapability::Unavailable;
        }
        caps.overlay_excluded_from_capture = i & 4 != 0;
        caps
    })
}
pub(crate) fn foreground(req: &gcu_core::driver::DriverAction) -> bool {
    use gcu_core::driver::*;
    match req {
        DriverAction::Activate { .. } => true,
        DriverAction::PointerClick { route, .. }
        | DriverAction::PointerScroll { route, .. }
        | DriverAction::PointerDrag { route, .. } => *route == PointerRoute::AllowForeground,
        DriverAction::PressKeys { route, .. } | DriverAction::TypeText { route, .. } => {
            *route == KeyRoute::AllowForeground
        }
        _ => false,
    }
}
