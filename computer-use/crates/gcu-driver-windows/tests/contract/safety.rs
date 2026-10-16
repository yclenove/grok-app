use super::*;
use gcu_core::{
    action::{ActionRouter, ToolCall},
    model::{Rect, States},
    redact::{self, CapturedFrame, NEUTRAL_COLOR},
    session::Session,
    tree::Pipeline,
};
use gcu_driver_windows::{
    api::WinApi,
    gate::IntegrityLevel,
    input::{hotkey::SafetyGuard, RealSafetyApi},
    RealWinApi,
};
#[cfg(feature = "test-worker-faults")]
use gcu_protocol::result::Outcome as ActionOutcome;
use gcu_protocol::ErrorCode;
use serde_json::json;

fn router(driver: Arc<dyn Driver>) -> (ActionRouter, Session) {
    gcu_core::testing::host::router(driver)
}
async fn observe(
    router: &ActionRouter,
    session: &Session,
    win: &WindowRef,
    image: bool,
) -> Result<gcu_core::action::ActionState, String> {
    router
        .observe(
            session,
            serde_json::from_value(
                json!({"app":win.app.info.id,"window":win.id,"include_screenshot":image}),
            )
            .unwrap(),
        )
        .await
        .map_err(|e| format!("observe: {e:?}"))
}
// Router owns Arc<Driver>; a borrowed native driver cannot be attached. Independent
// adapters below share the same process-instance stamped identity only after discovery.
async fn isolated() -> Result<Arc<WindowsDriver>, String> {
    Ok(Arc::new(
        WindowsDriver::isolated(WorkerConfig::new(
            std::env::var_os("GCU_WORKER_EXE").ok_or("GCU_WORKER_EXE")?,
        ))
        .await
        .map_err(|e| e.to_string())?,
    ))
}

pub async fn geometry_gate(_: &WindowsDriver, fixture: &Fixture, _: &WindowRef) -> CaseResult {
    let driver = isolated().await?;
    let win = window(&driver, fixture).await?;
    // A wrongly dispatched coordinate must hit an observable control. Moving
    // the HWND leaves this window-relative counter position unchanged.
    let at = center(&node(&driver, &win, "counter_button").await?)?;
    let (router, session) = router(driver);
    let state = observe(&router, &session, &win, true).await?;
    if state.image.is_none() {
        return Ok(Outcome::Skip(
            "fixture: screenshot withheld by redaction; stale geometry requires a delivered image"
                .into(),
        ));
    }
    let original = unsafe_desktop::window_rect(&win, fixture.pid())?;
    let mut moved = original;
    moved.x += 20.;
    unsafe_desktop::move_window(&win, fixture.pid(), moved)?;
    let before = fixture.read()?["counter"].clone();
    let result = router
        .execute(
            &session,
            ToolCall::new(
                "old-geometry",
                win.app.info.id.clone(),
                serde_json::from_value(json!({"click":{"x":at.x,"y":at.y}})).unwrap(),
            ),
        )
        .await;
    unsafe_desktop::move_window(&win, fixture.pid(), original)?;
    require(
        result
            .error
            .as_ref()
            .is_some_and(|e| e.code == ErrorCode::StaleGeometry && e.state.is_some()),
        format!("expected stale_geometry + fresh state; last outcome={result:?}"),
    )?;
    require(
        fixture.read()?["counter"] == before,
        "stale coordinate had effects",
    )?;
    Ok(Outcome::Passed)
}

