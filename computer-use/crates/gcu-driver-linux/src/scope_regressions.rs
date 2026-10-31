//! Production regressions for the S10 scope fixes.
//!
//! Each test calls a driver, `ScopedDriver`, or `Session` method. Helpers only
//! build fixtures.

use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use gcu_core::driver::{
    AppQuery, AppRef, Cancel, Driver, DriverAction, DriverError, TreeOpts, WindowRef,
    NATIVE_TIMEOUT,
};
use gcu_core::model::NativeHandle;
use gcu_core::scope::{
    BootIdentity, ExecutableProof, FixtureScope, FixtureTarget, InstanceEvidence, OsStartTime,
    ProcessInstance, RegisteredWindow, ScopeLease, ScopedDriver, WindowBinding,
};
use gcu_core::session::{StopTimings, RELEASE_INPUT_BUDGET, STOP_LATCH_BUDGET};
use gcu_core::testing::host::StopControl;
use gcu_protocol::{types::AppIdentity, AppInfo, Rect, WindowInfo};

use crate::atspi::{FakeNode, FakeSource};
use crate::driver::LinuxDriver;
use crate::identity::scope::{MapClock, ProcessClock, ScopeGate, ScopeInstance, ScopeWindow};
use crate::x11::fake::FakeXEvent;
use crate::x11::{FakeWindow, FakeX, XRootGeometry, XServer, XWindowGeometry};

const BOOT: &str = "boot-a";
const APP_ID: &str = "org.fixture.App";

fn process(pid: u32, start: u64) -> ProcessInstance {
    ProcessInstance::new(
        pid,
        OsStartTime::new(start).unwrap(),
        BootIdentity::new(BOOT).unwrap(),
    )
    .unwrap()
}

fn lease_for(native_id: &str) -> ScopeLease {
    lease_for_pid(42, native_id)
}

fn target_for(pid: u32, native_id: &str) -> FixtureTarget {
    let owner = process(pid, 1000);
    let window = WindowBinding::new(native_id, 5, owner.clone()).unwrap();
    FixtureTarget::new(
        owner,
        "fixture",
        ExecutableProof::new("/usr/bin/fixture").unwrap(),
        vec![RegisteredWindow::new(window)],
    )
    .unwrap()
}

fn generation_scope(targets: Vec<FixtureTarget>, generation: u64) -> FixtureScope {
    FixtureScope::try_new(
        "run-s10",
        generation,
        "engine-s10",
        "desktop-s10",
        "lease-s10",
        targets,
    )
    .unwrap()
}

fn lease_for_pid(pid: u32, native_id: &str) -> ScopeLease {
    ScopeLease::install(generation_scope(vec![target_for(pid, native_id)], 1))
}

fn empty_lease() -> ScopeLease {
    ScopeLease::install(generation_scope(Vec::new(), 1))
}

fn gate(native_id: &str) -> ScopeGate {
    gate_for(42, native_id)
}

fn gate_for(pid: u32, native_id: &str) -> ScopeGate {
    let owner = ScopeInstance::try_new(pid, 1000, BOOT).unwrap();
    let window = ScopeWindow::try_new(native_id, 5, owner, APP_ID).unwrap();
    ScopeGate::try_new(vec![window]).unwrap()
}

fn clock() -> Arc<MapClock> {
    let clock = Arc::new(MapClock::new(BOOT));
    clock.set(42, 1000);
    clock.set(99, 5000);
    clock
}

fn app(name: &str, pid: u32, executable: &str, hash: &str) -> AppInfo {
    AppInfo {
        id: APP_ID.into(),
        name: name.into(),
        pid: Some(pid),
        identity: Some(AppIdentity {
            id: APP_ID.into(),
            executable: Some(executable.into()),
            team_id: None,
            publisher: None,
            code_hash: Some(hash.into()),
        }),
    }
}

fn app_ref(info: AppInfo) -> AppRef {
    AppRef { info }
}

fn window_ref(info: &AppInfo, id: &str) -> WindowRef {
    WindowRef {
        app: app_ref(info.clone()),
        id: id.into(),
    }
}

fn frame() -> Rect {
    Rect {
        x: 0.0,
        y: 0.0,
        w: 20.0,
        h: 10.0,
    }
}

fn sibling_window() -> WindowInfo {
    WindowInfo {
        id: "sibling-b".into(),
        title: "sibling-canary".into(),
        frame: frame(),
        scale: 1.0,
        on_current_desktop: Some(true),
    }
}

fn bind(driver: &mut LinuxDriver, native_id: &str, clock: &Arc<MapClock>) {
    let clock: Arc<dyn ProcessClock> = Arc::clone(clock) as Arc<dyn ProcessClock>;
    driver.bind_scope(gate(native_id), clock);
}

fn assert_clean(text: &str, forbidden: &[&str]) {
    for item in forbidden {
        assert!(!text.contains(item), "{item} leaked into {text}");
    }
}

fn root_with(names: &[&str]) -> FakeNode {
    FakeNode {
        name: Some("replacement-canary".into()),
        children: names
            .iter()
            .enumerate()
            .map(|(index, name)| FakeNode {
                id: (index as u64) + 1,
                name: Some((*name).to_string()),
                ..FakeNode::default()
            })
            .collect(),
        ..FakeNode::default()
    }
}

struct ReleaseOnDrop(Arc<AtomicBool>);

impl Drop for ReleaseOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

fn x_window(id: u32, pid: u32) -> FakeWindow {
    FakeWindow {
        id,
        name: Some("x-window".into()),
        pid: Some(pid),
        geometry: XWindowGeometry {
            x: 1,
            y: 2,
            width: 30,
            height: 40,
            border_width: 0,
            depth: 24,
        },
        is_mapped: true,
        image: None,
    }
}

#[tokio::test(flavor = "current_thread")]
async fn stalled_x11_evidence_releases_stop_before_the_native_ceiling() {
    let x = Arc::new(FakeX::new(
        XRootGeometry {
            root_id: 1,
            width: 100,
            height: 100,
            scale: 1.0,
        },
        true,
    ));
    x.add_window(x_window(7, 42));
    x.set_window_incarnation(7, 5);
    let clock = clock();
    let mut driver = LinuxDriver::new_fake(
        FakeSource::default(),
        FakeNode::default(),
        Vec::new(),
        Vec::new(),
    );
    bind(&mut driver, "7", &clock);
    let server: Arc<dyn XServer> = Arc::clone(&x) as Arc<dyn XServer>;
    let driver = driver.with_x_server(server);
    let (entered, release) = x.arm_metadata_stall();
    let _guard = ReleaseOnDrop(Arc::clone(&release));
    let scoped = Arc::new(ScopedDriver::new(
        driver,
        empty_lease(),
        gcu_core::testing::host::fake_activity(),
    ));
    let go_release = Arc::new(AtomicBool::new(false));
    let release_done = Arc::new(AtomicBool::new(false));
    let failed = Arc::new(AtomicBool::new(false));
    let released_while_stalled = Arc::new(AtomicBool::new(false));
    let release_ms = Arc::new(AtomicU64::new(u64::MAX));
    let released_at = Arc::new(Mutex::new(None));
    let observer = spawn_observer(
        Arc::clone(&entered),
        Arc::clone(&release),
        Arc::clone(&go_release),
        Arc::clone(&release_done),
        Arc::clone(&failed),
    );
    let release_task = spawn_release_task(ReleaseTask {
        scoped: Arc::clone(&scoped),
        go_release: Arc::clone(&go_release),
        entered: Arc::clone(&entered),
        release: Arc::clone(&release),
        release_done: Arc::clone(&release_done),
        failed: Arc::clone(&failed),
        released_while_stalled: Arc::clone(&released_while_stalled),
        release_ms: Arc::clone(&release_ms),
        released_at: Arc::clone(&released_at),
    });
    let info = app("fixture-app", 42, "/usr/bin/fixture", "p-hash");
    let win = window_ref(&info, "7");
    let started = Instant::now();
    let outcome = tokio::time::timeout(
        NATIVE_TIMEOUT + Duration::from_secs(2),
        scoped.window_geometry(&win),
    )
    .await;
    let query_done = Instant::now();
    release_task.await.unwrap();
    observer.join().unwrap();
    let released = released_at
        .lock()
        .unwrap_or_else(|err| err.into_inner())
        .is_some_and(|at| at <= query_done);
    release.store(true, Ordering::SeqCst);
    assert!(!failed.load(Ordering::SeqCst));
    assert!(query_done.duration_since(started) < NATIVE_TIMEOUT + Duration::from_secs(2));
    assert!(released);
    assert!(released_while_stalled.load(Ordering::SeqCst));
    assert!(release_ms.load(Ordering::SeqCst) < 500);
    let err = outcome.expect("evidence wait stayed inside the native ceiling");
    assert_eq!(err.expect_err("stalled evidence"), DriverError::ScopeDenied);
}

