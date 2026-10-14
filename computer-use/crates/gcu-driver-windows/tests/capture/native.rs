//! Opt-in disposable-desktop evidence; normal CI compiles but does not run.
use super::*;
use gcu_core::driver::{AppQuery, WindowRef};
use gcu_driver_windows::WindowsDriver;

async fn fixture_window() -> (WindowsDriver, WindowRef) {
    let driver = WindowsDriver::new()
        .await
        .expect("unelevated interactive Windows desktop");
    let app = driver
        .resolve_app(&AppQuery::from("gcu-fixture"))
        .await
        .expect("open only the fixture first");
    let info = driver
        .list_windows(Some(&app))
        .await
        .unwrap()
        .into_iter()
        .next()
        .expect("fixture window");
    let win = WindowRef { app, id: info.id };
    (driver, win)
}
#[tokio::test]
#[ignore = "not_run: needs fully occluded fixture on a disposable Windows desktop"]
async fn live_wgc_occluded_window_keeps_fixture_pixels() {
    let (driver, win) = fixture_window().await;
    let capture = driver
        .capture(&win, &CaptureOpts::default(), &Cancel::default())
        .await
        .unwrap();
    assert_eq!(
        capture.geometry,
        driver.window_geometry(&win).await.unwrap()
    );
    assert_eq!(capture.geometry.frame.w, f64::from(capture.width));
    assert_eq!(capture.geometry.frame.h, f64::from(capture.height));
    assert_eq!(
        driver.capture_stats().unwrap().last_path,
        Some(CapturePath::Wgc)
    );
    assert!(
        decode(&capture.bytes)
            .as_chunks::<4>()
            .0
            .iter()
            .any(|p| p[..3] == [95, 115, 135]),
        "covered grid cell 0,0 must retain its own color"
    );
}
fn find_grid(node: &RawNode) -> Option<Rect> {
    if node.name.as_deref() == Some("grid_canvas")
        || node.identifier.as_deref() == Some("grid_canvas")
    {
        return node.frame;
    }
    node.children.iter().find_map(find_grid)
}
#[tokio::test]
#[ignore = "not_run: needs foreground fixture, state file, Canvas Grid tab and 150% DPI"]
async fn live_capture_150_percent_grid_click_matches_state_oracle() {
    let (driver, win) = fixture_window().await;
    let path = std::path::PathBuf::from(
        std::env::var_os("GCU_FIXTURE_STATE").expect("fixture state path"),
    );
    let read_state =
        || -> serde_json::Value { serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap() };
    let col = 2;
    let row = 3;
    assert_ne!(
        read_state()["grid_hit"],
        serde_json::json!([col, row]),
        "reset fixture first to avoid an already-satisfied oracle"
    );
    let tree = driver
        .snapshot_tree(&win, &TreeOpts::default(), &Cancel::default())
        .await
        .unwrap();
    assert!(tree.incomplete.is_none());
    let grid = find_grid(tree.root.as_ref().unwrap()).expect("select Canvas Grid tab before test");
    let capture = driver
        .capture(&win, &CaptureOpts::default(), &Cancel::default())
        .await
        .unwrap();
    assert_eq!(
        capture.geometry.scale, 1.5,
        "Windows display scaling must be 150%"
    );
    let x = grid.x + (f64::from(col) + 0.25) * grid.w / 8.;
    let y = grid.y + (f64::from(row) + 0.25) * grid.h / 6.;
    assert!(x >= 0. && y >= 0. && x < f64::from(capture.width) && y < f64::from(capture.height));
    let expected = [
        ((col * 31 + 45) % 180 + 50) as u8,
        ((row * 41 + 65) % 180 + 50) as u8,
        (((col + row) * 29 + 85) % 180 + 50) as u8,
    ];
    let pixels = decode(&capture.bytes);
    let index = (y as usize * capture.width as usize + x as usize) * 4;
    assert_eq!(
        &pixels[index..index + 3],
        &expected,
        "UIA bounds must align with screenshot pixels at 150% DPI"
    );
    let pid = win.app.info.pid.unwrap();
    let sx = (capture.geometry.frame.x + x).round() as i32;
    let sy = (capture.geometry.frame.y + y).round() as i32;
    tokio::task::spawn_blocking(move || ffi::click_fixture(pid, sx, sy))
        .await
        .unwrap();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    loop {
        if read_state()["grid_hit"] == serde_json::json!([col, row]) {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "fixture must report the exact screenshot-selected cell"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}
mod ffi {
    use windows::Win32::UI::{
        HiDpi::{SetThreadDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2},
        Input::KeyboardAndMouse::*,
        WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId, SetCursorPos},
    };
    pub(super) fn click_fixture(expected_pid: u32, x: i32, y: i32) {
        // SAFETY: test-only, explicitly invoked disposable fixture. Queries and
        // clicks run on one DPI-aware thread, and the exact foreground PID is
        // checked before any input. No activation of another app is attempted.
        unsafe {
            let previous = SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
            assert!(!previous.0.is_null());
            let mut pid = 0;
            GetWindowThreadProcessId(GetForegroundWindow(), Some(&mut pid));
            if pid != expected_pid {
                SetThreadDpiAwarenessContext(previous);
                panic!("only the disposable fixture may receive the grid click");
            }
            let result = SetCursorPos(x, y);
            if result.is_err() {
                SetThreadDpiAwarenessContext(previous);
                panic!("test cursor positioning failed");
            }
            let input = |flags| INPUT {
                r#type: INPUT_MOUSE,
                Anonymous: INPUT_0 {
                    mi: MOUSEINPUT {
                        dwFlags: flags,
                        ..Default::default()
                    },
                },
            };
            let sent = SendInput(
                &[input(MOUSEEVENTF_LEFTDOWN), input(MOUSEEVENTF_LEFTUP)],
                std::mem::size_of::<INPUT>() as i32,
            );
            if sent != 2 {
                SendInput(
                    &[input(MOUSEEVENTF_LEFTUP)],
                    std::mem::size_of::<INPUT>() as i32,
                );
            }
            SetThreadDpiAwarenessContext(previous);
            assert_eq!(
                sent, 2,
                "fixture click must deliver one complete down/up pair"
            );
        }
    }
}
