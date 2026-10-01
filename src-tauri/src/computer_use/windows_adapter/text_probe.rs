//! Low-level production text helpers against owned native EDIT controls only.
//! Standalone probe: never invoked in the App or on an interactive desktop.
use super::text;
use crate::computer_use::windows_clipboard::probe::IsolatedStation;
use std::cell::Cell;
use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::*;
mod identity;
mod peer;
pub(in crate::computer_use) use peer::run_peer;

struct State {
    original: WNDPROC,
    reject: Cell<u32>,
    effects: Cell<usize>,
    read_fault: Cell<u8>,
    alive: Cell<bool>,
    retire_on: Cell<u32>,
    reparent_on_down: Cell<Option<HWND>>,
    destroy_on_down: Cell<bool>,
    down: Cell<usize>,
    up: Cell<usize>,
}
struct Edit {
    hwnd: HWND,
    state: Box<State>,
}
unsafe extern "system" fn procedure(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    let p = GetWindowLongPtrW(h, GWLP_USERDATA) as *const State;
    if p.is_null() {
        return DefWindowProcW(h, m, w, l);
    }
    let state = &*p;
    if m == WM_NCDESTROY {
        state.alive.set(false);
    }
    if matches!(m, WM_KEYDOWN | WM_LBUTTONDOWN) {
        state.down.set(state.down.get() + 1);
        if state.destroy_on_down.replace(false) {
            let _ = DestroyWindow(h);
            return LRESULT(0);
        }
        if let Some(parent) = state.reparent_on_down.take() {
            let _ = SetParent(h, Some(parent));
        }
    }
    if matches!(m, WM_KEYUP | WM_LBUTTONUP) {
        state.up.set(state.up.get() + 1);
    }
    if state.retire_on.get() == m {
        state.retire_on.set(0);
        crate::computer_use::windows_identity::clear_stamp(h);
        crate::computer_use::windows_identity::stamp_window(h);
    }
    if m == WM_GETTEXTLENGTH {
        match state.read_fault.get() {
            1 => return LRESULT(CallWindowProcW(state.original, h, m, w, l).0 + 64),
            2 => return LRESULT(0),
            3 => return LRESULT(1024 * 1024 + 1),
            _ => {}
        }
    }
    if m == WM_GETTEXT {
        match state.read_fault.get() {
            4 => {
                if w.0 >= 2 {
                    *(l.0 as *mut u16) = 0xD800;
                    *((l.0 as *mut u16).add(1)) = 0;
                }
                return LRESULT(1);
            }
            5 => return LRESULT(isize::MAX),
            6 => return LRESULT(0),
            _ => {}
        }
    }
    if matches!(m, WM_SETTEXT | 0x00C2 | 0x0302) {
        state.effects.set(state.effects.get() + 1);
    }
    if state.reject.get() == m {
        return LRESULT(0);
    }
    CallWindowProcW(state.original, h, m, w, l)
}
impl Edit {
    fn target(&self) -> Result<super::native_target::NativeTarget, String> {
        super::native_target::NativeTarget::capture(self.hwnd)
    }

    fn new(initial: &str) -> Result<Self, String> {
        Self::with_parent(initial, None)
    }
    fn with_parent(initial: &str, parent: Option<HWND>) -> Result<Self, String> {
        let value: Vec<u16> = initial.encode_utf16().chain(Some(0)).collect();
        let hwnd = unsafe {
            CreateWindowExW(
                Default::default(),
                w!("EDIT"),
                windows::core::PCWSTR(value.as_ptr()),
                (if parent.is_some() { WS_CHILD } else { WS_POPUP })
                    | WINDOW_STYLE(ES_MULTILINE as u32 | ES_AUTOVSCROLL as u32),
                0,
                0,
                200,
                80,
                parent,
                None,
                None,
                None,
            )
        }
        .map_err(|e| e.to_string())?;
        let state = Box::new(State {
            original: unsafe {
                std::mem::transmute::<isize, WNDPROC>(GetWindowLongPtrW(hwnd, GWLP_WNDPROC))
            },
            reject: Cell::new(0),
            effects: Cell::new(0),
            read_fault: Cell::new(0),
            alive: Cell::new(true),
            retire_on: Cell::new(0),
            reparent_on_down: Cell::new(None),
            destroy_on_down: Cell::new(false),
            down: Cell::new(0),
            up: Cell::new(0),
        });
        unsafe {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, &*state as *const State as isize);
            SetWindowLongPtrW(hwnd, GWLP_WNDPROC, procedure as *const () as isize);
        }
        Ok(Self { hwnd, state })
    }
    fn assert_value(&self, expected: &str) -> Result<(), String> {
        // Independent oracle, not the production reader or verifier.
        let mut buffer = vec![0u16; 20_000];
        let copied = unsafe {
            SendMessageW(
                self.hwnd,
                WM_GETTEXT,
                Some(WPARAM(buffer.len())),
                Some(LPARAM(buffer.as_mut_ptr() as isize)),
            )
            .0
        };
        if copied < 0
            || copied as usize >= buffer.len()
            || String::from_utf16(&buffer[..copied as usize])
                .ok()
                .as_deref()
                != Some(expected)
        {
            return Err("native EDIT oracle mismatch (text omitted)".into());
        }
        Ok(())
    }
}
impl Drop for Edit {
    fn drop(&mut self) {
        if self.state.alive.get() {
            unsafe {
                let _ = DestroyWindow(self.hwnd);
            }
        }
    }
}

