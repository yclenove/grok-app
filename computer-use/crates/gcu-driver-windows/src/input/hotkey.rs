//! Independent parent-side Stop, native listener lifecycle and injectable boundary.
use super::{
    hooks::PhysicalClock,
    watchdog::{self, Heartbeat, MonotonicTime, SystemTime, POLL_INTERVAL, REINSTALL_INTERVAL},
};
use gcu_core::driver::{DriverError, Result};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

mod fake;
pub use fake::FakeSafetyApi;

/// Trusted Host owns and persists this latch. Implementations must be nonblocking;
/// persistence happens on the Host's independent control task. Driver creation,
/// reconnection and worker restarts never call a clear/resume method.
pub trait StopLatch: Send + Sync + 'static {
    /// Include Host state restored before a new engine may deliver input.
    fn is_stopped(&self) -> bool;
    /// Latch synchronously; arrange persistence without blocking the hook thread.
    fn request_stop(&self);
}
#[cfg(windows)]
pub(crate) struct UnboundStop;
#[cfg(windows)]
impl StopLatch for UnboundStop {
    fn is_stopped(&self) -> bool {
        true
    }
    fn request_stop(&self) {}
}
/// Configurable RegisterHotKey chord. Win32 modifier bits, without MOD_NOREPEAT.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HotkeyConfig {
    /// MOD_ALT=1, MOD_CONTROL=2, MOD_SHIFT=4, MOD_WIN=8.
    pub modifiers: u32,
    /// Windows virtual key; default VK_BACK.
    pub virtual_key: u32,
}
impl Default for HotkeyConfig {
    fn default() -> Self {
        Self {
            modifiers: 7,
            virtual_key: 8,
        }
    }
}
impl HotkeyConfig {
    /// Reject unsupported modifier/key values before any OS registration.
    pub fn validate(self) -> Result<()> {
        if self.modifiers == 0
            || self.modifiers & !15 != 0
            || !(1..=254).contains(&self.virtual_key)
        {
            Err(DriverError::InvalidRequest)
        } else {
            Ok(())
        }
    }
}
/// Thread-affine Win32 resource owner. Drop unregisters hooks/hotkey on that thread.
pub trait SafetyPump {
    /// Pump messages, prove hook health, check the desktop, periodically replace
    /// hooks. May block in FFI; the independent watchdog revokes its health lease.
    fn poll(&mut self, reinstall: bool) -> std::result::Result<(), String>;
}
/// All hooks, hotkey and desktop FFI live behind this injectable boundary.
pub trait SafetyApi: Send + Sync + 'static {
    /// Called only on the dedicated listener thread. No COM/action/approval queue.
    fn open(
        &self,
        config: HotkeyConfig,
        events: Arc<GuardEvents>,
    ) -> std::result::Result<Box<dyn SafetyPump>, String>;
}
/// Small callback target shared by the listener and independent health watchdog.
pub struct GuardEvents {
    clock: Arc<PhysicalClock>,
    heartbeat: Heartbeat,
    hotkey_heartbeat: Heartbeat,
    latch: Arc<dyn StopLatch>,
    local_stop: AtomicBool,
    failed: AtomicBool,
    armed: AtomicBool,
    reason: Mutex<Option<String>>,
    signal: Mutex<Vec<Arc<dyn Fn() + Send + Sync>>>,
}
impl GuardEvents {
    /// Copy only flags from a hook; callbacks never store keyboard/text payloads.
    pub fn clock(&self) -> &PhysicalClock {
        &self.clock
    }
    /// Confirm a responsive message pump and a successfully registered hotkey.
    pub fn heartbeat(&self) {
        if !self.failed.load(Ordering::Acquire) {
            self.heartbeat.beat();
        }
    }
    /// Refresh from the independent RegisterHotKey message thread.
    pub fn hotkey_heartbeat(&self) {
        if !self.failed.load(Ordering::Acquire) {
            self.hotkey_heartbeat.beat();
        }
    }
    /// True only while both hook proofs and the hotkey pump are fresh.
    pub fn available(&self) -> bool {
        let available = !self.failed.load(Ordering::Acquire)
            && self.heartbeat.alive()
            && self.hotkey_heartbeat.alive()
            && self.clock.available();
        if available {
            self.armed.store(true, Ordering::Release);
        }
        available
    }
    pub(super) fn check_health(&self) {
        if !self.available() && self.armed.load(Ordering::Acquire) {
            self.fail("listener heartbeat expired (hook/message pump/FFI unavailable)");
        }
    }
    /// Fail closed. Also stop an in-flight child independently of its RPC queue.
    pub fn fail(&self, reason: impl Into<String>) {
        if !self.failed.swap(true, Ordering::AcqRel) {
            let reason = reason.into();
            tracing::warn!(target: "gcu_windows_timing", stage = "stop_listener", phase = "failed", reason = %reason);
            *self.reason.lock().unwrap_or_else(|p| p.into_inner()) = Some(reason);
            self.clock.disable();
            self.heartbeat.invalidate();
            self.hotkey_heartbeat.invalidate();
            if self.armed.load(Ordering::Acquire) {
                self.request_stop();
            }
        }
    }
    /// The first operation is an atomic local latch, before any Host/worker work.
    pub fn request_stop(&self) {
        self.local_stop.store(true, Ordering::SeqCst);
        let signal = self
            .signal
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone();
        for signal in signal {
            signal();
        }
        self.latch.request_stop();
        if self.latch.is_stopped() {
            self.local_stop.store(false, Ordering::SeqCst);
        }
    }
}
/// Owns two bounded parent threads: Win32 pump and FFI-free health watchdog.
/// Drop signals exit without joining a possibly stuck native call. A stuck pump
/// is never replaced with unbounded new threads. Start waits at most 250ms.
pub struct SafetyGuard {
    events: Arc<GuardEvents>,
    close: Arc<AtomicBool>,
}
impl SafetyGuard {
    /// Bind a pre-restored trusted Host latch before starting native monitoring.
    pub async fn start(
        api: Arc<dyn SafetyApi>,
        config: HotkeyConfig,
        latch: Arc<dyn StopLatch>,
    ) -> Arc<Self> {
        let guard = Self::new(latch, Arc::new(SystemTime));
        let events = guard.events.clone();
        let close = guard.close.clone();
        if config.validate().is_err() {
            events.fail("invalid Stop hotkey configuration");
            return guard;
        }
        let (ready, wait) = tokio::sync::oneshot::channel();
        let monitored = events.clone();
        let pump_close = close.clone();
        let spawned = std::thread::Builder::new()
            .name("gcu-win-hooks".into())
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let mut pump = match api.open(config, monitored.clone()) {
                        Ok(pump) => pump,
                        Err(reason) => {
                            monitored.fail(reason);
                            let _ = ready.send(());
                            return;
                        }
                    };
                    let mut ready = Some(ready);
                    let mut replaced = Instant::now();
                    while !pump_close.load(Ordering::Acquire)
                        && !monitored.failed.load(Ordering::Acquire)
                    {
                        let reinstall = replaced.elapsed() >= REINSTALL_INTERVAL;
                        if reinstall {
                            replaced = Instant::now();
                        }
                        if let Err(reason) = pump.poll(reinstall) {
                            monitored.fail(reason);
                            break;
                        }
                        monitored.heartbeat();
                        if monitored.available() {
                            if let Some(ready) = ready.take() {
                                let _ = ready.send(());
                            }
                        }
                        std::thread::sleep(POLL_INTERVAL);
                    }
                }));
                if result.is_err() {
                    monitored.fail("hook thread crashed");
                }
                monitored.clock.invalidate();
                monitored.heartbeat.invalidate();
            });
        if spawned.is_err() || watchdog::spawn(events.clone(), close).is_err() {
            events.fail("Stop listener/watchdog thread unavailable");
        } else if tokio::time::timeout(Duration::from_millis(250), wait)
            .await
            .is_err()
        {
            events.fail("Stop listener startup/FFI timed out");
        }
        guard
    }
    fn new(latch: Arc<dyn StopLatch>, time: Arc<dyn MonotonicTime>) -> Arc<Self> {
        let events = Arc::new(GuardEvents {
            clock: Arc::new(PhysicalClock::with_time(time.clone())),
            heartbeat: Heartbeat::with_time(time.clone()),
            hotkey_heartbeat: Heartbeat::with_time(time.clone()),
            latch,
            local_stop: AtomicBool::new(false),
            failed: AtomicBool::new(false),
            armed: AtomicBool::new(false),
            reason: Mutex::default(),
            signal: Mutex::default(),
        });
        let close = Arc::new(AtomicBool::new(false));
        Arc::new(Self { events, close })
    }
    /// Attach the supervisor's independent signal, never a queued action.
    /// A pre-existing Host/local Stop is immediately forwarded to the new worker.
    pub fn bind_stop(&self, signal: Arc<dyn Fn() + Send + Sync>) {
        self.events
            .signal
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push(signal.clone());
        if self.is_stopped() {
            signal();
        }
    }
    /// Shared physical clock for the Host's existing UserActivity implementation.
    pub fn activity_clock(&self) -> Arc<PhysicalClock> {
        self.events.clock.clone()
    }
    /// Synchronously latch and wake the independent worker control path.
    pub fn request_stop(&self) {
        self.events.request_stop();
    }
    /// Revoke a listener/setup that cannot guarantee independent input release.
    pub fn disable(&self, reason: impl Into<String>) {
        self.events.fail(reason);
    }
    /// No activity/Stop consumer can mistake stale pump health for availability.
    pub fn available(&self) -> bool {
        self.events.available()
    }
    /// Includes both the Host-persisted latch and an immediate local latch.
    pub fn is_stopped(&self) -> bool {
        self.events.local_stop.load(Ordering::SeqCst) || self.events.latch.is_stopped()
    }
    /// Validate a mutation before queueing and again before input acknowledgement.
    pub fn check(&self, foreground: bool) -> Result<()> {
        if foreground && !self.available() {
            return Err(DriverError::CapabilityUnavailable);
        }
        if self.is_stopped() {
            return Err(DriverError::Cancelled {
                maybe_executed: false,
            });
        }
        Ok(())
    }
    /// Append metadata-only health diagnostics using the existing DoctorReport.
    pub fn doctor_issues(&self, issues: &mut Vec<String>) {
        issues.push(format!(
            "user_activity_guard: {}",
            if self.available() {
                "available"
            } else {
                "unavailable"
            }
        ));
        issues.push(format!(
            "stop_hotkey: {}{}",
            if self.available() {
                "available"
            } else {
                "unavailable"
            },
            self.events
                .reason
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .as_ref()
                .map(|r| format!(" ({r})"))
                .unwrap_or_default()
        ));
        if self.is_stopped() {
            issues.push("Stop latched; trusted Host resume required".into());
        }
    }
}
impl Drop for SafetyGuard {
    fn drop(&mut self) {
        self.close.store(true, Ordering::Release);
        self.events.clock.disable();
    }
}
