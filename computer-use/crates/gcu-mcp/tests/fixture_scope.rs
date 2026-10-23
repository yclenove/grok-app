//! Scoped MCP session. No tool can set, renew, or widen the fixture scope.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use gcu_core::action::ActionRouter;
use gcu_core::driver::{
    AppQuery, AppRef, Cancel, Capture, CaptureOpts, Delivery, DiscoveryStatus, DoctorReport,
    Driver, DriverAction, Platform, Result as DriverResult, TreeOpts, UiEvent, WindowRef,
};
use gcu_core::model::{Geometry, NativeHandle, RawNode, RawTree, Rect};
use gcu_core::scope::{
    BootIdentity, ElementBinding, ExecutableProof, FixtureScope, FixtureTarget, InstanceEvidence,
    OsStartTime, ProcessInstance, RegisteredWindow, ScopeLease, WindowBinding,
    SCOPE_DENIED_MESSAGE,
};
use gcu_core::session::Session;
use gcu_driver_fake::FakeDriver;
use gcu_host_api::testing::{
    FakeUserActivity, MemoryPolicyStore, RecordingEventSink, ScriptedApprovals,
};
use gcu_host_api::{ApprovalDecision, ConfirmationLevel, ForegroundPolicy, PolicyConfig};
use gcu_mcp::{Connection, Host, Profile, ScriptBody, ScriptRunner, ServeError, ServerConfig};
use gcu_protocol::capabilities::PrivateApiStatus;
use gcu_protocol::tools::{ComputerScriptParams, GetAppStateParams, ScreenshotMode, TextLimit};
use gcu_protocol::Capabilities;
use rmcp::model::CallToolResult;
use serde_json::{json, Value};
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream, ReadHalf, WriteHalf};
use tokio::task::JoinHandle;

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
(id: "w-a", title: "Allowed A", frame: (x: 0.0, y: 0.0, w: 320.0, h: 280.0), scale: 1.0,
root: Some((id: "root", role: window, states: "ENABLED", name: Some("Allowed A"),
frame: Some((x: 0.0, y: 0.0, w: 320.0, h: 280.0)), children: [
(id: "go", role: button, states: "ENABLED", name: Some("Go"),
frame: Some((x: 20.0, y: 40.0, w: 80.0, h: 24.0)), actions: [press])
])))
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
    discovery: AtomicUsize,
    content: AtomicUsize,
    capture: AtomicUsize,
    perform: AtomicUsize,
    launch: AtomicUsize,
    windows_none: AtomicUsize,
}

struct Shared {
    handles: Mutex<Vec<(NativeHandle, String)>>,
    flip: AtomicBool,
    flipped: AtomicBool,
    start_a: AtomicU64,
}

#[derive(Clone)]
struct Spy {
    inner: FakeDriver,
    caps: Capabilities,
    counts: Arc<Counts>,
    shared: Arc<Shared>,
}

impl Spy {
    fn new() -> Self {
        let inner = FakeDriver::from_ron(SCENE).unwrap();
        let mut caps = inner.capabilities().clone();
        caps.private_apis
            .insert("CANARY-PRIVATE".into(), PrivateApiStatus::Available);
        caps.delivery.targeted_pointer = true;
        caps.delivery.targeted_keys = true;
        Self {
            inner: inner.with_targeted_input(true, true),
            caps,
            counts: Arc::new(Counts {
                discovery: AtomicUsize::new(0),
                content: AtomicUsize::new(0),
                capture: AtomicUsize::new(0),
                perform: AtomicUsize::new(0),
                launch: AtomicUsize::new(0),
                windows_none: AtomicUsize::new(0),
            }),
            shared: Arc::new(Shared {
                handles: Mutex::new(Vec::new()),
                flip: AtomicBool::new(false),
                flipped: AtomicBool::new(false),
                start_a: AtomicU64::new(10),
            }),
        }
    }

    fn arm_flip(&self) {
        self.shared.flip.store(true, Ordering::SeqCst);
    }

