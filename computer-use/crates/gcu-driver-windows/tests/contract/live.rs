use super::harness::*;
use gcu_core::{
    driver::*,
    model::{RawNode, RawTree, WindowPoint},
};
use gcu_driver_windows::{worker::WorkerConfig, WindowsDriver};
use serde_json::Value;
use std::{
    path::PathBuf,
    process::{Child, Command},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};

#[path = "background.rs"]
mod background;
#[path = "cases.rs"]
mod cases;
#[path = "safety.rs"]
mod safety;
#[path = "unsafe_activation.rs"]
mod unsafe_activation;
#[path = "unsafe_background_probe.rs"]
mod unsafe_background_probe;
#[path = "unsafe_click_probe.rs"]
mod unsafe_click_probe;
#[path = "unsafe_desktop.rs"]
pub mod unsafe_desktop;
#[path = "unsafe_foreground.rs"]
mod unsafe_foreground;
#[path = "unsafe_hotkey.rs"]
mod unsafe_hotkey;
#[path = "unsafe_invoke.rs"]
mod unsafe_invoke;

static SEQUENCE: AtomicU64 = AtomicU64::new(0);
pub struct Fixture {
    child: Child,
    pub dir: PathBuf,
    pub state: PathBuf,
}
impl Fixture {
    pub async fn start() -> Result<Self, String> {
        let exe = std::env::var_os("GCU_FIXTURE_EXE").ok_or("GCU_FIXTURE_EXE is required")?;
        let dir = std::env::temp_dir().join(format!(
            "wc{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let state = dir.join("state.json");
        let child = Command::new(exe)
            .env("GCU_FIXTURE_STATE", &state)
            .env(
                "GCU_FIXTURE_ACTIVATION_TRACE_DIR",
                std::env::var_os("GCU_NATIVE_TRACE_DIR")
                    .unwrap_or_else(|| dir.clone().into_os_string()),
            )
            .spawn()
            .map_err(|e| {
                let _ = std::fs::remove_dir_all(&dir);
                e.to_string()
            })?;
        let fixture = Self { child, dir, state };
        // Process start, eframe and wgpu initialisation are fixture setup, not the behaviour
        // under test; a loaded runner has taken longer than the 5s used for observed effects.
        fixture
            .wait_within(Duration::from_secs(30), "initial fixture JSON", |v| {
                v["counter"].as_u64().is_some()
            })
            .await?;
        Ok(fixture)
    }
    pub fn read(&self) -> Result<Value, String> {
        read_oracle(&self.state)
    }
    pub async fn wait(
        &self,
        label: &str,
        accept: impl Fn(&Value) -> bool,
    ) -> Result<Value, String> {
        self.wait_within(Duration::from_secs(5), label, accept)
            .await
    }
    pub async fn wait_within(
        &self,
        budget: Duration,
        label: &str,
        accept: impl Fn(&Value) -> bool,
    ) -> Result<Value, String> {
        poll(
            &mut DesktopClock::default(),
            budget,
            label,
            || self.read(),
            accept,
        )
        .await
    }
    pub fn pid(&self) -> u32 {
        self.child.id()
    }
    async fn stop(&mut self) -> Result<(), String> {
        if self.child.try_wait().map_err(|e| e.to_string())?.is_none() {
            self.child
                .kill()
                .map_err(|e| format!("fixture reset kill: {e}"))?;
        }
        poll(
            &mut DesktopClock::default(),
            Duration::from_secs(5),
            "fixture reset exit",
            || self.child.try_wait().map_err(|e| e.to_string()),
            |status| status.is_some(),
        )
        .await?;
        Ok(())
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}
pub async fn driver() -> Result<WindowsDriver, String> {
    super::timings::install();
    if let super::mouse_matrix::Decision::Run { probe, marked } =
        super::unsafe_mouse_probe::decision()?
    {
        // Debug/test-only selection; actual tagged injected callbacks must still
        // renew both hook leases. Unmarked-only evidence never grants health.
        std::env::set_var("GCU_HOOK_MOUSE_PROBE", probe);
        progress(format!("MOUSE MATRIX: retry product proof with {probe}; observed marked={marked}; no environment Skip permitted"));
    }
    WindowsDriver::monitored(
        worker_config()?,
        Default::default(),
        Arc::new(crate::common::TestStop::default()),
    )
    .await
    .map_err(|e| format!("{e}; native_parent_timings={}", super::timings::snapshot()))
}
pub fn worker_config() -> Result<WorkerConfig, String> {
    let exe = std::env::var_os("GCU_WORKER_EXE").ok_or("GCU_WORKER_EXE is required")?;
    let mut config = WorkerConfig::new(exe);
    // Debug workers on this disposable desktop may report fixture node labels.
    config
        .environment
        .push(("GCU_UIA_DIAGNOSTICS".into(), "1".into()));
    if let Some(directory) = std::env::var_os("GCU_NATIVE_TRACE_DIR") {
        config
            .environment
            .push(("GCU_NATIVE_TRACE_DIR".into(), directory));
    }
    // Experiment switch for the pointer-gate stress test (debug workers only).
    if let Some(value) = std::env::var_os("GCU_POINTER_GATE_OFF") {
        config
            .environment
            .push(("GCU_POINTER_GATE_OFF".into(), value));
    }
    Ok(config)
}
pub async fn window(driver: &WindowsDriver, fixture: &Fixture) -> Result<WindowRef, String> {
    let exe = std::env::var("GCU_FIXTURE_EXE").map_err(|e| e.to_string())?;
    super::discovery::discover(
        &mut DesktopClock::default(),
        Duration::from_secs(30),
        driver,
        &exe,
        fixture.pid(),
        || fixture.read(),
    )
    .await
}

pub async fn tree(driver: &WindowsDriver, win: &WindowRef) -> Result<RawTree, String> {
    let tree = driver
        .snapshot_tree(win, &TreeOpts::default(), &Cancel::default())
        .await
        .map_err(|e| e.to_string())?;
    require(
        tree.incomplete.is_none(),
        format!(
            "native tree incomplete: {}; native_parent_timings={}",
            super::readiness::summary(&tree),
            super::timings::snapshot()
        ),
    )?;
    Ok(tree)
}
pub fn find<'a>(root: &'a RawNode, name: &str) -> Option<&'a RawNode> {
    if root.name.as_deref() == Some(name) || root.identifier.as_deref() == Some(name) {
        Some(root)
    } else {
        root.children.iter().find_map(|n| find(n, name))
    }
}
pub async fn node(driver: &WindowsDriver, win: &WindowRef, name: &str) -> Result<RawNode, String> {
    // JSON readiness and a created HWND precede the first AccessKit update.
    // Retry observations only; callers still dispatch each mutation once.
    poll_async(
        &mut DesktopClock::default(),
        Duration::from_secs(5),
        &format!("UIA control {name}"),
        async || {
            let raw = tree(driver, win).await?;
            raw.root
                .as_ref()
                .and_then(|r| find(r, name))
                .cloned()
                .ok_or_else(|| format!("missing control {name}; last tree={raw:?}"))
        },
        |_| true,
    )
    .await
}
async fn ready_fixture(
    driver: &WindowsDriver,
    win: &WindowRef,
    fixture: &Fixture,
) -> Result<(), String> {
    super::readiness::wait(
        &mut DesktopClock::default(),
        Duration::from_secs(30),
        "fixture counter_button",
        || fixture.read(),
        async || {
            driver
                .snapshot_tree(win, &TreeOpts::default(), &Cancel::default())
                .await
                .map_err(|e| e.to_string())
        },
        |raw| {
            raw.root
                .as_ref()
                .is_some_and(|r| find(r, "counter_button").is_some())
        },
    )
    .await?;
    Ok(())
}

async fn reset_fixture(driver: &WindowsDriver, fixture: &mut Fixture) -> Result<WindowRef, String> {
    driver.release_all_input().await;
    let previous = fixture.pid();
    fixture.stop().await?;
    *fixture = Fixture::start().await?;
    let state = fixture.read()?;
    require(
        state["counter"] == 0
            && state["tab"] == 0
            && state["modal_open"] == false
            && state["text"] == "",
        format!("fixture reset oracle mismatch: {state}"),
    )?;
    let win = window(driver, fixture).await?;
    ready_fixture(driver, &win, fixture).await?;
    progress(format!("FIXTURE RESET: previous_pid={previous}; pid={}; complete UIA counter_button; modal_open=false", fixture.pid()));
    Ok(win)
}
pub async fn perform(driver: &WindowsDriver, action: DriverAction) -> Result<Delivery, String> {
    driver
        .perform(&action, &Cancel::default())
        .await
        .map_err(|e| format!("action failed: {e}"))
}
pub async fn press(driver: &WindowsDriver, win: &WindowRef, name: &str) -> Result<(), String> {
    perform(
        driver,
        DriverAction::Press {
            el: node(driver, win, name).await?.handle,
        },
    )
    .await?;
    Ok(())
}
pub fn center(node: &RawNode) -> Result<WindowPoint, String> {
    let f = node.frame.ok_or("control has no physical frame")?;
    Ok(WindowPoint {
        x: f.x + f.w / 2.,
        y: f.y + f.h / 2.,
    })
}
pub async fn click(driver: &WindowsDriver, win: &WindowRef, at: WindowPoint) -> Result<(), String> {
    perform(
        driver,
        DriverAction::PointerClick {
            win: win.clone(),
            at,
            button: Button::Left,
            count: 1,
            route: PointerRoute::AllowForeground,
        },
    )
    .await?;
    Ok(())
}
pub async fn key(driver: &WindowsDriver, win: &WindowRef, key: &str) -> Result<(), String> {
    perform(
        driver,
        DriverAction::PressKeys {
            win: win.clone(),
            chord: KeyChord::parse(key).map_err(|e| e.to_string())?,
            route: KeyRoute::AllowForeground,
        },
    )
    .await?;
    Ok(())
}
pub async fn text(driver: &WindowsDriver, win: &WindowRef, text: &str) -> Result<(), String> {
    perform(
        driver,
        DriverAction::TypeText {
            win: win.clone(),
            focus: None,
            text: text.into(),
            route: KeyRoute::AllowForeground,
        },
    )
    .await?;
    Ok(())
}

#[tokio::test]
#[ignore = "not_run: disposable interactive unelevated Windows desktop required; CI runs --ignored"]
async fn live_contract_table_c01_to_c27() {
    let mut report = Report::default();
    report.pending_snapshot().save();
    progress(format!(
        "CONTRACT BEGIN: {} cases; setup fixture JSON, worker and UIA",
        CASES.len()
    ));
    for (id, description) in CASES {
        progress(format!("CONTRACT PLANNED: {id}: {description}"));
    }
    let setup = attempt_bounded(
        &mut DesktopClock::default(),
        Duration::from_secs(75),
        "setup",
        async {
            require(
                std::env::var("GCU_DESKTOP").as_deref() == Ok("1"),
                "explicit GCU_DESKTOP=1 required",
            )?;
            progress(format!(
                "STOP ALTERNATIVE NATIVE EVIDENCE: {:?}",
                unsafe_hotkey::probe().await
            ));
            progress("CONTRACT SETUP: fixture start; waiting for external state JSON");
            let fixture = Fixture::start().await?;
            progress(format!(
                "CONTRACT SETUP: fixture pid={}; worker start",
                fixture.pid()
            ));
            let driver = driver().await?;
            progress(format!("CONTRACT DOCTOR: {:?}", driver.doctor().await));
            progress("CONTRACT SETUP: window discovery");
            let win = window(&driver, &fixture).await?;
            progress("CONTRACT SETUP: waiting for UIA counter_button");
            ready_fixture(&driver, &win, &fixture).await?;
            Ok::<_, String>((fixture, driver, win))
        },
    )
    .await;
    let (mut fixture, driver, mut win) = match setup {
        Ok(setup) => setup,
        Err(error) => {
            for (id, _) in CASES {
                report.record(id, Ok(Outcome::NotRun(error.clone())));
            }
            report.finish();
            panic!("native suite not_run: {error}");
        }
    };
    // A fresh process resets egui memory, popup/focus state and the external
    // oracle. The driver/worker stays alive so reattachment is also exercised.
    let mut reset_failure = None;
    for (row, (id, description)) in CASES
        .iter()
        .filter(|r| r.0 != "C19")
        .chain(CASES.iter().filter(|r| r.0 == "C19"))
        .enumerate()
    {
        if let Some(reason) = &reset_failure {
            report.record(
                id,
                Ok(Outcome::NotRun(format!("fixture reset failed: {reason}"))),
            );
            continue;
        }
        if row != 0 {
            let reset = attempt_bounded(
                &mut DesktopClock::default(),
                Duration::from_secs(45),
                "fixture reset",
                reset_fixture(&driver, &mut fixture),
            )
            .await;
            match reset {
                Ok(fresh) => win = fresh,
                Err(reason) => {
                    report.record(
                        id,
                        Err(format!(
                            "fixture reset before {id} failed: {reason}; last fixture={:?}",
                            fixture.read()
                        )),
                    );
                    reset_failure = Some(reason);
                    continue;
                }
            }
        }
        progress(format!(
            "CONTRACT RUN: {id}: {description}; timeout_ms=45000; no retry"
        ));
        let result = attempt_bounded(
            &mut DesktopClock::default(),
            Duration::from_secs(45),
            id,
            cases::run(id, &driver, &fixture, &win),
        )
        .await;
        let result = match result {
            Ok(outcome) => Ok(outcome),
            Err(error) => {
                let health = unsafe_desktop::responsiveness(&win).await;
                Err(format!("{error}; fixture window health={health:?}; native_parent_timings={}; last fixture={:?}", super::timings::snapshot(), fixture.read()))
            }
        };
        report.record(id, result);
    }
    driver.release_all_input().await;
    report.finish();
    assert_eq!(
        report.failed + report.not_run,
        0,
        "native contract failures; first cause: {:?}",
        report.first_failure()
    );
}

/// Measurement only (no verdict): drives the real driver's foreground click and records where the pointer
/// went, which window sat under it, and whether the fixture counted the click. Run with
/// `--exact live::live_click_probe` from the standard-user console session.
#[tokio::test]
#[ignore = "not_run: disposable interactive unelevated Windows desktop required; measurement only"]
async fn live_click_probe() {
    require(
        std::env::var("GCU_DESKTOP").as_deref() == Ok("1"),
        "explicit GCU_DESKTOP=1 required",
    )
    .unwrap();
    let mut fixture = Fixture::start().await.unwrap();
    let driver = driver().await.unwrap();
    let win = window(&driver, &fixture).await.unwrap();
    ready_fixture(&driver, &win, &fixture).await.unwrap();
    let geometry = driver.window_geometry(&win).await.unwrap();
    let button = node(&driver, &win, "counter_button").await.unwrap();
    let at = center(&button).unwrap();
    let frame = geometry.frame;
    let target = (
        (frame.x + at.x).round() as i32,
        (frame.y + at.y).round() as i32,
    );
    progress(format!(
        "PROBE geometry: frame=({},{} {}x{}) scale={} button_center_in_window=({:.1},{:.1}) expected_screen={:?} fixture_pid={}",
        frame.x, frame.y, frame.w, frame.h, geometry.scale, at.x, at.y, target, fixture.pid()
    ));
    let (left, top) = (frame.x as i32, frame.y as i32);
    let hooklog = unsafe_click_probe::HookLog::start();
    progress(format!(
        "PROBE control (parent process SendInput, driver's formula) to {target:?}: {}",
        unsafe_click_probe::control_move(target.0, target.1)
    ));
    let inspect = std::process::Command::new("powershell")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
        .arg(r"D:\cu2\hw\worker-inspect.ps1")
        .output();
    progress(format!(
        "PROBE worker inspect: {}",
        inspect
            .map(|o| String::from_utf8_lossy(&o.stdout).replace('\n', " | "))
            .unwrap_or_else(|e| e.to_string())
    ));
    let starts: Vec<(&str, Option<(i32, i32)>)> = vec![
        ("A as-is", None),
        ("B far left monitor", Some((-2460, 100))),
        ("C title bar left part", Some((left + 120, top + 12))),
        (
            "D below window",
            Some((left + 300, top + frame.h as i32 + 60)),
        ),
        (
            "E inside window blank",
            Some((left + frame.w as i32 - 120, top + frame.h as i32 - 120)),
        ),
        ("F as-is again", None),
    ];
    for (name, start) in starts {
        if let Some((x, y)) = start {
            unsafe_click_probe::set_cursor(x, y);
            tokio::time::sleep(Duration::from_millis(400)).await;
        }
        let before = unsafe_click_probe::cursor();
        let counter_before = fixture.read().map(|v| v["counter"].clone());
        let _ = hooklog.take();
        let sampler = unsafe_click_probe::Sampler::start();
        let result = click(&driver, &win, at).await;
        let timeline = sampler.finish();
        let seen = hooklog.take();
        progress(format!(
            "PROBE {name} mouse events seen by an independent LL hook ({}): {:?}",
            seen.len(),
            {
                // Keep every untagged event; keep tagged heartbeat events only when the position changes.
                let mut last_pt = String::new();
                let mut kept = Vec::new();
                for e in &seen {
                    let pt = e
                        .split("pt=")
                        .nth(1)
                        .and_then(|r| r.split(' ').next())
                        .unwrap_or_default()
                        .to_string();
                    let tagged = e.contains("extra=1195573");
                    if !tagged || pt != last_pt {
                        kept.push(e.as_str());
                    }
                    last_pt = pt;
                }
                kept.truncate(60);
                kept
            }
        ));
        let after = unsafe_click_probe::cursor();
        let under = unsafe_click_probe::under_cursor();
        tokio::time::sleep(Duration::from_millis(900)).await;
        let state = fixture.read();
        progress(format!(
            "PROBE {name} cursor timeline (ms,x,y): {timeline:?}"
        ));
        progress(format!(
            "PROBE {name}: result={result:?} cursor {before:?} -> {after:?} (expected {target:?}) under_cursor=[{under}] fg={:?} counter {counter_before:?} -> {:?} events={}",
            unsafe_foreground::current(),
            state.as_ref().map(|v| v["counter"].clone()),
            state.as_ref().map(|v| v["event_log"].to_string()).unwrap_or_default()
        ));
    }
    progress("PROBE DONE");
    let _ = fixture.stop().await;
}

/// Measurement only (no verdict): types short strings through the real driver into the multiline field and
/// records the raw keyboard stream seen by an independent WH_KEYBOARD_LL hook next to the field text the
/// fixture ended up with. Run with `--exact live::live_text_probe` from the standard-user console session.
#[tokio::test]
#[ignore = "not_run: disposable interactive unelevated Windows desktop required; measurement only"]
async fn live_text_probe() {
    require(
        std::env::var("GCU_DESKTOP").as_deref() == Ok("1"),
        "explicit GCU_DESKTOP=1 required",
    )
    .unwrap();
    let mut fixture = Fixture::start().await.unwrap();
    let driver = driver().await.unwrap();
    let win = window(&driver, &fixture).await.unwrap();
    ready_fixture(&driver, &win, &fixture).await.unwrap();
    cases::focus(&driver, &fixture, &win, "multiline_field")
        .await
        .unwrap();
    let keylog = unsafe_click_probe::KeyLog::start();
    for sample in [
        "abc",
        "中文",
        "😀",
        "👩\u{200d}💻",
        "a\nb",
        "a\r\nb",
        "x😀y",
    ] {
        let _ = keylog.take();
        let before = fixture.read().map(|v| v["multiline"].to_string());
        let result = perform(
            &driver,
            DriverAction::TypeText {
                win: win.clone(),
                focus: None,
                text: sample.to_string(),
                route: KeyRoute::AllowForeground,
            },
        )
        .await;
        tokio::time::sleep(Duration::from_millis(900)).await;
        let keys = keylog.take();
        let after = fixture.read().map(|v| v["multiline"].to_string());
        progress(format!(
            "TEXT PROBE {sample:?} units={:?}: result={result:?} multiline {before:?} -> {after:?}",
            sample.encode_utf16().collect::<Vec<_>>()
        ));
        progress(format!("TEXT PROBE {sample:?} keyboard stream: {keys}"));
    }
    progress("TEXT PROBE DONE");
    let _ = fixture.stop().await;
}

/// Measurement only (no verdict): which keyboard event order makes the fixture's text field accept a
/// surrogate pair and a newline. Sends directly from the test process into the multiline field.
#[tokio::test]
#[ignore = "not_run: disposable interactive unelevated Windows desktop required; measurement only"]
async fn live_surrogate_probe() {
    use unsafe_click_probe::{send_key_groups, KeyStep};
    require(
        std::env::var("GCU_DESKTOP").as_deref() == Ok("1"),
        "explicit GCU_DESKTOP=1 required",
    )
    .unwrap();
    let mut fixture = Fixture::start().await.unwrap();
    let driver = driver().await.unwrap();
    let win = window(&driver, &fixture).await.unwrap();
    ready_fixture(&driver, &win, &fixture).await.unwrap();
    cases::focus(&driver, &fixture, &win, "multiline_field")
        .await
        .unwrap();
    let (h, l) = (0xD83Du16, 0xDE00u16);
    let u = |unit: u16, up: bool| -> KeyStep { (0, unit, up) };
    let cases: Vec<(&str, Vec<Vec<KeyStep>>, u64)> = vec![
        (
            "A H-dn H-up L-dn L-up (current driver order), one call",
            vec![vec![u(h, false), u(h, true), u(l, false), u(l, true)]],
            0,
        ),
        (
            "B H-dn L-dn H-up L-up, one call",
            vec![vec![u(h, false), u(l, false), u(h, true), u(l, true)]],
            0,
        ),
        (
            "C H-dn L-dn L-up H-up, one call",
            vec![vec![u(h, false), u(l, false), u(l, true), u(h, true)]],
            0,
        ),
        (
            "D H-dn L-dn in one call, then L-up H-up in another after 50 ms",
            vec![vec![u(h, false), u(l, false)], vec![u(l, true), u(h, true)]],
            50,
        ),
        (
            "E H-dn, 30 ms, L-dn L-up H-up",
            vec![vec![u(h, false)], vec![u(l, false), u(l, true), u(h, true)]],
            30,
        ),
        (
            "F H-dn only then L-dn only, ups after (four calls)",
            vec![
                vec![u(h, false)],
                vec![u(l, false)],
                vec![u(h, true)],
                vec![u(l, true)],
            ],
            20,
        ),
        (
            "G Enter as VK_RETURN between a and b",
            vec![
                vec![u(0x61, false), u(0x61, true)],
                vec![(0x0D, 0, false), (0x0D, 0, true)],
                vec![u(0x62, false), u(0x62, true)],
            ],
            30,
        ),
    ];
    for (name, groups, pause) in cases {
        cases::clear_for_probe(&driver, &fixture, &win)
            .await
            .unwrap();
        let slices: Vec<&[KeyStep]> = groups.iter().map(|g| g.as_slice()).collect();
        let report = send_key_groups(&slices, pause);
        tokio::time::sleep(Duration::from_millis(700)).await;
        let after = fixture.read().map(|v| v["multiline"].to_string());
        progress(format!(
            "SURROGATE PROBE {name}: sent={report} multiline -> {after:?}"
        ));
    }
    progress("SURROGATE PROBE DONE");
    let _ = fixture.stop().await;
}

/// Measurement only (no verdict): repeats the C15 drag with one long-lived driver and Stop guard, recording
/// every iteration. `GCU_POINTER_GATE_OFF=1` (debug builds) disables the pointer gate in host and worker so
/// the flake rate can be compared. `GCU_STRESS_N` sets the iteration count (default 12).
#[tokio::test]
#[ignore = "not_run: disposable interactive unelevated Windows desktop required; measurement only"]
async fn live_drag_stress() {
    require(
        std::env::var("GCU_DESKTOP").as_deref() == Ok("1"),
        "explicit GCU_DESKTOP=1 required",
    )
    .unwrap();
    let gate = if std::env::var_os("GCU_POINTER_GATE_OFF").is_some() {
        "off"
    } else {
        "on"
    };
    let n: usize = std::env::var("GCU_STRESS_N")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(12);
    let driver = driver().await.unwrap();
    let (mut ok, mut failed) = (0, 0);
    for i in 0..n {
        let mut fixture = Fixture::start().await.unwrap();
        let attempt = async {
            let win = window(&driver, &fixture).await?;
            ready_fixture(&driver, &win, &fixture).await?;
            let start = center(&node(&driver, &win, "drag_source").await?)?;
            let end = center(&node(&driver, &win, "drop_target").await?)?;
            perform(
                &driver,
                DriverAction::PointerDrag {
                    win: win.clone(),
                    path: vec![start, end],
                    route: PointerRoute::AllowForeground,
                },
            )
            .await?;
            fixture
                .wait("drop target", |v| v["drop_ok"] == true)
                .await?;
            Ok::<(), String>(())
        }
        .await;
        let foreground = driver.capabilities().delivery.foreground;
        match &attempt {
            Ok(()) => ok += 1,
            Err(_) => failed += 1,
        }
        progress(format!(
            "STRESS gate={gate} i={i}: {} foreground_capability={foreground:?}",
            match &attempt {
                Ok(()) => "ok".to_string(),
                Err(e) => format!("FAIL {}", &e[..e.len().min(160)]),
            }
        ));
        let _ = fixture.stop().await;
        if attempt.is_err() && format!("{foreground:?}").contains("Unavailable") {
            progress(format!(
                "STRESS gate={gate}: Stop guard unavailable after iteration {i}; stopping"
            ));
            break;
        }
    }
    progress(format!(
        "STRESS gate={gate} DONE ok={ok} failed={failed} of {n}"
    ));
}

/// Measurement only. Sends `MOVE|ABSOLUTE|VIRTUALDESK` to at least 200 pixels, including the
/// virtual-desktop edges, and prints landing error against the requested pixel and against the
/// inverse of the driver's normalization. Does not decide the arrival tolerance.
/// Run with `--exact live::live_move_landing_probe --ignored --nocapture`.
#[tokio::test]
#[ignore = "not_run: disposable interactive unelevated Windows desktop required; measurement only; do not merge until this histogram confirms inverse error is 0"]
async fn live_move_landing_probe() {
    require(
        std::env::var("GCU_DESKTOP").as_deref() == Ok("1"),
        "explicit GCU_DESKTOP=1 required",
    )
    .unwrap();
    let report = unsafe_click_probe::landing_histogram().unwrap();
    progress(report);
}
