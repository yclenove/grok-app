//! Portable contract bodies. They do not touch a real desktop, account, or TCC prompt.

use std::sync::Arc;
use std::time::Duration;

use gcu_core::driver::{
    Button, Cancel, Capture, Delivery, DriverAction, DriverError, PointerRoute, WindowRef,
    DRIVER_DEADLINE,
};
use gcu_core::model::{
    Geometry, NativeHandle, NodeFlags, NodeValue, RawNode, RawTree, Rect, Role, States, WindowPoint,
};
use gcu_core::redact::{self, CapturedFrame, NEUTRAL_COLOR};
use gcu_core::tree::{Pipeline, TreeOpts};
use gcu_driver_macos::ax::{
    build_raw_tree, is_terminal_like, perform, AxSource, FakeAct, FakeAxNode, FakeAxSource,
};
use gcu_driver_macos::capture::{encode_capture, take_capture, FakeCapture, SourceWindow};
use gcu_driver_macos::input::activity::{ActivityClock, ActivityMonitor, OWN_EVENT_TAG};
use gcu_driver_macos::input::hotkey::{
    register_hotkey, Guards, HotkeyConfig, HotkeyRegistrar, Registration, SharedStopLatch,
};
use gcu_driver_macos::input::watchdog::{release_stopped, RELEASE_DEADLINE};
use gcu_driver_macos::input::{
    plan_click, Call, EventApi, EventKind, FakeEvents, InputLedger, InputTarget, PostPath, Probe,
    FIELD_WINDOW_NUMBER,
};
use gcu_protocol::capabilities::PrivateApiStatus;

use super::table::{self, CaseResult, Facts};

/// Run every row that does not need a macOS GUI. Live rows skip honestly.
pub async fn run_portable_matrix() -> Vec<CaseResult> {
    let mut caps = table::base_capabilities();
    gcu_driver_macos::input::apply_probe(&mut caps, &Probe::verified_os());
    let facts = Facts {
        accessibility: true,
        screen_recording: true,
        field_51_ready: true,
        fixture_running: false,
    };
    let mut out = Vec::with_capacity(table::ROWS.len());
    for row in table::ROWS {
        if let Some(reason) = table::skip_reason(row, &caps, &facts) {
            out.push(CaseResult::skipped(row.id, row.description, reason));
            continue;
        }
        let result = match row.id {
            "C18" => stale_handle().await,
            "C19" => closed_window().await,
            "C20" => password_redaction().await,
            "C22" => uncertain_is_not_replayed().await,
            "C26" => execution_surfaces().await,
            "C27" => stop_budgets().await,
            "M-TCC" => tcc_missing().await,
            "M-CHROMIUM" => chromium_background().await,
            "M-FIELD51" => field_51_arrival().await,
            "C21" => Err("overlay capability was declared but no overlay window exists".into()),
            _ => Err(format!("{} has no portable body", row.id)),
        };
        out.push(match result {
            Ok(()) => CaseResult::passed(row.id, row.description),
            Err(error) => CaseResult::failed(row.id, row.description, error),
        });
    }
    out
}

async fn stale_handle() -> Result<(), String> {
    let source = FakeAxSource::default();
    let mut act = FakeAct::default();
    act.missing.push(7);
    *source.act.lock().unwrap() = act;
    let button = button(7, &["AXPress"]);
    source.bind_root(button.clone());
    let error = perform(
        &source,
        &DriverAction::Press {
            el: source.handle(&button),
        },
        &Cancel::default(),
        DRIVER_DEADLINE,
    )
    .await
    .unwrap_err();
    if error != DriverError::StaleElement || error.maybe_executed() {
        return Err(format!("stale press: {error}"));
    }
    if source
        .calls()
        .iter()
        .any(|call| call.starts_with("perform:"))
    {
        return Err(format!("stale press ran: {:?}", source.calls()));
    }
    Ok(())
}

