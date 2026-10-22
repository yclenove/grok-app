//! Scoped MCP session. No tool can set, renew, or widen the fixture scope.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use gcu_core::driver::{
    AppQuery, AppRef, Cancel, Capture, CaptureOpts, Delivery, DiscoveryStatus, DoctorReport,
    Driver, DriverAction, Platform, Result as DriverResult, TreeOpts, UiEvent, WindowRef,
};
use gcu_core::model::{NativeHandle, RawTree};
use gcu_core::scope::{
    BootIdentity, ElementBinding, ExecutableProof, FixtureScope, FixtureTarget, InstanceEvidence,
    OsStartTime, ProcessInstance, RegisteredWindow, ScopeLease, WindowBinding,
    SCOPE_DENIED_MESSAGE,
};
use gcu_driver_fake::FakeDriver;
use gcu_host_api::testing::{
    FakeUserActivity, MemoryPolicyStore, RecordingEventSink, ScriptedApprovals,
};
use gcu_host_api::{ApprovalDecision, ConfirmationLevel, ForegroundPolicy, PolicyConfig};
use gcu_mcp::{Connection, Host, Profile, ServerConfig};
use gcu_protocol::capabilities::PrivateApiStatus;
use gcu_protocol::Capabilities;
use rmcp::model::CallToolResult;
use serde_json::json;

const CANARIES: &[&str] = &[
    "CANARY-EXTERNAL-C",
    "CANARY-WINDOW-C",
    "/canary/external/c",
    "CANARY-IMPOSTOR-D",
    "CANARY-GHOST",
    "fixture.d",
    "CANARY-PRIVATE",
];

const SCENE: &str = r#"(
apps: [
(id: "fixture.a", name: "Fixture A", executable: "/fixtures/a", pid: 1101, windows: [
(id: "w-a", title: "Allowed A", frame: (x: 0.0, y: 0.0, w: 200.0, h: 120.0), scale: 1.0,
root: Some((id: "root", role: window, states: "ENABLED", name: Some("Allowed A"),
frame: Some((x: 0.0, y: 0.0, w: 200.0, h: 120.0)), children: [])))
]),
(id: "fixture.c", name: "CANARY-EXTERNAL-C", executable: "/canary/external/c", pid: 1103, windows: [
(id: "w-c", title: "CANARY-WINDOW-C", frame: (x: 0.0, y: 400.0, w: 200.0, h: 120.0), scale: 1.0,
root: Some((id: "root", role: window, states: "ENABLED", name: Some("CANARY-WINDOW-C"),
frame: Some((x: 0.0, y: 0.0, w: 200.0, h: 120.0)), children: [])))
]),
(id: "fixture.d", name: "Fixture A", executable: "/fixtures/a", pid: 1104, windows: [
(id: "w-d", title: "CANARY-IMPOSTOR-D", frame: (x: 220.0, y: 400.0, w: 200.0, h: 120.0), scale: 1.0,
root: Some((id: "root", role: window, states: "ENABLED", name: Some("CANARY-IMPOSTOR-D"),
frame: Some((x: 0.0, y: 0.0, w: 200.0, h: 120.0)), children: [])))
]),
(id: "fixture.g", name: "CANARY-GHOST", executable: "/canary/ghost", pid: 1105, windows: [
(id: "w-g", title: "CANARY-GHOST", frame: (x: 440.0, y: 400.0, w: 200.0, h: 120.0), scale: 1.0,
root: Some((id: "root", role: window, states: "ENABLED", name: Some("CANARY-GHOST"),
frame: Some((x: 0.0, y: 0.0, w: 200.0, h: 120.0)), children: [])))
])
])"#;

struct Counts {
    content: AtomicUsize,
    capture: AtomicUsize,
    perform: AtomicUsize,
    launch: AtomicUsize,
    windows_none: AtomicUsize,
}

#[derive(Clone)]
struct Spy {
    inner: FakeDriver,
    caps: Capabilities,
    counts: Arc<Counts>,
}

impl Spy {
    fn new() -> Self {
        let inner = FakeDriver::from_ron(SCENE).unwrap();
        let mut caps = inner.capabilities().clone();
        caps.private_apis
            .insert("CANARY-PRIVATE".into(), PrivateApiStatus::Available);
        Self {
            inner,
            caps,
            counts: Arc::new(Counts {
                content: AtomicUsize::new(0),
                capture: AtomicUsize::new(0),
                perform: AtomicUsize::new(0),
                launch: AtomicUsize::new(0),
                windows_none: AtomicUsize::new(0),
            }),
        }
    }

    fn load(&self, cell: &AtomicUsize) -> usize {
        cell.load(Ordering::SeqCst)
    }
}

fn instance(pid: u32, ticks: u64) -> ProcessInstance {
    ProcessInstance::new(
        pid,
        OsStartTime::new(ticks).unwrap(),
        BootIdentity::new("boot-1").unwrap(),
    )
    .unwrap()
}

