//! DOM fusion and action routing on a fake channel. No browser or account.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use async_trait::async_trait;
use gcu_core::{
    action::{ActionRouter, ToolCall},
    browser::{BrowserHub, DomChannel, DomEffect, DomFault, DomOperation, Share},
    driver::{self, Cancel, Driver, TreeOpts, DRIVER_DEADLINE, NATIVE_TIMEOUT},
    model::{Fingerprint, RawNode, Rect, Role},
    session::{Session, WindowLocks},
    settle::{Clock, SettleConfig, SettleEngine},
};
use gcu_driver_fake::{faults::Fault, FakeDriver};
use gcu_host_api::{
    testing::{FakeUserActivity, MemoryPolicyStore, RecordingEventSink, ScriptedApprovals},
    ApprovalDecision, ConfirmationLevel, ForegroundPolicy, PolicyConfig,
};
use gcu_protocol::{result::Outcome, tools::*, ErrorCode};
use serde_json::{json, Value};
use std::{
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use tokio::task::yield_now;

const PAGE: &str = r#"(apps: [(id: "fake.webarea", name: "Fixture webarea", executable: "/fixtures/webarea", pid: 1001,
windows: [(id: "page", title: "Fixture webarea", frame: (x: 100.0, y: 100.0, w: 800.0, h: 480.0), scale: 1.0,
root: Some((id: "root", role: window, states: "ENABLED", name: Some("Webarea"),
frame: Some((x: 0.0, y: 0.0, w: 800.0, h: 480.0)), children: [
(id: "omnibox", role: text_field, states: "ENABLED | FOCUSABLE | EDITABLE | SETTABLE | FOCUSED",
name: Some("Address"), frame: Some((x: 10.0, y: 4.0, w: 500.0, h: 28.0)),
value: Some(text("https://example.invalid/"))),
(id: "web", role: web_area, states: "ENABLED", name: Some("Offline page"),
frame: Some((x: 20.0, y: 40.0, w: 600.0, h: 400.0)), url: Some("https://example.invalid/"), children: [
(id: "link", role: link, states: "ENABLED", name: Some("Documentation"),
frame: Some((x: 40.0, y: 60.0, w: 160.0, h: 30.0)), actions: [press],
url: Some("https://example.invalid/docs")),
(id: "field", role: text_field, states: "ENABLED | FOCUSABLE | EDITABLE | SETTABLE",
name: Some("Comment"), frame: Some((x: 40.0, y: 110.0, w: 220.0, h: 30.0)), value: Some(text("")))
])])))])])"#;

struct InstantClock(Mutex<Duration>);

#[async_trait]
impl Clock for InstantClock {
    fn now(&self) -> Duration {
        *self.0.lock().unwrap_or_else(|e| e.into_inner())
    }
    async fn wait_until(&self, deadline: Duration) {
        {
            let mut now = self.0.lock().unwrap_or_else(|e| e.into_inner());
            if *now < deadline {
                *now = deadline;
            }
        }
        yield_now().await;
    }
}

struct FakeDom {
    extension_id: String,
    fault: Mutex<Option<DomFault>>,
    snapshot: Mutex<Value>,
    performs: AtomicUsize,
    unknown: AtomicBool,
}