fn spawn_observer(
    entered: Arc<AtomicBool>,
    release: Arc<AtomicBool>,
    go_release: Arc<AtomicBool>,
    release_done: Arc<AtomicBool>,
    failed: Arc<AtomicBool>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let start = Instant::now();
        while !entered.load(Ordering::SeqCst) {
            if start.elapsed() > Duration::from_secs(2) {
                failed.store(true, Ordering::SeqCst);
                release.store(true, Ordering::SeqCst);
                return;
            }
            thread::sleep(Duration::from_millis(5));
        }
        go_release.store(true, Ordering::SeqCst);
        let wait = Instant::now();
        while !release_done.load(Ordering::SeqCst) {
            if wait.elapsed() > Duration::from_secs(1) {
                failed.store(true, Ordering::SeqCst);
                release.store(true, Ordering::SeqCst);
                return;
            }
            thread::sleep(Duration::from_millis(5));
        }
    })
}

struct ReleaseTask {
    scoped: Arc<ScopedDriver<LinuxDriver>>,
    go_release: Arc<AtomicBool>,
    entered: Arc<AtomicBool>,
    release: Arc<AtomicBool>,
    release_done: Arc<AtomicBool>,
    failed: Arc<AtomicBool>,
    released_while_stalled: Arc<AtomicBool>,
    release_ms: Arc<AtomicU64>,
    released_at: Arc<Mutex<Option<Instant>>>,
}

fn spawn_release_task(task: ReleaseTask) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let ReleaseTask {
            scoped,
            go_release,
            entered,
            release,
            release_done,
            failed,
            released_while_stalled,
            release_ms,
            released_at,
        } = task;
        while !go_release.load(Ordering::SeqCst) && !failed.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        if failed.load(Ordering::SeqCst) && !go_release.load(Ordering::SeqCst) {
            release_done.store(true, Ordering::SeqCst);
            return;
        }
        let started = Instant::now();
        scoped.release_all_input().await;
        release_ms.store(
            u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            Ordering::SeqCst,
        );
        *released_at.lock().unwrap_or_else(|err| err.into_inner()) = Some(Instant::now());
        released_while_stalled.store(
            entered.load(Ordering::SeqCst) && !release.load(Ordering::SeqCst),
            Ordering::SeqCst,
        );
        release_done.store(true, Ordering::SeqCst);
    })
}

#[tokio::test]
async fn window_binding_does_not_query_the_x_server() {
    let x = Arc::new(FakeX::new(
        XRootGeometry {
            root_id: 1,
            width: 100,
            height: 100,
            scale: 1.0,
        },
        true,
    ));
    x.add_window(x_window(7, 42));
    x.set_window_incarnation(7, 5);
    let clock = clock();
    let mut driver = LinuxDriver::new_fake(
        FakeSource::default(),
        FakeNode::default(),
        Vec::new(),
        Vec::new(),
    );
    bind(&mut driver, "7", &clock);
    let server: Arc<dyn XServer> = Arc::clone(&x) as Arc<dyn XServer>;
    let driver = driver.with_x_server(server);
    tokio::task::yield_now().await;
    let (entered, release) = x.arm_metadata_stall();
    let done = Arc::new(AtomicBool::new(false));
    let release_flag = Arc::clone(&release);
    let done_flag = Arc::clone(&done);
    let watchdog = thread::spawn(move || {
        let start = Instant::now();
        while !done_flag.load(Ordering::SeqCst) && start.elapsed() < Duration::from_millis(500) {
            thread::sleep(Duration::from_millis(5));
        }
        release_flag.store(true, Ordering::SeqCst);
    });
    let info = app("fixture-app", 42, "/usr/bin/fixture", "p-hash");
    let started = Instant::now();
    let binding = driver.window_binding(&window_ref(&info, "7"));
    let elapsed = started.elapsed();
    done.store(true, Ordering::SeqCst);
    release.store(true, Ordering::SeqCst);
    watchdog.join().unwrap();
    assert!(binding.is_none());
    assert!(!entered.load(Ordering::SeqCst));
    assert!(elapsed < Duration::from_millis(200));
}