    fn set_version(&mut self, version: &str) {
        self.caps.version = version.into();
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
            "fixture.a" => Some(instance(1101, self.shared.start_a.load(Ordering::SeqCst))),
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

    fn element_binding(&self, handle: &NativeHandle) -> Option<ElementBinding> {
        let handles = self.shared.handles.lock().unwrap();
        let window = handles
            .iter()
            .find(|(known, _)| known.ptr_eq(handle))
            .map(|(_, id)| id.clone())?;
        let (id, owner) = match window.as_str() {
            "w-a" => (
                "w-a",
                instance(1101, self.shared.start_a.load(Ordering::SeqCst)),
            ),
            "w-c" => ("w-c", instance(1103, 12)),
            _ => return None,
        };
        Some(ElementBinding::new(
            WindowBinding::new(id, 1, owner).unwrap(),
        ))
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
        self.counts.discovery.fetch_add(1, Ordering::SeqCst);
        if self.shared.flip.load(Ordering::SeqCst)
            && !self.shared.flipped.load(Ordering::SeqCst)
            && self.counts.perform.load(Ordering::SeqCst) > 0
        {
            self.shared.flipped.store(true, Ordering::SeqCst);
            self.shared.start_a.store(99, Ordering::SeqCst);
        }
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
        let raw = self.inner.snapshot_tree(win, opts, cancel).await?;
        if let Some(root) = &raw.root {
            let mut handles = self.shared.handles.lock().unwrap();
            remember(root, &win.id, &mut handles);
        }
        Ok(raw)
    }

    async fn capture(
        &self,
        win: &WindowRef,
        opts: &CaptureOpts,
        cancel: &Cancel,
    ) -> DriverResult<Capture> {
        self.counts.capture.fetch_add(1, Ordering::SeqCst);
        if win.id == "w-c" {
            return Ok(Capture {
                bytes: canary_png(),
                width: 1,
                height: 1,
                mime: "image/png".into(),
                geometry: Geometry {
                    frame: Rect {
                        x: 0.0,
                        y: 0.0,
                        w: 1.0,
                        h: 1.0,
                    },
                    scale: 1.0,
                },
            });
        }
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
async fn connection_call_keeps_dispatch_state_out_of_the_callers_stack() {
    tokio::time::timeout(Duration::from_secs(5), async {
        let mut connection = open_scoped(Spy::new(), Profile::Full).await;
        let call = connection.call("list_apps", json!({}));
        // All tool branches share this type, even when no script is called.
        // Catch inline-future multiplication without raising the thread stack.
        assert!(std::mem::size_of_val(&call) < 32 * 1024);
        let result = call.await;
        assert_ne!(result.is_error, Some(true), "{}", rendered(&result));
        connection.close().await.unwrap();
    })
    .await
    .unwrap();
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

fn remember(node: &RawNode, window: &str, handles: &mut Vec<(NativeHandle, String)>) {
    handles.push((node.handle.clone(), window.to_owned()));
    for child in &node.children {
        remember(child, window, handles);
    }
}

fn canary_png() -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut encoder = png::Encoder::new(&mut bytes, 1, 1);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().unwrap();
    writer.write_image_data(&[0xC3, 0x0C, 0x0D, 0xFF]).unwrap();
    writer.finish().unwrap();
    bytes
}

fn has_canary(bytes: &[u8]) -> bool {
    if bytes.windows(3).any(|pixel| pixel == [0xC3, 0x0C, 0x0D]) {
        return true;
    }
    let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    let Ok(mut reader) = decoder.read_info() else {
        return false;
    };
    let mut buf = vec![0; reader.output_buffer_size().unwrap_or(0)];
    let Ok(frame) = reader.next_frame(&mut buf) else {
        return false;
    };
    buf[..frame.buffer_size()]
        .chunks(4)
        .any(|pixel| pixel.len() >= 3 && pixel[0] == 0xC3 && pixel[1] == 0x0C && pixel[2] == 0x0D)
}

struct ObserveRunner;

impl ScriptRunner for ObserveRunner {
    fn run<'a>(
        &'a self,
        router: &'a ActionRouter,
        session: &'a Session,
        _call_id: &'a str,
        params: ComputerScriptParams,
    ) -> Pin<Box<dyn Future<Output = ScriptBody> + Send + 'a>> {
        Box::pin(async move {
            let app = params.code.lines().next().unwrap_or("").trim().to_owned();
            let request = GetAppStateParams {
                app,
                window: None,
                include_screenshot: ScreenshotMode::Explicit(false),
                text_limit: TextLimit::default(),
                max_tree_nodes: 200,
                reason: None,
            };
            match router.observe_enqueued(session, request).await {
                Ok(state) => ScriptBody {
                    output: state.text,
                    error: None,
                },
                Err(error) => ScriptBody {
                    output: String::new(),
                    error: Some(*error),
                },
            }
        })
    }
}

fn host(decisions: usize) -> Host {
    let approvals = Arc::new(ScriptedApprovals::new());
    for _ in 0..decisions {
        approvals.push_decision(ApprovalDecision::Session);
    }
    Host {
        approvals,
        policy: Arc::new(MemoryPolicyStore::new(PolicyConfig {
            sensitive_action_confirmation: ConfirmationLevel::Off,
            allow_foreground: ForegroundPolicy::Always,
            timeout_secs: 2,
            ..PolicyConfig::default()
        })),
        events: Arc::new(RecordingEventSink::new()),
        activity: Arc::new(FakeUserActivity::new()),
    }
}

async fn open_scoped(spy: Spy, profile: Profile) -> Connection {
    Connection::open_scoped(
        spy,
        ScopeLease::install(allowed_scope()),
        host(32),
        ServerConfig {
            profile,
            script: Some(Arc::new(ObserveRunner)),
            ..ServerConfig::default()
        },
    )
    .await
    .unwrap()
}

struct Rpc {
    reader: BufReader<ReadHalf<DuplexStream>>,
    writer: WriteHalf<DuplexStream>,
    task: JoinHandle<Result<(), ServeError>>,
    next: u64,
}

impl Rpc {
    async fn serve(spy: Spy, profile: Profile) -> Self {
        Self::from_connection(open_scoped(spy, profile).await).await
    }

