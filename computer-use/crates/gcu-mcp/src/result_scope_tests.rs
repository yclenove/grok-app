//! Real WindowGone and element labels must carry the producing epoch through encode.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use gcu_core::action::{ActionRouter, ToolCall};
use gcu_core::driver::{
    AppQuery, AppRef, BoxStream, Cancel, Capture, CaptureOpts, Delivery, DiscoveryStatus,
    DoctorReport, Driver, DriverAction, DriverError, Platform, Result as DriverResult, TreeOpts,
    UiEvent, WindowRef,
};
use gcu_core::model::{NativeHandle, RawNode, RawTree};
use gcu_core::scope::{
    BootIdentity, ElementBinding, ExecutableProof, FixtureScope, FixtureTarget, InstanceEvidence,
    OsStartTime, ProcessInstance, RegisteredWindow, ScopeLease, WindowBinding,
};
use gcu_core::session::{Session, WindowLocks};
use gcu_driver_fake::faults::Fault;
use gcu_driver_fake::FakeDriver;
use gcu_host_api::testing::{
    FakeUserActivity, MemoryPolicyStore, RecordingEventSink, ScriptedApprovals,
};
use gcu_host_api::{
    ApprovalDecision, ApprovalProvider, ConfirmationLevel, ForegroundPolicy, PolicyConfig,
    UserActivity,
};
use gcu_protocol::tools::{BatchStep, ClickArgs, ComputerBatchParams, GetAppStateParams};
use gcu_protocol::{Capabilities, MouseButton};
use serde_json::Value;

use super::{Results, SessionFiles};

const SCENE: &str = r#"(
apps: [
(id: "fixture.a", name: "Fixture A", executable: "/fixtures/a", pid: 1101, windows: [
(id: "w-a", title: "Allowed A", frame: (x: 0.0, y: 0.0, w: 200.0, h: 120.0), scale: 1.0,
root: Some((id: "root", role: window, states: "ENABLED", name: Some("Allowed A"),
frame: Some((x: 0.0, y: 0.0, w: 200.0, h: 120.0)), children: [
(id: "go", role: button, states: "ENABLED", name: Some("Go"),
frame: Some((x: 20.0, y: 40.0, w: 80.0, h: 24.0)), actions: [press])
]))),
(id: "w-a2", title: "CANARY-SIBLING-A2", frame: (x: 220.0, y: 0.0, w: 200.0, h: 120.0), scale: 1.0,
root: Some((id: "root", role: window, states: "ENABLED", name: Some("CANARY-SIBLING-A2"),
frame: Some((x: 0.0, y: 0.0, w: 200.0, h: 120.0)), children: [
(id: "go", role: button, states: "ENABLED", name: Some("CANARY-ELEMENT"),
frame: Some((x: 20.0, y: 40.0, w: 80.0, h: 24.0)), actions: [press])
])))
]),
(id: "fixture.b", name: "Fixture B", executable: "/fixtures/b", pid: 1102, windows: [
(id: "w-b", title: "Allowed B", frame: (x: 0.0, y: 200.0, w: 200.0, h: 120.0), scale: 1.0,
root: Some((id: "root", role: window, states: "ENABLED", name: Some("Allowed B"),
frame: Some((x: 0.0, y: 0.0, w: 200.0, h: 120.0)), children: [
(id: "go", role: button, states: "ENABLED", name: Some("Go"),
frame: Some((x: 20.0, y: 40.0, w: 80.0, h: 24.0)), actions: [press])
])))
])
])"#;

const SIBLING: &str = "CANARY-SIBLING-A2";
const ELEMENT: &str = "CANARY-ELEMENT";

struct Evidence {
    apps: BTreeMap<String, ProcessInstance>,
    windows: BTreeMap<String, WindowBinding>,
    handles: Vec<(NativeHandle, String)>,
}

#[derive(Clone)]
struct EvidenceDriver {
    inner: FakeDriver,
    evidence: Arc<Mutex<Evidence>>,
}

impl EvidenceDriver {
    fn new() -> Self {
        let boot = BootIdentity::new("boot-s1").unwrap();
        let mut apps = BTreeMap::new();
        let mut windows = BTreeMap::new();
        for (id, pid, ticks, wins) in [
            ("fixture.a", 1101, 10, &["w-a", "w-a2"][..]),
            ("fixture.b", 1102, 11, &["w-b"][..]),
        ] {
            let instance =
                ProcessInstance::new(pid, OsStartTime::new(ticks).unwrap(), boot.clone()).unwrap();
            apps.insert(id.to_owned(), instance.clone());
            for win in wins {
                windows.insert(
                    (*win).to_owned(),
                    WindowBinding::new(*win, 1, instance.clone()).unwrap(),
                );
            }
        }
        Self {
            inner: FakeDriver::from_ron(SCENE).unwrap(),
            evidence: Arc::new(Mutex::new(Evidence {
                apps,
                windows,
                handles: Vec::new(),
            })),
        }
    }

