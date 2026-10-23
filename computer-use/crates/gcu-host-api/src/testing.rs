//! In-memory Host doubles for engine tests.
//!
//! Enabled with the `testing` feature, which is off by default. These types
//! are not production Hosts. [`MemoryPolicyStore`] can insert grants so a test
//! can simulate the Host writing one. That method is not on [`PolicyStore`].

use crate::activity::UserActivity;
use crate::approval::{ApprovalDecision, ApprovalProvider, Cancel, HandoffReason};
use crate::events::{CuEvent, EventSink};
use crate::overlay::{CursorKind, OverlayProvider, StatusText};
use crate::policy::{PolicyConfig, PolicyStore};
use crate::types::{
    ActionSummary, AppInfo, ImageRef, ModelReason, Risk, ScreenPoint, SessionCtx, WindowInfo,
};
use async_trait::async_trait;
use std::collections::VecDeque;
use std::future::Future;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Wake, Waker};
use std::thread::Thread;
use std::time::Instant;

/// Overlay that draws nothing and cannot exclude itself from capture.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoopOverlay;

impl OverlayProvider for NoopOverlay {
    fn show_target(
        &self,
        _session: &SessionCtx,
        _window: &WindowInfo,
        _at: ScreenPoint,
        _kind: CursorKind,
    ) {
    }

    fn status(&self, _session: &SessionCtx, _text: &StatusText) {}

    fn hide(&self, _session: &SessionCtx) {}

    fn exclude_from_capture_supported(&self) -> bool {
        false
    }
}

/// Policy snapshot the engine can read. Tests mutate it through inherent methods.
///
/// `insert_always` and `remove_always` stand in for the Host UI. They are not
/// [`PolicyStore`] methods, and a `dyn PolicyStore` cannot call them.
/// `stage` plus [`PolicyStore::refresh`] stands in for `engine.policy.changed`.
#[derive(Debug)]
pub struct MemoryPolicyStore {
    inner: Mutex<MemoryPolicy>,
}

#[derive(Debug)]
struct MemoryPolicy {
    active: PolicyConfig,
    staged: Option<PolicyConfig>,
}

impl MemoryPolicyStore {
    /// Start from `config`. Grants in `config.always` are normalized.
    ///
    /// Blocking: none. Timeout: none. Cancellation: none. Thread safety: the
    /// store is `Send + Sync` after this returns.
    pub fn new(mut config: PolicyConfig) -> Self {
        config.normalize_grants();
        Self {
            inner: Mutex::new(MemoryPolicy {
                active: config,
                staged: None,
            }),
        }
    }

    /// Record a grant the way the Host would when the user picks Always.
    ///
    /// A later identity that [`crate::AppIdentity::grant_eq`] this one is not
    /// stored twice. Visible to readers immediately, without [`PolicyStore::refresh`].
    ///
    /// Blocking: none. Timeout: none. Cancellation: none. Thread safety: locks internally.
    pub fn insert_always(&self, identity: crate::AppIdentity) {
        let mut guard = crate::lock(&self.inner);
        if !guard
            .active
            .always
            .iter()
            .any(|existing| existing.grant_eq(&identity))
        {
            guard.active.always.push(identity);
        }
    }

    /// Drop every stored grant that [`crate::AppIdentity::grant_eq`] `identity`.
    ///
    /// This simulates the user revoking a grant in Host settings.
    ///
    /// Blocking: none. Timeout: none. Cancellation: none. Thread safety: locks internally.
    pub fn remove_always(&self, identity: &crate::AppIdentity) {
        let mut guard = crate::lock(&self.inner);
        guard
            .active
            .always
            .retain(|existing| !existing.grant_eq(identity));
    }

    /// Replace the snapshot the next [`PolicyStore::refresh`] publishes.
    ///
    /// Does not change [`PolicyStore::load`] until `refresh`. The staged value
    /// is consumed by one refresh.
    ///
    /// Blocking: none. Timeout: none. Cancellation: none. Thread safety: locks internally.
    pub fn stage(&self, mut config: PolicyConfig) {
        config.normalize_grants();
        crate::lock(&self.inner).staged = Some(config);
    }
}

impl Default for MemoryPolicyStore {
    fn default() -> Self {
        Self::new(PolicyConfig::default())
    }
}

impl PolicyStore for MemoryPolicyStore {
    fn load(&self) -> PolicyConfig {
        crate::lock(&self.inner).active.clone()
    }

