//! One policy-bound attempt per tool call. No retry of uncertain or targeted input.
use super::{
    gate::{self, Gate},
    lock, route, ActionState, ToolCall, ToolOutcome,
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
    result::{ActionResult, Delivery, Effect, Outcome, Settle, SettleReason},
    tools::GetAppStateParams,
    ErrorCode, ToolError,
};
use std::{
    future::Future,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::time::Instant;

/// Portable action executor. Share one router/WindowLocks across sessions. Session
/// owns its snapshots/ledger, so reconnecting never inherits another store.
pub struct ActionRouter {
    pub(crate) driver: Arc<dyn Driver>,
    pub(crate) approvals: Arc<dyn ApprovalProvider>,
    pub(crate) policy: Arc<dyn PolicyStore>,
    events: Arc<dyn EventSink>,
    pub(super) locks: WindowLocks,
    pub(crate) policy_version: AtomicU64,
    pub(crate) epoch: Instant,
    pub(crate) tree_opts: TreeOpts,
    pub(crate) settle_engine: SettleEngine,
    pub(crate) auto_capture: AutoCapturePolicy,
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
        }
    }
    /// Inject settling configuration and monotonic time before sharing the router.
    /// In-memory construction only; no waits, IO, timeout or cancellation.
    pub fn with_settle_engine(mut self, engine: SettleEngine) -> Self {
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
            .enqueue(async {
                self.check_boundary(session)
                    .await
                    .map_err(|code| Box::new(error(code)))?;
                let win = self
                    .resolve(session, &params.app, params.window.as_deref())
                    .await
                    .map_err(|code| Box::new(error(code)))?;
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
                    .map_err(|code| Box::new(error(code)))?;
                let mut intent = ActionIntent::new(Tool::GetAppState);
                intent.parameters = fresh.signature.clone();
                intent.reason = params.reason.map(gcu_host_api::ModelReason::new);
                let _approved = self
                    .authorize(session, &mut intent, &fresh)
                    .await
                    .map_err(|code| Box::new(error(code)))?;
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
                    .map_err(|e| Box::new(error(e.error_code())))?;
                if current != win.app {
                    return Err(Box::new(error(ErrorCode::StaleElement)));
                }
                // Read after the authorization wait, rather than releasing pre-approval
                // contents that might now be obsolete.
                let fresh = self
                    .read_with_mode(session, &win, params.include_screenshot, &opts)
                    .await
                    .map_err(|code| Box::new(error(code)))?;
                let attach = self.observation_attachment(
                    session,
                    &fresh,
                    params.include_screenshot,
                    opts.text_limit,
                );
                self.publish_with_limit(session, fresh, attach, opts.text_limit)
            })
            .await
            .map_err(|_| Box::new(error(ErrorCode::ApprovalDenied)))?
    }
    /// Execute once: exact-handle/coordinate gates, policy and cancellable approval,
    /// revalidate, perform, bounded settle, then a fresh state/diff. Unknown is never
    /// retried. Session queue waits have no deadline; window-lock waits cap at 5s.
    /// Native waits cap at 5s; settle defaults to 800ms (type_text 1500ms). Stop is
    /// checked at every boundary and polled every 10ms while awaiting native/Host work.
    pub async fn execute(&self, session: &Session, call: ToolCall) -> ToolOutcome {
        let start = Instant::now();
        let initial = result(&call);
        let mut out = match self.check_boundary(session).await {
            Err(code) => outcome(initial, Some(error(code)), None),
            Ok(()) => match session.enqueue(self.execute_queued(session, &call)).await {
                Ok(out) => out,
                Err(_) => outcome(initial, Some(error(ErrorCode::ApprovalDenied)), None),
            },
        };
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
                .record(&call.call_id, &out.action, self.epoch.elapsed());
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
        out
    }
    async fn execute_queued(&self, session: &Session, call: &ToolCall) -> ToolOutcome {
        let mut action = result(call);
        let win = match self
            .resolve(session, &call.app, call.window.as_deref())
            .await
        {
            Ok(win) => win,
            Err(code) => return outcome(action, Some(error(code)), None),
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
            return self.failure(session, action, e.error_code(), &win).await;
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
            Err(code) => return self.failure(session, action, code, &win).await,
        };
        let before = match self
            .recheck(session, call, &gate, &fresh, &plan.intent, &approved)
            .await
        {
            Ok(f) => f,
            Err(code) => return self.failure(session, action, code, &win).await,
        };
        if let Err(code) = self.check_boundary(session).await {
            return outcome(action, Some(error(code)), None);
        }
        let events = self.driver.subscribe(&win);
        let cancel = Cancel::default();
        let delivery = self
            .native(
                session,
                &cancel,
                true,
                self.driver.perform(&plan.request, &cancel),
            )
            .await;
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
                } else {
                    Outcome::Failed
                };
                if e.maybe_executed() {
                    action.delivery = plan.delivery;
                    action
                        .notes
                        .push("Execution is uncertain; do not automatically retry.".into());
                }
                let code = if session.is_closed() {
                    ErrorCode::ApprovalDenied
                } else {
                    e.error_code()
                };
                return self.failure(session, action, code, &win).await;
            }
            Ok(delivery) => {
                action.delivery = wire_delivery(delivery);
                action.outcome = Outcome::Done;
            }
        }
        self.settle_and_publish(session, call, &gate, action, before, events)
            .await
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
        self.wait_cancel(session, None, work).await
    }
    async fn wait_cancel<T>(
        &self,
        session: &Session,
        cancel: Option<&Cancel>,
        work: impl Future<Output = T>,
    ) -> Result<T, ErrorCode> {
        if session.is_stopped() || session.is_closed() {
            if let Some(cancel) = cancel {
                cancel.cancel();
            }
        }
        self.check_boundary(session).await?;
        let generation = session.turn_generation();
        tokio::pin!(work);
        loop {
            tokio::select! {
                biased;
                _ = session.cancellation().cancelled() => { if let Some(cancel) = cancel { cancel.cancel(); } return Err(ErrorCode::ApprovalDenied); },
                _ = tokio::time::sleep(Duration::from_millis(10)) => {
                    if session.turn_generation() != generation {
                        if let Some(cancel) = cancel { cancel.cancel(); }
                        return Err(ErrorCode::ApprovalDenied);
                    }
                    if session.is_stopped() { if let Some(cancel) = cancel { cancel.cancel(); } }
                    self.check_boundary(session).await?;
                },
                value = &mut work => {
                    if session.is_stopped() || session.is_closed() { if let Some(cancel) = cancel { cancel.cancel(); } }
                    self.check_boundary(session).await?; return Ok(value);
                }
            }
        }
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
            Err(_) => {
                cancel.cancel();
                Err(DriverError::Cancelled {
                    maybe_executed: mutation,
                })
            }
        };
        cancel_on_drop.0 = None;
        result
    }
    async fn resolve(
        &self,
        session: &Session,
        app: &str,
        window: Option<&str>,
    ) -> Result<WindowRef, ErrorCode> {
        let cancel = Cancel::default();
        let app = self
            .native(
                session,
                &cancel,
                false,
                self.driver.resolve_app(&AppQuery::from(app)),
            )
            .await
            .map_err(|e| e.error_code())?;
        let id = if let Some(id) = window {
            id.into()
        } else {
            self.native(session, &cancel, false, self.driver.key_window(&app))
                .await
                .map_err(|e| e.error_code())?
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
pub(crate) fn error(code: ErrorCode) -> ToolError {
    ToolError {
        code,
        message: code.as_str().replace('_', " "),
        hint: "Use the attached state; obtain fresh user authorization when required.".into(),
        retryable: false,
        state: None,
    }
}
fn result(call: &ToolCall) -> ActionResult {
    ActionResult {
        tool: route::tool(&call.action),
        target: None,
        delivery: Delivery::None,
        outcome: Outcome::Refused,
        effect: Effect::NotChecked,
        verify: None,
        duration_ms: 0,
        settle: Settle {
            ms: 0,
            reason: SettleReason::Budget,
        },
        notes: Vec::new(),
    }
}
pub(crate) fn outcome(
    action: ActionResult,
    error: Option<ToolError>,
    state: Option<ActionState>,
) -> ToolOutcome {
    ToolOutcome {
        action,
        error,
        state,
        windows: Vec::new(),
    }
}
fn window_lock_key(win: &WindowRef) -> String {
    format!("{}:{}:{}", win.app.info.id.len(), win.app.info.id, win.id)
}
fn wire_delivery(d: driver::Delivery) -> Delivery {
    match d {
        driver::Delivery::Accessibility => Delivery::Accessibility,
        driver::Delivery::TargetedEvent => Delivery::TargetedEvent,
        driver::Delivery::Foreground => Delivery::Foreground,
        driver::Delivery::Clipboard => Delivery::Clipboard,
    }
}