async fn closed_window() -> Result<(), String> {
    let fake = Arc::new(FakeEvents::ready());
    fake.close_window(4, 7);
    let error = deliver(
        &fake,
        "com.example.Editor",
        "Editor",
        PointerRoute::TargetedOnly,
    )
    .await
    .unwrap_err();
    if error != DriverError::WindowGone || error.maybe_executed() {
        return Err(format!("closed window: {error}"));
    }
    if !fake.calls().is_empty() {
        return Err(format!("closed window posted: {:?}", fake.calls()));
    }
    Ok(())
}

async fn password_redaction() -> Result<(), String> {
    let source = FakeAxSource::default();
    let secret = FakeAxNode {
        id: 1,
        role: "AXSecureTextField".into(),
        title: Some("Password".into()),
        value: Some(NodeValue::Text("super-secret".into())),
        frame: Some(Rect {
            x: 0.0,
            y: 0.0,
            w: 4.0,
            h: 4.0,
        }),
        ..FakeAxNode::default()
    };
    source.bind_root(secret.clone());
    let raw = build_raw_tree(
        &source,
        &secret,
        &TreeOpts::default(),
        &Cancel::default(),
        1.0,
    )
    .await
    .map_err(|error| error.to_string())?;
    let text = raw
        .root
        .as_ref()
        .and_then(|node| node.value.as_ref())
        .map(|value| match value {
            // Debug no longer prints text, so inspect the masked value itself.
            NodeValue::Text(text) => text.clone(),
            other => format!("{other:?}"),
        })
        .unwrap_or_default();
    if text.contains("super-secret") || !text.contains('•') {
        return Err(format!("password value leaked: {text}"));
    }

    let mut node = raw_password();
    node.value = Some(NodeValue::Text("super-secret".into()));
    let mut tree = Pipeline::default().run(
        RawTree {
            root: Some(node),
            incomplete: None,
        },
        &TreeOpts::default(),
    );
    tree.state_version = Some(7);
    tree.observed_at = Some(Duration::from_millis(1_000));
    let rendered = tree
        .root
        .as_ref()
        .and_then(|node| node.node.value.as_ref())
        .map(|value| format!("{value:?}"))
        .unwrap_or_default();
    if rendered.contains("super-secret") {
        return Err(format!("pipeline kept the secret: {rendered}"));
    }
    let secret_px = [255, 0, 0, 255];
    let capture = solid_capture(secret_px)?;
    let redacted = redact::apply(
        &tree,
        CapturedFrame::new(capture, 7, Duration::from_millis(1_000)),
    )
    .map_err(|error| error.to_string())?;
    if redacted.pixels().chunks(4).any(|pixel| pixel == secret_px) {
        return Err("redacted image still contains the secret color".into());
    }
    if !redacted
        .pixels()
        .chunks(4)
        .any(|pixel| pixel == NEUTRAL_COLOR)
    {
        return Err("redaction did not paint the neutral color".into());
    }
    Ok(())
}

async fn uncertain_is_not_replayed() -> Result<(), String> {
    let source = FakeAxSource::default();
    let mut act = FakeAct::default();
    act.uncertain.push("AXPress".into());
    *source.act.lock().unwrap() = act;
    let button = button(3, &["AXPress"]);
    source.bind_root(button.clone());
    let error = perform(
        &source,
        &DriverAction::Press {
            el: source.handle(&button),
        },
        &Cancel::default(),
        DRIVER_DEADLINE,
    )
    .await
    .unwrap_err();
    if !error.maybe_executed() {
        return Err(format!("uncertain press lost maybe_executed: {error}"));
    }
    let performs = source
        .calls()
        .iter()
        .filter(|call| call.starts_with("perform:"))
        .count();
    if performs != 1 {
        return Err(format!("uncertain press replayed: {:?}", source.calls()));
    }
    Ok(())
}

