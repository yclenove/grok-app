//! Scoped MCP session. No tool can set, renew, or widen the fixture scope.
#![allow(clippy::unwrap_used, clippy::expect_used)]

#[path = "fixture_scope/transport.rs"]
mod transport;

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
    "CANARY-WINDOW-A3",
];

const SCENE: &str = include_str!("fixture_scope/desktop.ron");

struct Counts {
    discovery: AtomicUsize,
    content: AtomicUsize,
    capture: AtomicUsize,
    perform: AtomicUsize,
    launch: AtomicUsize,
    windows_none: AtomicUsize,
    // Read, capture, delivery, launch outside A/A2/B, including C and D.
    outside: [AtomicUsize; 4],
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
    clipboard: bool,
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
            clipboard: false,
            caps,
            counts: Arc::new(Counts {
                discovery: AtomicUsize::new(0),
                content: AtomicUsize::new(0),
                capture: AtomicUsize::new(0),
                perform: AtomicUsize::new(0),
                launch: AtomicUsize::new(0),
                windows_none: AtomicUsize::new(0),
                outside: std::array::from_fn(|_| AtomicUsize::new(0)),
            }),
            shared: Arc::new(Shared {
                handles: Mutex::new(Vec::new()),
                flip: AtomicBool::new(false),
                flipped: AtomicBool::new(false),
                start_a: AtomicU64::new(10),
            }),
        }
    }

    fn input_mode(mut self, mode: &str) -> Self {
        let targeted = mode == "targeted";
        self.inner = self.inner.with_targeted_input(targeted, targeted);
        self.caps.delivery.targeted_pointer = targeted;
        self.caps.delivery.targeted_keys = targeted;
        self.clipboard = mode == "clipboard";
        self
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

    fn track_window(&self, operation: usize, window: Option<&str>) {
        if !matches!(window, Some("w-a" | "w-a2" | "w-b")) {
            self.counts.outside[operation].fetch_add(1, Ordering::SeqCst);
        }
    }

    fn action_window(&self, action: &DriverAction) -> Option<String> {
        let handle = match action {
            DriverAction::PointerClick { win, .. }
            | DriverAction::PointerScroll { win, .. }
            | DriverAction::PointerDrag { win, .. }
            | DriverAction::TypeText { win, .. }
            | DriverAction::PressKeys { win, .. }
            | DriverAction::Activate { win } => return Some(win.id.clone()),
            DriverAction::Press { el }
            | DriverAction::SecondaryAction { el, .. }
            | DriverAction::SetValue { el, .. }
            | DriverAction::SelectText { el, .. }
            | DriverAction::ScrollElement { el, .. } => el,
        };
        self.shared
            .handles
            .lock()
            .unwrap()
            .iter()
            .find(|(known, _)| known.ptr_eq(handle))
            .map(|(_, window)| window.clone())
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
    scope_for_run("run-s1")
}

fn scope_for_run(run: &str) -> FixtureScope {
    let targets = [
        (1101, 10, "a", &["w-a", "w-a2"][..]),
        (1102, 11, "b", &["w-b"][..]),
    ]
    .into_iter()
    .map(|(pid, ticks, role, windows)| {
        let owner = instance(pid, ticks);
        FixtureTarget::new(
            owner.clone(),
            role,
            ExecutableProof::new(format!("/fixtures/{role}")).unwrap(),
            windows
                .iter()
                .map(|id| RegisteredWindow::new(WindowBinding::new(*id, 1, owner.clone()).unwrap()))
                .collect(),
        )
        .unwrap()
    })
    .collect();
    FixtureScope::try_new(run, 1, "engine-s1", "desktop-s1", "lease-s1", targets).unwrap()
}

impl InstanceEvidence for Spy {
    fn app_instance(&self, app: &AppRef) -> Option<ProcessInstance> {
        match app.info.id.as_str() {
            "fixture.a" => Some(instance(1101, self.shared.start_a.load(Ordering::SeqCst))),
            "fixture.b" => Some(instance(1102, 11)),
            "fixture.c" => Some(instance(1103, 12)),
            "fixture.d" => Some(instance(1104, 13)),
            _ => None,
        }
    }

    fn window_binding(&self, window: &WindowRef) -> Option<WindowBinding> {
        let (id, owner) = match window.id.as_str() {
            "w-a" | "w-a2" | "w-a3" => (window.id.as_str(), instance(1101, 10)),
            "w-b" => ("w-b", instance(1102, 11)),
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
            "w-a" | "w-a2" | "w-a3" => (
                window.as_str(),
                instance(1101, self.shared.start_a.load(Ordering::SeqCst)),
            ),
            "w-b" => ("w-b", instance(1102, 11)),
            "w-c" => ("w-c", instance(1103, 12)),
            "w-d" => ("w-d", instance(1104, 13)),
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
        if !matches!(app.info.id.as_str(), "fixture.a" | "fixture.b") {
            self.counts.outside[3].fetch_add(1, Ordering::SeqCst);
        }
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
        self.track_window(0, Some(&win.id));
        let mut raw = self.inner.snapshot_tree(win, opts, cancel).await?;
        if self.clipboard {
            if let Some(root) = &mut raw.root {
                native_text_only(root);
            }
        }
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
        self.track_window(1, Some(&win.id));
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
        self.track_window(2, self.action_window(req).as_deref());
        let delivery = self.inner.perform(req, cancel).await?;
        if self.clipboard && matches!(req, DriverAction::TypeText { .. }) {
            Ok(Delivery::Clipboard)
        } else {
            Ok(delivery)
        }
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

// Model a field without semantic SetValue support. The inner fake still edits
// its text through TypeText; the spy reports synthetic clipboard delivery.
fn native_text_only(node: &mut RawNode) {
    node.states.remove(gcu_core::model::States::SETTABLE);
    for child in &mut node.children {
        native_text_only(child);
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
                window: params.code.lines().nth(1).map(str::to_owned),
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
                    error: Some((*error).into()),
                },
            }
        })
    }
}

