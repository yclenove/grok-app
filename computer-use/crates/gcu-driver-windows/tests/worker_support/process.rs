use gcu_core::driver::{Cancel, Driver, DriverAction, DriverError};
use gcu_driver_windows::{
    worker::{Action, Request, Supervisor, Window, WorkerConfig},
    WindowsDriver,
};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture {
    dir: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let dir = std::env::current_dir()
            .unwrap()
            .join("target")
            .join(format!(
                "gw-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
        std::fs::create_dir_all(&dir).unwrap();
        Self { dir }
    }
    fn config(&self, fault: &str) -> WorkerConfig {
        let mut config = WorkerConfig::new(env!("CARGO_BIN_EXE_gcu-test-worker"));
        config.environment = vec![
            ("GCU_TEST_DIR".into(), self.dir.as_os_str().to_owned()),
            ("GCU_TEST_FAULT".into(), fault.into()),
        ];
        config.timeout = Duration::from_secs(2);
        config
    }
    fn pids(&self) -> Vec<u32> {
        std::fs::read_to_string(self.dir.join("pids"))
            .unwrap_or_default()
            .lines()
            .map(|s| s.parse().unwrap())
            .collect()
    }
    fn calls(&self) -> Vec<String> {
        std::fs::read_to_string(self.dir.join("calls"))
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}
fn mutation() -> Request {
    let app =
        serde_json::from_value(serde_json::json!({"id":"fixture", "name":"fixture"})).unwrap();
    Request::Perform(Action::Activate {
        win: Window {
            app,
            id: "fixture".into(),
        },
    })
}
async fn dispatched(f: &Fixture) {
    tokio::time::timeout(Duration::from_secs(2), async {
        while f.calls().is_empty() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
}
#[cfg(target_os = "linux")]
async fn reaped(pids: &[u32]) {
    tokio::time::timeout(Duration::from_secs(2), async {
        while pids
            .iter()
            .any(|pid| std::path::Path::new(&format!("/proc/{pid}")).exists())
        {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
}
#[tokio::test]
async fn crash_read_only_retries_once_and_recovers() {
    let f = Fixture::new();
    let s = Supervisor::start(f.config("crash_once")).await.unwrap();
    assert!(s
        .execute(Request::ListApps, &Cancel::default())
        .await
        .is_ok());
    assert_eq!(f.calls(), ["read", "read"]);
    assert_eq!(f.pids().len(), 2);
    s.shutdown().await.unwrap();
}
#[tokio::test]
async fn repeated_crash_read_only_stops_after_one_retry() {
    let f = Fixture::new();
    let s = Supervisor::start(f.config("crash_always")).await.unwrap();
    assert!(matches!(
        s.execute(Request::ListApps, &Cancel::default()).await,
        Err(DriverError::Crashed {
            maybe_executed: false
        })
    ));
    assert_eq!(f.calls(), ["read", "read"]);
    s.shutdown().await.unwrap();
}
#[tokio::test]
async fn crash_mutation_reports_unknown_and_never_replays() {
    let f = Fixture::new();
    let s = Supervisor::start(f.config("crash_once")).await.unwrap();
    assert!(matches!(
        s.execute(mutation(), &Cancel::default()).await,
        Err(DriverError::Crashed {
            maybe_executed: true
        })
    ));
    assert_eq!(f.calls(), ["mutation"]);
    assert!(s
        .execute(Request::ListApps, &Cancel::default())
        .await
        .is_ok());
    assert_eq!(f.calls(), ["mutation", "read"]);
    s.shutdown().await.unwrap();
}
#[tokio::test]
async fn timeout_kills_worker_and_read_only_retry_succeeds() {
    let f = Fixture::new();
    let mut config = f.config("hang_once");
    config.timeout = Duration::from_millis(300);
    let s = Supervisor::start(config).await.unwrap();
    assert!(s
        .execute(Request::ListApps, &Cancel::default())
        .await
        .is_ok());
    assert_eq!(f.pids().len(), 2);
    s.shutdown().await.unwrap();
    #[cfg(target_os = "linux")]
    reaped(&f.pids()).await;
}
#[tokio::test]
async fn timeout_mutation_reports_unknown_without_retry() {
    let f = Fixture::new();
    let mut config = f.config("hang_once");
    config.timeout = Duration::from_millis(300);
    let s = Supervisor::start(config).await.unwrap();
    assert!(matches!(
        s.execute(mutation(), &Cancel::default()).await,
        Err(DriverError::Timeout {
            maybe_executed: true
        })
    ));
    assert_eq!(f.calls(), ["mutation"]);
    s.shutdown().await.unwrap();
}

#[tokio::test]
async fn blocked_mutation_injected_deadline_reaps_worker_and_next_read_recovers() {
    let f = Fixture::new();
    let mut config = f.config("hang_once");
    config.timeout = Duration::from_secs(10);
    // Start before pausing time: child cold-start speed is not the assertion.
    let supervisor = std::sync::Arc::new(Supervisor::start(config).await.unwrap());
    let task = {
        let supervisor = supervisor.clone();
        tokio::spawn(async move { supervisor.execute(mutation(), &Cancel::default()).await })
    };
    dispatched(&f).await; // External trace proves the worker is inside the call.
    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(10)).await;
    tokio::time::resume();
    assert!(matches!(
        task.await.unwrap(),
        Err(DriverError::Timeout {
            maybe_executed: true
        })
    ));
    // This is a deadlock watchdog, not an assertion about process cold-start.
    assert!(tokio::time::timeout(
        Duration::from_secs(10),
        supervisor.execute(Request::ListApps, &Cancel::default())
    )
    .await
    .unwrap()
    .is_ok());
    assert_eq!(f.calls(), ["mutation", "read"]);
    assert_eq!(f.pids().len(), 2);
    supervisor.shutdown().await.unwrap();
    #[cfg(target_os = "linux")]
    reaped(&f.pids()).await;
}
#[tokio::test]
async fn malformed_truncated_oversized_uncorrelated_frames_restart_read_worker() {
    for fault in [
        "bad_once",
        "truncated_once",
        "overlong_once",
        "wrong_id_once",
    ] {
        let f = Fixture::new();
        let s = Supervisor::start(f.config(fault)).await.unwrap();
        assert!(
            s.execute(Request::ListApps, &Cancel::default())
                .await
                .is_ok(),
            "{fault}"
        );
        assert_eq!(f.pids().len(), 2, "{fault}");
        s.shutdown().await.unwrap();
    }
}
#[tokio::test]
async fn bad_frame_mutation_reports_unknown_without_retry() {
    let f = Fixture::new();
    let s = Supervisor::start(f.config("bad_once")).await.unwrap();
    assert!(matches!(
        s.execute(mutation(), &Cancel::default()).await,
        Err(DriverError::Crashed {
            maybe_executed: true
        })
    ));
    assert_eq!(f.calls(), ["mutation"]);
    s.shutdown().await.unwrap();
}
#[tokio::test]
async fn worker_exits_before_handshake_returns_startup_error() {
    let f = Fixture::new();
    assert!(Supervisor::start(f.config("fail_startup")).await.is_err());
    #[cfg(target_os = "linux")]
    reaped(&f.pids()).await;
}
#[tokio::test]
async fn stop_bypasses_blocked_worker_and_discards_queued_calls() {
    let f = Fixture::new();
    let s = std::sync::Arc::new(Supervisor::start(f.config("hang_once")).await.unwrap());
    let active = {
        let s = s.clone();
        tokio::spawn(async move { s.execute(mutation(), &Cancel::default()).await })
    };
    dispatched(&f).await;
    let queued_cancel = Cancel::default();
    let queued = s.execute(mutation(), &queued_cancel);
    tokio::pin!(queued);
    // Poll once to enqueue while the native worker is deliberately wedged.
    tokio::select! { biased; _ = &mut queued => panic!("queued action completed"), _ = tokio::time::sleep(Duration::from_millis(10)) => () }
    let started = std::time::Instant::now();
    s.stop_handle().stop();
    assert!(matches!(
        active.await.unwrap(),
        Err(DriverError::Cancelled {
            maybe_executed: true
        })
    ));
    assert!(matches!(
        queued.await,
        Err(DriverError::Cancelled {
            maybe_executed: false
        })
    ));
    assert!(started.elapsed() < Duration::from_millis(500));
    assert_eq!(f.calls(), ["mutation"]);
    s.shutdown().await.unwrap();
}
#[tokio::test]
async fn stop_during_restart_handshake_remains_independent() {
    let f = Fixture::new();
    let s = std::sync::Arc::new(Supervisor::start(f.config("hang_restart")).await.unwrap());
    let read = {
        let s = s.clone();
        tokio::spawn(async move { s.execute(Request::ListApps, &Cancel::default()).await })
    };
    tokio::time::timeout(Duration::from_secs(2), async {
        while f.pids().len() < 2 {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    let started = std::time::Instant::now();
    s.stop_handle().stop();
    assert!(matches!(
        read.await.unwrap(),
        Err(DriverError::Cancelled {
            maybe_executed: false
        })
    ));
    assert!(started.elapsed() < Duration::from_millis(500));
    s.shutdown().await.unwrap();
}
#[tokio::test]
async fn cancellation_and_abandoned_future_kill_inflight_worker() {
    for abandon in [false, true] {
        let f = Fixture::new();
        let s = std::sync::Arc::new(Supervisor::start(f.config("hang_once")).await.unwrap());
        let cancel = Cancel::default();
        let task_cancel = cancel.clone();
        let call = {
            let s = s.clone();
            tokio::spawn(async move { s.execute(mutation(), &task_cancel).await })
        };
        dispatched(&f).await;
        if abandon {
            call.abort();
            let _ = call.await;
        } else {
            cancel.cancel();
            assert!(matches!(
                call.await.unwrap(),
                Err(DriverError::Cancelled {
                    maybe_executed: true
                })
            ));
        }
        #[cfg(target_os = "linux")]
        reaped(&f.pids()).await;
        s.shutdown().await.unwrap();
    }
}
#[tokio::test]
async fn supervisor_shutdown_and_drop_kill_child() {
    for explicit in [false, true] {
        let f = Fixture::new();
        let s = Supervisor::start(f.config("none")).await.unwrap();
        #[cfg(target_os = "linux")]
        let pids = f.pids();
        if explicit {
            s.shutdown().await.unwrap();
        }
        drop(s);
        #[cfg(target_os = "linux")]
        reaped(&pids).await;
    }
}
#[tokio::test]
async fn tree_focus_and_action_round_trip_preserve_exact_runtime_identity() {
    let f = Fixture::new();
    let driver = WindowsDriver::isolated(f.config("normal")).await.unwrap();
    assert!(!driver.capabilities().observe.events);
    let app = driver
        .resolve_app(&gcu_core::driver::AppQuery("Editor".into()))
        .await
        .unwrap();
    let win = gcu_core::driver::WindowRef {
        app: app.clone(),
        id: driver.key_window(&app).await.unwrap().id,
    };
    let tree = driver
        .snapshot_tree(&win, &Default::default(), &Cancel::default())
        .await
        .unwrap();
    let root = tree.root.unwrap();
    assert_eq!(root.name.as_deref(), Some("worker fixture"));
    assert_eq!(root.children[0].name.as_deref(), Some("child"));
    let element = root
        .handle
        .downcast_ref::<gcu_driver_windows::worker::Element>()
        .unwrap();
    assert_eq!(element.hwnd, 8);
    assert!(!element.runtime_id.is_empty());
    assert_eq!(element.window.id, win.id);
    assert!(matches!(
        driver
            .perform(
                &DriverAction::Press {
                    el: root.handle.clone()
                },
                &Cancel::default()
            )
            .await,
        Ok(gcu_core::driver::Delivery::Accessibility)
    ));
    let capture = driver
        .capture(&win, &Default::default(), &Cancel::default())
        .await
        .unwrap();
    assert_eq!(capture.mime, "image/png");
    assert!(capture.bytes.starts_with(b"\x89PNG"));
    let focused = driver.focused_element(&app).await.unwrap().unwrap();
    assert!(focused
        .downcast_ref::<gcu_driver_windows::worker::Element>()
        .is_some());
    assert!(matches!(
        driver
            .perform(
                &DriverAction::Press {
                    el: root.handle.clone()
                },
                &Cancel::default()
            )
            .await,
        Ok(gcu_core::driver::Delivery::Accessibility)
    ));
    // Successful delivery proves the old remote reference reached the latest
    // native slot after unchanged tree/focus reads.
    let mut changed = root
        .handle
        .downcast_ref::<gcu_driver_windows::worker::Element>()
        .unwrap()
        .clone();
    changed.fingerprint ^= 1;
    assert!(matches!(
        driver
            .perform(
                &DriverAction::Press {
                    el: gcu_core::model::NativeHandle::new(changed)
                },
                &Cancel::default()
            )
            .await,
        Err(DriverError::StaleElement)
    ));
}
#[tokio::test]
#[ignore = "nightly: 300 observations; native moving-window variant needs Windows desktop"]
async fn observe_repeated_300_times_keeps_supervisor_alive() {
    let f = Fixture::new();
    let driver = WindowsDriver::isolated(f.config("normal")).await.unwrap();
    let app = driver
        .resolve_app(&gcu_core::driver::AppQuery("Editor".into()))
        .await
        .unwrap();
    let win = gcu_core::driver::WindowRef {
        app: app.clone(),
        id: driver.key_window(&app).await.unwrap().id,
    };
    for _ in 0..300 {
        assert!(driver
            .snapshot_tree(&win, &Default::default(), &Cancel::default())
            .await
            .is_ok());
    }
}

#[tokio::test]
async fn snapshot_after_crash_rehydrates_exact_window_and_rejects_old_epoch() {
    let f = Fixture::new();
    let driver = WindowsDriver::isolated(f.config("normal_restart"))
        .await
        .unwrap();
    let app = driver
        .resolve_app(&gcu_core::driver::AppQuery("Editor".into()))
        .await
        .unwrap();
    let win = gcu_core::driver::WindowRef {
        app: app.clone(),
        id: driver.key_window(&app).await.unwrap().id,
    };
    let old_focus = driver.focused_element(&app).await.unwrap().unwrap();
    let tree = driver
        .snapshot_tree(&win, &Default::default(), &Cancel::default())
        .await
        .unwrap();
    assert!(tree.root.is_some());
    assert_eq!(f.pids().len(), 2);
    assert!(matches!(
        driver
            .perform(&DriverAction::Press { el: old_focus }, &Cancel::default())
            .await,
        Err(DriverError::StaleElement)
    ));
}

#[tokio::test]
async fn cancelled_queued_mutation_returns_promptly_without_dispatch() {
    let f = Fixture::new();
    let s = std::sync::Arc::new(Supervisor::start(f.config("hang_once")).await.unwrap());
    let active = {
        let s = s.clone();
        tokio::spawn(async move { s.execute(mutation(), &Cancel::default()).await })
    };
    dispatched(&f).await;
    let cancel = Cancel::default();
    let pending_cancel = cancel.clone();
    let queued = {
        let s = s.clone();
        tokio::spawn(async move { s.execute(mutation(), &pending_cancel).await })
    };
    tokio::time::sleep(Duration::from_millis(10)).await;
    cancel.cancel();
    let result = tokio::time::timeout(Duration::from_millis(100), queued)
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(
        result,
        Err(DriverError::Cancelled {
            maybe_executed: false
        })
    ));
    assert_eq!(f.calls(), ["mutation"]);
    s.stop_handle().stop();
    let _ = active.await;
    s.shutdown().await.unwrap();
}

mod input;
mod semantic;
