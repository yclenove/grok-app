//! Nonblocking delivery/activation bindings. No automatic retry or elevation.
use super::{failure, hwnd};
use crate::{
    input::api::{Event, Target},
    uia::patterns::ActionContext,
};
use gcu_core::driver::{Button, DriverError, Result};
use windows::Win32::{
    Foundation::{LPARAM, POINT, WPARAM},
    Graphics::Gdi::ScreenToClient,
    System::Threading::{AttachThreadInput, GetCurrentThreadId},
    UI::{Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
};

pub(super) fn class(window: u64) -> Result<String> {
    let mut buffer = [0u16; 256];
    // SAFETY: fixed writable UTF-16 output, borrowed HWND, nonblocking metadata.
    let n = unsafe { GetClassNameW(hwnd(window), &mut buffer) };
    if n == 0 {
        return Err(DriverError::WindowGone);
    }
    Ok(String::from_utf16_lossy(&buffer[..n as usize]))
}
pub(super) fn activate(window: u64, context: &ActionContext) -> Result<()> {
    // SAFETY: thread IDs from live windows. Attachment guard always detaches,
    // including cancellation/errors; no synthetic keystroke activation tricks.
    unsafe {
        let current = GetCurrentThreadId();
        let foreground = GetWindowThreadProcessId(GetForegroundWindow(), None);
        let target = GetWindowThreadProcessId(hwnd(window), None);
        let mut guard = Attachment {
            current,
            threads: Vec::new(),
        };
        for thread in [foreground, target] {
            if thread != 0 && thread != current && !guard.threads.contains(&thread) {
                AttachThreadInput(current, thread, true)
                    .ok()
                    .map_err(|_| failure())?;
                guard.threads.push(thread);
            }
        }
        // This may fail under the foreground lock. It never grants policy;
        // SetForegroundWindow's real outcome is independently checked.
        let mut pid = 0;
        GetWindowThreadProcessId(hwnd(window), Some(&mut pid));
        let _ = AllowSetForegroundWindow(pid);
        context.begin()?;
        let accepted = SetForegroundWindow(hwnd(window)).as_bool();
        drop(guard);
        if !accepted {
            return Err(DriverError::Native {
                message: "Windows activation refused".into(),
                maybe_executed: true,
            });
        }
    }
    Ok(())
}
struct Attachment {
    current: u32,
    threads: Vec<u32>,
}
impl Drop for Attachment {
    fn drop(&mut self) {
        for thread in self.threads.iter().rev() {
            // SAFETY: balances only successful attachments on this same thread.
            if !unsafe { AttachThreadInput(self.current, *thread, false) }.as_bool() {
                tracing::warn!("Windows input thread detach failed");
            }
        }
    }
}
fn packed(target: &Target, p: gcu_core::model::WindowPoint, client: bool) -> Result<LPARAM> {
    let mut point = POINT {
        x: p.x.round() as i32,
        y: p.y.round() as i32,
    };
    // SAFETY: valid writable POINT; coordinates use PMv2 physical pixels.
    if client && !unsafe { ScreenToClient(hwnd(target.hwnd), &mut point) }.as_bool() {
        return Err(failure());
    }
    if i16::try_from(point.x).is_err() || i16::try_from(point.y).is_err() {
        return Err(DriverError::OutOfBounds);
    }
    Ok(LPARAM(
        ((point.x as u16 as u32) | ((point.y as u16 as u32) << 16)) as isize,
    ))
}
fn post_one(target: &Target, msg: u32, w: usize, l: LPARAM) -> Result<()> {
    // SAFETY: asynchronous messages carry only integer coordinates/keys (no
    // pointers into this process), exact child HWND validated before dispatch.
    unsafe { PostMessageW(Some(hwnd(target.hwnd)), msg, WPARAM(w), l) }.map_err(|e| {
        if e.code().0 as u32 == 0x80070005 {
            DriverError::ElevationRequired
        } else {
            failure()
        }
    })
}
pub(super) fn post(target: &Target, event: Event) -> Result<()> {
    match event {
        Event::Move(p, held) => post_one(
            target,
            WM_MOUSEMOVE,
            if held { 1 } else { 0 },
            packed(target, p, true)?,
        ),
        Event::Button(button, down, p, double) => {
            let (d, u, db, mask) = match button {
                Button::Left => (WM_LBUTTONDOWN, WM_LBUTTONUP, WM_LBUTTONDBLCLK, 1),
                Button::Right => (WM_RBUTTONDOWN, WM_RBUTTONUP, WM_RBUTTONDBLCLK, 2),
                Button::Middle => (WM_MBUTTONDOWN, WM_MBUTTONUP, WM_MBUTTONDBLCLK, 16),
            };
            post_one(
                target,
                if !down {
                    u
                } else if double {
                    db
                } else {
                    d
                },
                if down { mask } else { 0 },
                packed(target, p, true)?,
            )
        }
        Event::Wheel(dx, dy, p) => {
            if dy != 0 {
                post_one(
                    target,
                    WM_MOUSEWHEEL,
                    ((-(dy as i64)) as u16 as usize) << 16,
                    packed(target, p, false)?,
                )?;
            }
            if dx != 0 {
                post_one(
                    target,
                    WM_MOUSEHWHEEL,
                    ((dx as u16) as usize) << 16,
                    packed(target, p, false)?,
                )
                .map_err(|e| {
                    if dy != 0 {
                        DriverError::Native {
                            message: "horizontal wheel failed after vertical delivery".into(),
                            maybe_executed: true,
                        }
                    } else {
                        e
                    }
                })?;
            }
            Ok(())
        }
        Event::Key(key, down, extended, alt) => {
            // SAFETY: pure key/layout mapping for the recipient thread.
            let scan = unsafe {
                MapVirtualKeyExW(
                    u32::from(key),
                    MAPVK_VK_TO_VSC,
                    Some(GetKeyboardLayout(GetWindowThreadProcessId(
                        hwnd(target.hwnd),
                        None,
                    ))),
                )
            } & 255;
            let bits = 1
                | (scan << 16)
                | (u32::from(extended) << 24)
                | (u32::from(alt) << 29)
                | if down { 0 } else { 3 << 30 };
            let msg = match (alt, down) {
                (false, true) => WM_KEYDOWN,
                (false, false) => WM_KEYUP,
                (true, true) => WM_SYSKEYDOWN,
                (true, false) => WM_SYSKEYUP,
            };
            post_one(target, msg, key as usize, LPARAM(bits as isize))
        }
        Event::Unicode(unit, true) => post_one(target, WM_CHAR, unit as usize, LPARAM(1)),
        Event::Unicode(_, false) => Ok(()),
    }
}
pub(super) fn inputs(events: &[Event]) -> Result<Vec<INPUT>> {
    let mut inputs = Vec::new();
    let mut pending_lf = false;
    for event in events {
        let input = match *event {
            Event::Move(p, _) => {
                // SAFETY: these four metrics are nonblocking integer queries.
                let (x, y, w, h) = unsafe {
                    (
                        GetSystemMetrics(SM_XVIRTUALSCREEN),
                        GetSystemMetrics(SM_YVIRTUALSCREEN),
                        GetSystemMetrics(SM_CXVIRTUALSCREEN),
                        GetSystemMetrics(SM_CYVIRTUALSCREEN),
                    )
                };
                if w <= 1 || h <= 1 {
                    return Err(DriverError::CapabilityUnavailable);
                }
                if p.x < f64::from(x)
                    || p.y < f64::from(y)
                    || p.x >= f64::from(x + w)
                    || p.y >= f64::from(y + h)
                {
                    return Err(DriverError::OutOfBounds);
                }
                mouse(
                    crate::input::pointer_settle::normalize_axis(p.x, x, w),
                    crate::input::pointer_settle::normalize_axis(p.y, y, h),
                    0,
                    MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK,
                )
            }
            Event::Button(button, down, _, _) => mouse(
                0,
                0,
                0,
                match (button, down) {
                    (Button::Left, true) => MOUSEEVENTF_LEFTDOWN,
                    (Button::Left, false) => MOUSEEVENTF_LEFTUP,
                    (Button::Right, true) => MOUSEEVENTF_RIGHTDOWN,
                    (Button::Right, false) => MOUSEEVENTF_RIGHTUP,
                    (Button::Middle, true) => MOUSEEVENTF_MIDDLEDOWN,
                    (Button::Middle, false) => MOUSEEVENTF_MIDDLEUP,
                },
            ),
            Event::Wheel(dx, dy, _) => {
                if dy != 0 {
                    inputs.push(mouse(0, 0, (-(dy as i64)) as u32, MOUSEEVENTF_WHEEL));
                }
                if dx == 0 {
                    continue;
                }
                mouse(0, 0, dx as u32, MOUSEEVENTF_HWHEEL)
            }
            Event::Key(k, down, extended, _) => keyboard(
                k,
                0,
                if down {
                    KEYBD_EVENT_FLAGS(0)
                } else {
                    KEYEVENTF_KEYUP
                } | if extended {
                    KEYEVENTF_EXTENDEDKEY
                } else {
                    KEYBD_EVENT_FLAGS(0)
                },
            ),
            Event::Unicode(unit, down) => {
                match crate::input::messages::foreground_send_unit(unit, down, &mut pending_lf) {
                    Some(crate::input::messages::ForegroundSend::Enter(down)) => keyboard(
                        0x0d,
                        0,
                        if down {
                            KEYBD_EVENT_FLAGS(0)
                        } else {
                            KEYEVENTF_KEYUP
                        },
                    ),
                    Some(crate::input::messages::ForegroundSend::Unicode(unit, down)) => keyboard(
                        0,
                        unit,
                        KEYEVENTF_UNICODE
                            | if down {
                                KEYBD_EVENT_FLAGS(0)
                            } else {
                                KEYEVENTF_KEYUP
                            },
                    ),
                    None => continue,
                }
            }
        };
        inputs.push(input);
    }
    Ok(inputs)
}
fn mouse(x: i32, y: i32, data: u32, flags: MOUSE_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx: x,
                dy: y,
                mouseData: data,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}
fn keyboard(vk: u16, scan: u16, flags: KEYBD_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(vk),
                wScan: scan,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}
/// Insert an already-admitted packet. The caller holds the pointer gate for mouse input and
/// has already run the bounded admission check. `inject_release` does not come through here.
pub(super) fn deliver(inputs: &[INPUT]) -> Result<()> {
    if inputs.is_empty() {
        return Ok(());
    }
    if inputs.iter().any(|input| input.r#type == INPUT_MOUSE) {
        dispatch_pointer(inputs)
    } else {
        send_input(inputs)
    }
}
fn dispatch_pointer(inputs: &[INPUT]) -> Result<()> {
    send_input(inputs)?;
    // Windows applies an injected move to the cursor a moment after `SendInput` returns.
    // Without this wait the probe's unchanged-cursor check, which runs as soon as the gate
    // opens, sees the driver's own move land and reads it as someone else moving the mouse
    // (a deliberate fail-closed Stop latch). Real hardware, 2026-10-22: `cursor changed
    // during probe` latched Stop in about one drag in ten.
    if let Some(target) = last_absolute_target(inputs) {
        // The move is already in the input stream. Timeout or an unreadable cursor must not
        // look like success: the caller stops later button and drag packets, and the ledger
        // keeps this packet because `maybe_executed` is set.
        if let Some(error) = crate::input::pointer_settle::failure(settle_cursor(target)) {
            return Err(error);
        }
    }
    Ok(())
}
/// Longest the gate is held waiting for the cursor to arrive. Well below the probe's 40ms gate wait.
const CURSOR_SETTLE: std::time::Duration = std::time::Duration::from_millis(15);
/// Screen pixel of the last absolute move in the packet (inverse of the normalization in `inputs`).
fn last_absolute_target(inputs: &[INPUT]) -> Option<(i32, i32)> {
    let flags = MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK;
    let last = inputs.iter().rev().find_map(|input| {
        if input.r#type != INPUT_MOUSE {
            return None;
        }
        // SAFETY: the record is a mouse record (checked above).
        let mi = unsafe { input.Anonymous.mi };
        (mi.dwFlags & flags == flags).then_some((mi.dx, mi.dy))
    })?;
    // SAFETY: these four metrics are nonblocking integer queries.
    let (x, y, w, h) = unsafe {
        (
            GetSystemMetrics(SM_XVIRTUALSCREEN),
            GetSystemMetrics(SM_YVIRTUALSCREEN),
            GetSystemMetrics(SM_CXVIRTUALSCREEN),
            GetSystemMetrics(SM_CYVIRTUALSCREEN),
        )
    };
    if w <= 1 || h <= 1 {
        return None;
    }
    Some((
        crate::input::pointer_settle::denormalize_axis(last.0, x, w),
        crate::input::pointer_settle::denormalize_axis(last.1, y, h),
    ))
}
fn settle_cursor(target: (i32, i32)) -> crate::input::pointer_settle::Settle {
    let deadline = std::time::Instant::now() + CURSOR_SETTLE;
    let mut progress = crate::input::pointer_settle::SettleProgress::pending();
    loop {
        let mut point = POINT::default();
        // SAFETY: bounded local output; read-only cursor query.
        let sample = if unsafe { GetPhysicalCursorPos(&mut point) }.is_ok() {
            Ok((point.x, point.y))
        } else {
            Err(())
        };
        progress = crate::input::pointer_settle::fold_sample(progress, sample, target);
        if let crate::input::pointer_settle::SettleProgress::Done(done) = progress {
            return done;
        }
        if std::time::Instant::now() >= deadline {
            return progress.finish();
        }
        std::thread::sleep(std::time::Duration::from_micros(500));
    }
}
/// Release path (Stop, cleanup): never waits on the gate.
pub(super) fn inject_release(inputs: &[INPUT]) -> Result<()> {
    if inputs.is_empty() {
        return Ok(());
    }
    send_input(inputs)
}
fn send_input(inputs: &[INPUT]) -> Result<()> {
    // SAFETY: owned initialized INPUT records, exact cbSize. SendInput is a
    // nonblocking insertion; partial insertion is unknown and never replayed.
    let count = unsafe { SendInput(inputs, std::mem::size_of::<INPUT>() as i32) };
    if count as usize == inputs.len() {
        Ok(())
    } else {
        Err(DriverError::Native {
            message: "SendInput inserted an incomplete packet".into(),
            maybe_executed: count != 0,
        })
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use crate::input::api::Event;

    /// (virtual key, scan/unit, is_unicode, is_key_up) for each keyboard record.
    fn keys(events: &[Event]) -> Vec<(u16, u16, bool, bool)> {
        inputs(events)
            .unwrap()
            .iter()
            .map(|input| {
                // SAFETY: every record built from these events is a keyboard record.
                let ki = unsafe { input.Anonymous.ki };
                (
                    ki.wVk.0,
                    ki.wScan,
                    ki.dwFlags.contains(KEYEVENTF_UNICODE),
                    ki.dwFlags.contains(KEYEVENTF_KEYUP),
                )
            })
            .collect()
    }

    #[test]
    fn the_settle_target_is_the_last_absolute_move_in_the_packet() {
        let events = [
            Event::Move(gcu_core::model::WindowPoint { x: 100., y: 100. }, false),
            Event::Button(
                Button::Left,
                true,
                gcu_core::model::WindowPoint { x: 100., y: 100. },
                false,
            ),
        ];
        let target = last_absolute_target(&inputs(&events).unwrap()).expect("a move is present");
        // Arrival is the inverse of the normalization formula, not the requested pixel.
        // Round-trip may differ from (100, 100); a one-pixel neighbor is not arrived.
        assert!(
            crate::input::pointer_settle::cursor_arrived(target, target),
            "{target:?}"
        );
        assert!(
            !crate::input::pointer_settle::cursor_arrived((target.0 + 1, target.1), target),
            "{target:?}"
        );
        let buttons = [Event::Button(
            Button::Left,
            false,
            gcu_core::model::WindowPoint { x: 0., y: 0. },
            false,
        )];
        assert_eq!(last_absolute_target(&inputs(&buttons).unwrap()), None);
    }

    #[test]
    fn foreground_newline_is_an_enter_key_and_crlf_is_one_enter() {
        // Real hardware (2026-10-21): U+000A sent as VK_PACKET reached no text field; Enter does.
        let lf = [Event::Unicode(10, true), Event::Unicode(10, false)];
        assert_eq!(
            keys(&lf),
            vec![(0x0d, 0, false, false), (0x0d, 0, false, true)]
        );
        let crlf = [
            Event::Unicode(13, true),
            Event::Unicode(13, false),
            Event::Unicode(10, true),
            Event::Unicode(10, false),
        ];
        assert_eq!(
            keys(&crlf),
            vec![(0x0d, 0, false, false), (0x0d, 0, false, true)]
        );
    }

    #[test]
    fn other_units_stay_unicode_including_surrogate_halves() {
        let text = [
            Event::Unicode(0x4e2d, true),
            Event::Unicode(0x4e2d, false),
            Event::Unicode(0xd83d, true),
            Event::Unicode(0xd83d, false),
        ];
        assert_eq!(
            keys(&text),
            vec![
                (0, 0x4e2d, true, false),
                (0, 0x4e2d, true, true),
                (0, 0xd83d, true, false),
                (0, 0xd83d, true, true),
            ]
        );
    }
}