    fn always_allowed(&self, id: &crate::AppIdentity) -> bool {
        crate::lock(&self.inner)
            .active
            .always
            .iter()
            .any(|existing| existing.grant_eq(id))
    }

    fn refresh(&self) {
        let mut guard = crate::lock(&self.inner);
        if let Some(config) = guard.staged.take() {
            guard.active = config;
        }
    }
}

/// One `request_app` call observed by [`ScriptedApprovals`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedAppRequest {
    /// Session passed by the engine.
    pub session: SessionCtx,
    /// App passed by the engine, including its OS identity and icon bytes.
    pub app: AppInfo,
    /// Model reason, when the engine sent one.
    pub reason: Option<ModelReason>,
    /// Risk passed by the engine.
    pub risk: Risk,
}

/// One `confirm_action` call observed by [`ScriptedApprovals`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedConfirm {
    /// Session passed by the engine.
    pub session: SessionCtx,
    /// App passed by the engine.
    pub app: AppInfo,
    /// Summary the Host would bind the dialog to.
    pub summary: ActionSummary,
    /// Crop the engine passed, still in memory.
    pub crop: Option<ImageRef>,
}

/// One `handoff` call observed by [`ScriptedApprovals`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedHandoff {
    /// Session passed by the engine.
    pub session: SessionCtx,
    /// App passed by the engine.
    pub app: AppInfo,
    /// Engine-produced handoff reason.
    pub reason: HandoffReason,
}

/// Approval Host that returns queued decisions and records every call.
///
/// An empty decision queue waits until [`ScriptedApprovals::push_decision`] or
/// until `cancel` fires. Cancellation returns [`ApprovalDecision::Deny`] for
/// `request_app` and `false` for `confirm_action`, and it does not consume a
/// queued answer. Use one in-flight call per value; the queues are shared.
///
/// `handoff` records the call and returns. It does not wait.
#[derive(Debug, Default)]
pub struct ScriptedApprovals {
    inner: Mutex<ScriptState>,
}

#[derive(Debug, Default)]
struct ScriptState {
    decisions: VecDeque<ApprovalDecision>,
    confirms: VecDeque<bool>,
    app_requests: Vec<RecordedAppRequest>,
    confirm_requests: Vec<RecordedConfirm>,
    handoffs: Vec<RecordedHandoff>,
    wakers: Vec<Waker>,
}

impl ScriptedApprovals {
    /// Empty queues.
    ///
    /// Blocking: none. Timeout: none. Cancellation: none. Thread safety: the
    /// value is `Send + Sync`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Queue an app decision. Wakes one waiting `request_app`.
    ///
    /// Blocking: none. Timeout: none. Cancellation: none. Thread safety: locks internally.
    pub fn push_decision(&self, decision: ApprovalDecision) {
        crate::lock(&self.inner).decisions.push_back(decision);
        self.wake();
    }

    /// Queue a confirm answer. Wakes one waiting `confirm_action`.
    ///
    /// Blocking: none. Timeout: none. Cancellation: none. Thread safety: locks internally.
    pub fn push_confirm(&self, confirmed: bool) {
        crate::lock(&self.inner).confirms.push_back(confirmed);
        self.wake();
    }

    /// Calls to `request_app`, in order.
    ///
    /// Blocking: none. Timeout: none. Cancellation: none. Thread safety: locks internally.
    pub fn app_requests(&self) -> Vec<RecordedAppRequest> {
        crate::lock(&self.inner).app_requests.clone()
    }

    /// Calls to `confirm_action`, in order.
    ///
    /// Blocking: none. Timeout: none. Cancellation: none. Thread safety: locks internally.
    pub fn confirm_requests(&self) -> Vec<RecordedConfirm> {
        crate::lock(&self.inner).confirm_requests.clone()
    }

    /// Calls to `handoff`, in order.
    ///
    /// Blocking: none. Timeout: none. Cancellation: none. Thread safety: locks internally.
    pub fn handoffs(&self) -> Vec<RecordedHandoff> {
        crate::lock(&self.inner).handoffs.clone()
    }

    /// Decisions not yet consumed.
    ///
    /// Blocking: none. Timeout: none. Cancellation: none. Thread safety: locks internally.
    pub fn queued_decisions(&self) -> Vec<ApprovalDecision> {
        crate::lock(&self.inner).decisions.iter().copied().collect()
    }