    async fn from_connection(connection: Connection) -> Self {
        let (client, server) = tokio::io::duplex(64 * 1024);
        let (reader, writer) = tokio::io::split(client);
        let (input, output) = tokio::io::split(server);
        let task = tokio::spawn(async move { connection.serve_stream(input, output).await });
        Self {
            reader: BufReader::new(reader),
            writer,
            task,
            next: 1,
        }
    }

    async fn send(&mut self, value: Value) {
        let mut line = serde_json::to_vec(&value).unwrap();
        line.push(b'\n');
        self.writer.write_all(&line).await.unwrap();
    }

    async fn read(&mut self) -> Value {
        let mut line = String::new();
        let n = tokio::time::timeout(Duration::from_secs(20), self.reader.read_line(&mut line))
            .await
            .unwrap()
            .unwrap();
        assert!(n > 0, "server closed the catalog stream");
        serde_json::from_str(&line).unwrap()
    }

    async fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next;
        self.next += 1;
        self.send(json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))
            .await;
        let result = self.read().await;
        assert_eq!(result["id"], id, "{result}");
        result
    }

    async fn initialize(&mut self) {
        self.request(
            "initialize",
            json!({
                "protocolVersion": "2025-11-25",
                "capabilities": {},
                "clientInfo": {"name": "scope-s1", "version": "1"}
            }),
        )
        .await;
        self.send(json!({"jsonrpc":"2.0","method":"notifications/initialized"}))
            .await;
    }

    async fn call(&mut self, name: &str, arguments: Value) -> Value {
        self.request("tools/call", json!({"name": name, "arguments": arguments}))
            .await["result"]
            .clone()
    }

    async fn close(mut self) {
        self.writer.shutdown().await.unwrap();
        assert!(tokio::time::timeout(Duration::from_secs(10), self.task)
            .await
            .unwrap()
            .unwrap()
            .is_ok());
    }
}