async fn execution_surfaces() -> Result<(), String> {
    for (role, sub, name) in [
        ("AXGroup", None, Some("Integrated Terminal")),
        ("AXGroup", Some("AXTerminal"), None),
        ("AXTextArea", None, Some("Debug Console")),
        ("AXGroup", None, Some("Python REPL")),
    ] {
        if !is_terminal_like(role, sub, name) {
            return Err(format!("missed execution surface {name:?} {sub:?}"));
        }
    }
    if is_terminal_like("AXButton", None, Some("Submit")) {
        return Err("Submit was marked terminal-like".into());
    }
    let source = FakeAxSource::default();
    let node = FakeAxNode {
        id: 1,
        role: "AXGroup".into(),
        title: Some("Debug Console".into()),
        ..FakeAxNode::default()
    };
    source.bind_root(node.clone());
    let raw = build_raw_tree(
        &source,
        &node,
        &TreeOpts::default(),
        &Cancel::default(),
        1.0,
    )
    .await
    .map_err(|error| error.to_string())?;
    let native = raw
        .root
        .as_ref()
        .map(|root| root.native_role.to_string())
        .unwrap_or_default();
    if !native.to_ascii_lowercase().contains("terminal") {
        return Err(format!("driver did not mark the console: {native}"));
    }
    let processed = Pipeline::default().run(raw, &TreeOpts::default());
    let flags = processed
        .root
        .as_ref()
        .map(|root| root.flags)
        .unwrap_or_default();
    if !flags.contains(NodeFlags::TERMINAL_LIKE) {
        return Err("pipeline did not keep TERMINAL_LIKE".into());
    }
    Ok(())
}

async fn stop_budgets() -> Result<(), String> {
    if RELEASE_DEADLINE > Duration::from_millis(500) || DRIVER_DEADLINE > Duration::from_secs(5) {
        return Err("Stop budgets exceed 500ms release or 5s driver deadline".into());
    }
    let clock = Arc::new(gcu_driver_macos::input::clock::FakeClock::new());
    let latch = Arc::new(SharedStopLatch::new(false));
    let guards = Arc::new(Guards::new(
        clock,
        Arc::new(Idle {
            ms: Some(10_000),
            stop: true,
        }),
        latch.clone(),
        3_000,
    ));
    let started = std::time::Instant::now();
    guards.request_stop();
    if !guards.is_stopped() || started.elapsed() > Duration::from_millis(100) {
        return Err("Stop latch missed the 100ms budget".into());
    }
    let fake = Arc::new(FakeEvents::ready());
    fake.set_guards(Arc::clone(&guards));
    let ledger = Arc::new(InputLedger::new());
    let up = plan_click(
        &target("com.example.Editor", "Editor"),
        WindowPoint { x: 4.0, y: 6.0 },
        Button::Left,
        1,
        PostPath::ToPid,
    )
    .map_err(|error| error.to_string())?
    .into_iter()
    .next()
    .and_then(|event| event.release_event())
    .ok_or("click plan produced no release")?;
    ledger.record_press(up);
    release_stopped(fake.as_ref(), &ledger, &guards).await;
    if !ledger.is_empty() || started.elapsed() > Duration::from_millis(500) {
        return Err("Stop release missed the 500ms budget".into());
    }
    if guards.key_down(
        &HotkeyConfig::default(),
        47,
        HotkeyConfig::default().modifiers,
        OWN_EVENT_TAG,
    ) {
        return Err("own injected events must not trip the hotkey".into());
    }

    let monitor = ActivityMonitor::new(Arc::new(gcu_driver_macos::input::clock::FakeClock::new()));
    let mut registrar = DenyHotkey;
    if register_hotkey(&mut registrar, &monitor, &HotkeyConfig::default())
        != Registration::Unavailable
        || monitor.stop_available()
    {
        return Err("failed hotkey registration left Stop available".into());
    }
    let blocked = Guards::new(
        Arc::new(gcu_driver_macos::input::clock::FakeClock::new()),
        Arc::new(Idle {
            ms: None,
            stop: false,
        }),
        Arc::new(SharedStopLatch::new(false)),
        3_000,
    );
    if blocked.foreground_ready() || blocked.check(true).is_ok() {
        return Err("foreground stayed available without a Stop listener".into());
    }
    Ok(())
}

