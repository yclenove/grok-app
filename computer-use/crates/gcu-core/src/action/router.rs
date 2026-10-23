//! One policy-bound attempt per tool call. No retry of uncertain or targeted input.
use super::{
    gate::{self, Gate},
    lock, route,
    wire::{self, CallError},
    ActionState, ToolCall, ToolOutcome,
};
use crate::{
    driver::{self, AppQuery, Cancel, Driver, DriverError, TreeOpts, WindowRef},
    model::WindowPoint,
    observe::AutoCapturePolicy,
    policy::{ActionIntent, Tool},
    session::{InputRelease, Session, WindowLocks},
    settle::SettleEngine,
    snapshot::WindowKey,
};
use async_trait::async_trait;
use gcu_host_api::{ApprovalProvider, CuEvent, EventSink, PolicyStore, SessionCtx};
use gcu_protocol::{
    result::Outcome,
    tools::{ComputerBatchParams, GetAppStateParams},
    ErrorCode, ToolError,
};
use std::{
    future::Future,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};
use tokio::time::Instant;

/// Portable action executor. Share one router/WindowLocks across sessions. Session
/// owns its snapshots/ledger, so reconnecting never inherits another store.
pub struct ActionRouter {
    pub(crate) driver: Arc<dyn Driver>,
    pub(crate) approvals: Arc<dyn ApprovalProvider>,
    pub(crate) policy: Arc<dyn PolicyStore>,
    events: Arc<dyn EventSink>,
    pub(crate) locks: WindowLocks,
    pub(crate) policy_version: AtomicU64,
    pub(crate) epoch: Instant,
    pub(crate) tree_opts: TreeOpts,
    pub(crate) settle_engine: SettleEngine,
    pub(crate) auto_capture: AutoCapturePolicy,
    pub(crate) wait_engine: crate::wait::WaitEngine,
    pub(crate) browser: Option<std::sync::Arc<crate::browser::Hub>>,
    pub(crate) fixture_scoped: bool,
    pub(crate) lease: Option<crate::scope::ScopeLease>,
    pub(crate) publish_gate: std::sync::Arc<super::PublishGate>,
}
impl ActionRouter {
    /// Construct without native work or IO. No waits, timeout, or cancellation.
    /// All sessions in this engine must share `locks`.
    pub fn new(
        driver: Arc<dyn Driver>,
        approvals: Arc<dyn ApprovalProvider>,
        policy: Arc<dyn PolicyStore>,
        events: Arc<dyn EventSink>,
        locks: WindowLocks,
    ) -> Self {
        let tree_opts = TreeOpts {
            max_tree_nodes: policy.load().max_tree_nodes.clamp(1, 5000) as usize,
            ..TreeOpts::default()
        };
        Self {
            driver,
            approvals,
            policy,
            events,
            locks,
            policy_version: AtomicU64::new(1),
            epoch: Instant::now(),
            tree_opts,
            settle_engine: SettleEngine::default(),
            auto_capture: AutoCapturePolicy::default(),
            wait_engine: crate::wait::WaitEngine::default(),
            browser: None,
            fixture_scoped: false,
            lease: None,
            publish_gate: super::PublishGate::new(),
        }
    }
    /// Publication pause. Production leaves it disarmed, so entering it does not wait.
    ///
    /// Memory only. No timeout or cancellation. Cloning the handle does not copy a lease.
    pub fn publication_gate(&self) -> std::sync::Arc<super::PublishGate> {
        std::sync::Arc::clone(&self.publish_gate)
    }