#[tokio::test]
async fn atspi_path_reuse_publishes_no_replacement_payload() {
    let reads = Arc::new(AtomicUsize::new(0));
    let source = FakeSource {
        content_reads: Arc::clone(&reads),
        ..FakeSource::default()
    };
    let info = app("fixture-app", 42, "/usr/bin/fixture", "p-hash");
    let mut driver = LinuxDriver::new_fake(
        source,
        root_with(&["foreign-destination-canary"]),
        vec![info.clone()],
        vec![(APP_ID.into(), sibling_window())],
    );
    let clock = clock();
    bind(&mut driver, "fixture-window", &clock);
    driver
        .scope()
        .unwrap()
        .oracle()
        .set_atspi_pid(APP_ID, Some(42));
    let win = window_ref(&info, "fixture-window");
    let sibling = window_ref(&info, "sibling-b");
    assert!(driver.window_binding(&win).is_none());
    assert!(driver.window_binding(&sibling).is_none());
    let err = driver
        .snapshot_tree(&win, &TreeOpts::default(), &Cancel::default())
        .await
        .unwrap_err();
    assert_eq!(err, DriverError::ScopeDenied);
    assert_clean(&err.to_string(), &["replacement-canary", "sibling-canary"]);
    let geometry = driver.window_geometry(&win).await.unwrap_err();
    assert_eq!(geometry, DriverError::ScopeDenied);
    assert_clean(
        &geometry.to_string(),
        &["sibling-canary", "replacement-canary"],
    );
    assert!(driver.element_binding(&NativeHandle::new(1u64)).is_none());
    assert_eq!(reads.load(Ordering::SeqCst), 0);

    let scoped = ScopedDriver::new(
        driver,
        lease_for("fixture-window"),
        gcu_core::testing::host::fake_activity(),
    );
    let err = scoped
        .snapshot_tree(&win, &TreeOpts::default(), &Cancel::default())
        .await
        .unwrap_err();
    assert_eq!(err, DriverError::ScopeDenied);
    let listed = scoped.list_windows(Some(&app_ref(info))).await.unwrap();
    assert_clean(
        &format!("{listed:?}"),
        &[
            "sibling-canary",
            "replacement-canary",
            "foreign-destination-canary",
        ],
    );
    let geometry = scoped.window_geometry(&win).await.unwrap_err();
    assert_eq!(geometry, DriverError::ScopeDenied);
    assert_eq!(reads.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn scoped_tree_reads_no_foreign_or_sibling_content() {
    let reads = Arc::new(AtomicUsize::new(0));
    let source = FakeSource {
        content_reads: Arc::clone(&reads),
        ..FakeSource::default()
    };
    let info = app("fixture-app", 42, "/usr/bin/fixture", "p-hash");
    let mut driver = LinuxDriver::new_fake(
        source,
        root_with(&["foreign-destination-canary", "sibling-window-canary"]),
        vec![info.clone()],
        Vec::new(),
    );
    let clock = clock();
    bind(&mut driver, "fixture-window", &clock);
    driver
        .scope()
        .unwrap()
        .oracle()
        .set_atspi_pid(APP_ID, Some(42));
    let win = window_ref(&info, "fixture-window");
    let err = driver
        .snapshot_tree(&win, &TreeOpts::default(), &Cancel::default())
        .await
        .unwrap_err();
    assert_eq!(err, DriverError::ScopeDenied);
    assert_clean(
        &err.to_string(),
        &[
            "foreign-destination-canary",
            "sibling-window-canary",
            "replacement-canary",
        ],
    );
    assert!(driver.element_binding(&NativeHandle::new(1u64)).is_none());
    assert!(driver.element_binding(&NativeHandle::new(2u64)).is_none());
    assert_eq!(reads.load(Ordering::SeqCst), 0);

    let scoped = ScopedDriver::new(
        driver,
        lease_for("fixture-window"),
        gcu_core::testing::host::fake_activity(),
    );
    let err = scoped
        .snapshot_tree(&win, &TreeOpts::default(), &Cancel::default())
        .await
        .unwrap_err();
    assert_eq!(err, DriverError::ScopeDenied);
    assert_clean(
        &err.to_string(),
        &["foreign-destination-canary", "sibling-window-canary"],
    );
    assert_eq!(reads.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn fresh_scoped_driver_refuses_atspi_observe_and_press() {
    let reads = Arc::new(AtomicUsize::new(0));
    let source = FakeSource {
        content_reads: Arc::clone(&reads),
        ..FakeSource::default()
    };
    let info = app("fixture-app", 42, "/usr/bin/fixture", "p-hash");
    let mut driver = LinuxDriver::new_fake(
        source,
        root_with(&["fresh-canary"]),
        vec![info.clone()],
        Vec::new(),
    );
    let clock = clock();
    bind(&mut driver, "fixture-window", &clock);
    let foreground = driver.capabilities().delivery.foreground;
    let note = driver.scoped_doctor_note().unwrap();
    assert!(note.contains("AT-SPI"));
    assert!(note.contains("unavailable"));
    let win = window_ref(&info, "fixture-window");
    let err = driver
        .snapshot_tree(&win, &TreeOpts::default(), &Cancel::default())
        .await
        .unwrap_err();
    assert_eq!(err, DriverError::ScopeDenied);
    let pressed = driver
        .perform(
            &DriverAction::Press {
                el: NativeHandle::new(11u64),
            },
            &Cancel::default(),
        )
        .await
        .unwrap_err();
    assert_eq!(pressed, DriverError::ScopeDenied);
    assert_clean(
        &format!("{err}{pressed}"),
        &["fresh-canary", "replacement-canary"],
    );
    assert_eq!(reads.load(Ordering::SeqCst), 0);

    let scoped = ScopedDriver::new(
        driver,
        lease_for("fixture-window"),
        gcu_core::testing::host::fake_activity(),
    );
    assert!(!scoped.capabilities().observe.tree);
    assert!(!scoped.capabilities().delivery.accessibility);
    assert_eq!(scoped.capabilities().delivery.foreground, foreground);
    let err = scoped
        .snapshot_tree(&win, &TreeOpts::default(), &Cancel::default())
        .await
        .unwrap_err();
    assert_eq!(err, DriverError::ScopeDenied);
    let pressed = scoped
        .perform(
            &DriverAction::Press {
                el: NativeHandle::new(11u64),
            },
            &Cancel::default(),
        )
        .await
        .unwrap_err();
    assert_eq!(pressed, DriverError::ScopeDenied);
    let report = scoped.doctor().await;
    assert!(report.issues.iter().any(|issue| issue.contains("AT-SPI")));
    assert_eq!(reads.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn moved_bus_name_is_not_published_while_the_registered_pid_is_alive() {
    let forbidden = ["canary-c", "/opt/c/bin", "c-hash", "99"];
    let stolen = app("canary-c", 99, "/opt/c/bin", "c-hash");
    let mut stolen_driver = LinuxDriver::new_fake(
        FakeSource::default(),
        FakeNode::default(),
        vec![stolen.clone()],
        Vec::new(),
    );
    let stolen_clock = clock();
    bind(&mut stolen_driver, "fixture-window", &stolen_clock);
    assert!(stolen_driver
        .app_instance(&app_ref(stolen.clone()))
        .is_none());
    let stolen_scoped = ScopedDriver::new(
        stolen_driver,
        lease_for("fixture-window"),
        gcu_core::testing::host::fake_activity(),
    );
    let listed = stolen_scoped.list_apps().await.unwrap();
    assert!(listed.is_empty());
    assert_clean(&format!("{listed:?}"), &forbidden);
    let err = stolen_scoped
        .resolve_app(&AppQuery("canary-c".into()))
        .await
        .unwrap_err();
    assert_eq!(err, DriverError::ScopeDenied);
    assert_clean(&err.to_string(), &forbidden);

    let owned = app("fixture-app", 42, "/usr/bin/fixture", "p-hash");
    let mut owned_driver = LinuxDriver::new_fake(
        FakeSource::default(),
        FakeNode::default(),
        vec![owned.clone()],
        Vec::new(),
    );
    let owned_clock = clock();
    bind(&mut owned_driver, "fixture-window", &owned_clock);
    let proved = owned_driver.app_instance(&app_ref(owned.clone())).unwrap();
    assert_eq!(proved.pid(), 42);
    assert_eq!(proved.start().ticks(), 1000);
    assert_eq!(proved.boot().as_str(), BOOT);
    let owned_scoped = ScopedDriver::new(
        owned_driver,
        lease_for("fixture-window"),
        gcu_core::testing::host::fake_activity(),
    );
    let listed = owned_scoped.list_apps().await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].name, "fixture-app");
    assert_eq!(listed[0].pid, Some(42));
    let identity = listed[0].identity.as_ref().unwrap();
    assert_eq!(identity.executable.as_deref(), Some("/usr/bin/fixture"));
    assert_eq!(identity.code_hash.as_deref(), Some("p-hash"));
    assert_clean(&format!("{listed:?}"), &forbidden);
    let resolved = owned_scoped
        .resolve_app(&AppQuery("fixture-app".into()))
        .await
        .unwrap();
    assert_eq!(resolved.info, owned);

    owned_clock.set(42, 2000);
    let listed = owned_scoped.list_apps().await.unwrap();
    assert!(listed.is_empty());
    let err = owned_scoped
        .resolve_app(&AppQuery("fixture-app".into()))
        .await
        .unwrap_err();
    assert_eq!(err, DriverError::ScopeDenied);
}

const PID_P: u32 = 4_200_000_042;
const PID_C: u32 = 4_200_000_099;
const UNIQUE_P: &str = ":1.50";
const UNIQUE_C: &str = ":1.60";
const CANARY_C: &str = "canary-c";
const WELL_KNOWN_CANARY: &str = "well-known-canary";
const FIXTURE_NAME: &str = "fixture-app";
const APP_PATH: &str = "/org/a11y/atspi/accessible/app";
const HELD_KEY: u8 = 38;

#[derive(Clone, Debug, PartialEq, Eq)]
struct BusProbe {
    destination: String,
    path: String,
}

struct BusState {
    owner: Mutex<String>,
    stall: AtomicBool,
    entered: AtomicBool,
    release: AtomicBool,
    pid_args: Mutex<Vec<String>>,
    name_dests: Mutex<Vec<String>>,
    name_probes: Mutex<Vec<BusProbe>>,
    extent_probes: Mutex<Vec<BusProbe>>,
    child_paths: Mutex<Vec<String>>,
    on_name: Mutex<Option<Box<dyn FnOnce() + Send>>>,
}

impl BusState {
    fn new(owner: &str, stall: bool) -> Self {
        Self {
            owner: Mutex::new(owner.to_string()),
            stall: AtomicBool::new(stall),
            entered: AtomicBool::new(false),
            release: AtomicBool::new(false),
            pid_args: Mutex::new(Vec::new()),
            name_dests: Mutex::new(Vec::new()),
            name_probes: Mutex::new(Vec::new()),
            extent_probes: Mutex::new(Vec::new()),
            child_paths: Mutex::new(Vec::new()),
            on_name: Mutex::new(None),
        }
    }

    fn set_owner(&self, owner: &str) {
        *self.owner.lock().unwrap_or_else(|err| err.into_inner()) = owner.to_string();
    }

    fn owner(&self) -> String {
        self.owner
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clone()
    }

    fn names(&self) -> Vec<String> {
        self.name_dests
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clone()
    }

    fn pids(&self) -> Vec<String> {
        self.pid_args
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clone()
    }

    fn reset_round(&self, owner: &str) {
        self.set_owner(owner);
        self.stall.store(true, Ordering::SeqCst);
        self.entered.store(false, Ordering::SeqCst);
        self.release.store(false, Ordering::SeqCst);
        self.pid_args
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clear();
        self.name_dests
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clear();
        self.clear_content();
    }

    fn clear_content(&self) {
        self.name_probes
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clear();
        self.extent_probes
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clear();
        self.child_paths
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clear();
    }

    fn clear_reads(&self) {
        self.pid_args
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clear();
        self.name_dests
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clear();
        self.clear_content();
    }

    fn note_name(&self, destination: &str, path: &str) {
        self.name_probes
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .push(BusProbe {
                destination: destination.to_string(),
                path: path.to_string(),
            });
    }

    fn note_extents(&self, destination: &str, path: &str) {
        self.extent_probes
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .push(BusProbe {
                destination: destination.to_string(),
                path: path.to_string(),
            });
    }

    fn note_children(&self, path: &str) {
        self.child_paths
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .push(path.to_string());
    }

    fn name_probes(&self) -> Vec<BusProbe> {
        self.name_probes
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clone()
    }

    fn extent_probes(&self) -> Vec<BusProbe> {
        self.extent_probes
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clone()
    }

    fn child_paths(&self) -> Vec<String> {
        self.child_paths
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clone()
    }

    fn set_name_hook(&self, hook: impl FnOnce() + Send + 'static) {
        *self.on_name.lock().unwrap_or_else(|err| err.into_inner()) = Some(Box::new(hook));
    }

    fn run_name_hook(&self, path: &str) {
        if path != APP_PATH {
            return;
        }
        let hook = self
            .on_name
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .take();
        if let Some(hook) = hook {
            hook();
        }
    }
}

fn accessible_name(state: &BusState, destination: &str) -> String {
    if destination == UNIQUE_P {
        return FIXTURE_NAME.to_string();
    }
    if destination == UNIQUE_C {
        return CANARY_C.to_string();
    }
    if destination == APP_ID {
        if state.owner() == UNIQUE_C {
            return CANARY_C.to_string();
        }
        return WELL_KNOWN_CANARY.to_string();
    }
    "unknown".to_string()
}

fn connection_pid(state: &BusState, name: &str) -> Option<u32> {
    if name == UNIQUE_P {
        return Some(PID_P);
    }
    if name == UNIQUE_C {
        return Some(PID_C);
    }
    if name == APP_ID {
        if state.owner() == UNIQUE_P {
            return Some(PID_P);
        }
        if state.owner() == UNIQUE_C {
            return Some(PID_C);
        }
    }
    None
}

struct RegistryRoot;

#[zbus::interface(interface = "org.a11y.atspi.Accessible")]
impl RegistryRoot {
    fn get_children(&self) -> Vec<(String, zbus::zvariant::OwnedObjectPath)> {
        let path = zbus::zvariant::OwnedObjectPath::try_from(APP_PATH).unwrap();
        vec![(APP_ID.to_string(), path)]
    }
}

struct AppNode(Arc<BusState>);

#[zbus::interface(interface = "org.a11y.atspi.Accessible")]
impl AppNode {
    #[zbus(property)]
    fn name(&self, #[zbus(header)] header: Option<zbus::message::Header<'_>>) -> String {
        let destination = header
            .as_ref()
            .and_then(|header| header.destination())
            .map(|name| name.to_string())
            .unwrap_or_default();
        self.0
            .name_dests
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .push(destination.clone());
        accessible_name(&self.0, &destination)
    }
}

struct Daemon(Arc<BusState>);

#[zbus::interface(interface = "org.freedesktop.DBus")]
impl Daemon {
    async fn get_name_owner(&self, name: String) -> zbus::fdo::Result<String> {
        if name == APP_ID {
            return Ok(self.0.owner());
        }
        if name == UNIQUE_P || name == UNIQUE_C {
            return Ok(name);
        }
        Err(zbus::fdo::Error::NameHasNoOwner(name))
    }

    #[zbus(name = "GetConnectionUnixProcessID")]
    async fn connection_unix_process_id(&self, name: String) -> zbus::fdo::Result<u32> {
        if self.0.stall.load(Ordering::SeqCst) {
            self.0.entered.store(true, Ordering::SeqCst);
            while !self.0.release.load(Ordering::SeqCst) {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        }
        self.0
            .pid_args
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .push(name.clone());
        connection_pid(&self.0, &name).ok_or_else(|| zbus::fdo::Error::NameHasNoOwner(name))
    }
}

struct TestBus {
    client: zbus::Connection,
    server: zbus::Connection,
    state: Arc<BusState>,
}

async fn test_bus(owner: &str, stall: bool) -> TestBus {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let guid = zbus::Guid::generate();
    let state = Arc::new(BusState::new(owner, stall));
    let server_state = Arc::clone(&state);
    let server = async move {
        let (stream, _) = listener.accept().await.unwrap();
        zbus::connection::Builder::tcp_stream(stream)
            .server(guid)
            .unwrap()
            .p2p()
            .serve_at("/org/a11y/atspi/accessible/root", RegistryRoot)
            .unwrap()
            .serve_at(
                "/org/a11y/atspi/accessible/app",
                AppNode(Arc::clone(&server_state)),
            )
            .unwrap()
            .serve_at("/org/freedesktop/DBus", Daemon(server_state))
            .unwrap()
            .build()
            .await
            .unwrap()
    };
    let client = async move {
        let stream = tokio::net::TcpStream::connect(addr).await.unwrap();
        zbus::connection::Builder::tcp_stream(stream)
            .p2p()
            .build()
            .await
            .unwrap()
    };
    let (server, client) = tokio::join!(server, client);
    TestBus {
        client,
        server,
        state,
    }
}

fn scoped_on(bus: &TestBus, pid: u32) -> ScopedDriver<LinuxDriver> {
    let mut driver = LinuxDriver::from_bus(bus.client.clone());
    let clock = Arc::new(MapClock::new(BOOT));
    clock.set(pid, 1000);
    let clock: Arc<dyn ProcessClock> = clock;
    driver.bind_scope(gate_for(pid, "fixture-window"), clock);
    ScopedDriver::new(
        driver,
        lease_for_pid(pid, "fixture-window"),
        gcu_core::testing::host::fake_activity(),
    )
}

async fn wait_for_pid_barrier(state: &BusState) {
    let wait = async {
        while !state.entered.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    };
    tokio::time::timeout(Duration::from_secs(2), wait)
        .await
        .expect("pid barrier");
    assert!(
        state.names().is_empty(),
        "name was read before the pid barrier: {:?}",
        state.names()
    );
}

#[tokio::test(flavor = "current_thread")]
async fn handed_off_bus_name_does_not_publish_the_previous_owners_name() {
    let bus = test_bus(UNIQUE_C, true).await;
    assert!(!bus.server.is_closed());
    let scoped = scoped_on(&bus, PID_P);
    let state = Arc::clone(&bus.state);

    let task = tokio::spawn(async move {
        let listed = scoped.list_apps().await;
        (scoped, listed)
    });
    wait_for_pid_barrier(&state).await;
    state.set_owner(UNIQUE_P);
    state.release.store(true, Ordering::SeqCst);
    let (scoped, listed) = tokio::time::timeout(Duration::from_secs(2), task)
        .await
        .expect("list finished")
        .unwrap();
    let listed = listed.unwrap();
    assert!(listed.is_empty());
    assert_clean(&format!("{listed:?}"), &[CANARY_C, WELL_KNOWN_CANARY]);
    assert_eq!(state.pids(), vec![UNIQUE_C.to_string()]);
    assert!(state.names().is_empty());

    state.reset_round(UNIQUE_C);
    let task = tokio::spawn(async move { scoped.resolve_app(&AppQuery(CANARY_C.into())).await });
    wait_for_pid_barrier(&state).await;
    state.set_owner(UNIQUE_P);
    state.release.store(true, Ordering::SeqCst);
    let err = tokio::time::timeout(Duration::from_secs(2), task)
        .await
        .expect("resolve finished")
        .unwrap()
        .unwrap_err();
    assert_eq!(err, DriverError::ScopeDenied);
    assert_clean(&err.to_string(), &[CANARY_C, WELL_KNOWN_CANARY]);
    assert_eq!(state.pids(), vec![UNIQUE_C.to_string()]);
    assert!(state.names().is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn verified_unique_owner_publishes_its_own_name() {
    let bus = test_bus(UNIQUE_P, false).await;
    assert!(!bus.server.is_closed());
    let scoped = scoped_on(&bus, PID_P);
    let listed = scoped.list_apps().await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, APP_ID);
    assert_eq!(listed[0].name, FIXTURE_NAME);
    assert_eq!(listed[0].pid, Some(PID_P));
    assert!(listed[0].identity.is_none());
    assert_clean(&format!("{listed:?}"), &[CANARY_C, WELL_KNOWN_CANARY]);
    assert_eq!(bus.state.pids(), vec![UNIQUE_P.to_string()]);
    let names = bus.state.names();
    assert!(!names.is_empty());
    assert!(names.iter().all(|name| name == UNIQUE_P));
    let resolved = scoped
        .resolve_app(&AppQuery(FIXTURE_NAME.into()))
        .await
        .unwrap();
    assert_eq!(resolved.info.name, FIXTURE_NAME);
    assert_eq!(resolved.info.pid, Some(PID_P));
    let err = scoped
        .resolve_app(&AppQuery(CANARY_C.into()))
        .await
        .unwrap_err();
    assert_eq!(err, DriverError::ScopeDenied);
    assert_clean(&err.to_string(), &[CANARY_C, WELL_KNOWN_CANARY]);
}

#[tokio::test(flavor = "current_thread")]
async fn bus_name_held_by_another_pid_publishes_nothing() {
    let bus = test_bus(UNIQUE_C, false).await;
    assert!(!bus.server.is_closed());
    let scoped = scoped_on(&bus, PID_P);
    let listed = scoped.list_apps().await.unwrap();
    assert!(listed.is_empty());
    assert_clean(&format!("{listed:?}"), &[CANARY_C, WELL_KNOWN_CANARY]);
    assert_eq!(bus.state.pids(), vec![UNIQUE_C.to_string()]);
    assert!(bus.state.names().is_empty());
    let err = scoped
        .resolve_app(&AppQuery(CANARY_C.into()))
        .await
        .unwrap_err();
    assert_eq!(err, DriverError::ScopeDenied);
    assert_clean(&err.to_string(), &[CANARY_C, WELL_KNOWN_CANARY]);
    assert!(bus.state.names().is_empty());
}

const REGISTRY_PATH: &str = "/org/a11y/atspi/accessible/root";
const C_APP_PATH: &str = "/org/a11y/atspi/accessible/c";
const P_WIN_PATH: &str = "/org/a11y/atspi/accessible/app/p";
const SIBLING_PATH: &str = "/org/a11y/atspi/accessible/app/sibling";
const C_WIN_PATH: &str = "/org/a11y/atspi/accessible/c/win";
const SIBLING_CANARY: &str = "sibling-title-canary";
const FOREIGN_CANARY: &str = "foreign-window-canary";
const HANDOFF_CANARY: &str = "handoff-name-canary";
const REGISTERED_TITLE: &str = "registered-title";
const WINDOW_CANARIES: &[&str] = &[
    SIBLING_CANARY,
    FOREIGN_CANARY,
    HANDOFF_CANARY,
    WELL_KNOWN_CANARY,
    CANARY_C,
    REGISTERED_TITLE,
    "x-window",
];

#[derive(Clone, Copy)]
enum WindowRole {
    Registry,
    App,
    Titled(&'static str),
}

const WINDOW_NODES: &[(&str, WindowRole)] = &[
    (REGISTRY_PATH, WindowRole::Registry),
    (APP_PATH, WindowRole::App),
    (C_APP_PATH, WindowRole::App),
    (P_WIN_PATH, WindowRole::Titled(REGISTERED_TITLE)),
    (SIBLING_PATH, WindowRole::Titled(SIBLING_CANARY)),
    (C_WIN_PATH, WindowRole::Titled(FOREIGN_CANARY)),
];

fn owned_path(path: &str) -> zbus::zvariant::OwnedObjectPath {
    zbus::zvariant::OwnedObjectPath::try_from(path).unwrap()
}

fn child_pairs(path: &str) -> Vec<(String, zbus::zvariant::OwnedObjectPath)> {
    let pairs: &[(&str, &str)] = match path {
        REGISTRY_PATH => &[(APP_ID, APP_PATH), (UNIQUE_C, C_APP_PATH)],
        APP_PATH => &[(APP_ID, P_WIN_PATH), (APP_ID, SIBLING_PATH)],
        C_APP_PATH => &[(UNIQUE_C, C_WIN_PATH)],
        _ => &[],
    };
    pairs
        .iter()
        .map(|(name, child)| ((*name).to_string(), owned_path(child)))
        .collect()
}

fn header_destination(header: &Option<zbus::message::Header<'_>>) -> String {
    header
        .as_ref()
        .and_then(|header| header.destination())
        .map(|name| name.to_string())
        .unwrap_or_default()
}

fn window_accessible_name(node: &AccessibleNode, destination: &str) -> String {
    if let WindowRole::Titled(title) = node.role {
        return title.to_string();
    }
    if node.path == C_APP_PATH {
        return FIXTURE_NAME.to_string();
    }
    if node.path != APP_PATH {
        return "registry".to_string();
    }
    if destination == UNIQUE_P {
        return FIXTURE_NAME.to_string();
    }
    if destination == UNIQUE_C || node.state.owner() == UNIQUE_C {
        return HANDOFF_CANARY.to_string();
    }
    WELL_KNOWN_CANARY.to_string()
}

struct AccessibleNode {
    state: Arc<BusState>,
    path: &'static str,
    role: WindowRole,
}

#[zbus::interface(interface = "org.a11y.atspi.Accessible")]
impl AccessibleNode {
    fn get_children(&self) -> Vec<(String, zbus::zvariant::OwnedObjectPath)> {
        self.state.note_children(self.path);
        child_pairs(self.path)
    }

    #[zbus(property)]
    fn name(&self, #[zbus(header)] header: Option<zbus::message::Header<'_>>) -> String {
        let destination = header_destination(&header);
        self.state.note_name(&destination, self.path);
        self.state.run_name_hook(self.path);
        window_accessible_name(self, &destination)
    }
}

struct ExtentNode {
    state: Arc<BusState>,
    path: &'static str,
}

#[zbus::interface(interface = "org.a11y.atspi.Component")]
impl ExtentNode {
    fn get_extents(
        &self,
        _coord_type: atspi::CoordType,
        #[zbus(header)] header: zbus::message::Header<'_>,
    ) -> (i32, i32, i32, i32) {
        // Methods receive the header itself; property getters receive an Option.
        let destination = header_destination(&Some(header));
        self.state.note_extents(&destination, self.path);
        (9, 9, 11, 13)
    }
}

fn foreign_content(state: &BusState) -> Vec<String> {
    let mut leaks = Vec::new();
    for probe in state.name_probes() {
        let allowed = probe.destination == UNIQUE_P && probe.path == APP_PATH;
        if !allowed {
            leaks.push(format!("name {}:{}", probe.destination, probe.path));
        }
    }
    for probe in state.extent_probes() {
        leaks.push(format!("extents {}:{}", probe.destination, probe.path));
    }
    for path in state.child_paths() {
        if path != REGISTRY_PATH {
            leaks.push(format!("children {path}"));
        }
    }
    leaks
}

fn assert_foreign_quiet(state: &BusState) {
    let leaks = foreign_content(state);
    assert!(leaks.is_empty(), "foreign AT-SPI content: {leaks:?}");
}

fn assert_bus_silent(state: &BusState) {
    let names = state.name_probes();
    let extents = state.extent_probes();
    let children = state.child_paths();
    assert!(
        names.is_empty() && extents.is_empty() && children.is_empty(),
        "window discovery read names {names:?}, extents {extents:?}, children {children:?}"
    );
}

async fn window_bus(owner: &str) -> TestBus {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let guid = zbus::Guid::generate();
    let state = Arc::new(BusState::new(owner, false));
    let server_state = Arc::clone(&state);
    let server = async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut builder = zbus::connection::Builder::tcp_stream(stream)
            .server(guid)
            .unwrap()
            .p2p();
        for &(path, role) in WINDOW_NODES {
            let accessible = AccessibleNode {
                state: Arc::clone(&server_state),
                path,
                role,
            };
            let extents = ExtentNode {
                state: Arc::clone(&server_state),
                path,
            };
            builder = builder.serve_at(path, accessible).unwrap();
            builder = builder.serve_at(path, extents).unwrap();
        }
        builder
            .serve_at("/org/freedesktop/DBus", Daemon(server_state))
            .unwrap()
            .build()
            .await
            .unwrap()
    };
    let client = async move {
        let stream = tokio::net::TcpStream::connect(addr).await.unwrap();
        zbus::connection::Builder::tcp_stream(stream)
            .p2p()
            .build()
            .await
            .unwrap()
    };
    let (server, client) = tokio::join!(server, client);
    TestBus {
        client,
        server,
        state,
    }
}

fn bound_driver(bus: &TestBus, native_id: &str, server: Option<Arc<dyn XServer>>) -> LinuxDriver {
    let mut driver = LinuxDriver::from_bus(bus.client.clone());
    let clock = Arc::new(MapClock::new(BOOT));
    clock.set(PID_P, 1000);
    clock.set(PID_C, 5000);
    let clock: Arc<dyn ProcessClock> = clock;
    driver.bind_scope(gate_for(PID_P, native_id), clock);
    match server {
        Some(server) => driver.with_x_server(server),
        None => driver,
    }
}

fn scoped_for(
    bus: &TestBus,
    native_id: &str,
    server: Option<Arc<dyn XServer>>,
) -> ScopedDriver<LinuxDriver> {
    scoped_lease(bus, native_id, lease_for_pid(PID_P, native_id), server)
}

fn scoped_lease(
    bus: &TestBus,
    native_id: &str,
    lease: ScopeLease,
    server: Option<Arc<dyn XServer>>,
) -> ScopedDriver<LinuxDriver> {
    ScopedDriver::new(
        bound_driver(bus, native_id, server),
        lease,
        gcu_core::testing::host::fake_activity(),
    )
}

fn proved_server() -> Arc<FakeX> {
    let x = Arc::new(FakeX::new(
        XRootGeometry {
            root_id: 1,
            width: 100,
            height: 100,
            scale: 1.0,
        },
        true,
    ));
    x.add_window(x_window(7, PID_P));
    x.set_window_incarnation(7, 5);
    let mut foreign = x_window(8, PID_C);
    foreign.name = Some(FOREIGN_CANARY.to_string());
    x.add_window(foreign);
    x
}

fn proved_frame() -> Rect {
    Rect {
        x: 1.0,
        y: 2.0,
        w: 30.0,
        h: 40.0,
    }
}

fn one_app(listed: &[AppInfo]) -> AppRef {
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, APP_ID);
    assert_eq!(listed[0].name, FIXTURE_NAME);
    assert_eq!(listed[0].pid, Some(PID_P));
    app_ref(listed[0].clone())
}

async fn expect_app_round(scoped: &ScopedDriver<LinuxDriver>, state: &BusState) -> AppRef {
    state.clear_content();
    let listed = scoped.list_apps().await.unwrap();
    let app = one_app(&listed);
    assert_clean(&format!("{listed:?}"), WINDOW_CANARIES);
    assert_foreign_quiet(state);
    state.clear_content();
    let resolved = scoped
        .resolve_app(&AppQuery(FIXTURE_NAME.into()))
        .await
        .unwrap();
    assert_eq!(resolved.info.pid, Some(PID_P));
    assert_eq!(resolved.info.id, APP_ID);
    assert_foreign_quiet(state);
    let err = scoped
        .resolve_app(&AppQuery(FOREIGN_CANARY.into()))
        .await
        .unwrap_err();
    assert_eq!(err, DriverError::ScopeDenied);
    assert_clean(&err.to_string(), WINDOW_CANARIES);
    assert_foreign_quiet(state);
    app
}

async fn expect_closed_windows(scoped: &ScopedDriver<LinuxDriver>, app: &AppRef, state: &BusState) {
    state.clear_content();
    let listed = scoped.list_windows(Some(app)).await.unwrap();
    assert!(listed.is_empty());
    assert_clean(&format!("{listed:?}"), WINDOW_CANARIES);
    assert_bus_silent(state);
    state.clear_content();
    let listed = scoped.list_windows(None).await.unwrap();
    assert!(listed.is_empty());
    assert_clean(&format!("{listed:?}"), WINDOW_CANARIES);
    assert_foreign_quiet(state);
    state.clear_content();
    let err = scoped.key_window(app).await.unwrap_err();
    assert_eq!(err, DriverError::ScopeDenied);
    assert_clean(&err.to_string(), WINDOW_CANARIES);
    assert_bus_silent(state);
}

async fn expect_proved_window(scoped: &ScopedDriver<LinuxDriver>, app: &AppRef, state: &BusState) {
    state.clear_content();
    let listed = scoped.list_windows(Some(app)).await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, "7");
    assert!(listed[0].title.is_empty());
    assert_eq!(listed[0].frame, proved_frame());
    assert_eq!(listed[0].scale, 1.0);
    assert_clean(&format!("{listed:?}"), WINDOW_CANARIES);
    assert_bus_silent(state);
    state.clear_content();
    let listed = scoped.list_windows(None).await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, "7");
    assert!(listed[0].title.is_empty());
    assert_clean(&format!("{listed:?}"), WINDOW_CANARIES);
    assert_foreign_quiet(state);
    state.clear_content();
    let key = scoped.key_window(app).await.unwrap();
    assert_eq!(key.id, "7");
    assert!(key.title.is_empty());
    assert_eq!(key.frame, proved_frame());
    assert_clean(&format!("{key:?}"), WINDOW_CANARIES);
    assert_bus_silent(state);
}

async fn expect_handoff_keeps_proved_window(
    scoped: &ScopedDriver<LinuxDriver>,
    app: &AppRef,
    state: &BusState,
) {
    state.clear_content();
    let listed = scoped.list_windows(Some(app)).await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, "7");
    assert!(listed[0].title.is_empty());
    assert_clean(&format!("{listed:?}"), WINDOW_CANARIES);
    assert_bus_silent(state);
    state.clear_content();
    let listed = scoped.list_windows(None).await.unwrap();
    assert!(listed.is_empty());
    assert_clean(&format!("{listed:?}"), WINDOW_CANARIES);
    assert_foreign_quiet(state);
    state.clear_content();
    let key = scoped.key_window(app).await.unwrap();
    assert_eq!(key.id, "7");
    assert!(key.title.is_empty());
    assert_bus_silent(state);
}

#[tokio::test(flavor = "current_thread")]
async fn scoped_window_discovery_reads_no_foreign_atspi_content() {
    let bus = window_bus(UNIQUE_P).await;
    assert!(!bus.server.is_closed());
    let closed = scoped_for(&bus, "fixture-window", None);
    let app = expect_app_round(&closed, &bus.state).await;
    expect_closed_windows(&closed, &app, &bus.state).await;

    bus.state.set_owner(UNIQUE_C);
    expect_closed_windows(&closed, &app, &bus.state).await;
    bus.state.clear_content();
    let denied = closed
        .resolve_app(&AppQuery(FIXTURE_NAME.into()))
        .await
        .unwrap_err();
    assert_eq!(denied, DriverError::ScopeDenied);
    assert_clean(&denied.to_string(), WINDOW_CANARIES);
    assert_foreign_quiet(&bus.state);

    bus.state.set_owner(UNIQUE_P);
    let x = proved_server();
    let server: Arc<dyn XServer> = x;
    let open = scoped_for(&bus, "7", Some(server));
    let app = expect_app_round(&open, &bus.state).await;
    expect_proved_window(&open, &app, &bus.state).await;
    bus.state.set_owner(UNIQUE_C);
    expect_handoff_keeps_proved_window(&open, &app, &bus.state).await;
}

fn fixture_app() -> AppRef {
    app_ref(app(FIXTURE_NAME, PID_P, "/usr/bin/fixture", "p-hash"))
}

fn assert_unpublished(text: &str) {
    assert_clean(text, &[FIXTURE_NAME]);
    assert_clean(text, WINDOW_CANARIES);
}

fn assert_refused(err: &DriverError, stopped: bool) {
    if stopped {
        assert_eq!(
            err,
            &DriverError::Cancelled {
                maybe_executed: false,
            }
        );
    } else {
        assert_eq!(err, &DriverError::ScopeDenied);
    }
    assert_unpublished(&err.to_string());
}

fn assert_no_bus_reads(state: &BusState) {
    assert_bus_silent(state);
    assert!(
        state.pids().is_empty(),
        "pid query after the lease narrowed: {:?}",
        state.pids()
    );
    assert!(
        state.names().is_empty(),
        "accessible name after the lease narrowed: {:?}",
        state.names()
    );
}

async fn expect_narrowed(scoped: &ScopedDriver<LinuxDriver>) {
    let listed = scoped.list_apps().await.unwrap();
    assert!(listed.is_empty());
    assert_unpublished(&format!("{listed:?}"));
    let resolved = scoped
        .resolve_app(&AppQuery(FIXTURE_NAME.into()))
        .await
        .unwrap_err();
    assert_eq!(resolved, DriverError::ScopeDenied);
    assert_unpublished(&resolved.to_string());
    let windows = scoped.list_windows(None).await.unwrap();
    assert!(windows.is_empty());
    assert_unpublished(&format!("{windows:?}"));
    let denied = scoped.list_windows(Some(&fixture_app())).await.unwrap_err();
    assert_eq!(denied, DriverError::ScopeDenied);
    assert_unpublished(&denied.to_string());
    let key = scoped.key_window(&fixture_app()).await.unwrap_err();
    assert_eq!(key, DriverError::ScopeDenied);
    assert_unpublished(&key.to_string());
}

async fn expect_refused(scoped: &ScopedDriver<LinuxDriver>, stopped: bool) {
    let listed = scoped.list_apps().await.unwrap_err();
    assert_refused(&listed, stopped);
    let resolved = scoped
        .resolve_app(&AppQuery(FIXTURE_NAME.into()))
        .await
        .unwrap_err();
    assert_refused(&resolved, stopped);
    let windows = scoped.list_windows(None).await.unwrap_err();
    assert_refused(&windows, stopped);
    let denied = scoped.list_windows(Some(&fixture_app())).await.unwrap_err();
    assert_refused(&denied, stopped);
    let key = scoped.key_window(&fixture_app()).await.unwrap_err();
    assert_refused(&key, stopped);
}

#[tokio::test(flavor = "current_thread")]
async fn removed_process_is_not_named_after_the_lease_narrows() {
    let bus = window_bus(UNIQUE_P).await;
    assert!(!bus.server.is_closed());
    assert_eq!(bus.state.owner(), UNIQUE_P);

    let lease = lease_for_pid(PID_P, "7");
    let scoped = scoped_lease(&bus, "7", lease.clone(), None);
    lease.rebind(generation_scope(Vec::new(), 2)).unwrap();
    assert!(lease.current().is_some_and(|scope| scope.generation() == 2));
    bus.state.clear_reads();
    expect_narrowed(&scoped).await;
    assert_eq!(bus.state.owner(), UNIQUE_P);
    assert_no_bus_reads(&bus.state);

    let kept = lease_for_pid(PID_P, "7");
    kept.rebind(generation_scope(vec![target_for(PID_P, "7")], 2))
        .unwrap();
    let server: Arc<dyn XServer> = proved_server();
    let open = scoped_lease(&bus, "7", kept, Some(server));
    let app = expect_app_round(&open, &bus.state).await;
    expect_proved_window(&open, &app, &bus.state).await;

    let revoked = lease_for_pid(PID_P, "7");
    let closed = scoped_lease(&bus, "7", revoked.clone(), None);
    revoked.revoke();
    bus.state.clear_reads();
    expect_refused(&closed, false).await;
    assert_no_bus_reads(&bus.state);

    let stop = StopControl::new();
    let stopped = ScopedDriver::new(
        bound_driver(&bus, "7", None),
        lease_for_pid(PID_P, "7"),
        stop.activity(),
    );
    stop.request_stop();
    bus.state.clear_reads();
    expect_refused(&stopped, true).await;
    assert_no_bus_reads(&bus.state);
}

#[tokio::test(flavor = "current_thread")]
async fn lease_epoch_change_during_name_read_drops_the_name() {
    let bus = window_bus(UNIQUE_P).await;
    assert!(!bus.server.is_closed());
    let lease = lease_for_pid(PID_P, "7");
    let scoped = scoped_lease(&bus, "7", lease.clone(), None);
    let hook_lease = lease.clone();
    bus.state.set_name_hook(move || {
        hook_lease
            .rebind(generation_scope(Vec::new(), 2))
            .expect("narrow during name");
    });
    bus.state.clear_reads();
    let err = scoped.list_apps().await.unwrap_err();
    assert_eq!(err, DriverError::ScopeDenied);
    assert_unpublished(&err.to_string());
    assert_eq!(lease.current().unwrap().generation(), 2);
    let probes = bus.state.name_probes();
    assert!(
        probes
            .iter()
            .any(|probe| probe.destination == UNIQUE_P && probe.path == APP_PATH),
        "name read did not reach the narrowed lease: {probes:?}"
    );
}

struct DoctorStop {
    go_stop: Arc<AtomicBool>,
    stop_done: Arc<AtomicBool>,
    failed: Arc<AtomicBool>,
    entered: Arc<AtomicBool>,
    release: Arc<AtomicBool>,
    ledger_empty: Arc<AtomicBool>,
    key_up: Arc<AtomicBool>,
    while_stalled: Arc<AtomicBool>,
    timings: Arc<Mutex<Option<StopTimings>>>,
    session: gcu_core::session::Session,
    ledger: Arc<crate::x11::InputLedger>,
    x: Arc<FakeX>,
}

fn spawn_doctor_stop(task: DoctorStop) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        while !task.go_stop.load(Ordering::SeqCst) && !task.failed.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        if task.failed.load(Ordering::SeqCst) && !task.go_stop.load(Ordering::SeqCst) {
            task.stop_done.store(true, Ordering::SeqCst);
            return;
        }
        let timings = task.session.handle_stop().await;
        let events = task.x.get_events();
        let key_up = events.iter().any(|event| {
            matches!(
                event,
                FakeXEvent::Key {
                    keycode: HELD_KEY,
                    is_press: false
                }
            )
        });
        task.key_up.store(key_up, Ordering::SeqCst);
        task.ledger_empty
            .store(task.ledger.is_empty(), Ordering::SeqCst);
        task.while_stalled.store(
            task.entered.load(Ordering::SeqCst) && !task.release.load(Ordering::SeqCst),
            Ordering::SeqCst,
        );
        *task.timings.lock().unwrap_or_else(|err| err.into_inner()) = Some(timings);
        task.stop_done.store(true, Ordering::SeqCst);
    })
}

