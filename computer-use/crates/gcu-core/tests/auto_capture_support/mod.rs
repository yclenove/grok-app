#![allow(dead_code)]
use async_trait::async_trait;
use gcu_core::{
    action::{ActionRouter, ActionState},
    driver::*,
    model::*,
    observe::AutoCapturePolicy,
    session::{Session, WindowLocks},
};
use gcu_driver_fake::FakeDriver;
use gcu_host_api::{
    testing::{FakeUserActivity, MemoryPolicyStore, RecordingEventSink, ScriptedApprovals},
    ApprovalDecision, ConfirmationLevel, ForegroundPolicy, PolicyConfig,
};
use gcu_protocol::{GetAppStateParams, ToolError};
use serde_json::{json, Value};
use std::{
    io::Cursor,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex,
    },
};

pub struct ProbeDriver {
    pub inner: FakeDriver,
    raw: Mutex<Option<RawTree>>,
    after_capture: Mutex<Option<RawTree>>,
    after_action: Mutex<Option<RawTree>>,
    captures: AtomicUsize,
    pub paint: AtomicBool,
    pub paint_password: AtomicBool,
    pub paint_on_action: AtomicBool,
}
impl ProbeDriver {
    pub fn captures(&self) -> usize {
        self.captures.load(Ordering::SeqCst)
    }
    pub fn replace(&self, raw: RawTree) {
        *self.raw.lock().unwrap() = Some(raw);
    }
    pub fn replace_after_capture(&self, raw: RawTree) {
        *self.after_capture.lock().unwrap() = Some(raw);
    }
    pub fn replace_after_action(&self, raw: RawTree) {
        *self.after_action.lock().unwrap() = Some(raw);
    }
    pub async fn raw(&self, app: &str) -> RawTree {
        let win = self.inner.fixture_window(app).await.unwrap();
        self.snapshot_tree(&win, &TreeOpts::default(), &Cancel::default())
            .await
            .unwrap()
    }
}
#[async_trait]
impl Driver for ProbeDriver {
    fn platform(&self) -> Platform {
        self.inner.platform()
    }
    fn capabilities(&self) -> &Capabilities {
        self.inner.capabilities()
    }
    async fn doctor(&self) -> DoctorReport {
        self.inner.doctor().await
    }
    async fn list_apps(&self) -> Result<Vec<AppInfo>> {
        self.inner.list_apps().await
    }
    async fn resolve_app(&self, query: &AppQuery) -> Result<AppRef> {
        self.inner.resolve_app(query).await
    }
    async fn launch_app(&self, app: &AppRef) -> Result<AppRef> {
        self.inner.launch_app(app).await
    }
    async fn list_windows(&self, app: Option<&AppRef>) -> Result<Vec<WindowInfo>> {
        self.inner.list_windows(app).await
    }
    async fn key_window(&self, app: &AppRef) -> Result<WindowInfo> {
        self.inner.key_window(app).await
    }
    async fn snapshot_tree(
        &self,
        win: &WindowRef,
        opts: &TreeOpts,
        cancel: &Cancel,
    ) -> Result<RawTree> {
        let replacement = self.raw.lock().unwrap().clone();
        if let Some(raw) = replacement {
            return Ok(raw);
        }
        self.inner.snapshot_tree(win, opts, cancel).await
    }
    async fn capture(
        &self,
        win: &WindowRef,
        opts: &CaptureOpts,
        cancel: &Cancel,
    ) -> Result<Capture> {
        self.captures.fetch_add(1, Ordering::SeqCst);
        let mut capture = self.inner.capture(win, opts, cancel).await?;
        if self.paint.load(Ordering::SeqCst) || self.paint_password.load(Ordering::SeqCst) {
            let mut pixels = image::load_from_memory(&capture.bytes).unwrap().to_rgba8();
            if self.paint.load(Ordering::SeqCst) {
                pixels.put_pixel(700, 400, image::Rgba([0, 100, 200, 255]));
            }
            if self.paint_password.load(Ordering::SeqCst) {
                pixels.put_pixel(30, 100, image::Rgba([0, 100, 200, 255]));
            }
            let mut out = Cursor::new(Vec::new());
            pixels.write_to(&mut out, image::ImageFormat::Png).unwrap();
            capture.bytes = out.into_inner();
        }
        let replacement = self.after_capture.lock().unwrap().take();
        if let Some(raw) = replacement {
            self.replace(raw);
        }
        Ok(capture)
    }
    async fn window_geometry(&self, win: &WindowRef) -> Result<Geometry> {
        self.inner.window_geometry(win).await
    }
    async fn focused_element(&self, app: &AppRef) -> Result<Option<NativeHandle>> {
        self.inner.focused_element(app).await
    }
    async fn perform(&self, req: &DriverAction, cancel: &Cancel) -> Result<Delivery> {
        let delivery = self.inner.perform(req, cancel).await?;
        if self.paint_on_action.load(Ordering::SeqCst) {
            self.paint.store(true, Ordering::SeqCst);
        }
        let replacement = self.after_action.lock().unwrap().take();
        if let Some(raw) = replacement {
            self.replace(raw);
        }
        Ok(delivery)
    }
    fn subscribe(&self, win: &WindowRef) -> Option<BoxStream<'static, UiEvent>> {
        self.inner.subscribe(win)
    }
    async fn release_all_input(&self) {
        self.inner.release_all_input().await;
    }
}

pub struct Rig {
    pub driver: Arc<ProbeDriver>,
    pub router: ActionRouter,
    pub session: Session,
    app: String,
}
impl Rig {
    pub fn new(scene: &str, policy: AutoCapturePolicy) -> Self {
        let driver = Arc::new(ProbeDriver {
            inner: FakeDriver::from_scene(scene).unwrap(),
            raw: Mutex::new(None),
            after_capture: Mutex::new(None),
            after_action: Mutex::new(None),
            captures: AtomicUsize::new(0),
            paint: AtomicBool::new(false),
            paint_password: AtomicBool::new(false),
            paint_on_action: AtomicBool::new(false),
        });
        let approvals = Arc::new(ScriptedApprovals::new());
        approvals.push_decision(ApprovalDecision::Session);
        let router = ActionRouter::new(
            driver.clone(),
            approvals,
            Arc::new(MemoryPolicyStore::new(PolicyConfig {
                sensitive_action_confirmation: ConfirmationLevel::Off,
                allow_foreground: ForegroundPolicy::Always,
                ..PolicyConfig::default()
            })),
            Arc::new(RecordingEventSink::new()),
            WindowLocks::new(),
        )
        .with_auto_capture_policy(policy);
        let session = Session::open(
            "auto",
            Arc::new(FakeUserActivity::new()),
            router.input_release(),
        );
        session.begin_turn().unwrap();
        Self {
            driver,
            router,
            session,
            app: format!("fake.{scene}"),
        }
    }
    pub async fn observe(&self, mode: Value) -> std::result::Result<ActionState, Box<ToolError>> {
        let args: GetAppStateParams = serde_json::from_value(json!({
            "app":self.app,"include_screenshot":mode
        }))
        .unwrap();
        self.router.observe(&self.session, args).await
    }
    pub async fn observe_default(&self) -> std::result::Result<ActionState, Box<ToolError>> {
        self.router
            .observe(
                &self.session,
                serde_json::from_value(json!({"app":self.app})).unwrap(),
            )
            .await
    }
}
