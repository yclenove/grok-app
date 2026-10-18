//! Physical-pixel Win32 records shared with the disposable probe matrix.
use super::{Cursor, Desktop, MouseProbe};
use windows::Win32::{
    Foundation::POINT,
    UI::{HiDpi::*, Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
};

pub(super) struct Dpi(DPI_AWARENESS_CONTEXT);
impl Dpi {
    pub(super) fn enter() -> Result<Self, String> {
        // SAFETY: only this sender thread changes; Drop restores every exit path.
        let previous =
            unsafe { SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
        if previous.0.is_null() {
            Err("physical DPI context unavailable".into())
        } else {
            Ok(Self(previous))
        }
    }
}
impl Drop for Dpi {
    fn drop(&mut self) {
        // SAFETY: restore the successfully saved, same-thread DPI context.
        unsafe {
            SetThreadDpiAwarenessContext(self.0);
        }
    }
}
pub(super) fn cursor() -> Result<Cursor, String> {
    let mut point = POINT::default();
    // SAFETY: initialized writable POINT; physical pixels independent of app DPI.
    unsafe { GetPhysicalCursorPos(&mut point) }
        .map_err(|e| format!("GetPhysicalCursorPos: {e}"))?;
    Ok(Cursor {
        x: point.x,
        y: point.y,
    })
}
pub(super) fn desktop() -> Desktop {
    // SAFETY: scalar read-only metrics in the caller's per-monitor-aware context.
    unsafe {
        Desktop {
            left: GetSystemMetrics(SM_XVIRTUALSCREEN),
            top: GetSystemMetrics(SM_YVIRTUALSCREEN),
            width: GetSystemMetrics(SM_CXVIRTUALSCREEN),
            height: GetSystemMetrics(SM_CYVIRTUALSCREEN),
        }
    }
}
fn record(dx: i32, dy: i32, data: u32, flags: MOUSE_EVENT_FLAGS, tag: usize) -> INPUT {
    INPUT {
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
    }
}
pub(super) fn absolute(tag: usize, at: (i32, i32)) -> Vec<INPUT> {
    vec![record(
        at.0,
        at.1,
        0,
        MOUSEEVENTF_ABSOLUTE
            | MOUSEEVENTF_VIRTUALDESK
            | MOUSEEVENTF_MOVE
            | MOUSEEVENTF_MOVE_NOCOALESCE,
        tag,
    )]
}
pub(super) fn inputs(mode: MouseProbe, tag: usize) -> Result<Vec<INPUT>, String> {
    Ok(match mode {
        MouseProbe::ZeroMove => vec![record(
            0,
            0,
            0,
            MOUSEEVENTF_MOVE | MOUSEEVENTF_MOVE_NOCOALESCE,
            tag,
        )],
        MouseProbe::ZeroWheel => vec![record(0, 0, 0, MOUSEEVENTF_WHEEL, tag)],
        MouseProbe::ZeroHWheel => vec![record(0, 0, 0, MOUSEEVENTF_HWHEEL, tag)],
        // The product uses dispatch()'s idle and post-cursor checks. This direct
        // constructor is used only by the opt-in diagnostic matrix.
        #[cfg(any(test, all(feature = "test-worker-faults", debug_assertions)))]
        MouseProbe::AbsoluteSamePosition => {
            let _dpi = Dpi::enter()?;
            let point = cursor()?;
            let bounds = desktop();
            absolute(
                tag,
                (
                    super::hook_probe::normalize(point.x, bounds.left, bounds.width)?,
                    super::hook_probe::normalize(point.y, bounds.top, bounds.height)?,
                ),
            )
        }
        #[cfg(not(any(test, all(feature = "test-worker-faults", debug_assertions))))]
        MouseProbe::AbsoluteSamePosition => {
            return Err("absolute mouse probe requires guarded dispatch".into());
        }
        #[cfg(any(test, all(feature = "test-worker-faults", debug_assertions)))]
        MouseProbe::RelativePair => vec![
            record(1, 0, 0, MOUSEEVENTF_MOVE | MOUSEEVENTF_MOVE_NOCOALESCE, tag),
            record(
                -1,
                0,
                0,
                MOUSEEVENTF_MOVE | MOUSEEVENTF_MOVE_NOCOALESCE,
                tag,
            ),
        ],
        #[cfg(any(test, all(feature = "test-worker-faults", debug_assertions)))]
        MouseProbe::XButtonPair => vec![
            record(0, 0, 1, MOUSEEVENTF_XDOWN, tag),
            record(0, 0, 1, MOUSEEVENTF_XUP, tag),
        ],
    })
}