fn listed_count(profile: Profile) -> usize {
    match profile {
        Profile::Minimal => 9,
        Profile::Default => 15,
        Profile::Full if cfg!(all(unix, feature = "script")) => 16,
        Profile::Full => 15,
    }
}

fn tool_arguments(name: &str) -> Value {
    let app = "CANARY-EXTERNAL-C";
    match name {
        "list_apps" | "list_windows" | "capabilities" => json!({}),
        "launch_app" => json!({"app": app}),
        "get_app_state" => json!({"app": app, "include_screenshot": false}),
        "click" => json!({"app": app, "element_index": 1}),
        "perform_secondary_action" => json!({"app": app, "element_index": 1, "action": "press"}),
        "set_value" => json!({"app": app, "element_index": 1, "value": "x"}),
        "select_text" => json!({"app": app, "element_index": 1, "text": "x"}),
        "type_text" => json!({"app": app, "text": "nope"}),
        "press_key" => json!({"app": app, "key": "a"}),
        "scroll" => json!({"app": app, "direction": "down", "element_index": 1}),
        "drag" => json!({"app": app, "from_x": 1.0, "from_y": 1.0, "to_x": 2.0, "to_y": 2.0}),
        "wait_for" => {
            json!({"app": app, "until": {"text_present": "missing-token"}, "timeout_ms": 20})
        }
        "computer_batch" => json!({
            "app": app,
            "steps": [{"click": {"element_index": 1}}],
            "stop_on_error": false,
            "final_state": true
        }),
        "computer_script" => json!({"code": "CANARY-EXTERNAL-C\n", "timeout_ms": 5000}),
        other => panic!("tools/list returned {other} without a dispatch argument"),
    }
}

fn assert_rpc_clean(label: &str, result: &Value) {
    assert_clean(label, &result.to_string());
    for block in result["content"].as_array().into_iter().flatten() {
        if block["type"] == "image" {
            let bytes = decode_b64(block["data"].as_str().unwrap_or(""));
            assert!(!has_canary(&bytes), "{label} attached a canary pixel");
        }
    }
}

fn assert_rpc_denied(label: &str, result: &Value) {
    assert_eq!(result["isError"], true, "{label} {result}");
    let error = &result["structuredContent"]["error"];
    assert_eq!(error["code"], "app_denied", "{label} {result}");
    assert_eq!(error["message"], SCOPE_DENIED_MESSAGE, "{label}");
    assert!(error.get("state").is_none(), "{label} {result}");
    assert!(
        result["structuredContent"].get("state").is_none(),
        "{label} kept a top-level state"
    );
    assert_rpc_clean(label, result);
}

#[tokio::test]
async fn each_profile_dispatches_every_listed_tool_through_the_server() {
    for profile in [Profile::Minimal, Profile::Default, Profile::Full] {
        let spy = Spy::new();
        let probe = spy.clone();
        let mut rpc = Rpc::serve(spy, profile).await;
        rpc.initialize().await;
        let listed = rpc.request("tools/list", json!({})).await;
        let tools = listed["result"]["tools"].as_array().expect("tools/list");
        assert_eq!(tools.len(), listed_count(profile), "{profile:?} {listed}");
        let mut names = Vec::new();
        for tool in tools {
            let name = tool["name"].as_str().unwrap();
            names.push(name.to_owned());
            let result = rpc.call(name, tool_arguments(name)).await;
            if matches!(name, "list_apps" | "list_windows" | "capabilities") {
                assert_ne!(result["isError"], true, "{name} {result}");
                let body = result.to_string();
                if name == "list_apps" {
                    assert!(body.contains("fixture.a"), "{body}");
                }
                assert_rpc_clean(name, &result);
            } else {
                assert_rpc_denied(name, &result);
            }
        }
        assert!(names.iter().any(|name| name == "get_app_state"));
        assert_eq!(
            names.iter().any(|name| name == "computer_script"),
            profile == Profile::Full && cfg!(all(unix, feature = "script"))
        );
        assert_eq!(probe.load(&probe.counts.content), 0, "{profile:?}");
        assert_eq!(probe.load(&probe.counts.capture), 0, "{profile:?}");
        assert_eq!(probe.load(&probe.counts.perform), 0, "{profile:?}");
        assert_eq!(probe.load(&probe.counts.launch), 0, "{profile:?}");
        assert_eq!(probe.load(&probe.counts.windows_none), 0, "{profile:?}");
        rpc.close().await;
    }
}