async fn tcc_missing() -> Result<(), String> {
    if !gcu_driver_macos::capture::is_screen_recording_denial(
        gcu_driver_macos::capture::SCREEN_RECORDING_USER_DECLINED,
    ) {
        return Err("SCStreamError -3801 was not mapped as a Screen Recording denial".into());
    }
    let capture = FakeCapture::new();
    capture
        .permission()
        .store(false, std::sync::atomic::Ordering::Release);
    let error = take_capture(
        Arc::new(capture),
        &SourceWindow {
            cg_id: 1,
            frame: Rect {
                x: 0.0,
                y: 0.0,
                w: 8.0,
                h: 8.0,
            },
            scale: 1.0,
        },
        Duration::from_millis(200),
        &Cancel::default(),
    )
    .await
    .unwrap_err();
    if error
        != (DriverError::PermissionMissing {
            permission: "screen_recording".into(),
        })
        || error.maybe_executed()
    {
        return Err(format!("screen recording: {error}"));
    }

    let mut probe = Probe::verified_os();
    probe.accessibility = false;
    let fake = Arc::new(FakeEvents::new(probe));
    let error = deliver(
        &fake,
        "com.example.Editor",
        "Editor",
        PointerRoute::TargetedOnly,
    )
    .await
    .unwrap_err();
    if error
        != (DriverError::PermissionMissing {
            permission: "accessibility".into(),
        })
        || !fake.calls().is_empty()
    {
        return Err(format!("accessibility TCC: {error} {:?}", fake.calls()));
    }
    Ok(())
}

async fn chromium_background() -> Result<(), String> {
    let fake = Arc::new(FakeEvents::ready());
    let error = deliver(
        &fake,
        "com.google.Chrome",
        "Google Chrome",
        PointerRoute::TargetedOnly,
    )
    .await
    .unwrap_err();
    if error != DriverError::CapabilityUnavailable || error.maybe_executed() {
        return Err(format!("chromium background: {error}"));
    }
    if fake
        .calls()
        .iter()
        .any(|call| matches!(call, Call::Activate(_) | Call::Post(_)))
    {
        return Err(format!("chromium click was upgraded: {:?}", fake.calls()));
    }
    let mut caps = table::base_capabilities();
    gcu_driver_macos::input::apply_probe(&mut caps, &Probe::verified_os());
    if caps.background_operation != gcu_protocol::capabilities::BackgroundOperation::Partial {
        return Err("Chromium must not promote background operation to full".into());
    }
    if caps.private_apis.get("skylight") == Some(&PrivateApiStatus::Available)
        && caps.delivery.targeted_pointer
        && error != DriverError::CapabilityUnavailable
    {
        return Err("SkyLight was treated as a Chromium background path".into());
    }
    Ok(())
}

async fn field_51_arrival() -> Result<(), String> {
    let planned = plan_click(
        &target("com.example.Editor", "Editor"),
        WindowPoint { x: 4.0, y: 6.0 },
        Button::Left,
        1,
        PostPath::ToPid,
    )
    .map_err(|error| error.to_string())?;
    let down = planned
        .iter()
        .find(|event| event.kind == EventKind::MouseDown)
        .ok_or_else(|| format!("field {} plan has no mouse down", FIELD_WINDOW_NUMBER))?;
    if down.field_51 != Some(i64::from(7)) || down.window_id != 7 {
        return Err(format!(
            "field {} missing on the click plan: {down:?}",
            FIELD_WINDOW_NUMBER
        ));
    }
    let fake = Arc::new(FakeEvents::ready());
    let delivery = deliver(
        &fake,
        "com.example.Editor",
        "Editor",
        PointerRoute::TargetedOnly,
    )
    .await
    .map_err(|error| error.to_string())?;
    if delivery != Delivery::TargetedEvent {
        return Err(format!("field 51 delivery: {delivery:?}"));
    }
    let saw_verify = fake.calls().iter().any(|call| {
        matches!(
            call,
            Call::Verify {
                pid: 4,
                window_id: 7
            }
        )
    });
    let saw_field = fake.calls().iter().any(|call| match call {
        Call::Post(event) => event.field_51 == Some(7) && event.path == PostPath::ToPid,
        _ => false,
    });
    if !saw_verify
        || !saw_field
        || fake
            .calls()
            .iter()
            .any(|call| matches!(call, Call::Activate(_)))
    {
        return Err(format!("arrival check incomplete: {:?}", fake.calls()));
    }
    fake.fail_arrival();
    let error = deliver(
        &fake,
        "com.example.Editor",
        "Editor",
        PointerRoute::TargetedOnly,
    )
    .await
    .unwrap_err();
    if !error.maybe_executed() || fake.probe().field_51 {
        return Err(format!(
            "failed arrival was retried or kept field 51: {error}"
        ));
    }
    let after_failure = fake.calls().len();
    let upgraded = deliver(
        &fake,
        "com.example.Editor",
        "Editor",
        PointerRoute::TargetedOnly,
    )
    .await
    .unwrap_err();
    if upgraded != DriverError::CapabilityUnavailable || fake.calls().len() != after_failure {
        return Err(format!(
            "field 51 failure upgraded: {upgraded} {:?}",
            fake.calls()
        ));
    }
    Ok(())
}