impl FakeDom {
    fn new(snapshot: Value) -> Arc<Self> {
        Arc::new(Self {
            extension_id: "ext-test".into(),
            fault: Mutex::new(None),
            snapshot: Mutex::new(snapshot),
            performs: AtomicUsize::new(0),
            unknown: AtomicBool::new(false),
        })
    }
    fn performs(&self) -> usize {
        self.performs.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl DomChannel for FakeDom {
    fn extension_id(&self) -> &str {
        &self.extension_id
    }
    async fn observe(&self, _share_id: &str) -> Result<Value, DomFault> {
        if let Some(fault) = *self.fault.lock().unwrap() {
            return Err(fault);
        }
        Ok(self.snapshot.lock().unwrap().clone())
    }
    async fn perform(&self, _op: &DomOperation) -> Result<DomEffect, DomFault> {
        self.performs.fetch_add(1, Ordering::SeqCst);
        if let Some(fault) = *self.fault.lock().unwrap() {
            return Err(fault);
        }
        if self.unknown.load(Ordering::SeqCst) {
            return Ok(DomEffect::Unknown);
        }
        Ok(DomEffect::Done)
    }
}

struct Rig {
    driver: Arc<FakeDriver>,
    dom: Arc<FakeDom>,
    hub: Arc<BrowserHub>,
    activity: Arc<FakeUserActivity>,
    approvals: Arc<ScriptedApprovals>,
    events: Arc<RecordingEventSink>,
    router: Arc<ActionRouter>,
    session: Arc<Session>,
    app: String,
}

impl Rig {
    fn page(level: ConfirmationLevel) -> Self {
        Self::ron(PAGE, "fake.webarea", level, page_snapshot(false))
    }
    fn scene(name: &str, app: &str, level: ConfirmationLevel, snapshot: Value) -> Self {
        Self::driver(FakeDriver::from_scene(name).unwrap(), app, level, snapshot)
    }
    fn ron(source: &str, app: &str, level: ConfirmationLevel, snapshot: Value) -> Self {
        Self::driver(FakeDriver::from_ron(source).unwrap(), app, level, snapshot)
    }
    fn driver(driver: FakeDriver, app: &str, level: ConfirmationLevel, snapshot: Value) -> Self {
        let driver = Arc::new(driver);
        let dom = FakeDom::new(snapshot);
        let hub = Arc::new(BrowserHub::new(Arc::clone(&dom) as Arc<dyn DomChannel>));
        let activity = Arc::new(FakeUserActivity::new());
        let approvals = Arc::new(ScriptedApprovals::new());
        approvals.push_decision(ApprovalDecision::Session);
        approvals.push_decision(ApprovalDecision::Session);
        let events = Arc::new(RecordingEventSink::new());
        let router = Arc::new(
            ActionRouter::new(
                driver.clone(),
                approvals.clone(),
                Arc::new(MemoryPolicyStore::new(PolicyConfig {
                    sensitive_action_confirmation: level,
                    allow_foreground: ForegroundPolicy::Always,
                    ..PolicyConfig::default()
                })),
                events.clone(),
                WindowLocks::new(),
            )
            .with_settle_engine(SettleEngine::new(
                SettleConfig::default(),
                Arc::new(InstantClock(Mutex::new(Duration::ZERO))),
            ))
            .with_browser(hub.clone()),
        );
        let session = Arc::new(Session::open(
            "dom",
            activity.clone(),
            router.input_release(),
        ));
        session.begin_turn().unwrap();
        Self {
            driver,
            dom,
            hub,
            activity,
            approvals,
            events,
            router,
            session,
            app: app.into(),
        }
    }
    async fn bind_matched(&self) {
        let (win, fingerprint, frame) = live_web(&self.driver, &self.app).await;
        self.hub
            .bind(share(&self.app, &win.id, "ext-test", fingerprint, frame))
            .unwrap();
    }
    async fn observe(&self) -> gcu_core::action::ActionState {
        self.router
            .observe(
                &self.session,
                serde_json::from_value(json!({"app": self.app})).unwrap(),
            )
            .await
            .unwrap()
    }
    async fn execute(&self, step: BatchStep) -> gcu_core::action::ToolOutcome {
        let mut call = ToolCall::new("call", &self.app, step);
        call.settle_budget_ms = Some(0);
        self.router.execute(&self.session, call).await
    }
}

fn share(app: &str, window: &str, extension: &str, fingerprint: Fingerprint, frame: Rect) -> Share {
    Share {
        share_id: "share1".into(),
        session_id: "dom".into(),
        app_id: app.into(),
        window_id: window.into(),
        extension_id: extension.into(),
        web_area_fingerprint: fingerprint,
        web_area_frame: frame,
        document_id: "doc1".into(),
    }
}

async fn live_web(driver: &FakeDriver, app: &str) -> (driver::WindowRef, Fingerprint, Rect) {
    let win = driver.fixture_window(app).await.unwrap();
    let raw = driver
        .snapshot_tree(&win, &TreeOpts::default(), &Cancel::default())
        .await
        .unwrap();
    let node = find_role(raw.root.as_ref().unwrap(), Role::WebArea).unwrap();
    (win, node.fingerprint, node.frame.unwrap())
}

fn find_role(node: &RawNode, role: Role) -> Option<&RawNode> {
    if node.role == role {
        return Some(node);
    }
    node.children
        .iter()
        .find_map(|child| find_role(child, role))
}

fn find_name<'a>(node: &'a RawNode, name: &str) -> Option<&'a RawNode> {
    if node.name.as_deref() == Some(name) {
        return Some(node);
    }
    node.children
        .iter()
        .find_map(|child| find_name(child, name))
}

