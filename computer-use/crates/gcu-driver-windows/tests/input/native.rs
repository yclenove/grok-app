//! Disposable Windows desktop only. These tests never operate real accounts.
use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use windows::{
    core::w,
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, WPARAM},
        UI::WindowsAndMessaging::*,
    },
};
static CLICKS: AtomicUsize = AtomicUsize::new(0);
async fn native_driver() -> gcu_driver_windows::WindowsDriver {
    use gcu_driver_windows::worker::{ExecMode, WorkerConfig};
    let mode = std::env::var("GCU_TEST_EXEC_MODE").unwrap_or_else(|_| "worker".into());
    let mode: ExecMode = serde_json::from_value(serde_json::Value::String(mode)).unwrap();
    let worker = if mode == ExecMode::Worker {
        let path = std::path::PathBuf::from(
            std::env::var_os("GCU_WORKER_EXE").expect("absolute built gcu.exe path"),
        );
        assert!(path.is_absolute());
        assert_eq!(path.file_name().unwrap().to_string_lossy(), "gcu.exe");
        path
    } else {
        std::path::PathBuf::from("gcu")
    };
    if mode == ExecMode::Worker {
        gcu_driver_windows::WindowsDriver::monitored(
            WorkerConfig::new(worker),
            Default::default(),
            Arc::new(common::TestStop::default()),
        )
        .await
        .unwrap()
    } else {
        gcu_driver_windows::WindowsDriver::new_in_process()
            .await
            .unwrap()
    }
}
// SAFETY: Win32 supplies callback arguments; the body catches every panic.
unsafe extern "system" fn proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    std::panic::catch_unwind(|| {
        if message == WM_COMMAND && wparam.0 >> 16 == BN_CLICKED as usize {
            CLICKS.fetch_add(1, Ordering::SeqCst);
        }
        // SAFETY: forwarding untouched window-procedure arguments from Win32.
        unsafe { DefWindowProcW(window, message, wparam, lparam) }
    })
    .unwrap_or(LRESULT(0))
}
#[tokio::test]
#[ignore = "requires a disposable interactive unelevated Windows desktop"]
async fn live_fixture_background_message_click_keeps_foreground() {
    let (ready, started) = std::sync::mpsc::channel();
    let fixture = std::thread::spawn(move || {
        // SAFETY: this fixture thread owns/registers its windows and message
        // loop; no UI state or handles cross as pointers. WndProc catches panics.
        unsafe {
            let wc = WNDCLASSW {
                lpfnWndProc: Some(proc),
                lpszClassName: w!("GcuInputFixture"),
                ..Default::default()
            };
            assert_ne!(RegisterClassW(&wc), 0);
            let root = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("GcuInputFixture"),
                w!("GCU Input Fixture"),
                WS_OVERLAPPEDWINDOW,
                100,
                100,
                300,
                200,
                None,
                None,
                None,
                None,
            )
            .unwrap();
            let _button = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("BUTTON"),
                w!("Count"),
                WS_CHILD | WS_VISIBLE | WINDOW_STYLE(BS_PUSHBUTTON as u32),
                10,
                10,
                120,
                50,
                Some(root),
                None,
                None,
                None,
            )
            .unwrap();
            let _ = ShowWindow(root, SW_SHOWNOACTIVATE);
            ready.send(root.0 as usize as u64).unwrap();
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                if msg.message == WM_APP {
                    break;
                }
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            DestroyWindow(root).unwrap();
        }
    });
    let fixture_hwnd = started.recv().unwrap();
    let driver = native_driver().await;
    let executable = std::env::current_exe().unwrap();
    let app = driver
        .resolve_app(&AppQuery(executable.to_string_lossy().into()))
        .await
        .unwrap();
    let info = driver
        .list_windows(Some(&app))
        .await
        .unwrap()
        .into_iter()
        .find(|w| w.title == "GCU Input Fixture")
        .unwrap();
    let win = WindowRef { app, id: info.id };
    // Window coordinates include nonclient chrome. Map the center of the child
    // to screen, then to the driver's physical window-pixel origin.
    let mut point = windows::Win32::Foundation::POINT { x: 60, y: 25 };
    // SAFETY: live fixture handles, fixed writable POINT; queries do not activate.
    let before = unsafe {
        let root = HWND(fixture_hwnd as usize as *mut _);
        let button = FindWindowExW(Some(root), None, w!("BUTTON"), None).unwrap();
        assert!(windows::Win32::Graphics::Gdi::ClientToScreen(button, &mut point).as_bool());
        GetForegroundWindow()
    };
    let req = DriverAction::PointerClick {
        win,
        at: WindowPoint {
            x: f64::from(point.x) - info.frame.x,
            y: f64::from(point.y) - info.frame.y,
        },
        button: Button::Left,
        count: 1,
        route: PointerRoute::TargetedOnly,
    };
    CLICKS.store(0, Ordering::SeqCst);
    assert_eq!(
        driver.perform(&req, &Cancel::default()).await,
        Ok(Delivery::TargetedEvent)
    );
    assert_eq!(CLICKS.load(Ordering::SeqCst), 1);
    // SAFETY: nonblocking borrowed foreground probe and pointer-free exit message.
    unsafe {
        assert_eq!(GetForegroundWindow(), before);
        PostMessageW(
            Some(HWND(fixture_hwnd as usize as *mut _)),
            WM_APP,
            WPARAM(0),
            LPARAM(0),
        )
        .unwrap();
    }
    fixture.join().unwrap();
}
#[tokio::test]
#[ignore = "open a blank disposable Notepad document and focus its editor first"]
async fn live_notepad_foreground_chinese_preserves_text() {
    let driver = native_driver().await;
    if std::env::var("GCU_TEST_EXEC_MODE").as_deref() == Ok("in_process") {
        assert_eq!(
            driver.capabilities().delivery.foreground,
            gcu_protocol::capabilities::ForegroundCapability::Unavailable
        );
        return;
    }
    let app = driver
        .resolve_app(&AppQuery::from("notepad.exe"))
        .await
        .unwrap();
    let window = driver.key_window(&app).await.unwrap();
    let win = WindowRef { app, id: window.id };
    let text = "GCU 中文输入 😀";
    let req = DriverAction::TypeText {
        win: win.clone(),
        focus: None,
        text: text.into(),
        route: KeyRoute::AllowForeground,
    };
    assert_eq!(
        driver.perform(&req, &Cancel::default()).await,
        Ok(Delivery::Foreground)
    );
    let tree = driver
        .snapshot_tree(&win, &TreeOpts::default(), &Cancel::default())
        .await
        .unwrap();
    fn contains(node: &gcu_core::model::RawNode, text: &str) -> bool {
        node.name.as_ref().is_some_and(|v| v.contains(text))
            || matches!(&node.value,Some(gcu_core::model::NodeValue::Text(v)) if v.contains(text))
            || node
                .text_extras
                .as_ref()
                .and_then(|e| e.text.as_ref())
                .is_some_and(|v| v.contains(text))
            || node.children.iter().any(|n| contains(n, text))
    }
    assert!(tree.root.as_ref().is_some_and(|n| contains(n, text)));
}
