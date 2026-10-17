//! `wait_for`: event subscription with a 150ms poll fallback.
//!
//! The loop uses the settle clock and event drain. It never calls [`crate::driver::Driver::perform`].
//! Each tree read is capped by [`crate::driver::DRIVER_DEADLINE`] and the time remaining on the
//! wait horizon (default 5s, maximum 30s). [`crate::driver::NATIVE_TIMEOUT`] is unchanged.
//! Batch and script reuse [`crate::action::ActionRouter::wait_for`] so authorization, Stop, and
//! cancellation stay on the router. [`WaitEngine::wait`] is the shared loop that entry runs.

mod condition;
mod deadline;

pub(crate) use condition::supported;
pub use deadline::{Deadline, DEFAULT_TIMEOUT, MAX_TIMEOUT, POLL_INTERVAL};

use crate::{
    driver::{
        BoxStream, Driver, DriverError, TreeOpts, UiEvent, WindowRef, DRIVER_DEADLINE,
        NATIVE_TIMEOUT,
    },
    model::RawTree,
    session::Session,
    settle::{self, events, poll, Clock},
};
use gcu_protocol::{tools::Condition, ErrorCode, WindowInfo};
use std::{fmt, sync::Arc, time::Duration};

/// Lifecycle of one wait. Terminal phases stay terminal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WaitPhase {
    /// Condition is not yet true and the deadline remains.
    Waiting,
    /// Condition matched. The sample is the new state.
    Ready,
    /// The monotonic deadline elapsed. The sample is the last observation.
    TimedOut,
    /// Stop, session close, or turn end. No state is returned.
    Cancelled,
}

impl WaitPhase {
    /// Stable name: `ready`, `waiting`, `timed_out`, or `cancelled`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Waiting => "waiting",
            Self::Ready => "ready",
            Self::TimedOut => "timed_out",
            Self::Cancelled => "cancelled",
        }
    }
}

/// Input to [`transition`]. Pure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WaitSignal {
    /// Another sample is required.
    StillWaiting,
    /// The latest sample matches.
    ConditionMet,
    /// The horizon was reached.
    Deadline,
    /// Stop, close, or turn end.
    Cancel,
}

/// Advance the state machine. A terminal phase ignores later signals.
///
/// Blocking: none. Timeout: none. Cancellation: none.
pub fn transition(phase: WaitPhase, signal: WaitSignal) -> WaitPhase {
    if phase != WaitPhase::Waiting {
        return phase;
    }
    match signal {
        WaitSignal::StillWaiting => WaitPhase::Waiting,
        WaitSignal::ConditionMet => WaitPhase::Ready,
        WaitSignal::Deadline => WaitPhase::TimedOut,
        WaitSignal::Cancel => WaitPhase::Cancelled,
    }
}

/// One authorized wait. The caller has already checked observation policy.
pub struct WaitRequest<'a> {
    /// Driver used only for snapshots and window listing.
    pub driver: &'a dyn Driver,
    /// Stop, close, and turn generation cancel the wait.
    pub session: &'a Session,
    /// Exact window. It is never replaced after `window_gone`.
    pub window: &'a WindowRef,
    /// Tree read limits. Each read is further capped by the native deadline.
    pub tree_opts: &'a TreeOpts,
    /// Subscription taken before the loop, or `None` to poll.
    pub events: Option<BoxStream<'static, UiEvent>>,
    /// Condition to observe.
    pub until: Condition,
    /// Horizon, `1ms..=30s`. This is not wrapped in the 5s native timeout.
    pub timeout: Duration,
}

/// Result of a finished wait. Payloads are omitted from [`Debug`].
pub struct WaitReport {
    /// Terminal phase.
    pub phase: WaitPhase,
    /// Protocol error for timeout and cancellation. `None` when ready.
    pub code: Option<ErrorCode>,
    /// Monotonic time from the start of this wait.
    pub elapsed: Duration,
    /// Last or matching raw tree. Absent when cancelled before a sample.
    pub raw: Option<RawTree>,
    /// Window info for `raw`, including the title that was matched.
    pub info: Option<WindowInfo>,
}

impl fmt::Debug for WaitReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WaitReport")
            .field("phase", &self.phase.as_str())
            .field("code", &self.code)
            .field("elapsed", &self.elapsed)
            .finish_non_exhaustive()
    }
}

impl WaitReport {
    fn ready(raw: RawTree, info: WindowInfo, elapsed: Duration) -> Self {
        Self {
            phase: transition(WaitPhase::Waiting, WaitSignal::ConditionMet),
            code: None,
            elapsed,
            raw: Some(raw),
            info: Some(info),
        }
    }

