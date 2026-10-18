//! Actual subprocess stalls, parent ups and persistent Host-latch boundaries.
use super::*;
use gcu_driver_windows::{worker::WorkerConfig, WindowsDriver};
use std::{path::PathBuf, sync::atomic::AtomicU64};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let dir = std::env::current_dir()
            .unwrap()
            .join("target")
            .join(format!(
                "gs-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
    fn config(&self, fault: &str) -> WorkerConfig {
        let mut config = WorkerConfig::new(env!("CARGO_BIN_EXE_gcu-test-worker"));
        config.environment = vec![
            ("GCU_TEST_DIR".into(), self.0.as_os_str().to_owned()),
            ("GCU_TEST_FAULT".into(), fault.into()),
        ];
        config
    }
    async fn held(&self) {
        tokio::time::timeout(Duration::from_secs(3), async {
            while !self.0.join("held").exists() {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
    }
    async fn reaped(&self) {
        #[cfg(target_os = "linux")]
        tokio::time::timeout(Duration::from_secs(5), async {
            let pids = std::fs::read_to_string(self.0.join("pids")).unwrap();
            while pids
                .lines()
                .any(|pid| PathBuf::from(format!("/proc/{pid}")).exists())
            {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
async fn window(driver: &WindowsDriver) -> WindowRef {
    let app = driver
        .resolve_app(&AppQuery("Editor".into()))
        .await
        .unwrap();
    WindowRef {
        id: driver.key_window(&app).await.unwrap().id,
        app,
    }
}
fn drag(win: &WindowRef) -> DriverAction {
    DriverAction::PointerDrag {
        win: win.clone(),
        path: vec![
            WindowPoint { x: 10., y: 20. },
            WindowPoint { x: 70., y: 80. },
        ],
        route: PointerRoute::AllowForeground,
    }
}

#[tokio::test]
async fn worker_action_proof_missing_live_window_returns_window_gone_without_probe_or_mutation() {
    let f = Fixture::new();
    let release = Arc::new(FakeInput::default());
    let api = FakeSafetyApi::default();
    let guard = api.start(Default::default(), Arc::new(Latch::default()));
    let driver = WindowsDriver::isolated_with_releaser(f.config("input_normal"), release.clone())
        .await
        .unwrap()
        .with_safety(guard.clone());
    let mut win = window(&driver).await;
    win.id.push_str("-gone");
    api.remove_hooks();
    guard.activity_clock().invalidate();
    assert_eq!(
        driver.perform(&drag(&win), &Cancel::default()).await,
        Err(DriverError::WindowGone)
    );
    assert_eq!(api.proof_requests(), 0);
    assert!(!f.0.join("calls").exists(), "no mutation RPC before proof");
    assert!(release.state().sent.is_empty());
    assert!(!guard.is_stopped());
}

#[tokio::test]
async fn hotkey_during_native_ffi_stall_latches_releases_and_kills_within_budgets() {
    let f = Fixture::new();
    let release = Arc::new(FakeInput::default());
    let api = Arc::new(FakeSafetyApi::default());
    let latch = Arc::new(Latch::default());
    let guard = api.start(HotkeyConfig::default(), latch.clone());
    let driver = Arc::new(
        WindowsDriver::isolated_with_releaser(f.config("input_hang"), release.clone())
            .await
            .unwrap()
            .with_safety(guard.clone()),
    );
    let win = window(&driver).await;
    let task = {
        let driver = driver.clone();
        let win = win.clone();
        tokio::spawn(async move { driver.perform(&drag(&win), &Cancel::default()).await })
    };
    f.held().await;
    let start = Instant::now();
    api.trigger();
    tokio::time::timeout(Duration::from_millis(100), async {
        while !guard.is_stopped() {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .unwrap();
    assert!(latch.is_stopped());
    assert!(start.elapsed() <= Duration::from_millis(100));
    tokio::time::timeout(Duration::from_millis(500), async {
        while release.state().sent.is_empty() {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .unwrap();
    assert!(start.elapsed() <= Duration::from_millis(500));
    assert!(matches!(
        release.state().sent[0].event,
        gcu_driver_windows::input::api::Event::Button(Button::Left, false, ..)
    ));
    assert_eq!(
        task.await.unwrap(),
        Err(DriverError::Cancelled {
            maybe_executed: true
        })
    );
    f.reaped().await;
    assert!(start.elapsed() <= Duration::from_secs(5));
    assert_eq!(
        driver.perform(&drag(&win), &Cancel::default()).await,
        Err(DriverError::Cancelled {
            maybe_executed: false
        })
    );
    let new_guard = FakeSafetyApi::default().start(Default::default(), latch.clone());
    let restarted = WindowsDriver::isolated_with_releaser(f.config("input_normal"), release)
        .await
        .unwrap()
        .with_safety(new_guard);
    assert_eq!(
        restarted.perform(&drag(&win), &Cancel::default()).await,
        Err(DriverError::Cancelled {
            maybe_executed: false
        })
    );
    assert_eq!(
        restarted.capabilities().delivery.foreground,
        ForegroundCapability::Policy
    );
    // Only this simulated trusted Host event restores delivery. Driver has no clear.
    latch.0.store(false, Ordering::SeqCst);
    assert_eq!(
        restarted.capabilities().delivery.foreground,
        ForegroundCapability::Policy
    );
    restarted.release_all_input().await;
}

#[tokio::test]
async fn hook_ffi_hang_watchdog_stops_worker_without_waiting_for_pump() {
    let f = Fixture::new();
    let release = Arc::new(FakeInput::default());
    let api = Arc::new(FakeSafetyApi::default());
    let guard = api.start(Default::default(), Arc::new(Latch::default()));
    let driver = Arc::new(
        WindowsDriver::isolated_with_releaser(f.config("input_hang"), release.clone())
            .await
            .unwrap()
            .with_safety(guard.clone()),
    );
    let win = window(&driver).await;
    let task = {
        let driver = driver.clone();
        tokio::spawn(async move { driver.perform(&drag(&win), &Cancel::default()).await })
    };
    f.held().await;
    let start = Instant::now();
    api.stall_poll();
    api.poll();
    api.advance(HEARTBEAT_TIMEOUT + Duration::from_millis(1));
    tokio::time::timeout(Duration::from_millis(300), async {
        while !guard.is_stopped() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(guard.activity_clock().ms_since_physical_input(), None);
    tokio::time::timeout(Duration::from_millis(500), async {
        while release.state().sent.is_empty() {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .unwrap();
    assert!(start.elapsed() <= Duration::from_millis(500));
    assert!(task.await.unwrap().unwrap_err().maybe_executed());
    f.reaped().await;
    api.unstall();
    driver.release_all_input().await;
}

#[tokio::test]
async fn hotkey_mid_chord_releases_parent_keys_and_discards_queued_action() {
    let f = Fixture::new();
    let release = Arc::new(FakeInput::default());
    let api = Arc::new(FakeSafetyApi::default());
    let guard = api.start(Default::default(), Arc::new(Latch::default()));
    let driver = Arc::new(
        WindowsDriver::isolated_with_releaser(f.config("input_key_hang"), release.clone())
            .await
            .unwrap()
            .with_safety(guard.clone()),
    );
    let win = window(&driver).await;
    let task = {
        let driver = driver.clone();
        let win = win.clone();
        tokio::spawn(async move {
            driver
                .perform(
                    &DriverAction::PressKeys {
                        win,
                        chord: KeyChord::parse("ctrl+a").unwrap(),
                        route: KeyRoute::AllowForeground,
                    },
                    &Cancel::default(),
                )
                .await
        })
    };
    f.held().await;
    let queued = {
        let driver = driver.clone();
        tokio::spawn(async move { driver.perform(&drag(&win), &Cancel::default()).await })
    };
    tokio::task::yield_now().await;
    let start = Instant::now();
    api.trigger();
    tokio::time::timeout(Duration::from_millis(100), async {
        while !guard.is_stopped() {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .unwrap();
    assert!(start.elapsed() <= Duration::from_millis(100));
    tokio::time::timeout(Duration::from_millis(500), async {
        while release.state().sent.len() != 2 {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .unwrap();
    assert!(start.elapsed() <= Duration::from_millis(500));
    let events: Vec<_> = release.state().sent.iter().map(|s| s.event).collect();
    use gcu_driver_windows::input::api::Event;
    assert_eq!(
        events,
        [
            Event::Key(65, false, false, false),
            Event::Key(17, false, false, false)
        ]
    );
    assert_eq!(
        task.await.unwrap(),
        Err(DriverError::Cancelled {
            maybe_executed: true
        })
    );
    assert_eq!(
        queued.await.unwrap(),
        Err(DriverError::Cancelled {
            maybe_executed: false
        })
    );
    assert_eq!(
        std::fs::read_to_string(f.0.join("calls"))
            .unwrap()
            .lines()
            .count(),
        1
    );
    f.reaped().await;
    assert!(start.elapsed() <= Duration::from_secs(5));
    driver.release_all_input().await;
}
