//! Bounded settling shared by all drivers. No input is delivered or replayed.
//! Event quiet is validated against complete, non-busy raw trees. Missing/closed
//! subscriptions use polling; continuous events cannot starve deadlines or Stop.
mod clock;
pub(crate) mod events;
pub(crate) mod poll;
pub use clock::{Clock, RuntimeClock};

use crate::{
    driver::{BoxStream, Driver, TreeOpts, UiEvent, WindowRef},
    model::RawTree,
    session::Session,
};
use gcu_protocol::{
    result::{Effect, Settle, SettleReason},
    ErrorCode,
};
use std::{sync::Arc, time::Duration};

const STOP_POLL: Duration = Duration::from_millis(10);
const MAX_BUDGET: Duration = Duration::from_millis(1500);

/// Runtime tuning, initially identical across platforms pending native benchmarks.
#[derive(Clone, Debug)]
pub struct SettleConfig {
    /// Silence required after an event or a detected tree change (default 120ms).
    pub event_quiet: Duration,
    /// Interval between tree reads without notifications (default 150ms).
    pub poll_interval: Duration,
    /// Default action horizon (800ms); overrides can only shorten the 1500ms cap.
    pub action_budget: Duration,
    /// Default type_text horizon (1500ms), subject to the same cap.
    pub text_budget: Duration,
}
impl Default for SettleConfig {
    fn default() -> Self {
        Self {
            event_quiet: Duration::from_millis(120),
            poll_interval: Duration::from_millis(150),
            action_budget: Duration::from_millis(800),
            text_budget: MAX_BUDGET,
        }
    }
}
impl SettleConfig {
    fn bounded(mut self) -> Self {
        let bounded = |d: Duration| d.clamp(Duration::from_millis(1), MAX_BUDGET);
        self.event_quiet = bounded(self.event_quiet);
        self.poll_interval = bounded(self.poll_interval);
        self.action_budget = self.action_budget.min(MAX_BUDGET);
        self.text_budget = self.text_budget.min(MAX_BUDGET);
        self
    }
}

/// One authorized post-delivery observation. Subscription ownership ends on return
/// or cancellation. `before` must be the exact pre-delivery raw state.
pub struct SettleRequest<'a> {
    /// Exact driver; only snapshot_tree is called.
    pub driver: &'a dyn Driver,
    /// Session whose Stop, close and turn boundary cancel this wait.
    pub session: &'a Session,
    /// Exact authorized window; never substituted after WindowGone.
    pub window: &'a WindowRef,
    /// Tree read limits, further shortened to the remaining budget/driver ceiling.
    pub tree_opts: &'a TreeOpts,
    /// Subscription established before delivery, or None for polling fallback.
    pub events: Option<BoxStream<'static, UiEvent>>,
    /// Complete pre-delivery state when available; never logged or returned here.
    pub before: &'a RawTree,
    /// Hard horizon, capped at 1500ms; zero performs no reads.
    pub budget: Duration,
}

/// In-memory evidence plus unchanged protocol metadata; contains no UI payloads.
#[derive(Debug)]
pub struct SettleReport {
    /// Returned in structuredContent.action.settle (ms and reason).
    pub settle: Settle,
    /// Any complete raw sample differed, even if the final tree reverted.
    pub observed_change: bool,
}
impl SettleReport {
    /// Whether stability was unconfirmed at the deadline. The existing wire
    /// representation is `settle.reason == "budget"`; no schema extension.
    pub fn timed_out(&self) -> bool {
        self.settle.reason == SettleReason::Budget
    }

    /// Compare full raw observations and redacted pixels, never truncated text.
    /// `pixels` is Some(changed) only when both safe images are comparable.
    /// Missing evidence, incomplete/busy trees or timeout cannot establish absence
    /// of change. Pure memory work; no waits, cancellation, logs or replay.
    pub fn effect(
        &self,
        before: &RawTree,
        after: &RawTree,
        pixels: Option<bool>,
    ) -> Result<Effect, ErrorCode> {
        let complete = poll::usable(before) && poll::usable(after);
        if self.observed_change
            || pixels == Some(true)
            || (complete && poll::hash(before)? != poll::hash(after)?)
        {
            Ok(Effect::Changed)
        } else if self.timed_out() || !complete || pixels.is_none() {
            Ok(Effect::NotChecked)
        } else {
            Ok(Effect::NoVisibleChange)
        }
    }
}

/// Settling service with a single injectable clock. Construction does no IO.
pub struct SettleEngine {
    config: SettleConfig,
    clock: Arc<dyn Clock>,
}
impl Default for SettleEngine {
    fn default() -> Self {
        Self::new(SettleConfig::default(), Arc::new(RuntimeClock::default()))
    }
}
impl SettleEngine {
    /// Construct in memory; zero intervals are raised to 1ms to prevent spinning.
    /// No waits, IO or cancellation. All budgets retain the existing 1500ms cap.
    pub fn new(config: SettleConfig, clock: Arc<dyn Clock>) -> Self {
        Self {
            config: config.bounded(),
            clock,
        }
    }
    /// Validated tuning. Nonblocking; no timeout or cancellation.
    pub fn config(&self) -> &SettleConfig {
        &self.config
    }

    /// Clock shared with `wait_for`. Nonblocking; no timeout or cancellation.
    pub fn clock(&self) -> Arc<dyn Clock> {
        self.clock.clone()
    }

