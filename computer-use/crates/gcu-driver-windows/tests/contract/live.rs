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
        fixture
            .wait("initial fixture JSON", |v| v["counter"].as_u64().is_some())
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
        poll(
            &mut DesktopClock::default(),
            Duration::from_secs(5),
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