    async fn inject(&self, fault: Fault) {
        self.inner.inject(fault).await.unwrap();
    }

    fn remember(&self, tree: &RawTree, window: &str) {
        let mut ev = self.evidence.lock().unwrap_or_else(|err| err.into_inner());
        if let Some(root) = &tree.root {
            remember(root, window, &mut ev.handles);
        }
    }
}

fn remember(node: &RawNode, window: &str, handles: &mut Vec<(NativeHandle, String)>) {
    handles.push((node.handle.clone(), window.to_owned()));
    for child in &node.children {
        remember(child, window, handles);
    }
}

impl InstanceEvidence for EvidenceDriver {
    fn app_instance(&self, app: &AppRef) -> Option<ProcessInstance> {
        self.evidence
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .apps
            .get(&app.info.id)
            .cloned()
    }

    fn window_binding(&self, window: &WindowRef) -> Option<WindowBinding> {
        self.evidence
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .windows
            .get(&window.id)
            .cloned()
    }

    fn element_binding(&self, handle: &NativeHandle) -> Option<ElementBinding> {
        let ev = self.evidence.lock().unwrap_or_else(|err| err.into_inner());
        let window = ev
            .handles
            .iter()
            .find(|(known, _)| known.ptr_eq(handle))
            .map(|(_, id)| id.clone())?;
        ev.windows.get(&window).cloned().map(ElementBinding::new)
    }
}

#[async_trait]
impl Driver for EvidenceDriver {
    fn platform(&self) -> Platform {
        self.inner.platform()
    }
    fn capabilities(&self) -> &Capabilities {
        self.inner.capabilities()
    }
    async fn doctor(&self) -> DoctorReport {
        self.inner.doctor().await
    }
    async fn list_apps(&self) -> DriverResult<Vec<gcu_protocol::AppInfo>> {
        self.inner.list_apps().await
    }
    fn discovery_status(&self) -> DiscoveryStatus {
        self.inner.discovery_status()
    }
    async fn resolve_app(&self, query: &AppQuery) -> DriverResult<AppRef> {
        self.inner.resolve_app(query).await
    }
    async fn launch_app(&self, app: &AppRef) -> DriverResult<AppRef> {
        self.inner.launch_app(app).await
    }
    async fn list_windows(
        &self,
        app: Option<&AppRef>,
    ) -> DriverResult<Vec<gcu_protocol::WindowInfo>> {
        self.inner.list_windows(app).await
    }
    async fn key_window(&self, app: &AppRef) -> DriverResult<gcu_protocol::WindowInfo> {
        self.inner.key_window(app).await
    }
    async fn snapshot_tree(
        &self,
        win: &WindowRef,
        opts: &TreeOpts,
        cancel: &Cancel,
    ) -> DriverResult<RawTree> {
        let raw = self.inner.snapshot_tree(win, opts, cancel).await?;
        self.remember(&raw, &win.id);
        Ok(raw)
    }
    async fn capture(
        &self,
        win: &WindowRef,
        opts: &CaptureOpts,
        cancel: &Cancel,
    ) -> DriverResult<Capture> {
        self.inner.capture(win, opts, cancel).await
    }
    async fn window_geometry(&self, win: &WindowRef) -> DriverResult<gcu_core::model::Geometry> {
        self.inner.window_geometry(win).await
    }
    async fn focused_element(&self, app: &AppRef) -> DriverResult<Option<NativeHandle>> {
        self.inner.focused_element(app).await
    }
    async fn perform(
        &self,
        req: &DriverAction,
        cancel: &Cancel,
    ) -> std::result::Result<Delivery, DriverError> {
        self.inner.perform(req, cancel).await
    }
    fn subscribe(&self, win: &WindowRef) -> Option<BoxStream<'static, UiEvent>> {
        self.inner.subscribe(win)
    }
    async fn release_all_input(&self) {
        self.inner.release_all_input().await;
    }
}

struct Desk {
    driver: EvidenceDriver,
    router: ActionRouter,
    session: Arc<Session>,
    lease: ScopeLease,
    files: Arc<SessionFiles>,
}

