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

#[path = "cases.rs"]
mod cases;
#[path = "safety.rs"]
mod safety;
#[path = "unsafe_desktop.rs"]
pub mod unsafe_desktop;

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
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}
pub async fn driver() -> Result<WindowsDriver, String> {
    let exe = std::env::var_os("GCU_WORKER_EXE").ok_or("GCU_WORKER_EXE is required")?;
    WindowsDriver::monitored(
        WorkerConfig::new(exe),
        Default::default(),
        Arc::new(crate::common::TestStop::default()),
    )
    .await
    .map_err(|e| e.to_string())
}
pub async fn window(driver: &WindowsDriver, fixture: &Fixture) -> Result<WindowRef, String> {
    let exe = std::env::var("GCU_FIXTURE_EXE").map_err(|e| e.to_string())?;
    poll_async(
        &mut DesktopClock::default(),
        Duration::from_secs(5),
        "fixture discovery",
        async || {
            let app = driver
                .resolve_app(&AppQuery(exe.clone()))
                .await
                .map_err(|e| e.to_string())?;
            require(
                app.info.pid == Some(fixture.pid()),
                format!("unexpected fixture PID: {:?}", app.info.pid),
            )?;
            let info = driver
                .list_windows(Some(&app))
                .await
                .map_err(|e| e.to_string())?
                .into_iter()
                .find(|w| w.title == "GCU Fixture")
                .ok_or("fixture window missing")?;
            Ok(WindowRef { app, id: info.id })
        },
        |_| true,
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
        format!("native tree incomplete: {:?}", tree.incomplete),
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
    println!(
        "CONTRACT BEGIN: {} cases; setup fixture JSON, worker and UIA",
        CASES.len()
    );
    for (id, description) in CASES {
        println!("CONTRACT PLANNED: {id}: {description}");
    }
    let setup = attempt(async {
        require(
            std::env::var("GCU_DESKTOP").as_deref() == Ok("1"),
            "explicit GCU_DESKTOP=1 required",
        )?;
        println!("CONTRACT SETUP: fixture start; waiting for external state JSON");
        let fixture = Fixture::start().await?;
        println!(
            "CONTRACT SETUP: fixture pid={}; worker start",
            fixture.pid()
        );
        let driver = driver().await?;
        println!("CONTRACT SETUP: window discovery");
        let win = window(&driver, &fixture).await?;
        println!("CONTRACT SETUP: waiting for UIA counter_button");
        node(&driver, &win, "counter_button").await?;
        Ok::<_, String>((fixture, driver, win))
    })
    .await;
    let (fixture, driver, win) = match setup {
        Ok(setup) => setup,
        Err(error) => {
            for (id, _) in CASES {
                report.record(id, Ok(Outcome::NotRun(error.clone())));
            }
            report.finish();
            panic!("native suite not_run: {error}");
        }
    };
    // Each row observes its own initial oracle; actions are never retried.
    for (id, description) in CASES
        .iter()
        .filter(|r| r.0 != "C19")
        .chain(CASES.iter().filter(|r| r.0 == "C19"))
    {
        println!("{id}: {description}");
        let result = attempt(cases::run(id, &driver, &fixture, &win)).await;
        let result = result.map_err(|e| format!("{e}; last fixture={:?}", fixture.read()));
        report.record(id, result);
    }
    driver.release_all_input().await;
    report.finish();
    assert_eq!(
        report.failed + report.not_run,
        0,
        "native contract failures; see report rows"
    );
}