fn host(decisions: usize) -> Host {
    recording_host(decisions).0
}

fn recording_host(decisions: usize) -> (Host, Arc<ScriptedApprovals>) {
    let approvals = Arc::new(ScriptedApprovals::new());
    for _ in 0..decisions {
        approvals.push_decision(ApprovalDecision::Session);
    }
    let host = Host {
        approvals: approvals.clone(),
        policy: Arc::new(MemoryPolicyStore::new(PolicyConfig {
            sensitive_action_confirmation: ConfirmationLevel::Off,
            allow_foreground: ForegroundPolicy::Always,
            timeout_secs: 2,
            ..PolicyConfig::default()
        })),
        events: Arc::new(RecordingEventSink::new()),
        activity: Arc::new(FakeUserActivity::new()),
    };
    (host, approvals)
}

async fn open_scoped(spy: Spy, profile: Profile) -> Connection {
    open_scoped_with_host(spy, profile, host(32)).await
}

async fn open_scoped_with_host(spy: Spy, profile: Profile, host: Host) -> Connection {
    Connection::open_scoped(
        spy,
        ScopeLease::install(allowed_scope()),
        host,
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

// Every listed tool is mapped explicitly: catalog growth must add a row.
// Input variants are tool parameters; clipboard/foreground are driver routes.
fn tool_arguments(name: &str, app: &str, window: Option<&str>) -> Vec<Value> {
    let mut args = match name {
        "list_apps" | "capabilities" => vec![json!({})],
        "list_windows" | "launch_app" => vec![json!({"app": app})],
        "get_app_state" => [json!(false), json!(true), json!("auto")]
            .into_iter()
            .map(|shot| json!({"app": app, "include_screenshot": shot}))
            .collect(),
        "click" => vec![
            json!({"app": app, "element_index": 1}),
            json!({"app": app, "x": 25.0, "y": 45.0}),
            json!({"app": app, "x": 25.0, "y": 45.0, "screenshot_id": "i_foreign_run"}),
        ],
        "perform_secondary_action" => {
            vec![json!({"app": app, "element_index": 1, "action": "press"})]
        }
        "set_value" => vec![json!({"app": app, "element_index": 1, "value": "x"})],
        "select_text" => vec![json!({"app": app, "element_index": 1, "text": "x"})],
        "type_text" => vec![
            json!({"app": app, "text": "nope"}),
            json!({"app": app, "text": "paste".repeat(128), "element_index": 1}),
        ],
        "press_key" => vec![json!({"app": app, "key": "ctrl+v"})],
        "scroll" => vec![
            json!({"app": app, "direction": "down", "element_index": 1}),
            json!({"app": app, "direction": "down", "x": 25.0, "y": 45.0}),
        ],
        "drag" => {
            vec![json!({"app": app, "from_x": 25.0, "from_y": 45.0, "to_x": 30.0, "to_y": 50.0})]
        }
        "wait_for" => {
            vec![json!({"app": app, "until": {"text_present": "missing-token"}, "timeout_ms": 20})]
        }
        "computer_batch" => vec![json!({
            "app": app,
            "steps": [{"click": {"element_index": 1}}, {"type_text": {"text": "later"}}],
            "stop_on_error": false,
            "final_state": true
        })],
        // ObserveRunner is the existing in-process adapter. Actual JS hard-stop
        // behavior is covered separately in gcu-script/tests/fixture_scope.rs.
        "computer_script" => {
            vec![json!({"code": format!("{app}\n{}", window.unwrap_or("")), "timeout_ms": 5000})]
        }
        other => panic!("tools/list returned {other} without a dispatch argument"),
    };
    if !matches!(
        name,
        "list_apps" | "list_windows" | "launch_app" | "capabilities" | "computer_script"
    ) {
        if let Some(window) = window {
            for arg in &mut args {
                arg["window"] = json!(window);
            }
        }
    }
    args
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
    assert!(
        result["structuredContent"].get("windows").is_none(),
        "{label} {result}"
    );
    assert!(
        result["content"]
            .as_array()
            .unwrap()
            .iter()
            .all(|block| block["type"] == "text"),
        "{label} attached an image or resource: {result}"
    );
    assert_rpc_clean(label, result);
}

#[tokio::test]
async fn each_profile_dispatches_every_listed_tool_through_the_server() {
    tokio::time::timeout(Duration::from_secs(120), async {
        for profile in [Profile::Minimal, Profile::Default, Profile::Full] {
            for mode in ["targeted", "foreground", "clipboard"] {
                let spy = Spy::new().input_mode(mode);
                let probe = spy.clone();
                let (host, approvals) = recording_host(32);
                let mut rpc =
                    Rpc::from_connection(open_scoped_with_host(spy, profile, host).await).await;
                rpc.initialize().await;
                let listed = rpc.request("tools/list", json!({})).await;
                assert_rpc_clean("catalog", &listed);
                let tools = listed["result"]["tools"].as_array().expect("tools/list");
                assert_eq!(tools.len(), listed_count(profile), "{profile:?} {listed}");
                assert!(tools.iter().any(|tool| tool["name"] == "get_app_state"));
                assert_eq!(
                    tools.iter().any(|tool| tool["name"] == "computer_script"),
                    profile == Profile::Full && cfg!(all(unix, feature = "script"))
                );
                for tool in tools {
                    let name = tool["name"].as_str().unwrap();
                    for (app, window) in [
                        ("CANARY-EXTERNAL-C", None),
                        ("fixture.d", None),
                        ("missing-app", None),
                        ("fixture.a", Some("w-a3")),
                        ("fixture.a", Some("forged-window")),
                    ] {
                        for args in tool_arguments(name, app, window) {
                            let label = format!("{profile:?}/{mode}/{name}/{app}/{window:?}");
                            let result = rpc.call(name, args).await;
                            // Discovery and capabilities have no window parameter.
                            // App-level discovery of A is allowed and filters A3.
                            if matches!(name, "list_apps" | "capabilities")
                                || (name == "list_windows" && window.is_some())
                            {
                                assert_ne!(result["isError"], true, "{label} {result}");
                                assert_rpc_clean(&label, &result);
                            } else {
                                assert_rpc_denied(&label, &result);
                                assert!(
                                    approvals.app_requests().is_empty(),
                                    "{label}: requested app approval"
                                );
                                assert!(
                                    approvals.confirm_requests().is_empty(),
                                    "{label}: requested confirmation"
                                );
                            }
                        }
                    }
                }
                assert_eq!(probe.load(&probe.counts.content), 0, "{profile:?}/{mode}");
                assert_eq!(probe.load(&probe.counts.capture), 0, "{profile:?}/{mode}");
                assert_eq!(probe.load(&probe.counts.perform), 0, "{profile:?}/{mode}");
                assert_eq!(probe.load(&probe.counts.launch), 0, "{profile:?}/{mode}");
                assert_eq!(
                    probe.load(&probe.counts.windows_none),
                    0,
                    "{profile:?}/{mode}"
                );
                // Same closed engine, now prove A, A2, and registered B work.
                for (app, window) in [
                    ("fixture.a", "w-a"),
                    ("fixture.a", "w-a2"),
                    ("fixture.b", "w-b"),
                ] {
                    let seen = rpc
                        .call(
                            "get_app_state",
                            json!({
                                "app": app, "window": window, "include_screenshot": true
                            }),
                        )
                        .await;
                    assert_ne!(seen["isError"], true, "{seen}");
                    assert_rpc_clean("allowed state", &seen);
                    let text = seen["content"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .filter_map(|block| block["text"].as_str())
                        .collect::<Vec<_>>()
                        .join("\n");
                    let index = element_index(&text);
                    let clicked = rpc
                        .call(
                            "click",
                            json!({"app":app,"window":window,"element_index":index}),
                        )
                        .await;
                    assert_ne!(clicked["isError"], true, "{clicked}");
                    assert_rpc_clean("allowed click", &clicked);
                    // Click the editable field using the screenshot, then type
                    // through a real fake-driver text operation.
                    let seen = rpc
                        .call(
                            "get_app_state",
                            json!({"app":app,"window":window,"include_screenshot":true}),
                        )
                        .await;
                    assert_ne!(seen["isError"], true, "{seen}");
                    let clicked = rpc
                        .call(
                            "click",
                            json!({"app":app,"window":window,"x":25.0,"y":85.0}),
                        )
                        .await;
                    assert_ne!(clicked["isError"], true, "{clicked}");
                    assert_eq!(
                        clicked["structuredContent"]["action"]["delivery"],
                        if mode == "targeted" {
                            "targeted_event"
                        } else {
                            "foreground"
                        },
                        "{clicked}"
                    );
                    let typed = rpc
                        .call(
                            "type_text",
                            json!({"app":app,"window":window,"text":"scoped input"}),
                        )
                        .await;
                    assert_ne!(typed["isError"], true, "{typed}");
                    assert_rpc_clean("allowed typing", &typed);
                    if mode == "clipboard" {
                        assert_eq!(
                            typed["structuredContent"]["action"]["delivery"], "clipboard",
                            "{typed}"
                        );
                    }
                }
                for (operation, count) in ["read", "capture", "delivery", "launch"]
                    .iter()
                    .zip(&probe.counts.outside)
                {
                    assert_eq!(
                        probe.load(count),
                        0,
                        "{profile:?}/{mode}: outside {operation}"
                    );
                }
                rpc.close().await;
            }
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn scoped_launch_dispatch_refuses_before_approval_with_default_policy_and_stop() {
    tokio::time::timeout(Duration::from_secs(20), async {
        for app in ["fixture.a", "fixture.c"] {
            for stopped in [false, true] {
                let spy = Spy::new();
                let probe = spy.clone();
                let (mut host, approvals) = recording_host(0);
                approvals.push_decision(ApprovalDecision::Deny);
                approvals.push_confirm(false);
                host.policy = Arc::new(MemoryPolicyStore::new(PolicyConfig::default()));
                let activity = Arc::new(FakeUserActivity::new());
                host.activity = activity.clone();
                let mut rpc =
                    Rpc::from_connection(open_scoped_with_host(spy, Profile::Default, host).await)
                        .await;
                rpc.initialize().await;
                let discovery = probe.load(&probe.counts.discovery);
                if stopped {
                    activity.request_stop();
                }
                let result = rpc.call("launch_app", json!({"app":app})).await;
                if stopped {
                    assert_eq!(
                        result["structuredContent"]["error"]["code"], "stopped_by_user",
                        "{result}"
                    );
                    assert!(result["structuredContent"].get("state").is_none());
                    assert!(result["structuredContent"]["error"].get("state").is_none());
                    assert!(result["structuredContent"]["error"]
                        .get("windows")
                        .is_none());
                    assert!(result["content"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .all(|block| block["type"] == "text"));
                } else {
                    assert_rpc_denied(app, &result);
                }
                assert!(approvals.app_requests().is_empty());
                assert!(approvals.confirm_requests().is_empty());
                assert_eq!(probe.load(&probe.counts.discovery), discovery);
                assert_eq!(probe.load(&probe.counts.content), 0);
                assert_eq!(probe.load(&probe.counts.capture), 0);
                assert_eq!(probe.load(&probe.counts.perform), 0);
                assert_eq!(probe.load(&probe.counts.launch), 0);
                rpc.close().await;
            }
        }
    })
    .await
    .unwrap();
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
                    {"click": {"element_index": index}},
                    {"type_text": {"text": "later"}}
                ],
                "stop_on_error": false,
                "final_state": true
            }),
        )
        .await;
    let facts = denied.structured_content.as_ref().unwrap();
    assert_eq!(facts["error"]["message"], SCOPE_DENIED_MESSAGE, "{facts}");
    assert!(facts.get("failed_step").is_none(), "{facts}");
    assert!(facts.get("steps").is_none(), "{facts}");
    assert!(facts.get("action").is_none(), "{facts}");
    assert!(facts.get("state").is_none(), "{facts}");
    assert!(facts["error"].get("state").is_none(), "{facts}");
    assert!(
        attachment_bytes(&denied).is_empty(),
        "{}",
        rendered(&denied)
    );
    assert_clean("scope batch", &rendered(&denied));
    assert_files_have_no_canary(&connection).await;
    assert_eq!(
        probe.load(&probe.counts.perform),
        1,
        "batch ran after refusal"
    );
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
