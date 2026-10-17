//! Requires an interactive fixture desktop; never run on a user's work session.
use super::*;
use gcu_host_protocol::overlay::prepare_surface;
use tauri::{WebviewUrl, WebviewWindowBuilder};

#[test]
#[ignore = "requires an interactive Windows/X11 fixture desktop; see overlay/README.md"]
fn native_surface_capture_flags_and_nonactivation_fixture() {
    let mut app = tauri::Builder::default()
        .any_thread()
        .build(tauri::generate_context!())
        .unwrap();
    let build = |label, cursor| {
        let win = WebviewWindowBuilder::new(
            app.handle(),
            label,
            WebviewUrl::App("computer-use-overlay.html".into()),
        )
        .visible(false)
        .focused(false)
        .focusable(false)
        .decorations(false)
        .transparent(true)
        .always_on_top(true)
        .inner_size(240., 56.)
        .build()
        .unwrap();
        prepare_surface(&mut WindowSurface(&win), cursor).unwrap();
        win
    };
    let draw = build("cu-overlay", true);
    let controls = build("cu-overlay-controls", false);
    let p = gcu_host_protocol::overlay::placement(
        &[gcu_host_protocol::overlay::Monitor {
            frame: gcu_protocol::Rect {
                x: 0.,
                y: 0.,
                w: 800.,
                h: 600.,
            },
            scale: 1.,
        }],
        None,
    )
    .unwrap();
    #[cfg(windows)]
    // SAFETY: Read-only public foreground query on the test's desktop.
    let before = unsafe { windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow() };
    present(&draw, &controls, &p).unwrap();
    for _ in 0..3 {
        // This ignored smoke test pumps a fixed number of events; it does not
        // use the unbounded busy loop deprecated by Tauri.
        #[allow(deprecated)]
        app.run_iteration(|_, _| {});
    }
    assert!(!draw.is_focused().unwrap());
    assert!(!controls.is_focused().unwrap());
    #[cfg(windows)]
    {
        use windows::Win32::UI::WindowsAndMessaging::*;
        // SAFETY: Live owned HWNDs and initialized output. No input is injected.
        unsafe {
            assert_eq!(GetForegroundWindow(), before);
            assert_ne!(
                GetWindowLongPtrW(draw.hwnd().unwrap(), GWL_EXSTYLE) & WS_EX_TRANSPARENT.0 as isize,
                0
            );
            assert_eq!(
                GetWindowLongPtrW(controls.hwnd().unwrap(), GWL_EXSTYLE)
                    & WS_EX_TRANSPARENT.0 as isize,
                0
            );
            for win in [&draw, &controls] {
                let mut affinity = 0;
                GetWindowDisplayAffinity(win.hwnd().unwrap(), &mut affinity).unwrap();
                assert_eq!(affinity, WDA_EXCLUDEFROMCAPTURE.0);
            }
        }
    }
    draw.destroy().unwrap();
    controls.destroy().unwrap();
}
