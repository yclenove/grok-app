//! Controlled listener fixture; never spawns threads or consults wall time.
use super::*;
use std::sync::{atomic::AtomicUsize, Weak};

struct ManualTime(Mutex<Instant>);
impl Default for ManualTime {
    fn default() -> Self {
        Self(Mutex::new(Instant::now()))
    }
}
impl MonotonicTime for ManualTime {
    fn now(&self) -> Instant {
        *self.0.lock().unwrap_or_else(|p| p.into_inner())
    }
}

/// Portable fault injection with explicit time/pump steps. Use `start` instead
/// of the native threaded lifecycle: scheduler delays are not listener faults.
#[derive(Clone, Default)]
pub struct FakeSafetyApi(Arc<FakeState>);
#[derive(Default)]
struct FakeState {
    time: Arc<ManualTime>,
    events: Mutex<Weak<GuardEvents>>,
    open: Mutex<Option<&'static str>>,
    poll: Mutex<Option<&'static str>>,
    stall: AtomicBool,
    replacements: AtomicUsize,
    replaced: Mutex<Option<Instant>>,
    hooks_removed: AtomicBool,
    registered: Mutex<Option<HotkeyConfig>>,
}
impl FakeSafetyApi {
    /// Start one controlled listener with the same health checks and Stop
    /// callbacks as production, but without native FFI, threads or startup timers.
    pub fn start(&self, config: HotkeyConfig, latch: Arc<dyn StopLatch>) -> Arc<SafetyGuard> {
        let guard = SafetyGuard::new(latch, self.0.time.clone());
        *self.0.events.lock().unwrap_or_else(|p| p.into_inner()) = Arc::downgrade(&guard.events);
        if config.validate().is_err() {
            guard.disable("invalid Stop hotkey configuration");
        } else if let Some(reason) = *self.0.open.lock().unwrap_or_else(|p| p.into_inner()) {
            guard.disable(reason);
        } else {
            *self.0.registered.lock().unwrap_or_else(|p| p.into_inner()) = Some(config);
            *self.0.replaced.lock().unwrap_or_else(|p| p.into_inner()) = Some(self.0.time.now());
            self.poll();
        }
        guard
    }
    fn events(&self) -> Option<Arc<GuardEvents>> {
        self.0
            .events
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .upgrade()
    }
    /// Advance only the listener clock, then run the FFI-free watchdog. No pump
    /// heartbeat is synthesized: tests choose when a stalled lease expires.
    pub fn advance(&self, elapsed: Duration) {
        *self.0.time.0.lock().unwrap_or_else(|p| p.into_inner()) += elapsed;
        if let Some(events) = self.events() {
            events.check_health();
        }
    }
    /// Run one listener iteration at the controlled instant. A stalled pump does
    /// no work; the independent `advance`/watchdog and `trigger` still operate.
    pub fn poll(&self) {
        let Some(events) = self.events() else { return };
        if events.failed.load(Ordering::Acquire) || self.0.stall.load(Ordering::Acquire) {
            return;
        }
        if let Some(reason) = *self.0.poll.lock().unwrap_or_else(|p| p.into_inner()) {
            events.fail(reason);
            return;
        }
        let at = self.0.time.now();
        let mut replaced = self.0.replaced.lock().unwrap_or_else(|p| p.into_inner());
        if replaced.is_some_and(|last| at.saturating_duration_since(last) >= REINSTALL_INTERVAL) {
            self.0.replacements.fetch_add(1, Ordering::AcqRel);
            *replaced = Some(at);
        }
        if !self.0.hooks_removed.load(Ordering::Acquire) {
            events.clock.confirm(at);
        }
        events.heartbeat();
        events.hotkey_heartbeat();
        events.check_health();
    }
    /// Simulate silent OS hook removal while the message pump still responds.
    pub fn remove_hooks(&self) {
        self.0.hooks_removed.store(true, Ordering::Release);
    }
    /// Exact configuration passed to the registration boundary.
    pub fn registered_config(&self) -> Option<HotkeyConfig> {
        *self.0.registered.lock().unwrap_or_else(|p| p.into_inner())
    }
    /// Inject registration conflict/access denial.
    pub fn fail_open(&self, reason: &'static str) {
        *self.0.open.lock().unwrap_or_else(|p| p.into_inner()) = Some(reason);
    }
    /// Inject hook removal, pump crash, or secure desktop on the next poll.
    pub fn fail_poll(&self, reason: &'static str) {
        *self.0.poll.lock().unwrap_or_else(|p| p.into_inner()) = Some(reason);
    }
    /// Model synchronous FFI that never returns a successful poll.
    pub fn stall_poll(&self) {
        self.0.stall.store(true, Ordering::Release);
    }
    /// Allow polling again; a revoked lease remains permanently unavailable.
    pub fn unstall(&self) {
        self.0.stall.store(false, Ordering::Release);
    }
    /// Inject WM_HOTKEY through the independent callback even if hooks are stuck.
    pub fn trigger(&self) {
        if let Some(events) = self.events() {
            events.request_stop();
        }
    }
    /// Number of periodic hook replacements.
    pub fn replacements(&self) -> usize {
        self.0.replacements.load(Ordering::Acquire)
    }
}