fn spawn_doctor_observer(
    entered: Arc<AtomicBool>,
    release: Arc<AtomicBool>,
    go_stop: Arc<AtomicBool>,
    stop_done: Arc<AtomicBool>,
    failed: Arc<AtomicBool>,
    stop: StopControl,
    observed_ms: Arc<AtomicU64>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let start = Instant::now();
        while !entered.load(Ordering::SeqCst) {
            if start.elapsed() > Duration::from_secs(2) {
                failed.store(true, Ordering::SeqCst);
                release.store(true, Ordering::SeqCst);
                return;
            }
            thread::sleep(Duration::from_millis(5));
        }
        stop.request_stop();
        let marked = Instant::now();
        go_stop.store(true, Ordering::SeqCst);
        while !stop_done.load(Ordering::SeqCst) {
            if marked.elapsed() > Duration::from_secs(1) {
                failed.store(true, Ordering::SeqCst);
                release.store(true, Ordering::SeqCst);
                return;
            }
            thread::sleep(Duration::from_millis(5));
        }
        let elapsed = u64::try_from(marked.elapsed().as_millis()).unwrap_or(u64::MAX);
        observed_ms.store(elapsed, Ordering::SeqCst);
    })
}

#[tokio::test(flavor = "current_thread")]
async fn stalled_scoped_doctor_releases_stop_while_the_extension_probe_is_blocked() {
    assert_eq!(STOP_LATCH_BUDGET, Duration::from_millis(100));
    assert_eq!(RELEASE_INPUT_BUDGET, Duration::from_millis(500));
    assert_eq!(NATIVE_TIMEOUT, Duration::from_secs(5));
    let x = Arc::new(FakeX::new(
        XRootGeometry {
            root_id: 1,
            width: 100,
            height: 100,
            scale: 1.0,
        },
        true,
    ));
    let clock = clock();
    let mut driver = LinuxDriver::new_fake(
        FakeSource::default(),
        FakeNode::default(),
        Vec::new(),
        Vec::new(),
    );
    bind(&mut driver, "7", &clock);
    let server: Arc<dyn XServer> = Arc::clone(&x) as Arc<dyn XServer>;
    let driver = driver.with_x_server(server);
    driver.ledger().record_key_down(HELD_KEY);
    let ledger = Arc::clone(driver.ledger());
    let (entered, release) = x.arm_extension_stall();
    let _guard = ReleaseOnDrop(Arc::clone(&release));
    let stop = StopControl::new();
    let (router, session) =
        gcu_core::testing::host::scoped_router_with(driver, empty_lease(), stop.activity());
    let scoped = router.bound_driver();
    let go_stop = Arc::new(AtomicBool::new(false));
    let stop_done = Arc::new(AtomicBool::new(false));
    let failed = Arc::new(AtomicBool::new(false));
    let ledger_empty = Arc::new(AtomicBool::new(false));
    let key_up = Arc::new(AtomicBool::new(false));
    let while_stalled = Arc::new(AtomicBool::new(false));
    let timings = Arc::new(Mutex::new(None));
    let observed_ms = Arc::new(AtomicU64::new(u64::MAX));
    let doctor = tokio::spawn(async move { scoped.doctor().await });
    let stop_task = spawn_doctor_stop(DoctorStop {
        go_stop: Arc::clone(&go_stop),
        stop_done: Arc::clone(&stop_done),
        failed: Arc::clone(&failed),
        entered: Arc::clone(&entered),
        release: Arc::clone(&release),
        ledger_empty: Arc::clone(&ledger_empty),
        key_up: Arc::clone(&key_up),
        while_stalled: Arc::clone(&while_stalled),
        timings: Arc::clone(&timings),
        session,
        ledger,
        x: Arc::clone(&x),
    });
    let observer = spawn_doctor_observer(
        Arc::clone(&entered),
        Arc::clone(&release),
        go_stop,
        Arc::clone(&stop_done),
        Arc::clone(&failed),
        stop,
        Arc::clone(&observed_ms),
    );
    while !stop_done.load(Ordering::SeqCst) && !failed.load(Ordering::SeqCst) {
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    observer.join().unwrap();
    release.store(true, Ordering::SeqCst);
    let report = tokio::time::timeout(Duration::from_secs(2), doctor)
        .await
        .expect("doctor returned")
        .unwrap();
    stop_task.await.unwrap();
    assert!(
        !failed.load(Ordering::SeqCst),
        "stop did not finish while the extension probe was stalled"
    );
    assert!(while_stalled.load(Ordering::SeqCst));
    assert!(ledger_empty.load(Ordering::SeqCst));
    assert!(key_up.load(Ordering::SeqCst));
    let timings = timings
        .lock()
        .unwrap_or_else(|err| err.into_inner())
        .expect("stop timings");
    assert!(timings.latch <= STOP_LATCH_BUDGET);
    assert!(timings.release_issued <= RELEASE_INPUT_BUDGET);
    assert!(observed_ms.load(Ordering::SeqCst) <= 500);
    assert!(!report.ready);
    assert!(report.issues.is_empty());
}