impl Desk {
    fn open() -> Self {
        let driver = EvidenceDriver::new();
        let lease = ScopeLease::install(allowed(1));
        let activity: Arc<dyn UserActivity> = Arc::new(FakeUserActivity::new());
        let approvals = Arc::new(ScriptedApprovals::new());
        for _ in 0..64 {
            approvals.push_decision(ApprovalDecision::Session);
        }
        let router = ActionRouter::with_fixture_scope(
            driver.clone(),
            lease.clone(),
            Arc::clone(&activity),
            approvals as Arc<dyn ApprovalProvider>,
            Arc::new(MemoryPolicyStore::new(PolicyConfig {
                sensitive_action_confirmation: ConfirmationLevel::Off,
                allow_foreground: ForegroundPolicy::Always,
                ..PolicyConfig::default()
            })),
            Arc::new(RecordingEventSink::new()),
            WindowLocks::new(),
        );
        let session = Arc::new(Session::open("scope-r3", activity, router.input_release()));
        session.begin_turn().unwrap();
        let files = Arc::new(SessionFiles::new().unwrap());
        Self {
            driver,
            router,
            session,
            lease,
            files,
        }
    }

    fn results(&self) -> Results {
        Results {
            files: Arc::clone(&self.files),
            session: Arc::clone(&self.session),
            encoding: Default::default(),
            images: Arc::new(crate::image_cache::ImageCache::default()),
            scope: Some(self.lease.clone()),
            probe: super::FinishProbe::new(),
        }
    }
}

fn allowed(generation: u64) -> FixtureScope {
    let boot = BootIdentity::new("boot-s1").unwrap();
    let a = ProcessInstance::new(1101, OsStartTime::new(10).unwrap(), boot.clone()).unwrap();
    let b = ProcessInstance::new(1102, OsStartTime::new(11).unwrap(), boot).unwrap();
    let targets = if generation == 1 {
        vec![
            target(&a, "a", "/fixtures/a", &["w-a", "w-a2"]),
            target(&b, "b", "/fixtures/b", &["w-b"]),
        ]
    } else {
        vec![target(&b, "b", "/fixtures/b", &["w-b"])]
    };
    FixtureScope::try_new(
        "run-s1",
        generation,
        "engine-s1",
        "desktop-s1",
        "lease-s1",
        targets,
    )
    .unwrap()
}

fn target(instance: &ProcessInstance, role: &str, path: &str, windows: &[&str]) -> FixtureTarget {
    let registered = windows
        .iter()
        .map(|id| RegisteredWindow::new(WindowBinding::new(*id, 1, instance.clone()).unwrap()))
        .collect();
    FixtureTarget::new(
        instance.clone(),
        role,
        ExecutableProof::new(path).unwrap(),
        registered,
    )
    .unwrap()
}

fn only_b() -> gcu_core::scope::FixtureScope {
    allowed(2)
}

async fn observe(desk: &Desk, window: &str) -> gcu_core::action::ActionState {
    desk.router
        .observe(&desk.session, state_params(window))
        .await
        .unwrap()
}

fn state_params(window: &str) -> GetAppStateParams {
    serde_json::from_value(serde_json::json!({
        "app": "fixture.a",
        "window": window,
        "include_screenshot": false
    }))
    .unwrap()
}

async fn click(desk: &Desk, window: &str, index: u32) -> gcu_core::action::ToolOutcome {
    let mut call = ToolCall::new(
        "click",
        "fixture.a",
        BatchStep::Click(ClickArgs {
            element_index: Some(index),
            x: None,
            y: None,
            screenshot_id: None,
            click_count: 1,
            mouse_button: MouseButton::Left,
        }),
    );
    call.window = Some(window.to_owned());
    call.settle_budget_ms = Some(0);
    desk.router.execute(&desk.session, call).await
}

fn element_index(text: &str, label: &str) -> u32 {
    text.lines()
        .find(|line| line.contains(label))
        .and_then(|line| line.split_whitespace().next())
        .and_then(|token| token.parse().ok())
        .unwrap_or_else(|| panic!("missing {label} in observation"))
}

async fn rendered(result: rmcp::model::CallToolResult) -> (String, Value) {
    let value = serde_json::to_value(&result).unwrap();
    let text = value["content"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|block| block.get("text").and_then(|text| text.as_str()))
        .collect::<Vec<_>>()
        .join("\n");
    (text, value)
}