    fn timed_out(raw: Option<RawTree>, info: Option<WindowInfo>, elapsed: Duration) -> Self {
        Self {
            phase: transition(WaitPhase::Waiting, WaitSignal::Deadline),
            code: Some(ErrorCode::Timeout),
            elapsed,
            raw,
            info,
        }
    }

    fn cancelled(code: ErrorCode, elapsed: Duration) -> Self {
        Self {
            phase: transition(WaitPhase::Waiting, WaitSignal::Cancel),
            code: Some(code),
            elapsed,
            raw: None,
            info: None,
        }
    }
}

struct Held {
    raw: RawTree,
    info: WindowInfo,
}

/// Observation loop with one injected clock. Construction does no IO.
pub struct WaitEngine {
    clock: Arc<dyn Clock>,
}

impl Default for WaitEngine {
    fn default() -> Self {
        Self::new(Arc::new(settle::RuntimeClock::default()))
    }
}

impl WaitEngine {
    /// Share `clock` with settle. Nonblocking; no IO or cancellation.
    pub fn new(clock: Arc<dyn Clock>) -> Self {
        Self { clock }
    }

    /// Wait until the condition, the deadline, Stop, or session cancellation.
    ///
    /// The first sample is immediate. Later samples follow an event (yielding so a
    /// flood cannot spin) or [`POLL_INTERVAL`] when no subscription is active.
    /// Stop is observed at least every 10ms of this clock and calls
    /// [`Session::handle_stop`]. Dropping the future cancels an in-flight tree read.
    /// No input is delivered.
    ///
    /// Blocking: until a terminal phase, on `clock`. Timeout: `request.timeout`
    /// (`1ms..=30s`); each snapshot additionally stops at [`DRIVER_DEADLINE`].
    /// Cancellation: session close, turn end, Stop, or drop.
    pub async fn wait(&self, request: WaitRequest<'_>) -> Result<WaitReport, ErrorCode> {
        condition::supported(&request.until)?;
        let WaitRequest {
            driver,
            session,
            window,
            tree_opts,
            events,
            until,
            timeout,
        } = request;
        let start_at = self.clock.now();
        let deadline = Deadline::try_from_timeout(start_at, timeout)?;
        let generation = session.turn_generation();
        let mut events = events::Events::new(events, start_at, POLL_INTERVAL);
        let mut stability = condition::Stability::default();
        let mut held: Option<Held> = None;
        let mut read: Option<poll::Read<'_>> = None;
        let mut next_poll = start_at;
        let mut next_control = start_at;
        // Set when an event arrives during an in-flight read. That sample is kept
        // as the last state but cannot satisfy the condition.
        let mut dirty = false;

        loop {
            let now = self.clock.now();
            if let Some(code) = Self::boundary(session, generation) {
                drop(read.take());
                if code == ErrorCode::StoppedByUser {
                    session.handle_stop().await;
                }
                return Ok(WaitReport::cancelled(code, deadline.elapsed(now)));
            }
            if deadline.expired(now) {
                drop(read.take());
                let (raw, info) = split(held);
                return Ok(WaitReport::timed_out(raw, info, deadline.elapsed(now)));
            }
            if read.as_ref().is_some_and(|pending| now >= pending.deadline) {
                read = None;
                next_poll = now + POLL_INTERVAL;
            }
            if read.is_none() && now >= next_poll {
                if events.active() && now >= events.poll_at {
                    if let Some(batch) = events.try_ready().await {
                        if let Some(report) = self.on_event(
                            &mut events,
                            &mut stability,
                            &mut next_poll,
                            &mut dirty,
                            batch,
                        ) {
                            return report;
                        }
                    }
                }
                if self.clock.now() >= next_poll && !deadline.expired(self.clock.now()) {
                    let now = self.clock.now();
                    dirty = false;
                    read = Some(poll::Read::new(
                        driver,
                        window,
                        tree_opts.clone(),
                        now,
                        deadline.end(),
                    ));
                }
                continue;
            }
            if now >= next_control {
                next_control = now + deadline::CONTROL_INTERVAL;
            }
            let mut wake_at = deadline.end().min(next_control);
            if let Some(pending) = &read {
                wake_at = wake_at.min(pending.deadline);
            } else {
                wake_at = wake_at.min(next_poll);
            }
            // A due event stream is selected below. Folding `now` into the timer
            // would make the biased timer win every iteration and spin.
            if events.active() && events.poll_at > now {
                wake_at = wake_at.min(events.poll_at);
            }
            let poll_events = events.active() && now >= events.poll_at;
            enum Wake {
                Timer,
                Close,
                Read(Box<crate::driver::Result<RawTree>>),
                Events(events::Batch),
            }
            let wake = tokio::select! {
                biased;
                _ = session.cancellation().cancelled() => Wake::Close,
                _ = self.clock.wait_until(wake_at) => Wake::Timer,
                batch = events.next(), if poll_events => Wake::Events(batch),
                raw = async { match &mut read { Some(read) => read.work.as_mut().await, None => std::future::pending().await } } => Wake::Read(Box::new(raw)),
            };
            match wake {
                Wake::Close => {
                    drop(read.take());
                    return Ok(WaitReport::cancelled(
                        ErrorCode::ApprovalDenied,
                        deadline.elapsed(self.clock.now()),
                    ));
                }
                Wake::Timer => {}
                Wake::Events(batch) => {
                    if let Some(report) = self.on_event(
                        &mut events,
                        &mut stability,
                        &mut next_poll,
                        &mut dirty,
                        batch,
                    ) {
                        return report;
                    }
                }
                Wake::Read(result) => {
                    read = None;
                    let now = self.clock.now();
                    match *result {
                        Ok(raw) => match self.window_info(driver, window, &deadline).await {
                            Ok(info) => {
                                let matched = !dirty
                                    && condition::met(&until, &raw, &info.title, &mut stability)?;
                                if matched {
                                    return Ok(WaitReport::ready(raw, info, deadline.elapsed(now)));
                                }
                                held = Some(Held { raw, info });
                                next_poll = if dirty { now } else { now + POLL_INTERVAL };
                                dirty = false;
                            }
                            Err(ErrorCode::WindowGone) => return Err(ErrorCode::WindowGone),
                            Err(ErrorCode::Timeout) if deadline.expired(self.clock.now()) => {
                                let (raw, info) = split(held);
                                return Ok(WaitReport::timed_out(
                                    raw,
                                    info,
                                    deadline.elapsed(self.clock.now()),
                                ));
                            }
                            Err(code) => return Err(code),
                        },
                        Err(DriverError::WindowGone) => return Err(ErrorCode::WindowGone),
                        Err(DriverError::Cancelled { .. }) => {
                            let code = Self::boundary(session, generation)
                                .unwrap_or(ErrorCode::ApprovalDenied);
                            if code == ErrorCode::StoppedByUser {
                                session.handle_stop().await;
                            }
                            return Ok(WaitReport::cancelled(code, deadline.elapsed(now)));
                        }
                        Err(DriverError::Timeout { .. }) if deadline.expired(now) => {
                            let (raw, info) = split(held);
                            return Ok(WaitReport::timed_out(raw, info, deadline.elapsed(now)));
                        }
                        Err(DriverError::Timeout { .. }) => {
                            next_poll = now + POLL_INTERVAL;
                        }
                        Err(error) => return Err(error.error_code()),
                    }
                }
            }
        }
    }

