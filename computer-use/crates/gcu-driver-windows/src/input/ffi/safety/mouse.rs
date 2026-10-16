//! Win32 records shared with the CI-only probe matrix. Never selects a fallback.
use super::MouseProbe;
use windows::Win32::{
    Foundation::POINT,
    UI::{HiDpi::*, Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
};

pub(super) fn inputs(mode: MouseProbe, tag: usize) -> Result<Vec<INPUT>, String> {
    let record = |dx, dy, data, flags| INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx,
                dy,
                mouseData: data,
                dwFlags: flags,
                dwExtraInfo: tag,
                ..Default::default()
            },
        },
    };
    Ok(match mode {
        MouseProbe::ZeroMove => vec![record(
            0,
            0,
            0,
            MOUSEEVENTF_MOVE | MOUSEEVENTF_MOVE_NOCOALESCE,
        )],
        MouseProbe::ZeroWheel => vec![record(0, 0, 0, MOUSEEVENTF_WHEEL)],
        MouseProbe::ZeroHWheel => vec![record(0, 0, 0, MOUSEEVENTF_HWHEEL)],
        MouseProbe::AbsoluteSamePosition => {
            // SAFETY: change only this sender thread; restore on every exit path.
            let previous =
                unsafe { SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
            if previous.0.is_null() {
                return Err("physical DPI context unavailable".into());
            }
            struct Dpi(DPI_AWARENESS_CONTEXT);
            impl Drop for Dpi {
                fn drop(&mut self) {
                    // SAFETY: restore this thread's successfully saved DPI context.
                    unsafe {
                        SetThreadDpiAwarenessContext(self.0);
                    }
                }
            }
            let _dpi = Dpi(previous);
            let mut point = POINT::default();
            // SAFETY: scalar read-only desktop queries and bounded writable POINT.
            let (left, top, width, height) = unsafe {
                GetPhysicalCursorPos(&mut point)
                    .map_err(|e| format!("GetPhysicalCursorPos: {e}"))?;
                (
                    GetSystemMetrics(SM_XVIRTUALSCREEN),
                    GetSystemMetrics(SM_YVIRTUALSCREEN),
                    GetSystemMetrics(SM_CXVIRTUALSCREEN),
                    GetSystemMetrics(SM_CYVIRTUALSCREEN),
                )
            };
            // Target the middle of the current physical pixel's normalized bin.
            // Concurrent physical input can still race this query and be undone;
            // this candidate therefore requires an explicit debug/CI opt-in.
            let normalize = |position: i32, origin: i32, size: i32| {
                let offset = i64::from(position) - i64::from(origin);
                if size <= 0 || offset < 0 || offset >= i64::from(size) {
                    return Err("cursor outside virtual desktop".to_string());
                }
                Ok(((offset * 65536 + 32768) / i64::from(size)).min(65535) as i32)
            };
            vec![record(
                normalize(point.x, left, width)?,
                normalize(point.y, top, height)?,
                0,
                MOUSEEVENTF_ABSOLUTE
                    | MOUSEEVENTF_VIRTUALDESK
                    | MOUSEEVENTF_MOVE
                    | MOUSEEVENTF_MOVE_NOCOALESCE,
            )]
        }
        // Relative acceleration/clipping can prevent exact restoration. CI only.
        MouseProbe::RelativePair => vec![
            record(1, 0, 0, MOUSEEVENTF_MOVE | MOUSEEVENTF_MOVE_NOCOALESCE),
            record(-1, 0, 0, MOUSEEVENTF_MOVE | MOUSEEVENTF_MOVE_NOCOALESCE),
        ],
        // May trigger application navigation, activation or a global shortcut.
        MouseProbe::XButtonPair => vec![
            record(0, 0, 1, MOUSEEVENTF_XDOWN),
            record(0, 0, 1, MOUSEEVENTF_XUP),
        ],
    })
}