    /// Inject settling configuration and monotonic time before sharing the router.
    /// In-memory construction only; no waits, IO, timeout or cancellation.
    pub fn with_settle_engine(mut self, engine: SettleEngine) -> Self {
        self.wait_engine = crate::wait::WaitEngine::new(engine.clock());
        self.settle_engine = engine;
        self
    }
    /// Supply evaluated screenshot-quality boundaries before sharing the router.
    /// In-memory construction only; no IO, waits, timeout or cancellation.
    pub fn with_auto_capture_policy(mut self, policy: AutoCapturePolicy) -> Self {
        self.auto_capture = policy;
        self
    }
    /// Input-release adapter for Session::open. Native release is bounded by the
    /// Session stop channel, independently of action/approval queues.
    pub fn input_release(&self) -> Arc<dyn InputRelease> {
        Arc::new(Release(self.driver.clone()))
    }
    /// Publish a Host policy change and invalidate outstanding approval bindings.
    /// No IO/user waits; may acquire the Host's memory lock. No cancellation.
    pub fn policy_changed(&self) {
        self.policy.refresh();
        self.policy_version.fetch_add(1, Ordering::AcqRel);
    }
    /// Authorize and deliver one observation, defaulting to a safe screenshot.
    /// Serialized by session and exact window lock. Native calls have a 5s ceiling;
    /// approvals use Host timeout. Stop/close cancel waits and never resume input.
    pub async fn observe(
        &self,
        session: &Session,
        params: GetAppStateParams,
    ) -> Result<ActionState, Box<ToolError>> {
        session
            .enqueue(self.observe_enqueued(session, params))
            .await
            .map_err(|_| Box::new(error(ErrorCode::ApprovalDenied)))?
    }

    /// Observe while the caller already holds the session queue.
    ///
    /// Script and batch hold that queue. A nested [`Self::observe`] would deadlock.
    /// Authorization, Stop, and the 5s native ceiling match [`Self::observe`].
    ///
    /// Blocking: window-lock wait caps at 5s, then native and approval waits.
    /// Timeout: native calls cap at 5s. Cancellation: Stop, session close, or drop.
    pub async fn observe_enqueued(
        &self,
        session: &Session,
        params: GetAppStateParams,
    ) -> Result<ActionState, Box<ToolError>> {
        self.check_boundary(session)
            .await
            .map_err(|code| Box::new(error(code)))?;
        let win = self
            .resolve(session, &params.app, params.window.as_deref())
            .await
            .map_err(|err| Box::new(err.tool_error()))?;
        let _window = self
            .locks
            .acquire(&window_lock_key(&win))
            .await
            .map_err(|e| Box::new(error(e.code())))?;
        let mut opts = self.tree_opts.clone();
        opts.text_limit = match params.text_limit {
            gcu_protocol::tools::TextLimit::Characters(n) => Some(n as usize),
            _ => None,
        };
        opts.max_tree_nodes = params.max_tree_nodes.clamp(1, 5000) as usize;
        let fresh = self
            .read_with_mode(session, &win, params.include_screenshot, &opts)
            .await
            .map_err(|err| Box::new(err.tool_error()))?;
        let mut intent = ActionIntent::new(Tool::GetAppState);
        intent.parameters = fresh.signature.clone();
        intent.reason = params.reason.map(gcu_host_api::ModelReason::new);
        let approved = self
            .authorize(session, &mut intent, &fresh)
            .await
            .map_err(|err| Box::new(err.tool_error()))?;
        if !self.scope_binding_current(
            approved.binding.scope_run.as_deref(),
            approved.binding.scope_generation,
        ) {
            return Err(Box::new(CallError::ScopeDenied.tool_error()));
        }
        let epoch = self.binding_epoch(&approved);
        crate::scope::with_attempt(epoch, async {
            self.check_boundary(session)
                .await
                .map_err(|code| Box::new(error(code)))?;
            let cancel = Cancel::default();
            let current = self
                .native(
                    session,
                    &cancel,
                    false,
                    self.driver.resolve_app(&AppQuery(win.app.info.id.clone())),
                )
                .await
                .map_err(|e| Box::new(CallError::from_driver(&e).tool_error()))?;
            if current != win.app {
                return Err(Box::new(error(ErrorCode::StaleElement)));
            }
            // Read after the authorization wait, rather than releasing pre-approval
            // contents that might now be obsolete.
            let fresh = self
                .read_with_mode(session, &win, params.include_screenshot, &opts)
                .await
                .map_err(|err| Box::new(err.tool_error()))?;
            let attach = self.observation_attachment(
                session,
                &fresh,
                params.include_screenshot,
                opts.text_limit,
            );
            self.publish_with_limit(session, fresh, attach, opts.text_limit)
        })
        .await
    }
    /// Execute once: exact-handle/coordinate gates, policy and cancellable approval,
    /// revalidate, perform, bounded settle, then a fresh state/diff. `wait_for` does
    /// not perform input; its horizon is at most 30s while each snapshot stays within
    /// the 5s native ceiling. Unknown is never retried. Session queue waits have no
    /// deadline; window-lock waits cap at 5s. Native waits cap at 5s; settle defaults
    /// to 800ms (type_text 1500ms). Stop is checked at every boundary and polled every
    /// 10ms while awaiting native/Host work.
    pub async fn execute(&self, session: &Session, call: ToolCall) -> ToolOutcome {
        let start = Instant::now();
        let initial = result(&call);
        let mut out = match self.check_boundary(session).await {
            Err(code) => outcome(initial, Some(error(code)), None),
            Ok(()) => match session.enqueue(self.dispatch_queued(session, &call)).await {
                Ok(out) => out,
                Err(_) => outcome(initial, Some(error(ErrorCode::ApprovalDenied)), None),
            },
        };
        self.finish(session, &call.call_id, &mut out, start);
        out
    }