fn element_index(text: &str) -> u32 {
    text.lines()
        .find(|line| line.contains("Go"))
        .and_then(|line| line.split_whitespace().next())
        .and_then(|token| token.parse().ok())
        .expect("allowed observation did not index Go")
}

fn attachment_bytes(result: &CallToolResult) -> Vec<Vec<u8>> {
    result
        .content
        .iter()
        .filter_map(|block| block.as_image().map(|image| decode_b64(&image.data)))
        .collect()
}

async fn assert_files_have_no_canary(connection: &Connection) {
    let exported = connection.export_private_files().await.unwrap();
    for (path, bytes) in exported {
        assert!(!has_canary(&bytes), "{}", path.display());
        assert_clean("export", &String::from_utf8_lossy(&bytes));
    }
}

#[tokio::test]
async fn allowed_binding_is_observable_and_exports_omit_the_canary_pixel() {
    let mut spy = Spy::new();
    spy.set_version("CANARY-VERSION");
    let probe = spy.clone();
    let external = probe
        .resolve_app(&AppQuery("fixture.c".into()))
        .await
        .unwrap();
    let shot = probe
        .capture(
            &WindowRef {
                app: external,
                id: "w-c".into(),
            },
            &CaptureOpts::default(),
            &Cancel::default(),
        )
        .await
        .unwrap();
    assert!(
        has_canary(&shot.bytes),
        "direct capture did not plant the canary"
    );
    let captured = probe.load(&probe.counts.capture);
    let mut connection = open_scoped(spy, Profile::Default).await;

    let caps = connection.call("capabilities", json!({})).await;
    let caps_body = rendered(&caps);
    assert!(caps_body.contains("unlisted"), "{caps_body}");
    assert!(!caps_body.contains("CANARY-VERSION"), "{caps_body}");
    assert_clean("version", &caps_body);

    let denied = connection
        .call(
            "get_app_state",
            json!({"app": "CANARY-EXTERNAL-C", "window": "w-c", "include_screenshot": true}),
        )
        .await;
    assert_denied("external shot", &denied);
    assert!(
        attachment_bytes(&denied).is_empty(),
        "{}",
        rendered(&denied)
    );
    assert_eq!(probe.load(&probe.counts.capture), captured);
    assert_files_have_no_canary(&connection).await;

    let seen = connection
        .call(
            "get_app_state",
            json!({"app": "fixture.a", "window": "w-a", "include_screenshot": true}),
        )
        .await;
    assert_ne!(seen.is_error, Some(true), "{}", rendered(&seen));
    let body = rendered(&seen);
    assert!(body.contains("Allowed A"), "{body}");
    assert!(body.contains("Go"), "{body}");
    assert_clean("allowed state", &body);
    let images = attachment_bytes(&seen);
    assert!(!images.is_empty(), "allowed screenshot was not attached");
    for bytes in &images {
        assert!(!has_canary(bytes));
    }
    let facts = seen.structured_content.as_ref().unwrap();
    let id = facts["state"]["screenshot"]["id"]
        .as_str()
        .expect("screenshot id");
    let cached = connection
        .cached_image(id)
        .expect("image cache missed the shot");
    assert!(!has_canary(&cached.bytes));
    assert_files_have_no_canary(&connection).await;
    assert!(probe.load(&probe.counts.content) > 0);
    connection.close().await.unwrap();
}

