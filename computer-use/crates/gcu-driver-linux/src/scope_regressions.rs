//! Production regressions for the S10 scope fixes.
//!
//! Each test calls a driver or `ScopedDriver` method. Helpers only build fixtures.

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
use gcu_protocol::{types::AppIdentity, AppInfo, Rect, WindowInfo};

use crate::atspi::{FakeNode, FakeSource};
use crate::driver::LinuxDriver;
use crate::identity::scope::{MapClock, ProcessClock, ScopeGate, ScopeInstance, ScopeWindow};
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
    let owner = process(42, 1000);
    let window = WindowBinding::new(native_id, 5, owner.clone()).unwrap();
    let target = FixtureTarget::new(
        owner,
        "fixture",
        ExecutableProof::new("/usr/bin/fixture").unwrap(),
        vec![RegisteredWindow::new(window)],
    )
    .unwrap();
    ScopeLease::install(
        FixtureScope::try_new(
            "run-s10",
            1,
            "engine-s10",
            "desktop-s10",
            "lease-s10",
            vec![target],
        )
        .unwrap(),
    )
}

fn empty_lease() -> ScopeLease {
    ScopeLease::install(
        FixtureScope::try_new(
            "run-s10",
            1,
            "engine-s10",
            "desktop-s10",
            "lease-s10",
            vec![],
        )
        .unwrap(),
    )
}

fn gate(native_id: &str) -> ScopeGate {
    let owner = ScopeInstance::try_new(42, 1000, BOOT).unwrap();
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
