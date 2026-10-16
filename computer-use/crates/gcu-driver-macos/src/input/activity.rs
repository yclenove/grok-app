//! Listen-only input accounting. Unmarked input is conservatively user activity.

use super::clock::Clock;
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

/// `kCGEventSourceUserData` marker written on every event, including releases.
pub const OWN_EVENT_TAG: i64 = 0x4743_5532_494e_5054;
/// An unresponsive event loop loses its safety capability after 100ms.
pub const LISTENER_DEADLINE: Duration = Duration::from_millis(100);

/// Non-blocking input/Stop health source, injectable by a Host or test.
pub trait ActivityClock: Send + Sync {
    /// Unknown, unavailable or stale monitoring is `None`, never an idle guess.
    fn ms_since_physical_input(&self) -> Option<u64>;
    /// Registration succeeded and the listener is healthy.
    fn stop_available(&self) -> bool;
}

#[derive(Default)]
struct State {
    connected: bool,
    hotkey: bool,
    heartbeat: Duration,
    last_input: Option<Duration>,
}

/// Clock and registration state fed by the macOS event-tap thread.
pub struct ActivityMonitor {
    clock: Arc<dyn Clock>,
    state: Mutex<State>,
}

impl ActivityMonitor {
    /// Initially disconnected and with no physical input history.
    pub fn new(clock: Arc<dyn Clock>) -> Self {
        Self {
            clock,
            state: Mutex::new(State::default()),
        }
    }
    /// Attach/detach a listener. Reattachment discards stale input history and
    /// requires an explicit successful hotkey registration before enabling Stop.
    pub fn connected(&self, available: bool) {
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        state.connected = available;
        state.hotkey = false;
        state.last_input = None;
        state.heartbeat = self.clock.now();
    }
    /// Record Carbon registration success or failure independently of activity.
    pub fn hotkey_registered(&self, registered: bool) {
        self.state.lock().unwrap_or_else(|p| p.into_inner()).hotkey = registered;
    }
    /// Event loop heartbeat. A disconnected listener cannot revive itself.
    pub fn heartbeat(&self) {
        self.state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .heartbeat = self.clock.now();
    }
    /// Observe input, filtering our own marker. External synthetic input counts
    /// as activity too: it must not make the desktop appear safer to take over.
    pub fn observe(&self, source_user_data: i64) {
        if source_user_data == OWN_EVENT_TAG {
            return;
        }
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if state.connected {
            state.last_input = Some(self.clock.now());
        }
    }
    /// Listener is connected and has not missed its deadline.
    pub fn is_available(&self) -> bool {
        let state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        state.connected && self.clock.now().saturating_sub(state.heartbeat) <= LISTENER_DEADLINE
    }
}

impl ActivityClock for ActivityMonitor {
    fn ms_since_physical_input(&self) -> Option<u64> {
        let state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        let now = self.clock.now();
        if !state.connected || now.saturating_sub(state.heartbeat) > LISTENER_DEADLINE {
            return None;
        }
        let elapsed = now.saturating_sub(state.last_input?);
        Some(u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX))
    }
    fn stop_available(&self) -> bool {
        let state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        state.connected
            && state.hotkey
            && self.clock.now().saturating_sub(state.heartbeat) <= LISTENER_DEADLINE
    }
}

pub(crate) struct IdleFixture;
impl ActivityClock for IdleFixture {
    fn ms_since_physical_input(&self) -> Option<u64> {
        Some(3000)
    }
    fn stop_available(&self) -> bool {
        true
    }
}