    /// Run one step that is already inside the session queue.
    ///
    /// `computer_batch` holds that queue for the whole batch. Calling [`Self::execute`]
    /// again would deadlock. Authorization, gates, sensitive confirmation, Stop, and
    /// the unknown-outcome rule are unchanged. Duration includes only this step.
    ///
    /// Blocking: window-lock waits cap at 5s, then native and approval waits.
    /// Timeout: native calls cap at 5s; `wait_for` may use its own horizon.
    /// Cancellation: Stop, session close, turn end, or dropping the future.
    pub async fn execute_enqueued(&self, session: &Session, call: ToolCall) -> ToolOutcome {
        let start = Instant::now();
        let initial = result(&call);
        let mut out = match self.check_boundary(session).await {
            Err(code) => outcome(initial, Some(error(code)), None),
            Ok(()) => self.dispatch_queued(session, &call).await,
        };
        self.finish(session, &call.call_id, &mut out, start);
        out
    }

    /// Preflight `params`, then run each step through this router.
    ///
    /// A preflight failure does not enqueue and does not deliver input. Otherwise the
    /// batch enqueues once. Each step uses the same approval, gate, confirm, Stop, and
    /// stale paths as a single call. The batch wall clock is 60s on the settle clock.
    /// Each native call still uses the 5s ceiling and [`crate::driver::DRIVER_DEADLINE`].
    /// Those constants are not raised. An in-flight step may finish after 60s; later
    /// steps do not start. Unknown outcomes are not replayed. Nested batches cannot be
    /// represented.
    ///
    /// Blocking: the session queue has no deadline. Timeout: 60s total on the settle
    /// clock, and 5s for each native call. Cancellation: Stop, session close, or
    /// dropping the future.
    pub async fn run_batch(
        &self,
        session: &Session,
        call_id: &str,
        params: ComputerBatchParams,
    ) -> crate::batch::BatchReport {
        crate::batch::run(self, session, call_id, params).await
    }

