//! Controlled listener fixture; never spawns threads or consults wall time.
use super::*;
use std::sync::{atomic::AtomicUsize, Weak};
use std::time::Duration;

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
    proofs: AtomicUsize,
    replaced: Mutex<Option<Instant>>,
    hooks_removed: AtomicBool,
    registered: Mutex<Option<HotkeyConfig>>,
    /// Mouse callback delay, then keyboard delay after that. `None` confirms both at `now`.
    latency: Mutex<Option<(Duration, Duration)>>,
    probes: Mutex<ProbeRuntime>,
}
#[derive(Default)]
struct ProbeRuntime {
    /// Callback instants for the in-flight pair. Keyboard is strictly after mouse.
    inflight: Option<(Instant, Instant)>,
    /// Pair finished on an earlier poll. Applied on the next poll, matching a
    /// report that arrives after the hook has already been observed.
    queued: Option<(Instant, Instant)>,
}
impl FakeSafetyApi {
    /// Start one controlled listener with the same health checks and Stop
    /// callbacks as production, but without native FFI, threads or startup timers.
    pub fn start(&self, config: HotkeyConfig, latch: Arc<dyn StopLatch>) -> Arc<SafetyGuard> {
        let guard = SafetyGuard::new(latch, self.0.time.clone());
        *self.0.events.lock().unwrap_or_else(|p| p.into_inner()) = Arc::downgrade(&guard.events);
        let api = self.clone();
        *guard
            .events
            .proof_request
            .lock()
            .unwrap_or_else(|p| p.into_inner()) = Some(Arc::new(move || {
            api.0.proofs.fetch_add(1, Ordering::AcqRel);
            if api.0.hooks_removed.load(Ordering::Acquire) {
                if let Some(events) = api.events() {
                    events.refuse_proof("missing fake hook callbacks".into());
                }
            } else {
                api.poll();
            }
        }));
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
        if let Some((mouse, keyboard)) = *self.0.latency.lock().unwrap_or_else(|p| p.into_inner()) {
            if events.action_open() {
                self.poll_serial(&events, at, mouse, keyboard);
            }
        } else if !self.0.hooks_removed.load(Ordering::Acquire) {
            let cycle = events.foreground_cycle();
            if let Err(reason) = events.confirm_hook_proofs(cycle, Some(at), Some(at)) {
                events.fail(reason);
                return;
            }
        }
        events.heartbeat();
        events.hotkey_heartbeat();
        events.setup_complete.store(true, Ordering::Release);
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
    /// Serial probe callbacks for one action window. Zero keeps the instant pair.
    /// The keyboard callback is `mouse + keyboard` after the probe is submitted.
    pub fn set_serial_probe_delay(&self, mouse: Duration, keyboard: Duration) {
        *self.0.latency.lock().unwrap_or_else(|p| p.into_inner()) = Some((mouse, keyboard));
    }
    fn poll_serial(&self, events: &GuardEvents, now: Instant, mouse: Duration, keyboard: Duration) {
        if self.0.hooks_removed.load(Ordering::Acquire) {
            return;
        }
        let apply = {
            let mut probes = self.0.probes.lock().unwrap_or_else(|p| p.into_inner());
            let apply = probes.queued.take();
            if let Some((mouse_due, keyboard_due)) = probes.inflight {
                if now >= keyboard_due {
                    probes.queued = Some((mouse_due, keyboard_due));
                    probes.inflight = None;
                }
            }
            apply
        };
        if let Some((mouse_at, keyboard_at)) = apply {
            let cycle = events.foreground_cycle();
            if let Err(reason) =
                events.confirm_hook_proofs(cycle, Some(keyboard_at), Some(mouse_at))
            {
                events.fail(reason);
                return;
            }
        }
        let pending = {
            let probes = self.0.probes.lock().unwrap_or_else(|p| p.into_inner());
            probes.inflight.is_some() || probes.queued.is_some()
        };
        if events.action_open()
            && super::should_dispatch_action_probe(
                pending,
                events.clock().available(),
                events.injecting(),
            )
        {
            let mut probes = self.0.probes.lock().unwrap_or_else(|p| p.into_inner());
            if probes.inflight.is_none() && probes.queued.is_none() {
                probes.inflight = Some((now + mouse, now + mouse + keyboard));
            }
        }
    }
    /// Number of action preparation cycles (inner checks never request one).
    pub fn proof_requests(&self) -> usize {
        self.0.proofs.load(Ordering::Acquire)
    }
    /// Number of periodic hook replacements.
    pub fn replacements(&self) -> usize {
        self.0.replacements.load(Ordering::Acquire)
    }
}