    fn boundary(session: &Session, generation: u64) -> Option<ErrorCode> {
        if session.is_stopped() || session.activity().stop_requested() {
            Some(ErrorCode::StoppedByUser)
        } else if session.is_closed() || session.turn_generation() != generation {
            Some(ErrorCode::ApprovalDenied)
        } else {
            None
        }
    }

    fn on_event(
        &self,
        events: &mut events::Events,
        stability: &mut condition::Stability,
        next_poll: &mut Duration,
        dirty: &mut bool,
        batch: events::Batch,
    ) -> Option<Result<WaitReport, ErrorCode>> {
        if batch.gone {
            return Some(Err(ErrorCode::WindowGone));
        }
        if batch.changed {
            stability.invalidate();
            *dirty = true;
            *next_poll = self.clock.now();
        }
        events.received(&batch, self.clock.now());
        None
    }

    async fn window_info(
        &self,
        driver: &dyn Driver,
        window: &WindowRef,
        deadline: &Deadline,
    ) -> Result<WindowInfo, ErrorCode> {
        let now = self.clock.now();
        let limit = NATIVE_TIMEOUT
            .min(DRIVER_DEADLINE)
            .min(deadline.remaining(now));
        if limit.is_zero() {
            return Err(ErrorCode::Timeout);
        }
        let listed = tokio::time::timeout(limit, driver.list_windows(Some(&window.app))).await;
        match listed {
            Ok(Ok(windows)) => windows
                .into_iter()
                .find(|info| info.id == window.id)
                .ok_or(ErrorCode::WindowGone),
            Ok(Err(error)) => Err(error.error_code()),
            Err(_) => Err(ErrorCode::Timeout),
        }
    }
}

fn split(held: Option<Held>) -> (Option<RawTree>, Option<WindowInfo>) {
    match held {
        Some(held) => (Some(held.raw), Some(held.info)),
        None => (None, None),
    }
}
