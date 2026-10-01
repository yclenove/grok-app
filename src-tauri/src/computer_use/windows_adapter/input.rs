use super::*;

/// Revalidate the original request immediately before every new input effect.
/// Finishing an already-sent down/up pair is cleanup, not new authorization.
pub(super) struct InputGate<'a> {
    pub adapter: &'a WindowsAdapter,
    pub request: &'a DispatchRequest,
    pub uncertain: &'a Cell<bool>,
}

impl InputGate<'_> {
    fn bind(&self, hwnd: HWND) -> Result<super::native_target::NativeTarget, String> {
        self.check()?;
        super::native_target::NativeTarget::authorized(hwnd, &self.request.target_id)
    }

    pub(super) fn check(&self) -> Result<(), String> {
        self.request.admit(self.adapter)?;
        if !self
            .adapter
            .foreground_input_available(&self.request.target_id)
        {
            return Err("focus drifted; foreground input paused".into());
        }
        if user_input_held() {
            return Err("user-held input; run paused".into());
        }
        self.request.cancellation.check()
    }
}

pub(super) fn user_input_held() -> bool {
    use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
    // High bit is physically down; the low toggle bit must not reject CapsLock
    // or NumLock. Never release these keys on the user's behalf.
    (1..=254).any(|key| unsafe { GetAsyncKeyState(key) } < 0)
}

pub(super) fn pack_lparam(x: i32, y: i32) -> LPARAM {
    LPARAM((((y as u32) << 16) | (x as u32 & 0xffff)) as isize)
}

fn send_child_click(
    gate: &InputGate<'_>,
    target: &super::native_target::NativeTarget,
    lp: LPARAM,
    button: &str,
) -> Result<(), String> {
    let (down, up, flags) = match button {
        "right" => (WM_RBUTTONDOWN, WM_RBUTTONUP, 0x0002),
        "middle" => (WM_MBUTTONDOWN, WM_MBUTTONUP, 0x0010),
        _ => (WM_LBUTTONDOWN, WM_LBUTTONUP, 0x0001),
    };
    gate.check()?;
    target.send_pair(
        (down, WPARAM(flags), lp),
        (up, WPARAM(0), lp),
        gate.uncertain,
    )
}

pub(super) fn send_click_client(
    gate: &InputGate<'_>,
    hwnd: HWND,
    x: f64,
    y: f64,
    button: &str,
    count: u32,
) -> Result<(), String> {
    ensure_dpi_aware();
    gate.check()?;
    let cx = x.round() as i32;
    let cy = y.round() as i32;
    let client = POINT { x: cx, y: cy };
    let child = unsafe { ChildWindowFromPoint(hwnd, client) };
    let repeats = count.clamp(1, 2);
    if !child.0.is_null() && child != hwnd {
        let target = gate.bind(child)?;
        let mut cpt = client;
        unsafe {
            MapWindowPoints(Some(hwnd), Some(child), std::slice::from_mut(&mut cpt));
        }
        let lp = pack_lparam(cpt.x, cpt.y);
        for _ in 0..repeats {
            send_child_click(gate, &target, lp, button)?;
        }
        return Ok(());
    }
    let target = gate.bind(hwnd)?;
    let mut screen = client;
    let mapped = unsafe {
        if !ClientToScreen(hwnd, &mut screen).as_bool() {
            Err("ClientToScreen failed".to_string())
        } else {
            gate.check()?;
            target.check()?;
            SetCursorPos(screen.x, screen.y).map_err(|e| e.to_string())
        }
    };
    if let Err(e) = mapped {
        return Err(format!(
            "{e} client=({x},{y}) screen=({},{})",
            screen.x, screen.y
        ));
    }
    let mut result = Ok(());
    for _ in 0..repeats {
        gate.check()?;
        target.check()?;
        result = send_click_at_cursor(gate, button);
        if result.is_err() {
            break;
        }
    }
    result.map_err(|e| format!("{e} client=({x},{y}) screen=({},{})", screen.x, screen.y))
}