pub async fn redaction(driver: &WindowsDriver, fixture: &Fixture, win: &WindowRef) -> CaseResult {
    const SECRET: &str = "fixture-secret-29";
    let password = node(driver, win, "password_field").await?;
    if !password.states.contains(States::PASSWORD) {
        return Ok(Outcome::Skip("provider: egui password_field does not expose UIA IsPassword; native secure-edit fixture required; portable redaction test remains mandatory".into()));
    }
    super::cases::focus(driver, fixture, win, "password_field").await?;
    text(driver, win, SECRET).await?;
    fixture
        .wait("synthetic password entered", |v| {
            v["password_len"] == SECRET.len()
        })
        .await?;
    let raw = tree(driver, win).await?;
    let region = raw
        .root
        .as_ref()
        .and_then(|n| find(n, "password_field"))
        .and_then(|n| n.frame)
        .ok_or("password region missing")?;
    require(
        !format!("{raw:?}").contains(SECRET),
        "raw UIA debug leaked password",
    )?;
    require(
        !fixture.read()?.to_string().contains(SECRET),
        "fixture JSON leaked password",
    )?;
    let capture = driver
        .capture(win, &Default::default(), &Cancel::default())
        .await
        .map_err(|e| e.to_string())?;
    let mut processed = Pipeline::default().run(
        raw,
        &TreeOpts {
            max_tree_nodes: 1,
            ..Default::default()
        },
    );
    require(
        !processed.sensitive_regions.is_empty(),
        "budget erased sensitive collector",
    )?;
    processed.state_version = Some(1);
    processed.observed_at = Some(Duration::ZERO);
    let image = redact::apply(&processed, CapturedFrame::new(capture, 1, Duration::ZERO))
        .map_err(|e| e.to_string())?;
    for y in region.y.ceil() as u32..(region.y + region.h).floor() as u32 {
        for x in region.x.ceil() as u32..(region.x + region.w).floor() as u32 {
            let i = ((y * image.width() + x) * 4) as usize;
            require(
                image.pixels().get(i..i + 4) == Some(NEUTRAL_COLOR.as_slice()),
                format!("password pixel ({x},{y}) is not masked"),
            )?;
        }
    }
    let inner = Rect {
        x: region.x + 5.,
        y: region.y + 5.,
        w: region.w - 10.,
        h: region.h - 10.,
    };
    let crop = image.approval_crop(inner).map_err(|e| e.to_string())?;
    for derivative in [
        crop.clone(),
        crop.thumbnail(256).map_err(|e| e.to_string())?,
    ] {
        require(
            derivative
                .pixels()
                .as_chunks::<4>()
                .0
                .iter()
                .all(|p| *p == NEUTRAL_COLOR),
            "secret remained in approval/thumbnail pixels",
        )?;
        let encoded = derivative
            .encode(Default::default())
            .map_err(|e| e.to_string())?;
        require(
            super::cases::decode(encoded.bytes())?
                .as_chunks::<4>()
                .0
                .iter()
                .all(|p| *p == NEUTRAL_COLOR),
            "encoded derivative leaked pixels",
        )?;
    }
    Ok(Outcome::Passed)
}

#[cfg(feature = "test-worker-faults")]
pub async fn worker_timeout(
    primary: &WindowsDriver,
    fixture: &Fixture,
    primary_win: &WindowRef,
) -> CaseResult {
    // Focus the real counter through one explicit native click. Its setup effect
    // is accounted for before the timeout attempt; the action under test is Return.
    let setup_count = fixture.read()?["counter"]
        .as_u64()
        .ok_or("counter missing")?;
    click(
        primary,
        primary_win,
        center(&node(primary, primary_win, "counter_button").await?)?,
    )
    .await?;
    fixture
        .wait("counter focus click", |v| v["counter"] == setup_count + 1)
        .await?;
    let mut config = WorkerConfig::new(env!("CARGO_BIN_EXE_gcu-contract-worker"));
    config
        .environment
        .push(("GCU_FAULT_DIR".into(), fixture.dir.as_os_str().to_owned()));
    let driver = Arc::new(
        WindowsDriver::monitored(
            config,
            // A separate real Stop registration avoids competing with the
            // primary driver; this is trusted test setup, not a missing guard.
            gcu_driver_windows::input::hotkey::HotkeyConfig {
                modifiers: 7,
                virtual_key: 0x7b,
            },
            Arc::new(crate::common::TestStop::default()),
        )
        .await
        .map_err(|e| e.to_string())?,
    );
    let win = window(&driver, fixture).await?;
    let (router, session) = router(driver.clone());
    observe(&router, &session, &win, false).await?;
    let before = fixture.read()?["counter"].as_u64().unwrap();
    let out = router
        .execute(
            &session,
            ToolCall::new(
                "worker-timeout",
                win.app.info.id,
                serde_json::from_value(json!({"press_key":{"key":"Return"}})).unwrap(),
            ),
        )
        .await;
    require(
        out.action.outcome == ActionOutcome::Unknown && session.action_ledger().has_unknown(),
        format!("worker native timeout must be unknown; last outcome={out:?}"),
    )?;
    require(out.state.is_some(), "unknown must include fresh state")?;
    fixture
        .wait("timeout effect at most once", |v| {
            v["counter"] == before + 1
        })
        .await?;
    // The fresh-state read has waited for a new ready worker. No cold-start
    // sleep is used and the append-only trace must contain exactly one mutation.
    let calls = std::fs::read_to_string(fixture.dir.join("calls")).map_err(|e| e.to_string())?;
    require(
        calls.lines().collect::<Vec<_>>() == ["mutation"],
        format!("worker replayed: {calls:?}"),
    )?;
    require(
        fixture.read()?["counter"] == before + 1,
        "worker replay changed counter",
    )?;
    driver.release_all_input().await;
    Ok(Outcome::Passed)
}
#[cfg(not(feature = "test-worker-faults"))]
pub async fn worker_timeout(_: &WindowsDriver, _: &Fixture, _: &WindowRef) -> CaseResult {
    Ok(Outcome::NotRun(
        "test-worker-faults feature required for native worker adversary".into(),
    ))
}

