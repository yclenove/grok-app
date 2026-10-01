//! Real owned HWNDs; stamp retirement is not a claim of forced HWND recycling.
use super::*;
use crate::computer_use::windows_adapter::native_target::NativeTarget;
use crate::computer_use::windows_identity::{clear_stamp, format_target_id, stamp_window};

type GateRunner<'a> = dyn FnMut(&str, &dyn Fn() -> Result<(), String>) + 'a;

fn mutate(edit: &Edit, mode: u8, check: &dyn Fn() -> Result<(), String>) -> Result<(), String> {
    match mode {
        0 => text::set_value(&edit.target()?, "new", check),
        _ => text::type_text(&edit.target()?, "new", mode == 2, check),
    }
}

fn retire(hwnd: HWND) {
    clear_stamp(hwnd);
    stamp_window(hwnd);
}

fn pair(edit: &Edit, uncertain: &Cell<bool>) -> Result<(), String> {
    NativeTarget::capture(edit.hwnd)?.send_pair(
        (WM_KEYDOWN, WPARAM(0x23), LPARAM(1)),
        (WM_KEYUP, WPARAM(0x23), LPARAM(0xC0000001)),
        uncertain,
    )
}

pub(super) fn run(run: &mut GateRunner<'_>) {
    run(
        "retired control identity cannot accept pending text",
        &|| {
            for mode in 0..3 {
                let edit = Edit::new("old")?;
                stamp_window(edit.hwnd);
                let checks = Cell::new(0);
                let result = mutate(&edit, mode, &|| {
                    checks.set(checks.get() + 1);
                    if checks.get() == 2 {
                        retire(edit.hwnd);
                    }
                    Ok(())
                });
                if result.is_ok() || edit.state.effects.get() != 0 {
                    return Err("pending write followed a retired control identity".into());
                }
                edit.assert_value("old")?;
            }
            Ok(())
        },
    );
    run("reparented control cannot accept pending text", &|| {
        for mode in 0..3 {
            let parent = Edit::new("root one")?;
            let replacement = Edit::new("root two")?;
            let edit = Edit::with_parent("old", Some(parent.hwnd))?;
            let checks = Cell::new(0);
            let result = mutate(&edit, mode, &|| {
                checks.set(checks.get() + 1);
                if checks.get() == 2 {
                    unsafe { SetParent(edit.hwnd, Some(replacement.hwnd)) }
                        .map_err(|e| e.to_string())?;
                }
                Ok(())
            });
            if result.is_ok() || edit.state.effects.get() != 0 {
                return Err("pending write followed a control into another root".into());
            }
            edit.assert_value("old")?;
        }
        Ok(())
    });
    run("root retirement fences every native text route", &|| {
        for mode in 0..3 {
            let parent = Edit::new("root")?;
            let edit = Edit::with_parent("old", Some(parent.hwnd))?;
            let checks = Cell::new(0);
            let result = mutate(&edit, mode, &|| {
                checks.set(checks.get() + 1);
                if checks.get() == 2 {
                    retire(parent.hwnd);
                }
                Ok(())
            });
            if result.is_ok() || edit.state.effects.get() != 0 {
                return Err("retired root admitted a text effect".into());
            }
            edit.assert_value("old")?;
        }
        Ok(())
    });
    run(
        "destruction does not transfer pending text to a fresh control",
        &|| {
            for mode in 0..3 {
                let edit = Edit::new("old")?;
                let replacement = Edit::new("replacement")?;
                let checks = Cell::new(0);
                let result = mutate(&edit, mode, &|| {
                    checks.set(checks.get() + 1);
                    if checks.get() == 2 {
                        unsafe { DestroyWindow(edit.hwnd) }.map_err(|e| e.to_string())?;
                    }
                    Ok(())
                });
                if result.is_ok()
                    || edit.state.alive.get()
                    || edit.state.effects.get() != 0
                    || replacement.state.effects.get() != 0
                {
                    return Err("destroyed control accepted or redirected text".into());
                }
                replacement.assert_value("replacement")?;
            }
            Ok(())
        },
    );
    run(
        "native read and selection callbacks cannot refresh retired identity",
        &|| {
            for message in [WM_GETTEXTLENGTH, WM_GETTEXT, 0x00B1, 0x00B0] {
                for clipboard in [false, true] {
                    let edit = Edit::new("old")?;
                    edit.state.retire_on.set(message);
                    if text::type_text(&edit.target()?, "new", clipboard, &|| Ok(())).is_ok()
                        || edit.state.effects.get() != 0
                    {
                        return Err(format!(
                            "retirement inside message {message} accepted input"
                        ));
                    }
                    edit.assert_value("old")?;
                }
            }
            Ok(())
        },
    );
    run(
        "identity retirement after a write is unknown and never replayed",
        &|| {
            for (mode, message, expected) in [
                (0, WM_SETTEXT, "new"),
                (1, 0x00C2, "oldnew"),
                (2, 0x0302, "oldnew"),
            ] {
                let edit = Edit::new("old")?;
                edit.state.retire_on.set(message);
                if mutate(&edit, mode, &|| Ok(())).is_ok() || edit.state.effects.get() != 1 {
                    return Err("retired post-write identity accepted or replayed".into());
                }
                edit.assert_value(expected)?;
            }
            Ok(())
        },
    );
    run(
        "clipboard restoration survives target retirement before paste",
        &|| {
            let before = crate::computer_use::windows_clipboard::get_text();
            let edit = Edit::new("old")?;
            let checks = Cell::new(0);
            let result = text::type_text(&edit.target()?, "new", true, &|| {
                checks.set(checks.get() + 1);
                if checks.get() == 5 {
                    retire(edit.hwnd);
                }
                Ok(())
            });
            if result.is_ok()
                || checks.get() != 5
                || edit.state.effects.get() != 0
                || crate::computer_use::windows_clipboard::get_text() != before
            {
                return Err("retired paste mutated target or lost private clipboard".into());
            }
            edit.assert_value("old")
        },
    );
    run("child binding requires the exact authorized root", &|| {
        let parent = Edit::new("root")?;
        let wrong = Edit::new("other root")?;
        let edit = Edit::with_parent("child", Some(parent.hwnd))?;
        let id = format_target_id(std::process::id(), parent.hwnd, stamp_window(parent.hwnd));
        let bound = NativeTarget::authorized(edit.hwnd, &id)?;
        let wrong_id = format_target_id(std::process::id(), wrong.hwnd, stamp_window(wrong.hwnd));
        let wrong_pid = format_target_id(0, parent.hwnd, stamp_window(parent.hwnd));
        if NativeTarget::authorized(edit.hwnd, &wrong_id).is_ok()
            || NativeTarget::authorized(edit.hwnd, &wrong_pid).is_ok()
        {
            return Err("foreign authorization accepted".into());
        }
        retire(parent.hwnd);
        if bound.check().is_ok() || NativeTarget::authorized(edit.hwnd, &id).is_ok() {
            return Err("retired root authority was recaptured".into());
        }
        Ok(())
    });
    run(
        "directed down and up address one unchanged native instance",
        &|| {
            let edit = Edit::new("old")?;
            let uncertain = Cell::new(false);
            pair(&edit, &uncertain)?;
            if edit.state.down.get() != 1 || edit.state.up.get() != 1 || uncertain.get() {
                return Err("directed pair did not complete exactly once".into());
            }
            Ok(())
        },
    );
    run(
        "retired live identity receives no cleanup up and retains occupancy",
        &|| {
            let edit = Edit::new("old")?;
            edit.state.retire_on.set(WM_KEYDOWN);
            let uncertain = Cell::new(false);
            if pair(&edit, &uncertain).is_ok()
                || edit.state.down.get() != 1
                || edit.state.up.get() != 0
                || !uncertain.get()
            {
                return Err("cleanup crossed native identity or claimed idle".into());
            }
            Ok(())
        },
    );
    run(
        "reparented original receives only its owned cleanup up",
        &|| {
            let parent = Edit::new("root")?;
            let replacement = Edit::new("other root")?;
            let edit = Edit::with_parent("old", Some(parent.hwnd))?;
            edit.state.reparent_on_down.set(Some(replacement.hwnd));
            let uncertain = Cell::new(false);
            if pair(&edit, &uncertain).is_ok()
                || edit.state.down.get() != 1
                || edit.state.up.get() != 1
                || uncertain.get()
                || unsafe { GetAncestor(edit.hwnd, GA_ROOT) } != replacement.hwnd
            {
                return Err("original directed input was not safely released".into());
            }
            Ok(())
        },
    );
    run(
        "destroyed directed target receives no up or global release",
        &|| {
            let edit = Edit::new("old")?;
            edit.state.destroy_on_down.set(true);
            let uncertain = Cell::new(false);
            if pair(&edit, &uncertain).is_ok()
                || edit.state.alive.get()
                || edit.state.down.get() != 1
                || edit.state.up.get() != 0
                || uncertain.get()
            {
                return Err("destroyed directed input cleanup was misclassified".into());
            }
            Ok(())
        },
    );
}
