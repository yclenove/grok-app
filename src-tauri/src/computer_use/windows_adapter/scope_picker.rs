//! Bounded, explicit Host candidate discovery. Never activates or captures a window.
use super::super::adapter::{ScopeCandidate, ScopeCandidatePage};
use super::*;

const MAX_SCANNED_WINDOWS: usize = 1024;

struct PickerState<'a> {
    adapter: &'a WindowsAdapter,
    is_current: &'a (dyn Fn() -> bool + Send + Sync),
    limit: usize,
    scanned: usize,
    candidates: Vec<ScopeCandidate>,
    truncated: bool,
    cancelled: bool,
}

pub(super) fn discover(
    adapter: &WindowsAdapter,
    limit: usize,
    is_current: &(dyn Fn() -> bool + Send + Sync),
) -> Result<ScopeCandidatePage, String> {
    if limit == 0 || limit > grok_computer_use_core::broker::MAX_SCOPE_CANDIDATES {
        return Err("invalid Windows picker bound".into());
    }
    if !is_current() || !adapter.input_available() {
        return Err("Windows picker request retired or desktop unavailable".into());
    }
    let mut state = PickerState {
        adapter,
        is_current,
        limit,
        scanned: 0,
        candidates: Vec::new(),
        truncated: false,
        cancelled: false,
    };
    let result = unsafe {
        EnumWindows(
            Some(picker_proc),
            LPARAM(&mut state as *mut PickerState<'_> as isize),
        )
    };
    if state.cancelled || !is_current() || !adapter.input_available() {
        return Err("Windows picker request retired or desktop unavailable".into());
    }
    if !state.truncated {
        result.map_err(|_| "Windows picker enumeration failed")?;
    }
    Ok(ScopeCandidatePage {
        candidates: state.candidates,
        truncated: state.truncated,
    })
}

unsafe extern "system" fn picker_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let state = &mut *(lparam.0 as *mut PickerState<'_>);
    if !(state.is_current)() || !state.adapter.input_available() {
        state.cancelled = true;
        return false.into();
    }
    if state.candidates.len() >= state.limit || state.scanned >= MAX_SCANNED_WINDOWS {
        state.truncated = true;
        return false.into();
    }
    state.scanned += 1;
    if let Ok(candidate) = candidate(state, hwnd) {
        state.candidates.push(candidate);
    }
    if !(state.is_current)() {
        state.cancelled = true;
        return false.into();
    }
    true.into()
}

fn candidate(state: &PickerState<'_>, hwnd: HWND) -> Result<ScopeCandidate, String> {
    let check = || {
        if (state.is_current)() && state.adapter.input_available() {
            Ok(())
        } else {
            Err("Windows picker request retired".to_string())
        }
    };
    check()?;
    if !unsafe { IsWindowVisible(hwnd) }.as_bool()
        || unsafe { IsIconic(hwnd) }.as_bool()
        || unsafe { GetAncestor(hwnd, GA_ROOT) } != hwnd
    {
        return Err("Windows picker target hidden or not a root".into());
    }
    let mut pid = 0;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    check()?;
    if pid == 0
        || super::super::windows_identity::is_protected_window(hwnd, pid)
        || !super::super::windows_identity::can_control_process(pid)
    {
        return Err("Windows picker target inaccessible".into());
    }
    let stamp = super::super::windows_identity::stamp_window(hwnd);
    let target = super::super::windows_identity::format_target_id(pid, hwnd, stamp);
    let identity = state.adapter.scope_identity(&target)?;
    check()?;
    // Exact metadata performs native instance checks around title reads; never
    // enumerate again, follow the foreground window or query an AX tree.
    let info = state.adapter.scope_target_info("", &target)?;
    check()?;
    if state.adapter.scope_identity(&target)? != identity {
        return Err("Windows picker target instance changed".into());
    }
    check()?;
    Ok(ScopeCandidate { info, identity })
}
