//! Scoped mode does not deliver a click that an unscoped router sends to DOM.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod fixture_scope_support;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use fixture_scope_support::{element_index, permissive, scope};
use gcu_core::action::ToolCall;
use gcu_core::browser::{BrowserHub, DomChannel, DomEffect, DomFault, DomOperation, Share};
use gcu_core::driver::{Cancel, Driver, DriverAction, TreeOpts, UiEvent, WindowRef};
use gcu_core::model::{NativeHandle, RawNode, RawTree, Role};
use gcu_core::scope::{
    BootIdentity, ElementBinding, ExecutableProof, FixtureScope, FixtureTarget, InstanceEvidence,
    OsStartTime, ProcessInstance, RegisteredWindow, ScopeLease, WindowBinding,
};
use gcu_core::session::{Session, WindowLocks};
use gcu_core::settle::{Clock, SettleConfig, SettleEngine};
use gcu_driver_fake::FakeDriver;
use gcu_host_api::testing::{
    FakeUserActivity, MemoryPolicyStore, RecordingEventSink, ScriptedApprovals,
};
use gcu_host_api::ApprovalDecision;
use gcu_protocol::tools::*;
use serde_json::{json, Value};

const PAGE: &str = r#"(apps: [(id: "fake.webarea", name: "Fixture webarea", executable: "/fixtures/webarea", pid: 1001,
windows: [(id: "page", title: "Fixture webarea", frame: (x: 100.0, y: 100.0, w: 800.0, h: 480.0), scale: 1.0,
root: Some((id: "root", role: window, states: "ENABLED", name: Some("Webarea"),
frame: Some((x: 0.0, y: 0.0, w: 800.0, h: 480.0)), children: [
(id: "web", role: web_area, states: "ENABLED", name: Some("Offline page"),
frame: Some((x: 20.0, y: 40.0, w: 600.0, h: 400.0)), url: Some("https://example.invalid/"), children: [
(id: "link", role: link, states: "ENABLED", name: Some("Documentation"),
frame: Some((x: 40.0, y: 60.0, w: 160.0, h: 30.0)), actions: [press])
])])))])])"#;

#[tokio::test]
async fn unscoped_dom_click_uses_the_channel_and_scoped_click_does_not() {
    let unscoped = dom_router(false).await;
    unscoped.bind().await;
    let text = unscoped.observe().await.text;
    assert!(text.contains("Dom Only"), "{text}");
    let outcome = unscoped
        .execute(click_index(element_index(&text, "Dom Only")))
        .await;
    assert!(outcome.error.is_none(), "{outcome:?}");
    assert!(
        unscoped.dom.performs() > 0,
        "unscoped click missed the DOM channel"
    );

    let scoped = dom_router(true).await;
    scoped.bind().await;
    let text = scoped.observe().await.text;
    assert!(text.contains("Documentation"), "{text}");
    assert!(
        !text.contains("Dom Only"),
        "scoped observe fused DOM: {text}"
    );
    let outcome = scoped
        .execute(click_index(element_index(&text, "Documentation")))
        .await;
    assert!(outcome.error.is_none(), "{outcome:?}");
    assert_eq!(
        scoped.dom.performs(),
        0,
        "scoped click used the DOM channel"
    );
    assert!(
        scoped.driver_performs() > 0,
        "scoped click never reached the driver"
    );
    assert_eq!(scoped.keys(), 0, "scoped path restored a window");
}

struct DomRig {
    driver: WebEvidence,
    dom: Arc<CountingDom>,
    hub: Arc<BrowserHub>,
    router: gcu_core::action::ActionRouter,
    session: Session,
}

impl DomRig {
    async fn bind(&self) {
        let win = self
            .driver
            .inner
            .fixture_window("fake.webarea")
            .await
            .unwrap();
        let raw = self
            .driver
            .inner
            .snapshot_tree(&win, &TreeOpts::default(), &Cancel::default())
            .await
            .unwrap();
        let node = find_role(raw.root.as_ref().unwrap(), Role::WebArea).unwrap();
        self.hub
            .bind(Share {
                share_id: "share1".into(),
                session_id: "dom".into(),
                app_id: "fake.webarea".into(),
                window_id: win.id,
                extension_id: "ext-test".into(),
                web_area_fingerprint: node.fingerprint,
                web_area_frame: node.frame.unwrap(),
                document_id: "doc1".into(),
            })
            .unwrap();
    }

    async fn observe(&self) -> gcu_core::action::ActionState {
        self.router
            .observe(
                &self.session,
                serde_json::from_value(json!({"app": "fake.webarea", "include_screenshot": false}))
                    .unwrap(),
            )
            .await
            .unwrap()
    }

    async fn execute(&self, step: BatchStep) -> gcu_core::action::ToolOutcome {
        let mut call = ToolCall::new("dom", "fake.webarea", step);
        call.settle_budget_ms = Some(0);
        self.router.execute(&self.session, call).await
    }