fn allowed_scope() -> FixtureScope {
    let owner = instance(1101, 10);
    let window = RegisteredWindow::new(WindowBinding::new("w-a", 1, owner.clone()).unwrap());
    let target = FixtureTarget::new(
        owner,
        "a",
        ExecutableProof::new("/fixtures/a").unwrap(),
        vec![window],
    )
    .unwrap();
    FixtureScope::try_new(
        "run-s1",
        1,
        "engine-s1",
        "desktop-s1",
        "lease-s1",
        vec![target],
    )
    .unwrap()
}

impl InstanceEvidence for Spy {
    fn app_instance(&self, app: &AppRef) -> Option<ProcessInstance> {
        match app.info.id.as_str() {
            "fixture.a" => Some(instance(1101, 10)),
            "fixture.c" => Some(instance(1103, 12)),
            "fixture.d" => Some(instance(1104, 13)),
            _ => None,
        }
    }

    fn window_binding(&self, window: &WindowRef) -> Option<WindowBinding> {
        let (id, owner) = match window.id.as_str() {
            "w-a" => ("w-a", instance(1101, 10)),
            "w-c" => ("w-c", instance(1103, 12)),
            "w-d" => ("w-d", instance(1104, 13)),
            _ => return None,
        };
        Some(WindowBinding::new(id, 1, owner).unwrap())
    }

    fn element_binding(&self, _handle: &NativeHandle) -> Option<ElementBinding> {
        None
    }
}

#[async_trait]
impl Driver for Spy {
    fn platform(&self) -> Platform {
        Platform::Linux
    }

    fn capabilities(&self) -> &Capabilities {
        &self.caps
    }

    async fn doctor(&self) -> DoctorReport {
        let mut report = self.inner.doctor().await;
        report.issues.push("CANARY-EXTERNAL-C".into());
        report.capabilities = self.caps.clone();
        report
    }

    async fn list_apps(&self) -> DriverResult<Vec<gcu_protocol::AppInfo>> {
        self.inner.list_apps().await
    }

    fn discovery_status(&self) -> DiscoveryStatus {
        DiscoveryStatus::default()
    }

    async fn resolve_app(&self, query: &AppQuery) -> DriverResult<AppRef> {
        self.inner.resolve_app(query).await
    }

    async fn launch_app(&self, app: &AppRef) -> DriverResult<AppRef> {
        self.counts.launch.fetch_add(1, Ordering::SeqCst);
        self.inner.launch_app(app).await
    }

    async fn list_windows(
        &self,
        app: Option<&AppRef>,
    ) -> DriverResult<Vec<gcu_protocol::WindowInfo>> {
        if app.is_none() {
            self.counts.windows_none.fetch_add(1, Ordering::SeqCst);
        }
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
        self.counts.content.fetch_add(1, Ordering::SeqCst);
        self.inner.snapshot_tree(win, opts, cancel).await
    }

    async fn capture(
        &self,
        win: &WindowRef,
        opts: &CaptureOpts,
        cancel: &Cancel,
    ) -> DriverResult<Capture> {
        self.counts.capture.fetch_add(1, Ordering::SeqCst);
        self.inner.capture(win, opts, cancel).await
    }

    async fn window_geometry(&self, win: &WindowRef) -> DriverResult<gcu_core::model::Geometry> {
        self.inner.window_geometry(win).await
    }

    async fn focused_element(&self, app: &AppRef) -> DriverResult<Option<NativeHandle>> {
        self.inner.focused_element(app).await
    }

    async fn perform(&self, req: &DriverAction, cancel: &Cancel) -> DriverResult<Delivery> {
        self.counts.perform.fetch_add(1, Ordering::SeqCst);
        self.inner.perform(req, cancel).await
    }

    fn subscribe(&self, win: &WindowRef) -> Option<gcu_core::driver::BoxStream<'static, UiEvent>> {
        self.inner.subscribe(win)
    }

    async fn release_all_input(&self) {
        self.inner.release_all_input().await;
    }
}

fn rendered(result: &CallToolResult) -> String {
    let text = result
        .content
        .iter()
        .filter_map(|block| block.as_text().map(|item| item.text.as_str()))
        .collect::<Vec<_>>()
        .join("\n");
    let structured = serde_json::to_string(&result.structured_content).unwrap_or_default();
    format!("{text}\n{structured}\n{result:?}")
}

fn assert_clean(label: &str, text: &str) {
    for canary in CANARIES {
        assert!(!text.contains(canary), "{label} leaked {canary}: {text}");
    }
}

fn assert_denied(label: &str, result: &CallToolResult) {
    assert_eq!(result.is_error, Some(true), "{label}");
    let body = rendered(result);
    let error = result.structured_content.as_ref().unwrap();
    assert_eq!(error["error"]["code"], "app_denied", "{label} {body}");
    assert_eq!(error["error"]["message"], SCOPE_DENIED_MESSAGE, "{label}");
    assert!(error["error"].get("state").is_none(), "{label} {body}");
    assert_clean(label, &body);
}