#[tokio::test]
async fn scope_batch_result_drops_state_images_and_files() {
    let spy = Spy::new();
    let probe = spy.clone();
    let mut connection = open_scoped(spy, Profile::Default).await;
    let seen = connection
        .call(
            "get_app_state",
            json!({"app": "fixture.a", "window": "w-a", "include_screenshot": false}),
        )
        .await;
    assert_ne!(seen.is_error, Some(true), "{}", rendered(&seen));
    let index = element_index(&rendered(&seen));
    probe.arm_flip();
    let denied = connection
        .call(
            "computer_batch",
            json!({
                "app": "fixture.a",
                "window": "w-a",
                "steps": [
                    {"click": {"element_index": index}},
                    {"click": {"element_index": index}}
                ],
                "stop_on_error": false,
                "final_state": true
            }),
        )
        .await;
    let facts = denied.structured_content.as_ref().unwrap();
    assert_eq!(facts["error"]["message"], SCOPE_DENIED_MESSAGE, "{facts}");
    assert_eq!(facts["failed_step"], 1, "{facts}");
    assert!(facts.get("state").is_none(), "{facts}");
    assert!(facts["error"].get("state").is_none(), "{facts}");
    assert!(
        attachment_bytes(&denied).is_empty(),
        "{}",
        rendered(&denied)
    );
    assert_clean("scope batch", &rendered(&denied));
    assert_files_have_no_canary(&connection).await;
    connection.close().await.unwrap();

    let spy = Spy::new();
    let mut connection = open_scoped(spy, Profile::Default).await;
    let seen = connection
        .call(
            "get_app_state",
            json!({"app": "fixture.a", "window": "w-a", "include_screenshot": false}),
        )
        .await;
    let index = element_index(&rendered(&seen));
    let kept = connection
        .call(
            "computer_batch",
            json!({
                "app": "fixture.a",
                "window": "w-a",
                "steps": [
                    {"click": {"element_index": index}},
                    {"wait_for": {"until": {"text_present": "missing-token"}, "timeout_ms": 20}}
                ],
                "stop_on_error": false,
                "final_state": true
            }),
        )
        .await;
    let facts = kept.structured_content.as_ref().unwrap();
    assert_ne!(facts["error"]["message"], SCOPE_DENIED_MESSAGE, "{facts}");
    assert!(facts["state"].is_object(), "{facts}");
    assert_clean("timeout batch", &rendered(&kept));
    connection.close().await.unwrap();
}

fn decode_b64(data: &str) -> Vec<u8> {
    let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut vals = Vec::new();
    for byte in data.bytes() {
        if byte == b'=' {
            break;
        }
        if byte.is_ascii_whitespace() {
            continue;
        }
        let value = alphabet.iter().position(|item| *item == byte).unwrap();
        vals.push(value as u8);
    }
    let mut out = Vec::new();
    for chunk in vals.chunks(4) {
        let a = chunk[0];
        let b = chunk.get(1).copied().unwrap_or(0);
        let c = chunk.get(2).copied().unwrap_or(0);
        let d = chunk.get(3).copied().unwrap_or(0);
        out.push((a << 2) | (b >> 4));
        if chunk.len() >= 3 {
            out.push((b << 4) | (c >> 2));
        }
        if chunk.len() == 4 {
            out.push((c << 6) | d);
        }
    }
    out
}

// The call future is polled into the delivery queue before Rebind. This also
// models rmcp's separately spawned handlers: the service, not the parent task,
// must install the authority captured when the connection was accepted.
#[cfg(feature = "script")]
struct QueueRunner {
    entered: tokio::sync::Notify,
    release: tokio::sync::Notify,
    calls: AtomicUsize,
}
#[cfg(feature = "script")]
impl ScriptRunner for QueueRunner {
    fn run<'a>(
        &'a self,
        router: &'a ActionRouter,
        session: &'a Session,
        id: &'a str,
        params: ComputerScriptParams,
    ) -> Pin<Box<dyn Future<Output = ScriptBody> + Send + 'a>> {
        Box::pin(async move {
            if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
                self.entered.notify_one();
                self.release.notified().await;
            }
            ObserveRunner.run(router, session, id, params).await
        })
    }
}