pub(super) fn vk_from_params(parameters: &serde_json::Value) -> Result<u16, String> {
    let key = parameters
        .get("key")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "key required".to_string())?;
    Ok(match super::super::protocol::normalize_key(key)? {
        "enter" => 0x0D,
        "tab" => 0x09,
        "escape" => 0x1B,
        "space" => 0x20,
        "down" => 0x28,
        "up" => 0x26,
        "left" => 0x25,
        "right" => 0x27,
        "backspace" => 0x08,
        "delete" => 0x2E,
        "home" => 0x24,
        "end" => 0x23,
        "pageup" => 0x21,
        "pagedown" => 0x22,
        other => return Err(format!("key is not in the allowed set: {other}")),
    })
}

pub(super) fn send_key(gate: &InputGate<'_>, target: HWND, vk: u16) -> Result<(), String> {
    let bound = gate.bind(target)?;
    unsafe {
        let _ = SetFocus(Some(target));
    }
    gate.check()?;
    bound.send_pair(
        (WM_KEYDOWN, WPARAM(vk as usize), LPARAM(1)),
        (WM_KEYUP, WPARAM(vk as usize), LPARAM(0xC0000001)),
        gate.uncertain,
    )
}

pub(super) fn send_scroll_client(
    gate: &InputGate<'_>,
    hwnd: HWND,
    x: f64,
    y: f64,
    delta: super::super::protocol::ScrollDelta,
) -> Result<(), String> {
    gate.check()?;
    let cx = x.round() as i32;
    let cy = y.round() as i32;
    let client = POINT { x: cx, y: cy };
    let child = unsafe { ChildWindowFromPoint(hwnd, client) };
    let target = if !child.0.is_null() { child } else { hwnd };
    let bound = gate.bind(target)?;
    let mut screen = client;
    unsafe {
        if !ClientToScreen(hwnd, &mut screen).as_bool() {
            return Err("ClientToScreen failed".into());
        }
    }
    // WM_MOUSEWHEEL positive is UP, opposite the down-positive wire contract.
    let wp = WPARAM((((delta.native_positive_up() as u32) << 16) & 0xffff_0000) as usize);
    gate.check()?;
    bound.check()?;
    unsafe {
        let _ = SendMessageW(
            target,
            WM_MOUSEWHEEL,
            Some(wp),
            Some(pack_lparam(screen.x, screen.y)),
        );
    }
    // A directed wheel message is one action, not message + scrollbar command
    // + a second global wheel injection. Never replay on an unverified effect.
    bound.check()
}

pub(super) fn send_drag_client(
    gate: &InputGate<'_>,
    hwnd: HWND,
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
) -> Result<(), String> {
    let bound = gate.bind(hwnd)?;
    let p0 = POINT {
        x: x0.round() as i32,
        y: y0.round() as i32,
    };
    let p1 = POINT {
        x: x1.round() as i32,
        y: y1.round() as i32,
    };
    gate.check()?;
    bound.check()?;
    unsafe {
        let _ = SendMessageW(
            hwnd,
            WM_LBUTTONDOWN,
            Some(WPARAM(0x0001)),
            Some(pack_lparam(p0.x, p0.y)),
        );
        let continuation = gate.check().and_then(|()| bound.check());
        if continuation.is_ok() {
            let _ = SendMessageW(
                hwnd,
                WM_MOUSEMOVE,
                Some(WPARAM(0x0001)),
                Some(pack_lparam(p1.x, p1.y)),
            );
        }
        // Release only the directed down we sent, even if Stop arrived while
        // the target's synchronous handler was running.
        let release_at = if continuation.is_ok() { p1 } else { p0 };
        bound.release_owned(
            (
                WM_LBUTTONUP,
                WPARAM(0),
                pack_lparam(release_at.x, release_at.y),
            ),
            gate.uncertain,
        )?;
        continuation?;
    }
    bound.check()
}