pub async fn execution_surfaces(_: &WindowsDriver, fixture: &Fixture, _: &WindowRef) -> CaseResult {
    let driver = isolated().await?;
    let win = window(&driver, fixture).await?;
    let (router, session) = router(driver);
    let before = fixture.read()?;
    for title in [
        "IDE Debug Console",
        "Web SSH",
        "Cloud Shell",
        "Container Console",
        "Remote Desktop",
    ] {
        let _title = unsafe_desktop::Title::set(&win, fixture.pid(), title)?;
        observe(&router, &session, &win, false).await?;
        for step in [
            json!({"type_text":{"text":"fixture\n"}}),
            json!({"press_key":{"key":"Return"}}),
        ] {
            let step =
                serde_json::from_value(step).map_err(|e| format!("invalid contract step: {e}"))?;
            let out = router
                .execute(
                    &session,
                    ToolCall::new("execution-denied", win.app.info.id.clone(), step),
                )
                .await;
            require(
                out.error.as_ref().is_some_and(|e| {
                    matches!(
                        e.code,
                        ErrorCode::AppDenied
                            | ErrorCode::HandoffRequired
                            | ErrorCode::ElementNotActionable
                    )
                }),
                format!("{title}: unexpected outcome={out:?}"),
            )?;
            let after = fixture.read()?;
            for field in [
                "counter",
                "text",
                "multiline",
                "checked",
                "grid_hit",
                "submitted_count",
                "send_clicked",
            ] {
                require(
                    after[field] == before[field],
                    format!("{title}: execution entry changed {field}; last oracle={after}"),
                )?;
            }
        }
    }
    Ok(Outcome::Skip("fixture/core: full C26 needs native local execution controls and stable Router handle/screenshot gates; native IDE/remote title key/text refusal exercised, local terminal and all-tool policy matrix covered by portable tests".into()))
}

pub async fn hotkey_conflict(
    driver: &WindowsDriver,
    fixture: &Fixture,
    win: &WindowRef,
) -> CaseResult {
    require(
        driver.capabilities().delivery.foreground
            == gcu_protocol::capabilities::ForegroundCapability::Policy,
        "primary Stop listener unavailable",
    )?;
    // Give a wrongly delivered text event an observable effect. Native listener
    // health probes may update fixture key history, so do not compare timestamps
    // or the whole JSON document for this zero-effect assertion.
    super::cases::focus(driver, fixture, win, "name_field").await?;
    // Primary monitored driver owns the default hotkey. A second real registration
    // must fail and its foreground route must be revoked before any native action.
    let guard = SafetyGuard::start(
        Arc::new(RealSafetyApi),
        Default::default(),
        Arc::new(crate::common::TestStop::default()),
    )
    .await;
    require(
        !guard.available(),
        "conflicting RegisterHotKey unexpectedly succeeded",
    )?;
    let blocked = isolated().await?;
    let blocked = Arc::try_unwrap(blocked)
        .map_err(|_| "unexpected shared driver")?
        .with_safety(guard);
    require(
        blocked.capabilities().delivery.foreground
            == gcu_protocol::capabilities::ForegroundCapability::Unavailable,
        "hotkey conflict left foreground enabled",
    )?;
    let before = fixture.read()?;
    let out = blocked
        .perform(
            &DriverAction::TypeText {
                win: win.clone(),
                focus: None,
                text: "MUST-NOT-ARRIVE".into(),
                route: KeyRoute::AllowForeground,
            },
            &Cancel::default(),
        )
        .await;
    require(
        matches!(out, Err(DriverError::CapabilityUnavailable)),
        format!("hotkey failure route: {out:?}"),
    )?;
    let after = fixture.read()?;
    for field in [
        "text",
        "multiline",
        "password_len",
        "counter",
        "checked",
        "grid_hit",
    ] {
        require(
            after[field] == before[field],
            format!("hotkey-failed route changed {field}; last oracle={after}"),
        )?;
    }
    Ok(Outcome::Passed)
}

pub async fn uipi(_: &Fixture) -> CaseResult {
    let Some(pid) = std::env::var("GCU_ELEVATED_PID")
        .ok()
        .and_then(|s| s.parse::<u32>().ok())
        .filter(|pid| *pid != 0)
    else {
        return Ok(Outcome::NotRun("GCU_ELEVATED_PID must identify the launcher's disposable elevated fixture; no UAC automation".into()));
    };
    let api = RealWinApi::new().map_err(|e| e.to_string())?;
    require(
        api.process_security(pid)
            .map_err(|e| e.to_string())?
            .integrity
            >= IntegrityLevel::High,
        "UIPI fixture is not elevated",
    )?;
    let path = api.process_path(pid).map_err(|e| e.to_string())?;
    let driver = isolated().await?;
    let result = driver.resolve_app(&AppQuery(path)).await;
    match result {
        Err(DriverError::ElevationRequired) => (),
        Ok(app) => {
            let result = driver.key_window(&app).await;
            require(
                matches!(result, Err(DriverError::ElevationRequired)),
                format!("high integrity target accepted: {result:?}"),
            )?;
        }
        Err(error) => return Err(format!("wrong UIPI refusal: {error}")),
    }
    Ok(Outcome::Passed)
}