    /// Confirm answers not yet consumed.
    ///
    /// Blocking: none. Timeout: none. Cancellation: none. Thread safety: locks internally.
    pub fn queued_confirms(&self) -> Vec<bool> {
        crate::lock(&self.inner).confirms.iter().copied().collect()
    }

    fn wake(&self) {
        let wakers = std::mem::take(&mut crate::lock(&self.inner).wakers);
        for waker in wakers {
            waker.wake();
        }
    }
}

async fn next_copy<T: Copy>(
    inner: &Mutex<ScriptState>,
    cancel: &Cancel,
    take: fn(&mut ScriptState) -> Option<T>,
    restore: fn(&mut ScriptState, T),
) -> Option<T> {
    std::future::poll_fn(|context| {
        if cancel.is_cancelled() {
            return Poll::Ready(None);
        }
        {
            let mut guard = crate::lock(inner);
            if let Some(value) = take(&mut guard) {
                if cancel.is_cancelled() {
                    restore(&mut guard, value);
                    return Poll::Ready(None);
                }
                return Poll::Ready(Some(value));
            }
            crate::register_waker(&mut guard.wakers, context.waker());
        }
        cancel.register(context.waker());
        if cancel.is_cancelled() {
            return Poll::Ready(None);
        }
        let mut guard = crate::lock(inner);
        match take(&mut guard) {
            Some(value) if cancel.is_cancelled() => {
                restore(&mut guard, value);
                Poll::Ready(None)
            }
            Some(value) => Poll::Ready(Some(value)),
            None if cancel.is_cancelled() => Poll::Ready(None),
            None => Poll::Pending,
        }
    })
    .await
}

fn take_decision(state: &mut ScriptState) -> Option<ApprovalDecision> {
    state.decisions.pop_front()
}

fn restore_decision(state: &mut ScriptState, decision: ApprovalDecision) {
    state.decisions.push_front(decision);
}

fn take_confirm(state: &mut ScriptState) -> Option<bool> {
    state.confirms.pop_front()
}

fn restore_confirm(state: &mut ScriptState, confirmed: bool) {
    state.confirms.push_front(confirmed);
}

#[async_trait]
impl ApprovalProvider for ScriptedApprovals {
    async fn request_app(
        &self,
        session: &SessionCtx,
        app: &AppInfo,
        reason: Option<&ModelReason>,
        risk: Risk,
        cancel: &Cancel,
    ) -> ApprovalDecision {
        crate::lock(&self.inner)
            .app_requests
            .push(RecordedAppRequest {
                session: session.clone(),
                app: app.clone(),
                reason: reason.cloned(),
                risk,
            });
        next_copy(&self.inner, cancel, take_decision, restore_decision)
            .await
            .unwrap_or(ApprovalDecision::Deny)
    }

    async fn confirm_action(
        &self,
        session: &SessionCtx,
        app: &AppInfo,
        summary: &ActionSummary,
        crop: Option<&ImageRef>,
        cancel: &Cancel,
    ) -> bool {
        crate::lock(&self.inner)
            .confirm_requests
            .push(RecordedConfirm {
                session: session.clone(),
                app: app.clone(),
                summary: summary.clone(),
                crop: crop.cloned(),
            });
        next_copy(&self.inner, cancel, take_confirm, restore_confirm)
            .await
            .unwrap_or(false)
    }

    async fn handoff(
        &self,
        session: &SessionCtx,
        app: &AppInfo,
        reason: HandoffReason,
        _cancel: &Cancel,
    ) {
        crate::lock(&self.inner).handoffs.push(RecordedHandoff {
            session: session.clone(),
            app: app.clone(),
            reason,
        });
    }
}

/// Clock and stop latch a test can set by hand.
///
/// [`FakeUserActivity::request_stop`] sets the latch.
/// [`UserActivity::clear_stop_by_user`] is the only clear.
#[derive(Debug, Default)]
pub struct FakeUserActivity {
    inner: Mutex<ActivityState>,
    stop_wakers: StopWakers,
}

#[derive(Default)]
struct StopWakers(Mutex<Vec<Waker>>);

impl std::fmt::Debug for StopWakers {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("StopWakers")
    }
}

#[derive(Debug, Default)]
struct ActivityState {
    last_input: Option<Instant>,
    now: Option<Instant>,
    latch: bool,
}