#[cfg(feature = "script")]
fn next_scope(generation: u64) -> FixtureScope {
    let owner = instance(1101, 10);
    FixtureScope::try_new(
        "run-s1",
        generation,
        "engine-s1",
        "desktop-s1",
        "lease-s1",
        vec![FixtureTarget::new(
            owner.clone(),
            "a",
            ExecutableProof::new("/fixtures/a").unwrap(),
            vec![RegisteredWindow::new(
                WindowBinding::new("w-a", 1, owner).unwrap(),
            )],
        )
        .unwrap()],
    )
    .unwrap()
}

#[cfg(feature = "script")]
#[tokio::test]
async fn queued_connection_handlers_never_acquire_rebound_generation() {
    tokio::time::timeout(Duration::from_secs(5), async {
        let spy = Spy::new();
        let lease = ScopeLease::install(allowed_scope());
        let gate = Arc::new(QueueRunner {
            entered: tokio::sync::Notify::new(),
            release: tokio::sync::Notify::new(),
            calls: AtomicUsize::new(0),
        });
        let connection = Arc::new(
            Connection::open_scoped(
                spy.clone(),
                lease.clone(),
                host(32),
                ServerConfig {
                    profile: Profile::Full,
                    script: Some(gate.clone()),
                    ..ServerConfig::default()
                },
            )
            .await
            .unwrap(),
        );
        let first = tokio::spawn({
            let connection = connection.clone();
            async move {
                connection
                    .call(
                        "computer_script",
                        json!({"code":"fixture.a","timeout_ms":5000}),
                    )
                    .await
            }
        });
        gate.entered.notified().await;
        {
            let read = connection.call(
                "get_app_state",
                json!({"app":"fixture.a","include_screenshot":false}),
            );
            let script = connection.call(
                "computer_script",
                json!({"code":"fixture.a","timeout_ms":5000}),
            );
            tokio::pin!(read, script);
            for call in [&mut read, &mut script] {
                assert!(
                    std::future::poll_fn(|cx| std::task::Poll::Ready(
                        call.as_mut().poll(cx).is_pending()
                    ))
                    .await
                );
            }
            lease.rebind(next_scope(2)).unwrap();
            gate.release.notify_one();
            assert_denied("nested script reply", &first.await.unwrap());
            assert_denied("queued read", &read.await);
            assert_denied("queued script", &script.await);
        }
        assert_eq!(gate.calls.load(Ordering::SeqCst), 1);
        assert_eq!(spy.load(&spy.counts.discovery), 0);
        assert_eq!(spy.load(&spy.counts.content), 0);
        assert_eq!(spy.load(&spy.counts.capture), 0);
        assert_eq!(spy.load(&spy.counts.perform), 0);
        let mut connection = Arc::try_unwrap(connection).ok().unwrap();
        connection.close().await.unwrap();
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn data_and_commit_expire_without_polling_a_watcher_and_renew_uses_same_deadline() {
    tokio::time::timeout(Duration::from_secs(20), async {
        let spy = Spy::new();
        #[derive(Debug, Default)]
        struct ManualClock(AtomicU64);
        impl gcu_core::scope::Clock for ManualClock {
            fn now(&self) -> Duration {
                Duration::from_secs(self.0.load(Ordering::SeqCst))
            }
        }
        let clock = Arc::new(ManualClock::default());
        let lease = ScopeLease::install_with_deadline(
            allowed_scope(),
            clock.clone(),
            Duration::from_secs(3),
        );
        let commit_lease = ScopeLease::install_with_deadline(
            allowed_scope(),
            clock.clone(),
            Duration::from_secs(3),
        );
        let mut connection = Connection::open_scoped(
            spy.clone(),
            lease.clone(),
            host(32),
            ServerConfig::default(),
        )
        .await
        .unwrap();
        let bound = lease.arm_attempt().unwrap();
        // No watcher exists in this test. A missing handler authority also refuses.
        let raw = ActionRouter::with_fixture_scope(
            spy.clone(),
            lease.clone(),
            Arc::new(FakeUserActivity::new()),
            Arc::new(ScriptedApprovals::new()),
            Arc::new(MemoryPolicyStore::new(PolicyConfig::default())),
            Arc::new(RecordingEventSink::new()),
            gcu_core::session::WindowLocks::new(),
        );
        assert!(raw.bound_driver().list_apps().await.is_err());
        clock.0.store(2, Ordering::SeqCst);
        lease.renew_deadline(Duration::from_secs(6)).unwrap();
        commit_lease.renew_deadline(Duration::from_secs(6)).unwrap();
        clock.0.store(4, Ordering::SeqCst);
        assert!(bound.enter(raw.bound_driver().list_apps()).await.is_ok());
        assert!(lease.commit_if_current(&[("run-s1", 1)], || ()).is_ok());
        let renewed = connection
            .call(
                "get_app_state",
                json!({"app":"fixture.a","include_screenshot":false}),
            )
            .await;
        assert_ne!(
            renewed.is_error,
            Some(true),
            "renewed lease refused content: {}",
            rendered(&renewed)
        );
        let reads = spy.load(&spy.counts.content);
        assert!(
            reads > 0,
            "positive renewed operation never reached the data plane"
        );
        clock.0.store(7, Ordering::SeqCst);
        assert_denied(
            "expired data",
            &connection
                .call(
                    "get_app_state",
                    json!({"app":"fixture.a","include_screenshot":false}),
                )
                .await,
        );
        assert!(bound.enter(raw.bound_driver().list_apps()).await.is_err());
        let committed = AtomicBool::new(false);
        assert!(commit_lease
            .commit_if_current(&[("run-s1", 1)], || committed.store(true, Ordering::SeqCst))
            .is_err());
        assert!(!committed.load(Ordering::SeqCst));
        assert!(lease.arm_attempt().is_none());
        assert!(lease.renew_deadline(Duration::from_secs(12)).is_err());
        assert_eq!(spy.load(&spy.counts.content), reads);
        assert_eq!(spy.load(&spy.counts.perform), 0);
        connection.close().await.unwrap();
    })
    .await
    .unwrap();
}

#[cfg(feature = "script")]
#[tokio::test]
async fn rmcp_spawned_handlers_keep_the_accepted_connection_authority() {
    tokio::time::timeout(Duration::from_secs(5), async {
        let spy = Spy::new();
        let lease = ScopeLease::install(allowed_scope());
        let gate = Arc::new(QueueRunner {
            entered: tokio::sync::Notify::new(), release: tokio::sync::Notify::new(),
            calls: AtomicUsize::new(0),
        });
        let connection = Connection::open_scoped(spy.clone(), lease.clone(), host(32), ServerConfig {
            profile: Profile::Full, script: Some(gate.clone()), ..ServerConfig::default()
        }).await.unwrap();
        let mut rpc = Rpc::from_connection(connection).await;
        rpc.initialize().await;
        rpc.send(json!({"jsonrpc":"2.0","id":10,"method":"tools/call","params":{
            "name":"computer_script","arguments":{"code":"fixture.a","timeout_ms":5000}
        }})).await;
        gate.entered.notified().await;
        for (id, name, arguments) in [
            (11, "get_app_state", json!({"app":"fixture.a","include_screenshot":false})),
            (12, "computer_script", json!({"code":"fixture.a","timeout_ms":5000})),
        ] {
            rpc.send(json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":name,"arguments":arguments}})).await;
        }
        lease.rebind(next_scope(2)).unwrap();
        gate.release.notify_one();
        let mut ids = Vec::new();
        while ids.len() < 3 {
            let answer = rpc.read().await;
            if let Some(id) = answer["id"].as_u64() {
                assert!([10, 11, 12].contains(&id));
                assert_rpc_denied("rmcp old connection", &answer["result"]);
                ids.push(id);
            }
        }
        ids.sort_unstable();
        assert_eq!(ids, vec![10, 11, 12]);
        assert_eq!(spy.load(&spy.counts.discovery), 0);
        assert_eq!(spy.load(&spy.counts.content), 0);
        assert_eq!(spy.load(&spy.counts.perform), 0);
        rpc.close().await;
    }).await.unwrap();
}