fn button(id: u64, actions: &[&str]) -> FakeAxNode {
    FakeAxNode {
        id,
        role: "AXButton".into(),
        raw_actions: Some(actions.iter().map(|name| (*name).to_string()).collect()),
        ..FakeAxNode::default()
    }
}

fn raw_password() -> RawNode {
    RawNode {
        role: Role::TextField,
        native_role: "AXSecureTextField".into(),
        subrole: None,
        name: Some("Password".into()),
        value: None,
        description: None,
        placeholder: None,
        help: None,
        identifier: Some("password_field".into()),
        states: States::PASSWORD,
        actions: Default::default(),
        frame: Some(Rect {
            x: 0.0,
            y: 0.0,
            w: 4.0,
            h: 4.0,
        }),
        text_extras: None,
        url: None,
        children: Vec::new(),
        handle: NativeHandle::new(()),
        fingerprint: Default::default(),
    }
}

fn solid_capture(pixel: [u8; 4]) -> Result<Capture, String> {
    let mut rgba = Vec::new();
    for _ in 0..16 {
        rgba.extend(pixel);
    }
    encode_capture(
        &rgba,
        4,
        4,
        Geometry {
            frame: Rect {
                x: 0.0,
                y: 0.0,
                w: 4.0,
                h: 4.0,
            },
            scale: 1.0,
        },
    )
    .map_err(|error| error.to_string())
}

fn target(bundle: &str, name: &str) -> InputTarget {
    InputTarget {
        pid: 4,
        window_id: 7,
        bundle_id: bundle.into(),
        app_name: name.into(),
        origin_x: 10.0,
        origin_y: 20.0,
        width_px: 160.0,
        height_px: 80.0,
        scale: 2.0,
    }
}

async fn deliver(
    fake: &Arc<FakeEvents>,
    bundle: &str,
    name: &str,
    route: PointerRoute,
) -> Result<Delivery, DriverError> {
    let action = DriverAction::PointerClick {
        win: WindowRef {
            app: gcu_core::driver::AppRef {
                info: gcu_protocol::AppInfo {
                    id: bundle.into(),
                    name: name.into(),
                    pid: Some(4),
                    identity: None,
                },
            },
            id: "cg:7:4".into(),
        },
        at: WindowPoint { x: 4.0, y: 6.0 },
        button: Button::Left,
        count: 1,
        route,
    };
    gcu_driver_macos::input::deliver(
        Arc::clone(fake) as Arc<dyn EventApi>,
        Arc::new(InputLedger::new()),
        &target(bundle, name),
        &action,
        &Cancel::default(),
        DRIVER_DEADLINE,
    )
    .await
}

struct Idle {
    ms: Option<u64>,
    stop: bool,
}

impl ActivityClock for Idle {
    fn ms_since_physical_input(&self) -> Option<u64> {
        self.ms
    }
    fn stop_available(&self) -> bool {
        self.stop
    }
}

struct DenyHotkey;

impl HotkeyRegistrar for DenyHotkey {
    fn register(&mut self, _config: &HotkeyConfig) -> Registration {
        Registration::Unavailable
    }
}