    fn driver_performs(&self) -> usize {
        self.driver.performs.load(Ordering::SeqCst)
    }

    fn keys(&self) -> usize {
        self.driver.keys.load(Ordering::SeqCst)
    }
}

async fn dom_router(scoped: bool) -> DomRig {
    let driver = WebEvidence::new();
    let dom = CountingDom::new(page_snapshot());
    let hub = Arc::new(BrowserHub::new(dom.clone() as Arc<dyn DomChannel>));
    let activity = Arc::new(FakeUserActivity::new());
    let approvals = Arc::new(ScriptedApprovals::new());
    for _ in 0..8 {
        approvals.push_decision(ApprovalDecision::Session);
    }
    let policy = Arc::new(MemoryPolicyStore::new(permissive()));
    let events = Arc::new(RecordingEventSink::new());
    let router = if scoped {
        gcu_core::action::ActionRouter::with_fixture_scope(
            driver.clone(),
            ScopeLease::install(web_scope()),
            activity.clone(),
            approvals,
            policy,
            events,
            WindowLocks::new(),
        )
    } else {
        gcu_core::action::ActionRouter::new(
            Arc::new(driver.inner.clone()),
            approvals,
            policy,
            events,
            WindowLocks::new(),
        )
    };
    let router = router
        .with_settle_engine(SettleEngine::new(
            SettleConfig::default(),
            Arc::new(InstantClock(Mutex::new(Duration::ZERO))),
        ))
        .with_browser(hub.clone());
    let session = Session::open("dom", activity, router.input_release());
    session.begin_turn().unwrap();
    DomRig {
        driver,
        dom,
        hub,
        router,
        session,
    }
}

fn web_scope() -> FixtureScope {
    let owner = proved();
    let window = RegisteredWindow::new(WindowBinding::new("page", 1, owner.clone()).unwrap());
    let target = FixtureTarget::new(
        owner,
        "web",
        ExecutableProof::new("/fixtures/webarea").unwrap(),
        vec![window],
    )
    .unwrap();
    scope(1, "run-dom", vec![target])
}

fn proved() -> ProcessInstance {
    ProcessInstance::new(
        1001,
        OsStartTime::new(10).unwrap(),
        BootIdentity::new("boot-dom").unwrap(),
    )
    .unwrap()
}

fn find_role(node: &RawNode, role: Role) -> Option<&RawNode> {
    if node.role == role {
        return Some(node);
    }
    node.children
        .iter()
        .find_map(|child| find_role(child, role))
}

fn page_snapshot() -> Value {
    json!({
        "id": "snap1",
        "root": {
            "id": "root", "role": "document", "native_role": "document", "name": null,
            "value": null, "states": [], "frame": {"x": 0.0, "y": 0.0, "w": 600.0, "h": 400.0},
            "children": [{
                "id": "dom-btn", "role": "button", "native_role": "button", "name": "Dom Only",
                "value": null, "states": ["enabled"],
                "frame": {"x": 20.0, "y": 20.0, "w": 160.0, "h": 30.0}, "children": []
            }]
        },
        "frameSpace": "tab_viewport_physical",
        "geometry": {"width": 600.0, "height": 400.0, "scale": 1.0},
        "incomplete": null,
        "handoff": false
    })
}

fn click_index(index: u32) -> BatchStep {
    BatchStep::Click(ClickArgs {
        element_index: Some(index),
        x: None,
        y: None,
        screenshot_id: None,
        click_count: 1,
        mouse_button: MouseButton::Left,
    })
}

struct InstantClock(Mutex<Duration>);

#[async_trait]
impl Clock for InstantClock {
    fn now(&self) -> Duration {
        *self.0.lock().unwrap_or_else(|err| err.into_inner())
    }
    async fn wait_until(&self, deadline: Duration) {
        {
            let mut now = self.0.lock().unwrap_or_else(|err| err.into_inner());
            if *now < deadline {
                *now = deadline;
            }
        }
        tokio::task::yield_now().await;
    }
}

struct CountingDom {
    snapshot: Mutex<Value>,
    performs: AtomicUsize,
}

