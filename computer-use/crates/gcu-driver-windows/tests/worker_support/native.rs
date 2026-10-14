//! Opt-in Windows desktop soak; normal CI only compiles this test.
use gcu_core::driver::{AppQuery, Cancel, Driver, DriverError, WindowRef};
use gcu_driver_windows::{worker::WorkerConfig, WindowsDriver};

#[tokio::test]
#[ignore = "nightly not_run: disposable Windows fixture desktop and GCU_WORKER_EXE required"]
async fn moving_window_observe_300_times_keeps_parent_alive() {
    let executable = std::env::var_os("GCU_WORKER_EXE").expect("absolute built gcu.exe");
    let driver = WindowsDriver::isolated(WorkerConfig::new(executable))
        .await
        .unwrap();
    let fixture = std::env::var("GCU_FIXTURE_EXE").expect("open the disposable fixture first");
    let app = driver.resolve_app(&AppQuery(fixture)).await.unwrap();
    let window = driver
        .list_windows(Some(&app))
        .await
        .unwrap()
        .into_iter()
        .next()
        .unwrap();
    let hwnd = u64::from_str_radix(window.id.split(':').next().unwrap(), 16).unwrap();
    let pid = app.info.pid.unwrap();
    let win = WindowRef { app, id: window.id };
    let active = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    let moving = active.clone();
    let movement =
        tokio::task::spawn_blocking(move || unsafe_native::move_fixture(hwnd, pid, moving));
    for _ in 0..300 {
        let result = driver
            .snapshot_tree(&win, &Default::default(), &Cancel::default())
            .await;
        // A moving provider can time out; the parent must survive and keep serving.
        assert!(
            result.is_ok()
                || matches!(
                    result,
                    Err(DriverError::Timeout {
                        maybe_executed: false
                    })
                )
        );
    }
    active.store(false, std::sync::atomic::Ordering::Release);
    movement.await.unwrap();
    assert!(driver.doctor().await.ready);
}
mod unsafe_native {
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    use windows::Win32::{
        Foundation::{HWND, RECT},
        UI::WindowsAndMessaging::*,
    };
    pub(super) fn move_fixture(hwnd: u64, expected_pid: u32, active: Arc<AtomicBool>) {
        let hwnd = HWND(hwnd as usize as *mut _);
        // SAFETY: explicitly selected disposable fixture only; check the owning
        // PID before every move and never activate/change focus or emit input.
        unsafe {
            let mut original = RECT::default();
            GetWindowRect(hwnd, &mut original).unwrap();
            let mut step = 0;
            while active.load(Ordering::Acquire) {
                let mut pid = 0;
                GetWindowThreadProcessId(hwnd, Some(&mut pid));
                assert_eq!(pid, expected_pid);
                SetWindowPos(
                    hwnd,
                    None,
                    original.left + step % 2 * 20,
                    original.top,
                    0,
                    0,
                    SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
                )
                .unwrap();
                step += 1;
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            SetWindowPos(
                hwnd,
                None,
                original.left,
                original.top,
                0,
                0,
                SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
            )
            .unwrap();
        }
    }
}

#[tokio::test]
#[ignore = "not_run: disposable Windows fixture desktop and GCU_WORKER_EXE required"]
async fn worker_restart_preserves_live_window_stamp() {
    let driver = WindowsDriver::isolated(WorkerConfig::new(
        std::env::var_os("GCU_WORKER_EXE").unwrap(),
    ))
    .await
    .unwrap();
    let app = driver
        .resolve_app(&AppQuery(std::env::var("GCU_FIXTURE_EXE").unwrap()))
        .await
        .unwrap();
    let id = driver
        .list_windows(Some(&app))
        .await
        .unwrap()
        .into_iter()
        .next()
        .unwrap()
        .id;
    let win = WindowRef { app, id };
    assert!(driver
        .snapshot_tree(&win, &Default::default(), &Cancel::default())
        .await
        .is_ok());
    driver.release_all_input().await;
    // Stop has retired the worker; the new process must accept the same exact HWND stamp.
    assert!(driver
        .snapshot_tree(&win, &Default::default(), &Cancel::default())
        .await
        .is_ok());
}
