//! Exact standard BUTTON activation without UIA Invoke/Legacy or SetFocus.
use super::*;
use windows::Win32::{
    Foundation::{HWND, LPARAM, WPARAM},
    UI::WindowsAndMessaging::*,
};

pub(super) fn post(el: &IUIAutomationElement, r: &Guard<'_>) -> Prepared<PatternResult> {
    let recipient = read!(r, el.CurrentNativeWindowHandle());
    let root = HWND(r.hwnd as usize as *mut _);
    let mut class = [0u16; 256];
    // SAFETY: read-only borrowed HWNDs; bounded writable class buffer. Virtual
    // controls are handled by the ledger-backed targeted route in Observer.
    let valid = unsafe {
        let length = GetClassNameW(recipient, &mut class).max(0) as usize;
        !recipient.0.is_null()
            && GetAncestor(recipient, GA_ROOT) == root
            && String::from_utf16_lossy(&class[..length]).eq_ignore_ascii_case("Button")
    };
    if !valid {
        return Err(DriverError::CapabilityUnavailable.into());
    }
    Ok(r.dispatch(|| {
        // SAFETY: exact live BUTTON HWND/ancestry was checked; Guard revalidates
        // UIA identity/security immediately before this pointer-free single post.
        // BM_CLICK represents one complete control action, with no held input.
        unsafe { PostMessageW(Some(recipient), BM_CLICK, WPARAM(0), LPARAM(0)) }
            .map(|()| PatternResult::TargetedPerformed)
            .map_err(|e| DriverError::Native {
                message: format!("PostMessage(BM_CLICK): {e}"),
                maybe_executed: false,
            })
    })?)
}