impl CountingDom {
    fn new(snapshot: Value) -> Arc<Self> {
        Arc::new(Self {
            snapshot: Mutex::new(snapshot),
            performs: AtomicUsize::new(0),
        })
    }
    fn performs(&self) -> usize {
        self.performs.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl DomChannel for CountingDom {
    fn extension_id(&self) -> &str {
        "ext-test"
    }
    async fn observe(&self, _share_id: &str) -> Result<Value, DomFault> {
        Ok(self.snapshot.lock().unwrap().clone())
    }
    async fn perform(&self, _op: &DomOperation) -> Result<DomEffect, DomFault> {
        self.performs.fetch_add(1, Ordering::SeqCst);
        Ok(DomEffect::Done)
    }
}

#[derive(Clone)]
struct WebEvidence {
    inner: FakeDriver,
    performs: Arc<AtomicUsize>,
    keys: Arc<AtomicUsize>,
    handles: Arc<Mutex<Vec<(NativeHandle, String)>>>,
}

impl WebEvidence {
    fn new() -> Self {
        Self {
            inner: FakeDriver::from_ron(PAGE).unwrap(),
            performs: Arc::new(AtomicUsize::new(0)),
            keys: Arc::new(AtomicUsize::new(0)),
            handles: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

impl InstanceEvidence for WebEvidence {
    fn app_instance(&self, app: &gcu_core::driver::AppRef) -> Option<ProcessInstance> {
        (app.info.id == "fake.webarea").then(proved)
    }
    fn window_binding(&self, window: &WindowRef) -> Option<WindowBinding> {
        (window.id == "page").then(|| WindowBinding::new("page", 1, proved()).unwrap())
    }
    fn element_binding(&self, handle: &NativeHandle) -> Option<ElementBinding> {
        let handles = self.handles.lock().unwrap();
        let window = handles
            .iter()
            .find(|(known, _)| known.ptr_eq(handle))
            .map(|(_, id)| id.clone())?;
        (window == "page")
            .then(|| ElementBinding::new(WindowBinding::new("page", 1, proved()).unwrap()))
    }
}

#[async_trait]
impl Driver for WebEvidence {
    fn platform(&self) -> gcu_core::driver::Platform {
        self.inner.platform()
    }
    fn capabilities(&self) -> &gcu_protocol::Capabilities {
        self.inner.capabilities()
    }
    async fn doctor(&self) -> gcu_core::driver::DoctorReport {
        self.inner.doctor().await
    }
    async fn list_apps(&self) -> gcu_core::driver::Result<Vec<gcu_protocol::AppInfo>> {
        self.inner.list_apps().await
    }
    async fn resolve_app(
        &self,
        query: &gcu_core::driver::AppQuery,
    ) -> gcu_core::driver::Result<gcu_core::driver::AppRef> {
        self.inner.resolve_app(query).await
    }
    async fn launch_app(
        &self,
        app: &gcu_core::driver::AppRef,
    ) -> gcu_core::driver::Result<gcu_core::driver::AppRef> {
        self.inner.launch_app(app).await
    }
    async fn list_windows(
        &self,
        app: Option<&gcu_core::driver::AppRef>,
    ) -> gcu_core::driver::Result<Vec<gcu_protocol::WindowInfo>> {
        self.inner.list_windows(app).await
    }
    async fn key_window(
        &self,
        app: &gcu_core::driver::AppRef,
    ) -> gcu_core::driver::Result<gcu_protocol::WindowInfo> {
        self.keys.fetch_add(1, Ordering::SeqCst);
        self.inner.key_window(app).await
    }
    async fn snapshot_tree(
        &self,
        win: &WindowRef,
        opts: &TreeOpts,
        cancel: &Cancel,
    ) -> gcu_core::driver::Result<RawTree> {
        let raw = self.inner.snapshot_tree(win, opts, cancel).await?;
        if let Some(root) = &raw.root {
            let mut handles = self.handles.lock().unwrap();
            remember(root, &win.id, &mut handles);
        }
        Ok(raw)
    }
    async fn capture(
        &self,
        win: &WindowRef,
        opts: &gcu_core::driver::CaptureOpts,
        cancel: &Cancel,
    ) -> gcu_core::driver::Result<gcu_core::driver::Capture> {
        self.inner.capture(win, opts, cancel).await
    }
    async fn window_geometry(
        &self,
        win: &WindowRef,
    ) -> gcu_core::driver::Result<gcu_core::model::Geometry> {
        self.inner.window_geometry(win).await
    }
    async fn focused_element(
        &self,
        app: &gcu_core::driver::AppRef,
    ) -> gcu_core::driver::Result<Option<NativeHandle>> {
        self.inner.focused_element(app).await
    }
    async fn perform(
        &self,
        req: &DriverAction,
        cancel: &Cancel,
    ) -> gcu_core::driver::Result<gcu_core::driver::Delivery> {
        self.performs.fetch_add(1, Ordering::SeqCst);
        self.inner.perform(req, cancel).await
    }
    fn subscribe(&self, win: &WindowRef) -> Option<gcu_core::driver::BoxStream<'static, UiEvent>> {
        self.inner.subscribe(win)
    }
    async fn release_all_input(&self) {
        self.inner.release_all_input().await;
    }
}

fn remember(node: &RawNode, window: &str, handles: &mut Vec<(NativeHandle, String)>) {
    handles.push((node.handle.clone(), window.to_owned()));
    for child in &node.children {
        remember(child, window, handles);
    }
}
