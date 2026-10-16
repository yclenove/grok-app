//! Host-owned lifecycle. No process is spawned here.
//!
//! The stop latch survives engine replacement. Idle exit and crash recovery are
//! pure transitions so tests can drive them with a fake clock.

use super::policy::{BREAKER_LIMIT, BREAKER_WINDOW_MS, IDLE_MS};

/// Lifecycle phase visible to the App.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// No engine process.
    Stopped,
    /// Spawn or handshake in progress.
    Starting,
    /// Handshake succeeded.
    Running,
    /// Shutdown or kill requested.
    Stopping,
    /// Breaker open. The feature is off until the user disables it.
    Tripped,
}

/// What the process runner should do after [`HostState::apply`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// State changed or stayed; no process action.
    None,
    /// Start the admitted binary.
    Spawn,
    /// Ask a running engine to exit. Not a crash.
    Shutdown,
    /// Drop a process that never finished the handshake, or one the user disabled.
    Kill,
    /// Start again after an unexpected exit. The stop latch is unchanged.
    Restart,
}

/// Signals the runner feeds into [`HostState::apply`]. Time is monotonic milliseconds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Signal {
    /// User or settings toggle.
    SetEnabled(bool),
    /// Handshake finished and the token is not privileged.
    HandshakeOk,
    /// Handshake or spawn failed. Counts toward the breaker. Does not loop.
    HandshakeFailed,
    /// Child left. `clean` is true only when this host asked it to exit.
    ChildExited {
        /// Host-requested shutdown, idle exit, or kill.
        clean: bool,
    },
    /// Enabled local sessions. Zero starts the idle clock.
    Sessions(u32),
    /// Clock tick used for idle exit.
    Tick,
    /// Explicit user stop. Restart must observe the latch.
    UserStop,
    /// Explicit user resume. Crash recovery never sends this.
    UserResume,
    /// Wake the runner without changing policy.
    Ping,
}

/// Host-owned lifecycle. The stop latch survives engine replacement.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostState {
    /// Current phase.
    pub phase: Phase,
    /// Feature switch. The breaker clears this.
    pub enabled: bool,
    /// Persistent user stop. Engine restart must be told.
    pub stop_latched: bool,
    /// Enabled sessions, for idle accounting.
    pub sessions: u32,
    crashes: Vec<u64>,
    empty_since: Option<u64>,
    expect_clean: bool,
    respawn: bool,
}

impl Default for HostState {
    fn default() -> Self {
        Self {
            phase: Phase::Stopped,
            enabled: false,
            stop_latched: false,
            sessions: 0,
            crashes: Vec::new(),
            empty_since: None,
            expect_clean: false,
            respawn: false,
        }
    }
}

impl HostState {
    /// New disabled host. Enabling twice and disabling twice are idempotent.
    pub fn new() -> Self {
        Self::default()
    }

    /// Advance one signal. `now_ms` is monotonic and only compared to itself.
    pub fn apply(&mut self, signal: Signal, now_ms: u64) -> Step {
        match signal {
            Signal::Ping => Step::None,
            Signal::UserStop => {
                self.stop_latched = true;
                Step::None
            }
            Signal::UserResume => {
                self.stop_latched = false;
                Step::None
            }
            Signal::Sessions(count) => self.note_sessions(count, now_ms),
            Signal::Tick => self.tick(now_ms),
            Signal::SetEnabled(false) => self.disable(),
            Signal::SetEnabled(true) => self.enable(),
            Signal::HandshakeOk => {
                if self.phase == Phase::Starting {
                    self.phase = Phase::Running;
                    if self.sessions == 0 {
                        self.empty_since = Some(now_ms);
                    }
                }
                Step::None
            }
            Signal::HandshakeFailed => {
                if self.trip(now_ms) {
                    Step::None
                } else {
                    self.phase = Phase::Stopped;
                    Step::None
                }
            }
            Signal::ChildExited { clean } => self.exited(clean, now_ms),
        }
    }

    fn note_sessions(&mut self, count: u32, now_ms: u64) -> Step {
        self.sessions = count;
        if count == 0 {
            if self.empty_since.is_none() {
                self.empty_since = Some(now_ms);
            }
            return Step::None;
        }
        self.empty_since = None;
        if self.enabled && self.phase == Phase::Stopped {
            self.phase = Phase::Starting;
            self.expect_clean = false;
            Step::Spawn
        } else {
            Step::None
        }
    }

    fn tick(&mut self, now_ms: u64) -> Step {
        let idle = self.phase == Phase::Running
            && self.sessions == 0
            && self
                .empty_since
                .is_some_and(|start| now_ms.saturating_sub(start) >= IDLE_MS);
        if !idle {
            return Step::None;
        }
        self.phase = Phase::Stopping;
        self.expect_clean = true;
        Step::Shutdown
    }

    fn disable(&mut self) -> Step {
        self.enabled = false;
        self.empty_since = None;
        self.respawn = false;
        match self.phase {
            Phase::Tripped => {
                self.phase = Phase::Stopped;
                self.crashes.clear();
                self.expect_clean = false;
                Step::None
            }
            Phase::Stopped | Phase::Stopping => Step::None,
            Phase::Starting => {
                self.phase = Phase::Stopping;
                self.expect_clean = true;
                Step::Kill
            }
            Phase::Running => {
                self.phase = Phase::Stopping;
                self.expect_clean = true;
                Step::Shutdown
            }
        }
    }

    fn enable(&mut self) -> Step {
        if self.phase == Phase::Tripped {
            return Step::None;
        }
        if matches!(self.phase, Phase::Running | Phase::Starting) {
            self.enabled = true;
            return Step::None;
        }
        if self.phase == Phase::Stopping {
            self.enabled = true;
            self.respawn = true;
            return Step::None;
        }
        self.enabled = true;
        self.phase = Phase::Starting;
        self.expect_clean = false;
        Step::Spawn
    }

    fn exited(&mut self, clean: bool, now_ms: u64) -> Step {
        if clean || self.expect_clean {
            let again = self.respawn && self.enabled;
            self.expect_clean = false;
            self.respawn = false;
            if again {
                self.phase = Phase::Starting;
                return Step::Spawn;
            }
            self.phase = Phase::Stopped;
            return Step::None;
        }
        if self.trip(now_ms) {
            return Step::None;
        }
        if self.enabled && self.phase != Phase::Stopping {
            self.phase = Phase::Starting;
            self.expect_clean = false;
            return Step::Restart;
        }
        self.phase = Phase::Stopped;
        Step::None
    }

    fn trip(&mut self, now_ms: u64) -> bool {
        self.crashes
            .retain(|at| now_ms.saturating_sub(*at) < BREAKER_WINDOW_MS);
        self.crashes.push(now_ms);
        if self.crashes.len() >= BREAKER_LIMIT {
            self.phase = Phase::Tripped;
            self.enabled = false;
            self.expect_clean = false;
            self.respawn = false;
            true
        } else {
            false
        }
    }
}