impl FakeUserActivity {
    /// No input observed and the latch clear.
    ///
    /// Blocking: none. Timeout: none. Cancellation: none. Thread safety: the
    /// value is `Send + Sync`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a physical-input timestamp.
    ///
    /// Does not change the stop latch.
    ///
    /// Blocking: none. Timeout: none. Cancellation: none. Thread safety: locks internally.
    pub fn set_last_physical_input(&self, at: Instant) {
        crate::lock(&self.inner).last_input = Some(at);
    }

    /// Forget physical input. Later reads return `None`.
    ///
    /// Does not change the stop latch.
    ///
    /// Blocking: none. Timeout: none. Cancellation: none. Thread safety: locks internally.
    pub fn clear_physical_input(&self) {
        crate::lock(&self.inner).last_input = None;
    }

    /// Use `now` instead of [`Instant::now`] when measuring idle time.
    ///
    /// Blocking: none. Timeout: none. Cancellation: none. Thread safety: locks internally.
    pub fn set_now(&self, now: Instant) {
        crate::lock(&self.inner).now = Some(now);
    }

    /// Set the stop latch. Idempotent while it is already set.
    ///
    /// Blocking: none. Timeout: none. Cancellation: none. Thread safety: locks internally.
    pub fn request_stop(&self) {
        crate::lock(&self.inner).latch = true;
        let wakers = std::mem::take(&mut *crate::lock(&self.stop_wakers.0));
        for waker in wakers {
            waker.wake();
        }
    }
}

impl UserActivity for FakeUserActivity {
    fn ms_since_physical_input(&self) -> Option<u64> {
        let guard = crate::lock(&self.inner);
        let last = guard.last_input?;
        let now = guard.now.unwrap_or_else(Instant::now);
        let millis = now.saturating_duration_since(last).as_millis();
        Some(u64::try_from(millis).unwrap_or(u64::MAX))
    }

    fn stop_requested(&self) -> bool {
        crate::lock(&self.inner).latch
    }

    fn latched_stop(&self) -> bool {
        crate::lock(&self.inner).latch
    }

    fn clear_stop_by_user(&self) {
        crate::lock(&self.inner).latch = false;
    }

    fn register_stop_waker(&self, waker: &Waker) {
        let mut wakers = crate::lock(&self.stop_wakers.0);
        if wakers.iter().any(|existing| existing.will_wake(waker)) {
            return;
        }
        wakers.push(waker.clone());
    }
}

/// Event sink that keeps every event.
#[derive(Debug, Default)]
pub struct RecordingEventSink {
    inner: Mutex<Vec<(SessionCtx, CuEvent)>>,
}

impl RecordingEventSink {
    /// Empty log.
    ///
    /// Blocking: none. Timeout: none. Cancellation: none. Thread safety: the
    /// value is `Send + Sync`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Events in emit order, with the session each one arrived on.
    ///
    /// Blocking: none. Timeout: none. Cancellation: none. Thread safety: locks internally.
    pub fn events(&self) -> Vec<(SessionCtx, CuEvent)> {
        crate::lock(&self.inner).clone()
    }

    /// Drop recorded events.
    ///
    /// Blocking: none. Timeout: none. Cancellation: none. Thread safety: locks internally.
    pub fn clear(&self) {
        crate::lock(&self.inner).clear();
    }
}

impl EventSink for RecordingEventSink {
    fn emit(&self, session: &SessionCtx, event: CuEvent) {
        crate::lock(&self.inner).push((session.clone(), event));
    }
}

/// Park the current thread until `future` completes.
///
/// Test helper, not a runtime: no timer, no IO driver, and no timeout. A
/// future that never wakes parks this thread until the process ends.
///
/// Blocking: parks this thread. Timeout: none. Cancellation: only if `future`
/// itself waits on a [`Cancel`]. Thread safety: call from the thread that
/// should block. The future must be `Send` only if it crosses threads itself.
pub fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = std::pin::pin!(future);
    let waker = Waker::from(Arc::new(ThreadWaker(std::thread::current())));
    let mut context = Context::from_waker(&waker);
    loop {
        match future.as_mut().poll(&mut context) {
            Poll::Ready(value) => return value,
            Poll::Pending => std::thread::park(),
        }
    }
}

struct ThreadWaker(Thread);

impl Wake for ThreadWaker {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
}