fn index_of(text: &str, needle: &str) -> u32 {
    text.lines()
        .find(|line| line.contains(needle))
        .unwrap_or_else(|| panic!("missing {needle}"))
        .split_whitespace()
        .next()
        .unwrap()
        .parse()
        .unwrap()
}

fn click(index: u32) -> BatchStep {
    BatchStep::Click(ClickArgs {
        element_index: Some(index),
        x: None,
        y: None,
        screenshot_id: None,
        click_count: 1,
        mouse_button: MouseButton::Left,
    })
}

fn point(x: f64, y: f64) -> BatchStep {
    BatchStep::Click(ClickArgs {
        element_index: None,
        x: Some(x),
        y: Some(y),
        screenshot_id: None,
        click_count: 1,
        mouse_button: MouseButton::Left,
    })
}

fn set(index: u32, value: &str) -> BatchStep {
    BatchStep::SetValue(SetValueArgs {
        element_index: index,
        value: value.into(),
    })
}

fn typed(index: Option<u32>, text: &str) -> BatchStep {
    BatchStep::TypeText(TypeTextArgs {
        text: text.into(),
        element_index: index,
    })
}

fn key(key: &str) -> BatchStep {
    BatchStep::PressKey(PressKeyArgs { key: key.into() })
}

fn code(out: &gcu_core::action::ToolOutcome) -> ErrorCode {
    out.error.as_ref().unwrap().code
}

fn page_snapshot(handoff: bool) -> Value {
    snap(
        600.0,
        400.0,
        handoff,
        vec![
            button("dom-btn", "Dom Only", [20.0, 20.0, 160.0, 30.0]),
            field("dom-field", "Comment", [20.0, 70.0, 220.0, 30.0]),
        ],
    )
}

fn snap(width: f64, height: f64, handoff: bool, children: Vec<Value>) -> Value {
    json!({
        "id": "snap1",
        "root": {
            "id": "root", "role": "document", "native_role": "document", "name": null,
            "value": null, "states": [], "frame": {"x": 0.0, "y": 0.0, "w": width, "h": height},
            "children": children
        },
        "frameSpace": "tab_viewport_physical",
        "geometry": {"width": width, "height": height, "scale": 1.0},
        "incomplete": null,
        "handoff": handoff
    })
}

fn button(id: &str, name: &str, frame: [f64; 4]) -> Value {
    node(id, "button", name, json!(null), json!(["enabled"]), frame)
}

fn field(id: &str, name: &str, frame: [f64; 4]) -> Value {
    node(
        id,
        "text_field",
        name,
        json!({"text": ""}),
        json!(["enabled", "settable", "focusable", "editable"]),
        frame,
    )
}

fn node(id: &str, role: &str, name: &str, value: Value, states: Value, frame: [f64; 4]) -> Value {
    json!({
        "id": id, "role": role, "native_role": role, "name": name, "value": value, "states": states,
        "frame": {"x": frame[0], "y": frame[1], "w": frame[2], "h": frame[3]}, "children": []
    })
}