    /// Observe until event quiet, two equal usable trees, or the hard budget.
    /// Unchanged early samples retain the full horizon to catch delayed activation
    /// and silent effects. At the horizon they count as stable only if fresh,
    /// complete stable evidence exists and no read is pending; otherwise Budget.
    /// Stop/turn changes are checked every 10ms; close wakes immediately. Dropping
    /// this future cancels the native token before dropping its read/stream.
    /// Stop issues Session's independently bounded input cleanup after cancellation.
    pub async fn wait(&self, request: SettleRequest<'_>) -> Result<SettleReport, ErrorCode> {
        let session = request.session;
        let result = self.run(request).await;
        if matches!(result, Err(ErrorCode::StoppedByUser)) {
            session.handle_stop().await;
        }
        result
    }

    async fn run(&self, request: SettleRequest<'_>) -> Result<SettleReport, ErrorCode> {
        let start = self.clock.now();
        let deadline = start + request.budget.min(MAX_BUDGET);
        let generation = request.session.turn_generation();
        let mut events = events::Events::new(request.events, start, self.config.event_quiet);
        let mut evidence = poll::Evidence::new(request.before)?;
        let mut read: Option<poll::Read<'_>> = None;
        let mut next_poll = start;
        let mut next_control = start + STOP_POLL;
        loop {
            let now = self.clock.now();
            if request.session.is_stopped() || request.session.activity().stop_requested() {
                return Err(ErrorCode::StoppedByUser);
            }
            if request.session.is_closed() || request.session.turn_generation() != generation {
                return Err(ErrorCode::ApprovalDenied);
            }

            // Inspect queued events before any confirmation, without waiting
            // for an event or draining an unbounded burst.
            let may_finish = now >= deadline
                || (evidence.changed
                    && read.is_none()
                    && evidence.confirmed(now, self.config.poll_interval));
            if may_finish && events.active() && now >= events.poll_at {
                let batch = std::future::poll_fn(|cx| {
                    let next = events.next();
                    tokio::pin!(next);
                    match std::future::Future::poll(next.as_mut(), cx) {
                        std::task::Poll::Ready(batch) => std::task::Poll::Ready(Some(batch)),
                        std::task::Poll::Pending => std::task::Poll::Ready(None),
                    }
                })
                .await;
                if let Some(batch) = batch {
                    if batch.gone {
                        return Err(ErrorCode::WindowGone);
                    }
                    if batch.changed {
                        evidence.invalidate();
                    }
                    events.received(&batch, now);
                }
            }
            let confirmed = read.is_none() && evidence.confirmed(now, self.config.poll_interval);
            if now >= deadline || (evidence.changed && confirmed) {
                let reason = if confirmed {
                    if events.active() && now >= events.quiet_at {
                        SettleReason::EventQuiet
                    } else {
                        SettleReason::TreeStable
                    }
                } else {
                    SettleReason::Budget
                };
                return Ok(SettleReport {
                    settle: Settle {
                        ms: u64::try_from(now.saturating_sub(start).as_millis())
                            .unwrap_or(u64::MAX),
                        reason,
                    },
                    observed_change: evidence.changed,
                });
            }
            if read.as_ref().is_some_and(|pending| now >= pending.deadline) {
                return Err(ErrorCode::Timeout);
            }
            if now >= next_control {
                next_control = now + STOP_POLL;
            }
            if read.is_none() && (now >= next_poll || (events.active() && now >= events.check_at)) {
                if now >= events.check_at {
                    events.check_at = now + self.config.poll_interval;
                }
                read = Some(poll::Read::new(
                    request.driver,
                    request.window,
                    request.tree_opts.clone(),
                    now,
                    deadline,
                ));
            }
            let mut wake_at = deadline.min(next_control);
            if let Some(pending) = &read {
                wake_at = wake_at.min(pending.deadline);
            } else {
                wake_at = wake_at.min(next_poll);
                if events.active() {
                    wake_at = wake_at.min(events.check_at);
                }
            }
            if events.active() && events.poll_at > now {
                wake_at = wake_at.min(events.poll_at);
            }
            enum Wake {
                Timer,
                Close,
                Read(crate::driver::Result<Box<RawTree>>),
                Events(events::Batch),
            }
            let poll_events = now >= events.poll_at;
            let wake = tokio::select! {
                biased;
                _ = request.session.cancellation().cancelled() => Wake::Close,
                _ = self.clock.wait_until(wake_at) => Wake::Timer,
                batch = events.next(), if poll_events => Wake::Events(batch),
                raw = async { match &mut read { Some(read) => read.work.as_mut().await, None => std::future::pending().await } } => Wake::Read(raw.map(Box::new)),
            };
            match wake {
                Wake::Close => return Err(ErrorCode::ApprovalDenied),
                Wake::Timer => {}
                Wake::Events(batch) => {
                    if batch.gone {
                        return Err(ErrorCode::WindowGone);
                    }
                    if batch.changed {
                        evidence.invalidate();
                    }
                    events.received(&batch, self.clock.now());
                }
                Wake::Read(raw) => {
                    // Drop the token owner before processing any read/error.
                    read = None;
                    let now = self.clock.now();
                    let raw = match raw {
                        Ok(raw) => raw,
                        Err(crate::driver::DriverError::Timeout { .. }) if now >= deadline => {
                            continue
                        }
                        Err(error) => return Err(error.error_code()),
                    };
                    if evidence.observe(&raw, now)? {
                        events.invalidate(now);
                    }
                    next_poll = now + self.config.poll_interval;
                }
            }
        }
    }
}