pub fn run() -> Result<(), String> {
    let _isolation = IsolatedStation::new()?;
    let mut failures = Vec::new();
    let mut run = |name: &str, test: &dyn Fn() -> Result<(), String>| match test() {
        Ok(()) => println!("gate: {name}: PASS"),
        Err(e) => {
            eprintln!("gate: {name}: FAIL {e}");
            failures.push(name.to_string());
        }
    };
    run("long UTF-16 text readback", &|| {
        let expected = "界🙂".repeat(1800);
        let edit = Edit::new(&expected)?;
        if text::read(edit.hwnd)? != expected {
            return Err("full value was truncated".into());
        }
        edit.assert_value(&expected)
    });
    run("type appends at explicit end", &|| {
        let edit = Edit::new("prefix")?;
        unsafe {
            SendMessageW(edit.hwnd, 0x00B1, Some(WPARAM(0)), Some(LPARAM(0)));
        }
        text::type_text(&edit.target()?, "追加🙂", false, &|| Ok(()))?;
        edit.assert_value("prefix追加🙂")
    });
    for (name, message, clipboard) in [
        ("ignored insert is not success", 0x00C2, false),
        ("ignored paste is not success", 0x0302, true),
    ] {
        run(name, &|| {
            let edit = Edit::new("already-present")?;
            edit.state.reject.set(message);
            let result = text::type_text(&edit.target()?, "present", clipboard, &|| Ok(()));
            edit.assert_value("already-present")?;
            if result.is_ok() || edit.state.effects.get() != 1 {
                return Err("ignored write accepted or replayed".into());
            }
            Ok(())
        });
    }
    run("ignored set-value is not success", &|| {
        let edit = Edit::new("original")?;
        edit.state.reject.set(WM_SETTEXT);
        let result = text::set_value(&edit.target()?, "replacement", &|| Ok(()));
        edit.assert_value("original")?;
        if result.is_ok() || edit.state.effects.get() != 1 {
            return Err("ignored set-value accepted or replayed".into());
        }
        Ok(())
    });
    run("successful set-value and empty replacement", &|| {
        let edit = Edit::new("old")?;
        text::set_value(&edit.target()?, "new🙂", &|| Ok(()))?;
        edit.assert_value("new🙂")?;
        text::set_value(&edit.target()?, "", &|| Ok(()))?;
        edit.assert_value("")?;
        if edit.state.effects.get() != 2 {
            return Err("set-value was replayed".into());
        }
        Ok(())
    });
    run("long append and paste execute exactly once", &|| {
        for clipboard in [false, true] {
            let before = "界🙂".repeat(1800);
            let suffix = "追加🙂".repeat(1000);
            let edit = Edit::new(&before)?;
            text::type_text(&edit.target()?, &suffix, clipboard, &|| Ok(()))?;
            edit.assert_value(&format!("{before}{suffix}"))?;
            if edit.state.effects.get() != 1 {
                return Err("append was replayed".into());
            }
        }
        Ok(())
    });
    run("empty append sends no input", &|| {
        let edit = Edit::new("unchanged")?;
        for clipboard in [false, true] {
            text::type_text(&edit.target()?, "", clipboard, &|| Ok(()))?;
        }
        edit.assert_value("unchanged")?;
        if edit.state.effects.get() != 0 {
            return Err("empty append dispatched input".into());
        }
        Ok(())
    });
    run("pre-write cancellation sends no text", &|| {
        for canceled_check in [1, 4] {
            let edit = Edit::new("old")?;
            let checks = Cell::new(0);
            let result = text::type_text(&edit.target()?, "new", false, &|| {
                checks.set(checks.get() + 1);
                if checks.get() == canceled_check {
                    Err("canceled".into())
                } else {
                    Ok(())
                }
            });
            edit.assert_value("old")?;
            if result.is_ok() || edit.state.effects.get() != 0 {
                return Err("canceled text dispatched".into());
            }
        }
        Ok(())
    });
    run("read-only controls and invalid text refuse input", &|| {
        let edit = Edit::new("old")?;
        unsafe {
            SendMessageW(edit.hwnd, 0x00CF, Some(WPARAM(1)), None);
        }
        if text::set_value(&edit.target()?, "new", &|| Ok(())).is_ok()
            || text::type_text(&edit.target()?, "new", false, &|| Ok(())).is_ok()
            || text::type_text(&edit.target()?, "x\0y", false, &|| Ok(())).is_ok()
            || edit.state.effects.get() != 0
        {
            return Err("read-only or invalid input accepted".into());
        }
        edit.assert_value("old")
    });
    run("partial edit result is never replayed", &|| {
        let edit = Edit::new("prefix")?;
        unsafe {
            SendMessageW(edit.hwnd, 0x00C5, Some(WPARAM(12)), None);
        }
        if text::type_text(&edit.target()?, "abcdefghijklmnop", false, &|| Ok(())).is_ok()
            || edit.state.effects.get() != 1
        {
            return Err("partial edit accepted or replayed".into());
        }
        edit.assert_value("prefixabcdef")
    });
    run(
        "overestimated length is accepted; malformed reads refused",
        &|| {
            let edit = Edit::new("old")?;
            edit.state.read_fault.set(1);
            if text::read(edit.hwnd)? != "old" {
                return Err("length overestimate rejected".into());
            }
            for fault in 2..=6 {
                edit.state.read_fault.set(fault);
                if text::read(edit.hwnd).is_ok() {
                    return Err(format!("invalid native read {fault} accepted"));
                }
            }
            Ok(())
        },
    );
    identity::run(&mut run);
    run(
        "cross-process edit and private clipboard",
        &peer::run_parent,
    );
    if failures.is_empty() {
        Ok(())
    } else {
        Err(format!("failed groups: {}", failures.join(", ")))
    }
}