fn assert_absent(label: &str, text: &str, value: &Value) {
    let rendered = format!("{text} {value}");
    assert!(
        !rendered.contains(SIBLING),
        "{label} leaked sibling: {rendered}"
    );
    assert!(
        !rendered.contains(ELEMENT),
        "{label} leaked element: {rendered}"
    );
    assert!(
        value["structuredContent"].get("windows").is_none(),
        "{label}"
    );
    assert!(value["structuredContent"].get("state").is_none(), "{label}");
    assert!(
        value["structuredContent"]["error"].get("state").is_none()
            || value["structuredContent"].get("error").is_none(),
        "{label}"
    );
    assert!(
        value.pointer("/structuredContent/action/target").is_none(),
        "{label}"
    );
}

#[tokio::test]
async fn window_gone_sibling_title_is_dropped_on_revoke_or_rebind_before_encode() {
    for remove in [false, true] {
        let desk = Desk::open();
        observe(&desk, "w-a").await;
        desk.driver
            .inject(Fault::WindowGone {
                window: "w-a".into(),
            })
            .await;
        let fresh = click(&desk, "w-a", 1).await;
        let results = desk.results();
        let (text, value) = rendered(results.outcome(fresh).await).await;
        assert!(text.contains(SIBLING), "{text}");
        assert!(value["structuredContent"]["windows"]
            .to_string()
            .contains(SIBLING));
        assert_eq!(value["structuredContent"]["error"]["code"], "window_gone");
        assert_eq!(value["structuredContent"]["action"]["outcome"], "refused");

        let stale = click(&desk, "w-a", 1).await;
        if remove {
            desk.lease.rebind(only_b()).unwrap();
        } else {
            desk.lease.revoke();
        }
        let (text, value) = rendered(results.outcome(stale).await).await;
        assert_absent(if remove { "rebind" } else { "revoke" }, &text, &value);
        assert_eq!(value["structuredContent"]["error"]["code"], "window_gone");
        assert_eq!(
            value["structuredContent"]["error"]["message"], "window gone",
            "{value}"
        );
        assert_eq!(value["structuredContent"]["action"]["outcome"], "refused");
        results.files.close().unwrap();
    }
}

#[tokio::test]
async fn element_label_and_batch_target_are_dropped_when_the_epoch_is_stale() {
    for remove in [false, true] {
        let desk = Desk::open();
        let seen = observe(&desk, "w-a2").await;
        let index = element_index(&seen.text, ELEMENT);
        let results = desk.results();
        let fresh = click(&desk, "w-a2", index).await;
        let (text, value) = rendered(results.outcome(fresh).await).await;
        assert!(
            value
                .pointer("/structuredContent/action/target")
                .unwrap()
                .to_string()
                .contains(ELEMENT),
            "{text} {value}"
        );

        let stale = click(&desk, "w-a2", index).await;
        if remove {
            desk.lease.rebind(only_b()).unwrap();
        } else {
            desk.lease.revoke();
        }
        let (text, value) = rendered(results.outcome(stale).await).await;
        assert_absent(if remove { "rebind" } else { "revoke" }, &text, &value);
        assert_eq!(value["structuredContent"]["action"]["outcome"], "done");
        results.files.close().unwrap();
    }

    let desk = Desk::open();
    let seen = observe(&desk, "w-a2").await;
    let index = element_index(&seen.text, ELEMENT);
    let params = ComputerBatchParams {
        app: "fixture.a".into(),
        window: Some("w-a2".into()),
        steps: vec![BatchStep::Click(ClickArgs {
            element_index: Some(index),
            x: None,
            y: None,
            screenshot_id: None,
            click_count: 1,
            mouse_button: MouseButton::Left,
        })],
        stop_on_error: true,
        final_state: false,
    };
    let results = desk.results();
    let kept = desk
        .router
        .run_batch(&desk.session, "batch-live", params.clone())
        .await;
    let (text, value) = rendered(results.batch(kept).await).await;
    assert!(
        value
            .pointer("/structuredContent/action/target")
            .unwrap()
            .to_string()
            .contains(ELEMENT),
        "same epoch dropped the label: {text} {value}"
    );
    let stale = desk
        .router
        .run_batch(&desk.session, "batch-stale", params)
        .await;
    desk.lease.rebind(only_b()).unwrap();
    let (text, value) = rendered(results.batch(stale).await).await;
    assert_absent("batch", &text, &value);
    assert_eq!(value["structuredContent"]["action"]["outcome"], "done");
    results.files.close().unwrap();
}
