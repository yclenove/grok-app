//! Physical input clock. Health is a lease, never an old last-input fallback.
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
    confirmed: Option<Instant>,
    physical: Option<Instant>,
}
/// Monotonic physical-input clock with a bounded hook/message-pump health lease.
/// First healthy observation starts an idle interval conservatively at zero;
/// losing health discards history. No GetLastInputInfo (injected input included).
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
        if !state
            .confirmed
            .is_some_and(|last| at.saturating_duration_since(last) <= HEARTBEAT_TIMEOUT)
        {
            state.physical = Some(at);
        }
        state.confirmed = Some(at);
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
        self.state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .physical = Some(at);
        true
    }
    /// Read against an injected monotonic instant for deterministic tests.
    pub fn ms_since_at(&self, at: Instant) -> Option<u64> {
        if self.disabled.load(Ordering::Acquire) {
            return None;
        }
        let state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        let confirmed = state.confirmed?;
        if at.saturating_duration_since(confirmed) > HEARTBEAT_TIMEOUT {
            return None;
        }
        Some(
            at.saturating_duration_since(state.physical?)
                .as_millis()
                .try_into()
                .unwrap_or(u64::MAX),
        )
    }
    /// Host activity adapter input. Unhealthy listeners always return None.
    pub fn ms_since_physical_input(&self) -> Option<u64> {
        self.ms_since_at(self.time.now())
    }
    /// Whether both hooks and the message pump are currently confirmed healthy.
    pub fn available(&self) -> bool {
        self.ms_since_physical_input().is_some()
    }
}