pub(super) fn resolve_client_point(
    hwnd: HWND,
    pid: u32,
    stamp: u64,
    target: &ActionTarget,
) -> Result<(f64, f64), String> {
    match target {
        ActionTarget::Coord { x, y } => Ok((*x, *y)),
        ActionTarget::Element { element_ref } => {
            let resolved = super::super::windows_uia::resolve(hwnd, pid, stamp, element_ref)?;
            Ok((resolved.x, resolved.y))
        }
    }
}

pub(super) fn resolve_child(
    hwnd: HWND,
    pid: u32,
    stamp: u64,
    target: &ActionTarget,
) -> Result<HWND, String> {
    match target {
        ActionTarget::Element { element_ref } => {
            super::super::windows_uia::resolve(hwnd, pid, stamp, element_ref)?
                .native
                .filter(|h| !h.0.is_null())
                .ok_or_else(|| "semantic target has no native input control".into())
        }
        ActionTarget::Coord { x, y } => {
            let child = unsafe {
                ChildWindowFromPoint(
                    hwnd,
                    POINT {
                        x: x.round() as i32,
                        y: y.round() as i32,
                    },
                )
            };
            if child.0.is_null() {
                Err("coordinate has no native input control".into())
            } else {
                Ok(child)
            }
        }
    }
}

pub(super) fn set_child_text(gate: &InputGate<'_>, hwnd: HWND, text: &str) -> Result<(), String> {
    let target = gate.bind(hwnd)?;
    super::text::set_value(&target, text, &|| gate.check())
}

pub(super) fn type_into_child(
    gate: &InputGate<'_>,
    child: HWND,
    text: &str,
    via_clipboard: bool,
) -> Result<(), String> {
    let target = gate.bind(child)?;
    super::text::type_text(&target, text, via_clipboard, &|| gate.check())
}

pub(super) fn send_click_at_cursor(gate: &InputGate<'_>, button: &str) -> Result<(), String> {
    let (down, up) = match button {
        "right" => (MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP),
        "middle" => (MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP),
        _ => (MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP),
    };
    let mouse = |flags| INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx: 0,
                dy: 0,
                mouseData: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    let inputs = [mouse(down), mouse(up)];
    let n = unsafe { SendInput(&inputs, size_of::<INPUT>() as i32) };
    finish_click_pair(n, gate.uncertain, || unsafe {
        SendInput(&[mouse(up)], size_of::<INPUT>() as i32)
    })
}

fn finish_click_pair(
    n: u32,
    uncertain: &Cell<bool>,
    release_owned: impl FnOnce() -> u32,
) -> Result<(), String> {
    match n {
        2 => Ok(()),
        1 => {
            // SendInput inserts the supplied events in order. Only our down
            // was accepted: release that button, never all global buttons.
            let released = release_owned();
            if released == 1 {
                Err("partial native click; owned button released; action was not replayed".into())
            } else {
                uncertain.set(true);
                Err("native input cleanup unconfirmed".into())
            }
        }
        _ => Err("SendInput failed".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn complete_or_rejected_pairs_never_release_unowned_buttons() {
        for sent in [0, 2] {
            let uncertain = Cell::new(false);
            let result = finish_click_pair(sent, &uncertain, || panic!("no owned down to release"));
            assert_eq!(result.is_ok(), sent == 2);
            assert!(!uncertain.get());
        }
    }

    #[test]
    fn partial_pair_cleans_only_its_down_once_and_never_reports_success() {
        let calls = Cell::new(0);
        let uncertain = Cell::new(false);
        assert!(finish_click_pair(1, &uncertain, || {
            calls.set(calls.get() + 1);
            1
        })
        .is_err());
        assert_eq!(calls.get(), 1);
        assert!(!uncertain.get());
    }

    #[test]
    fn unconfirmed_owned_release_is_separate_from_error_wording() {
        let uncertain = Cell::new(false);
        let result =
            finish_click_pair(1, &uncertain, || 0).map_err(|e| format!("native call: {e}"));
        assert!(result.is_err());
        assert!(uncertain.get());
    }
}