#[tokio::test]
async fn merged_dom_subtree_uses_web_area_frame_origin() {
    let rig = Rig::page(ConfirmationLevel::Off);
    rig.bind_matched().await;
    let (win, fingerprint, _) = live_web(&rig.driver, &rig.app).await;
    let raw = rig
        .driver
        .snapshot_tree(&win, &TreeOpts::default(), &Cancel::default())
        .await
        .unwrap();
    let merged = rig.hub.align(rig.session.id(), &win, raw).await;
    let root = merged.root.as_ref().unwrap();
    assert_eq!(
        find_name(root, "Dom Only").unwrap().frame,
        Some(Rect {
            x: 40.0,
            y: 60.0,
            w: 160.0,
            h: 30.0
        })
    );
    assert!(find_name(root, "Documentation").is_none());
    assert_eq!(
        find_role(root, Role::WebArea).unwrap().fingerprint,
        fingerprint
    );
    let text = rig.observe().await.text;
    assert!(text.contains("Dom Only"));
    assert!(!text.contains("Documentation"));
}

#[tokio::test]
async fn unshared_tab_keeps_l0_tree_and_l0_action() {
    let rig = Rig::page(ConfirmationLevel::Off);
    let text = rig.observe().await.text;
    assert!(text.contains("Documentation"));
    assert!(!text.contains("Dom Only"));
    let comment = index_of(&text, "Comment");
    let blocked = rig.execute(set(comment, "http://localhost/x")).await;
    assert_eq!(code(&blocked), ErrorCode::KeyBlocked);
    assert_eq!(rig.driver.perform_calls(), 0);
    assert_eq!(rig.dom.performs(), 0);
    let link = index_of(&text, "Documentation");
    let out = rig.execute(click(link)).await;
    assert!(out.error.is_none());
    assert_eq!(rig.driver.perform_calls(), 1);
    assert_eq!(rig.dom.performs(), 0);
}

#[tokio::test]
async fn extension_mismatch_keeps_declared_l0() {
    let rig = Rig::page(ConfirmationLevel::Off);
    let (win, fingerprint, frame) = live_web(&rig.driver, &rig.app).await;
    rig.hub
        .bind(share(&rig.app, &win.id, "other", fingerprint, frame))
        .unwrap();
    let text = rig.observe().await.text;
    assert!(text.contains("Documentation"));
    assert!(!text.contains("Dom Only"));
    assert_eq!(rig.dom.performs(), 0);
}

#[tokio::test]
async fn fingerprint_mismatch_keeps_declared_l0() {
    let rig = Rig::page(ConfirmationLevel::Off);
    let (win, _, frame) = live_web(&rig.driver, &rig.app).await;
    rig.hub
        .bind(share(&rig.app, &win.id, "ext-test", Fingerprint(1), frame))
        .unwrap();
    let text = rig.observe().await.text;
    assert!(text.contains("Documentation") && !text.contains("Dom Only"));
}

#[tokio::test]
async fn frame_mismatch_keeps_declared_l0() {
    let rig = Rig::page(ConfirmationLevel::Off);
    let (win, fingerprint, frame) = live_web(&rig.driver, &rig.app).await;
    let shifted = Rect {
        x: frame.x + 40.0,
        ..frame
    };
    rig.hub
        .bind(share(&rig.app, &win.id, "ext-test", fingerprint, shifted))
        .unwrap();
    let text = rig.observe().await.text;
    assert!(text.contains("Documentation") && !text.contains("Dom Only"));
}

#[tokio::test]
async fn focused_address_bar_blocks_text_with_dom_channel_present() {
    let rig = Rig::page(ConfirmationLevel::Off);
    rig.bind_matched().await;
    let text = rig.observe().await.text;
    assert!(text.contains("Dom Only"));
    let address = index_of(&text, "Address");
    for step in [typed(None, "hello"), set(address, "hello"), key("Return")] {
        assert_eq!(code(&rig.execute(step).await), ErrorCode::KeyBlocked);
    }
    assert_eq!(rig.dom.performs(), 0);
    assert_eq!(rig.driver.perform_calls(), 0);
    let out = rig.execute(click(index_of(&text, "Dom Only"))).await;
    assert!(out.error.is_none());
    assert_eq!(rig.dom.performs(), 1);
    assert_eq!(rig.driver.perform_calls(), 0);
}