#[tokio::test]
async fn scoped_mcp_tools_hide_external_targets_and_reject_scope_editors() {
    let source = include_str!("../src/catalog/profiles.rs");
    assert!(!source.contains("set_scope"));
    assert!(!source.contains("set_fixture_scope"));
    let spy = Spy::new();
    let probe = spy.clone();
    let approvals = Arc::new(ScriptedApprovals::new());
    for _ in 0..8 {
        approvals.push_decision(ApprovalDecision::Session);
    }
    let host = Host {
        approvals,
        policy: Arc::new(MemoryPolicyStore::new(PolicyConfig {
            sensitive_action_confirmation: ConfirmationLevel::Off,
            allow_foreground: ForegroundPolicy::Always,
            timeout_secs: 2,
            ..PolicyConfig::default()
        })),
        events: Arc::new(RecordingEventSink::new()),
        activity: Arc::new(FakeUserActivity::new()),
    };
    let mut connection = Connection::open_scoped(
        spy,
        ScopeLease::install(allowed_scope()),
        host,
        ServerConfig {
            profile: Profile::Full,
            ..ServerConfig::default()
        },
    )
    .await
    .unwrap();

    let apps = connection.call("list_apps", json!({})).await;
    assert_ne!(apps.is_error, Some(true), "{}", rendered(&apps));
    let apps_body = rendered(&apps);
    assert!(apps_body.contains("fixture.a"), "{apps_body}");
    assert_clean("list_apps", &apps_body);

    let windows = connection.call("list_windows", json!({})).await;
    assert_ne!(windows.is_error, Some(true), "{}", rendered(&windows));
    let windows_body = rendered(&windows);
    assert!(windows_body.contains("w-a"), "{windows_body}");
    assert_clean("list_windows", &windows_body);

    let allowed_windows = connection
        .call("list_windows", json!({"app": "fixture.a"}))
        .await;
    assert_ne!(allowed_windows.is_error, Some(true));
    assert_clean("allowed windows", &rendered(&allowed_windows));

    let external_windows = connection
        .call("list_windows", json!({"app": "CANARY-EXTERNAL-C"}))
        .await;
    let missing_windows = connection
        .call("list_windows", json!({"app": "missing-app"}))
        .await;
    let impostor_windows = connection
        .call("list_windows", json!({"app": "fixture.d"}))
        .await;
    assert_denied("external windows", &external_windows);
    assert_eq!(
        external_windows.structured_content,
        missing_windows.structured_content
    );
    assert_eq!(
        external_windows.structured_content,
        impostor_windows.structured_content
    );

    for shot in [json!(false), json!(true), json!("auto")] {
        let denied = connection
            .call(
                "get_app_state",
                json!({"app": "CANARY-EXTERNAL-C", "include_screenshot": shot}),
            )
            .await;
        let missing = connection
            .call(
                "get_app_state",
                json!({"app": "missing-app", "include_screenshot": shot}),
            )
            .await;
        assert_denied("state", &denied);
        assert_eq!(denied.structured_content, missing.structured_content);
    }

    for app in ["CANARY-EXTERNAL-C", "fixture.a", "fixture.d"] {
        let launched = connection.call("launch_app", json!({"app": app})).await;
        assert_denied("launch", &launched);
    }
    let clicked = connection
        .call(
            "click",
            json!({"app": "CANARY-EXTERNAL-C", "element_index": 1}),
        )
        .await;
    assert_denied("click", &clicked);

    let caps = connection.call("capabilities", json!({})).await;
    assert_ne!(caps.is_error, Some(true), "{}", rendered(&caps));
    let caps_body = rendered(&caps);
    assert!(
        caps_body.contains("\"private_apis\":{}") || caps_body.contains("\"private_apis\": {}")
    );
    assert_clean("capabilities", &caps_body);

    for name in ["set_fixture_scope", "set_scope"] {
        let rejected = connection.call(name, json!({"scope": "wider"})).await;
        assert_eq!(rejected.is_error, Some(true), "{name}");
        let body = rendered(&rejected);
        assert!(body.contains("capability_unavailable"), "{name} {body}");
        assert!(
            body.contains("This tool will be available in a later release or another profile."),
            "{name} {body}"
        );
        assert_clean(name, &body);
    }

    assert_eq!(probe.load(&probe.counts.content), 0);
    assert_eq!(probe.load(&probe.counts.capture), 0);
    assert_eq!(probe.load(&probe.counts.perform), 0);
    assert_eq!(probe.load(&probe.counts.launch), 0);
    assert_eq!(probe.load(&probe.counts.windows_none), 0);
    connection.close().await.unwrap();
}