    fn finish(&self, session: &Session, call_id: &str, out: &mut ToolOutcome, start: Instant) {
        out.action.duration_ms = start.elapsed().as_millis() as u64;
        if let Some(state) = &mut out.state {
            state.state.action = Some(out.action.clone());
        }
        if let Some(error) = &mut out.error {
            if out.state.is_some() {
                error.state = out.state.as_ref().map(|s| s.state.clone());
            }
        }
        if !session.is_closed() {
            lock(&session.actions)
                .ledger
                .record(call_id, &out.action, self.epoch.elapsed());
            self.events.emit(
                &SessionCtx {
                    session_id: session.id().into(),
                },
                CuEvent {
                    tool: out.action.tool,
                    target: out
                        .action
                        .target
                        .as_ref()
                        .map(|t| match t {
                            gcu_protocol::result::ActionTarget::Element {
                                element_index, ..
                            } => format!("element {element_index}"),
                            gcu_protocol::result::ActionTarget::Coordinates { x, y, .. } => {
                                format!("({x},{y})")
                            }
                        })
                        .unwrap_or_default(),
                    outcome: out.action.outcome,
                    duration_ms: out.action.duration_ms,
                    thumbnail: None,
                },
            );
        }
    }
    pub(crate) async fn execute_queued(&self, session: &Session, call: &ToolCall) -> ToolOutcome {
        let mut action = result(call);
        let win = match self
            .resolve(session, &call.app, call.window.as_deref())
            .await
        {
            Ok(win) => win,
            Err(err) => return outcome(action, Some(err.tool_error()), None),
        };
        let _window = match self.locks.acquire(&window_lock_key(&win)).await {
            Ok(g) => g,
            Err(e) => return outcome(action, Some(error(e.code())), None),
        };
        let key = WindowKey {
            app_id: win.app.info.id.clone(),
            window_id: win.id.clone(),
        };
        let cancel = Cancel::default();
        if let Err(e) = self
            .native(session, &cancel, false, self.driver.window_geometry(&win))
            .await
        {
            return self
                .failure(session, action, CallError::from_driver(&e), &win)
                .await;
        }
        let mut gate = match gate::prepare(session, call, &key) {
            Ok(gate) => gate,
            Err(code) => return self.failure(session, action, code, &win).await,
        };
        let fresh = match self
            .read(
                session,
                &win,
                self.driver.capabilities().observe.screenshot,
                &self.tree_opts,
            )
            .await
        {
            Ok(f) => f,
            Err(code) => return self.failure(session, action, code, &win).await,
        };
        if route::pointer_needed(call, &gate, &fresh, self.driver.capabilities()) {
            if let Err(code) = self.element_pointer(session, &key, &mut gate) {
                return self.failure(session, action, code, &win).await;
            }
        }
        if let Err(err) = super::epoch::screenshot_epoch(
            self,
            session,
            &key,
            &gate.screenshot_id,
            super::epoch::current_attempt_epoch(self),
        ) {
            return self.failure(session, action, err, &win).await;
        }
        if let Err(code) = gate::check(&gate, &fresh) {
            return self.failure(session, action, code, &win).await;
        }
        let mut plan = match route::plan(call, &gate, &fresh, self.driver.capabilities()) {
            Ok(p) => p,
            Err(code) => return self.failure(session, action, code, &win).await,
        };
        action.target = plan.target.clone();
        if let Err(code) = self.bind_parameters(&mut plan.intent, call, &fresh, &gate) {
            return self.failure(session, action, code, &win).await;
        }
        let approved = match self.authorize(session, &mut plan.intent, &fresh).await {
            Ok(a) => a,
            Err(err) => return self.failure(session, action, err, &win).await,
        };
        let before = match self
            .recheck(session, call, &gate, &fresh, &plan.intent, &approved)
            .await
        {
            Ok(f) => f,
            Err(code) => return self.failure(session, action, code, &win).await,
        };
        let epoch = self.binding_epoch(&approved);
        return crate::scope::with_attempt(epoch, async {
            if let Err(code) = self.check_boundary(session).await {
                return outcome(action, Some(error(code)), None);
            }
            let events = self.driver.subscribe(&win);
            let cancel = Cancel::default();
            let delivery =
                crate::browser::dispatch(self, session, &cancel, &plan.request, &before.raw).await;
            match delivery {
                Err(e) => {
                    if e.maybe_executed()
                        && crate::session::drive_native(self.driver.release_all_input())
                            .await
                            .is_err()
                    {
                        action
                            .notes
                            .push("Input release reached the native timeout.".into());
                    }
                    action.outcome = if e.maybe_executed() {
                        Outcome::Unknown
                    } else if matches!(e, DriverError::ScopeDenied) {
                        Outcome::Refused
                    } else {
                        Outcome::Failed
                    };
                    if e.maybe_executed() {
                        action.delivery = plan.delivery;
                        action
                            .notes
                            .push("Execution is uncertain; do not automatically retry.".into());
                    }
                    let failure = if session.is_closed() {
                        CallError::Code(ErrorCode::ApprovalDenied)
                    } else {
                        CallError::from_driver(&e)
                    };
                    return self.failure(session, action, failure, &win).await;
                }
                Ok(delivery) => {
                    action.delivery = wire::wire_delivery(delivery);
                    action.outcome = Outcome::Done;
                }
            }
            self.settle_and_publish(session, call, &gate, action, before, events)
                .await
        })
        .await;
    }
    pub(crate) async fn check_boundary(&self, session: &Session) -> Result<(), ErrorCode> {
        if session.is_stopped() || session.activity().stop_requested() {
            session.handle_stop().await;
            return Err(ErrorCode::StoppedByUser);
        }
        if session.is_closed() {
            return Err(ErrorCode::ApprovalDenied);
        }
        Ok(())
    }
    pub(crate) async fn wait<T>(
        &self,
        session: &Session,
        work: impl Future<Output = T>,
    ) -> Result<T, ErrorCode> {
        super::epoch::wait_cancel(self, session, None, work).await
    }
    async fn wait_cancel<T>(
        &self,
        session: &Session,
        cancel: Option<&Cancel>,
        work: impl Future<Output = T>,
    ) -> Result<T, ErrorCode> {
        super::epoch::wait_cancel(self, session, cancel, work).await
    }
    pub(crate) async fn native<T>(
        &self,
        session: &Session,
        cancel: &Cancel,
        mutation: bool,
        work: impl Future<Output = driver::Result<T>>,
    ) -> driver::Result<T> {
        let mut cancel_on_drop = CancelOnDrop(Some(cancel.clone()));
        let result = match self
            .wait_cancel(
                session,
                Some(cancel),
                tokio::time::timeout(driver::NATIVE_TIMEOUT, work),
            )
            .await
        {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => {
                cancel.cancel();
                Err(DriverError::Timeout {
                    maybe_executed: mutation,
                })
            }
            Err(code) => {
                cancel.cancel();
                if code == ErrorCode::AppDenied {
                    Err(DriverError::ScopeDenied)
                } else {
                    Err(DriverError::Cancelled {
                        maybe_executed: mutation,
                    })
                }
            }
        };
        cancel_on_drop.0 = None;
        result
    }
    pub(crate) async fn resolve(
        &self,
        session: &Session,
        app: &str,
        window: Option<&str>,
    ) -> Result<WindowRef, CallError> {
        let cancel = Cancel::default();
        let app = self
            .native(
                session,
                &cancel,
                false,
                self.driver.resolve_app(&AppQuery::from(app)),
            )
            .await
            .map_err(|e| CallError::from_driver(&e))?;
        let id = if let Some(id) = window {
            id.into()
        } else {
            self.native(session, &cancel, false, self.driver.key_window(&app))
                .await
                .map_err(|e| CallError::from_driver(&e))?
                .id
        };
        Ok(WindowRef { app, id })
    }
    fn element_pointer(
        &self,
        session: &Session,
        key: &WindowKey,
        gate: &mut Gate,
    ) -> Result<(), ErrorCode> {
        let frame = gate
            .element
            .as_ref()
            .and_then(|e| e.frame)
            .ok_or(ErrorCode::ElementNotActionable)?;
        let at = WindowPoint {
            x: frame.x + frame.w / 2.,
            y: frame.y + frame.h / 2.,
        };
        let resources = lock(&session.actions);
        let shot = resources
            .snapshots
            .latest_with_screenshot(key)
            .ok_or(ErrorCode::StaleScreenshot)?;
        let image = shot.screenshot().ok_or(ErrorCode::StaleScreenshot)?;
        gate.hashes
            .push(image.target_hash(at).map_err(|_| ErrorCode::OutOfBounds)?);
        gate.points.push(at);
        gate.geometry = Some(shot.geometry());
        gate.screenshot_id = format!("i_{}", shot.id().0);
        Ok(())
    }
}

struct Release(Arc<dyn Driver>);
#[async_trait]
impl InputRelease for Release {
    async fn release_all_input(&self) {
        self.0.release_all_input().await;
    }
}
struct CancelOnDrop(Option<Cancel>);
impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        if let Some(cancel) = &self.0 {
            cancel.cancel();
        }
    }
}
pub(crate) use super::wire::{error, outcome, result};

pub(crate) fn window_lock_key(win: &WindowRef) -> String {
    format!("{}:{}:{}", win.app.info.id.len(), win.app.info.id, win.id)
}