#[tokio::test]
async fn dangerous_schemes_and_metadata_and_private_hosts_block_dom_input() {
    let rig = Rig::page(ConfirmationLevel::Off);
    rig.bind_matched().await;
    let text = rig.observe().await.text;
    let comment = index_of(&text, "Comment");
    for input in [
        "javascript:alert(1)",
        "data:text/html,hi",
        "file:///etc/passwd",
        "http://169.254.169.254/latest",
        "http://metadata.google.internal/",
        "http://127.0.0.1/",
        "http://localhost/x",
        "http://10.1.2.3/",
        "http://192.168.0.1/",
        "http://172.16.0.1/",
        "::1",
    ] {
        assert_eq!(
            code(&rig.execute(set(comment, input)).await),
            ErrorCode::KeyBlocked
        );
        assert_eq!(
            code(&rig.execute(typed(Some(comment), input)).await),
            ErrorCode::KeyBlocked
        );
    }
    assert_eq!(rig.dom.performs(), 0);
    assert_eq!(rig.driver.perform_calls(), 0);
    let l0 = Rig::page(ConfirmationLevel::Off);
    let l0_text = l0.observe().await.text;
    let blocked = l0
        .execute(set(
            index_of(&l0_text, "Comment"),
            "http://localhost/secret",
        ))
        .await;
    assert_eq!(code(&blocked), ErrorCode::KeyBlocked);
    assert_eq!(l0.driver.perform_calls(), 0);
}

#[tokio::test]
async fn frame_error_refuses_dom_action_without_l0_or_dom_delivery() {
    let rig = Rig::page(ConfirmationLevel::Off);
    rig.bind_matched().await;
    let text = rig.observe().await.text;
    let button = index_of(&text, "Dom Only");
    *rig.dom.fault.lock().unwrap() = Some(DomFault::Frame);
    let out = rig.execute(click(button)).await;
    assert_eq!(code(&out), ErrorCode::StaleElement);
    assert_eq!(rig.dom.performs(), 0);
    assert_eq!(rig.driver.perform_calls(), 0);
}

#[tokio::test]
async fn stale_coordinates_refuse_before_dom_or_l0_delivery() {
    let rig = Rig::page(ConfirmationLevel::Off);
    rig.bind_matched().await;
    rig.observe().await;
    rig.driver
        .inject(Fault::NodeChanged {
            window: "page".into(),
            identifier: "link".into(),
            name: "ZZZZZZZZZZZZZZZZZZZZ".into(),
            value: Some(gcu_core::model::NodeValue::Text(
                "YYYYYYYYYYYYYYYYYYYY".into(),
            )),
        })
        .await
        .unwrap();
    let out = rig.execute(point(50.0, 66.0)).await;
    assert_eq!(code(&out), ErrorCode::StaleScreenshot);
    assert_eq!(rig.dom.performs(), 0);
    assert_eq!(rig.driver.perform_calls(), 0);
}

#[tokio::test]
async fn remote_ide_and_handoff_flag_refuse_without_dom_delivery() {
    let ide = Rig::scene(
        "ide",
        "fake.ide",
        ConfirmationLevel::Off,
        snap(
            200.0,
            80.0,
            false,
            vec![button("bypass", "Bypass", [4.0, 4.0, 40.0, 20.0])],
        ),
    );
    ide.hub
        .bind(share(
            "fake.ide",
            "ide-window",
            "ext-test",
            Fingerprint(999),
            Rect {
                x: 0.0,
                y: 0.0,
                w: 10.0,
                h: 10.0,
            },
        ))
        .unwrap();
    let ide_text = ide.observe().await.text;
    assert!(!ide_text.contains("Bypass"));
    let cleared = ide
        .execute(click(index_of(&ide_text, "Clear buffer")))
        .await;
    assert_eq!(code(&cleared), ErrorCode::ElementNotActionable);
    assert_eq!(ide.dom.performs(), 0);

    let remote = Rig::scene(
        "web_terminal",
        "fake.web_terminal",
        ConfirmationLevel::Off,
        snap(
            760.0,
            450.0,
            false,
            vec![button("run", "Dom Run", [20.0, 20.0, 80.0, 32.0])],
        ),
    );
    remote.bind_matched().await;
    let remote_text = remote.observe().await.text;
    assert!(remote_text.contains("Dom Run"));
    let handed = remote
        .execute(click(index_of(&remote_text, "Dom Run")))
        .await;
    assert_eq!(code(&handed), ErrorCode::HandoffRequired);
    assert_eq!(remote.dom.performs(), 0);

    let flagged = Rig::ron(
        PAGE,
        "fake.webarea",
        ConfirmationLevel::Off,
        page_snapshot(true),
    );
    flagged.bind_matched().await;
    let text = flagged.observe().await.text;
    let refused = flagged.execute(click(index_of(&text, "Dom Only"))).await;
    assert_eq!(code(&refused), ErrorCode::HandoffRequired);
    assert_eq!(flagged.dom.performs(), 0);
    assert_eq!(flagged.driver.perform_calls(), 0);
}

