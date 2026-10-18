//! Independent injected-proof lease and physical activity coverage.
use super::watchdog::{MonotonicTime, SystemTime, HEARTBEAT_TIMEOUT};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Instant,
};

/// Flags copied from a low-level hook; no pointer or payload leaves the callback.
#[derive(Clone, Copy, Debug)]
pub enum HookEvent {
    /// KBDLLHOOKSTRUCT.flags (LLKHF_INJECTED / LOWER_IL_INJECTED).
    Keyboard(u32),
    /// MSLLHOOKSTRUCT.flags (LLMHF_INJECTED / LOWER_IL_INJECTED).
    Mouse(u32),
}
impl HookEvent {
    /// Synthetic input includes every integrity level, including our SendInput.
    pub fn injected(self) -> bool {
        match self {
            Self::Keyboard(flags) => flags & 0x12 != 0,
            Self::Mouse(flags) => flags & 3 != 0,
        }
    }
}
#[derive(Default)]
struct State {
    covered: bool,
    confirmed: Option<Instant>,
    physical: Option<Instant>,
}
/// Physical activity covers this listener generation, independently of the
/// 100ms injection lease. Only non-injected callbacks create activity history.
pub struct PhysicalClock {
    state: Mutex<State>,
    disabled: AtomicBool,
    time: Arc<dyn MonotonicTime>,
}
impl Default for PhysicalClock {
    fn default() -> Self {
        Self::with_time(Arc::new(SystemTime))
    }
}
impl PhysicalClock {
    pub(crate) fn with_time(time: Arc<dyn MonotonicTime>) -> Self {
        Self {
            state: Mutex::default(),
            disabled: AtomicBool::new(false),
            time,
        }
    }
    /// Confirm both hooks on the current interactive desktop at this instant.
    pub fn confirm(&self, at: Instant) {
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if self.disabled.load(Ordering::Acquire) {
            return;
        }
        state.covered = true;
        state.confirmed = Some(at);
    }
    /// Discard an action's lease without discarding physical activity coverage.
    pub(crate) fn discard_lease(&self) {
        self.state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .confirmed = None;
    }
    /// Clear history when a hook, desktop or message pump becomes unavailable.
    pub fn invalidate(&self) {
        *self.state.lock().unwrap_or_else(|p| p.into_inner()) = State::default();
    }
    /// Permanently revoke a failed listener; late FFI completions cannot revive it.
    pub fn disable(&self) {
        self.disabled.store(true, Ordering::Release);
        self.invalidate();
    }
    /// Apply a fake or native event. Returns true only for physical input.
    pub fn event(&self, event: HookEvent, at: Instant) -> bool {
        if event.injected() {
            return false;
        }
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if state.covered && !self.disabled.load(Ordering::Acquire) {
            state.physical = Some(at);
        }
        true
    }
    /// Read against an injected monotonic instant for deterministic tests.
    pub fn ms_since_at(&self, at: Instant) -> Option<u64> {
        let state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if self.disabled.load(Ordering::Acquire) {
            return None;
        }
        Some(
            at.saturating_duration_since(state.physical?)
                .as_millis()
                .try_into()
                .unwrap_or(u64::MAX),
        )
    }
    /// Listener-generation activity history; disabled listeners return None.
    /// Host consumers use SafetyGuard's accessor to check transport as well.
    pub fn ms_since_physical_input(&self) -> Option<u64> {
        self.ms_since_at(self.time.now())
    }
    /// Whether the last dual-hook injected proof is within the 100ms lease.
    pub fn available(&self) -> bool {
        let state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        !self.disabled.load(Ordering::Acquire)
            && state.confirmed.is_some_and(|at| {
                self.time.now().saturating_duration_since(at) <= HEARTBEAT_TIMEOUT
            })
    }
}