#[tokio::test]
async fn latched_stop_refuses_dom_click() {
    let rig = Rig::page(ConfirmationLevel::Off);
    rig.bind_matched().await;
    let button = index_of(&rig.observe().await.text, "Dom Only");
    rig.activity.request_stop();
    assert_eq!(
        code(&rig.execute(click(button)).await),
        ErrorCode::StoppedByUser
    );
    let report = rig
        .router
        .run_batch(
            &rig.session,
            "b",
            ComputerBatchParams {
                app: rig.app.clone(),
                window: None,
                steps: vec![click(button)],
                stop_on_error: true,
                final_state: false,
            },
        )
        .await;
    assert_eq!(report.error.unwrap().code, ErrorCode::StoppedByUser);
    assert_eq!(rig.dom.performs(), 0);
    assert_eq!(rig.driver.perform_calls(), 0);
}

#[tokio::test]
async fn unknown_dom_outcome_is_not_replayed() {
    let rig = Rig::page(ConfirmationLevel::Off);
    rig.bind_matched().await;
    let button = index_of(&rig.observe().await.text, "Dom Only");
    rig.dom.unknown.store(true, Ordering::SeqCst);
    let out = rig.execute(click(button)).await;
    assert_eq!(out.action.outcome, Outcome::Unknown);
    assert!(out
        .action
        .notes
        .iter()
        .any(|note| note.contains("do not automatically retry")));
    assert_eq!(rig.dom.performs(), 1);
    assert_eq!(rig.driver.perform_calls(), 0);
}

#[tokio::test]
async fn batch_dom_click_then_dangerous_value_stops_and_uses_router() {
    let rig = Rig::page(ConfirmationLevel::Off);
    rig.bind_matched().await;
    let text = rig.observe().await.text;
    let report = rig
        .router
        .run_batch(
            &rig.session,
            "b",
            ComputerBatchParams {
                app: rig.app.clone(),
                window: None,
                steps: vec![
                    click(index_of(&text, "Dom Only")),
                    set(index_of(&text, "Comment"), "javascript:alert(1)"),
                ],
                stop_on_error: true,
                final_state: true,
            },
        )
        .await;
    assert_eq!(report.steps[0].outcome, Outcome::Done);
    assert_eq!(report.error.unwrap().code, ErrorCode::KeyBlocked);
    assert_eq!(rig.dom.performs(), 1);
    assert_eq!(rig.driver.perform_calls(), 0);
}

#[tokio::test]
async fn dom_click_requires_approval_before_channel_delivery() {
    assert_eq!(NATIVE_TIMEOUT, Duration::from_secs(5));
    assert_eq!(DRIVER_DEADLINE, Duration::from_millis(4750));
    let rig = Rig::page(ConfirmationLevel::Standard);
    rig.bind_matched().await;
    let button = index_of(&rig.observe().await.text, "Dom Only");
    rig.approvals.push_confirm(false);
    let denied = rig.execute(click(button)).await;
    assert_eq!(code(&denied), ErrorCode::ApprovalDenied);
    assert!(!rig.approvals.confirm_requests().is_empty());
    assert_eq!(rig.dom.performs(), 0);
    rig.approvals.push_confirm(true);
    let allowed = rig.execute(click(button)).await;
    assert!(allowed.error.is_none());
    assert_eq!(allowed.action.outcome, Outcome::Done);
    assert_eq!(rig.dom.performs(), 1);
    assert!(!rig.events.events().is_empty());
    assert_eq!(rig.driver.perform_calls(), 0);
}
