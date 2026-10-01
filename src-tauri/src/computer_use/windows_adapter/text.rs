//! Native text effects; the caller retains its NativeActionSlot until return.
use super::native_target::NativeTarget;
use super::*;
use windows::Win32::Foundation::{GetLastError, SetLastError, ERROR_SUCCESS};
use windows::Win32::UI::WindowsAndMessaging::{
    GetClassNameW, GetWindowLongPtrW, ES_PASSWORD, ES_READONLY, GWL_STYLE, WM_GETTEXT,
    WM_GETTEXTLENGTH,
};

const EM_GETSEL: u32 = 0x00B0;
const EM_SETSEL: u32 = 0x00B1;
const EM_REPLACESEL: u32 = 0x00C2;
const WM_PASTE: u32 = 0x0302;
// Bound existing-control readback independently of the 4,000-character request
// limit. WM_GETTEXT on Rich Edit is documented only through 64K characters.
const MAX_CONTROL_UNITS: usize = 1024 * 1024;

fn edit_limit(hwnd: HWND) -> Result<usize, String> {
    if !unsafe { IsWindow(Some(hwnd)) }.as_bool() {
        return Err("native text control is no longer alive".into());
    }
    let mut class = [0u16; 64];
    let n = unsafe { GetClassNameW(hwnd, &mut class) };
    let class = String::from_utf16_lossy(&class[..n.max(0) as usize]);
    match class.to_ascii_lowercase().as_str() {
        "edit" => Ok(MAX_CONTROL_UNITS),
        "richedit" | "richedit20a" | "richedit20w" | "richedit50w" => Ok(65535),
        _ => Err("native text target is not a supported edit control".into()),
    }
}

fn writable(hwnd: HWND) -> Result<(), String> {
    edit_limit(hwnd)?;
    let style = unsafe { GetWindowLongPtrW(hwnd, GWL_STYLE) } as u32;
    if style & (ES_PASSWORD as u32 | ES_READONLY as u32) != 0 {
        return Err("native text control is password-protected or read-only".into());
    }
    Ok(())
}

fn validate_request(text: &str) -> Result<(), String> {
    if text.contains('\0') || text.chars().count() > super::super::protocol::TEXT_MAX_CHARS {
        return Err("invalid native text request".into());
    }
    Ok(())
}

fn verify(target: &NativeTarget, expected: &str) -> Result<(), String> {
    if read_bound(target)? == expected {
        Ok(())
    } else {
        // The effect may have partially happened. Never replay or substitute a
        // second input route, and never leak field contents in diagnostics.
        Err("native text effect is unverified; input was not replayed".into())
    }
}

pub(super) fn set_value(
    target: &NativeTarget,
    text: &str,
    check: &dyn Fn() -> Result<(), String>,
) -> Result<(), String> {
    validate_request(text)?;
    check()?;
    target.check()?;
    let hwnd = target.hwnd();
    writable(hwnd)?;
    let wide: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
    unsafe {
        let _ = SetFocus(Some(hwnd));
        check()?;
        target.check()?;
        writable(hwnd)?;
        if SendMessageW(hwnd, WM_SETTEXT, None, Some(LPARAM(wide.as_ptr() as isize))).0 == 0 {
            return Err("native set-value was rejected; input was not replayed".into());
        }
    }
    verify(target, text)
}

#[cfg(feature = "computer-use-probe")]
pub(super) fn read(hwnd: HWND) -> Result<String, String> {
    read_bound(&NativeTarget::capture(hwnd)?)
}

fn read_bound(target: &NativeTarget) -> Result<String, String> {
    target.check()?;
    let hwnd = target.hwnd();
    let limit = edit_limit(hwnd)?;
    // GetWindowTextW cannot read EDIT values in another process. These system
    // messages marshal the buffer across processes. Keep the call synchronous:
    // a timeout must not free a buffer still used by a native window procedure.
    let length = unsafe {
        SetLastError(ERROR_SUCCESS);
        let length = SendMessageW(hwnd, WM_GETTEXTLENGTH, None, None).0;
        if GetLastError() != ERROR_SUCCESS {
            return Err("native text length read failed".into());
        }
        length
    };
    target.check()?;
    let length = usize::try_from(length).map_err(|_| "invalid native text length")?;
    if length > limit {
        return Err("native text readback exceeds its bounded capacity".into());
    }
    // One extra code unit distinguishes a complete result from racing growth
    // that exactly fills the originally advertised capacity. DBCS estimates
    // may overestimate: trust the checked copied count, not equality to length.
    let mut buf = vec![u16::MAX; length + 2];
    let copied = unsafe {
        SetLastError(ERROR_SUCCESS);
        let copied = SendMessageW(
            hwnd,
            WM_GETTEXT,
            Some(WPARAM(buf.len())),
            Some(LPARAM(buf.as_mut_ptr() as isize)),
        )
        .0;
        if GetLastError() != ERROR_SUCCESS || !IsWindow(Some(hwnd)).as_bool() {
            return Err("native text readback failed".into());
        }
        copied
    };
    target.check()?;
    let n = usize::try_from(copied).map_err(|_| "invalid native text copied count")?;
    if n > length || buf[n] != 0 || buf[..n].contains(&0) {
        return Err("native text readback is incomplete".into());
    }
    String::from_utf16(&buf[..n]).map_err(|_| "native text readback is invalid UTF-16".into())
}

pub(super) fn type_text(
    target: &NativeTarget,
    text: &str,
    via_clipboard: bool,
    check: &dyn Fn() -> Result<(), String>,
) -> Result<(), String> {
    validate_request(text)?;
    check()?;
    target.check()?;
    let child = target.hwnd();
    let check = &|| {
        check()?;
        target.check()
    };
    writable(child)?;
    if text.is_empty() {
        return Ok(());
    }
    let wide: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
    unsafe {
        let _ = SetFocus(Some(child));
    }
    check()?;
    let before = read_bound(target)?;
    let end = before.encode_utf16().count();
    if end + wide.len() - 1 > edit_limit(child)? {
        return Err("native append exceeds bounded verification capacity".into());
    }
    let expected = format!("{before}{text}");
    check()?;
    unsafe {
        SendMessageW(
            child,
            EM_SETSEL,
            Some(WPARAM(end)),
            Some(LPARAM(end as isize)),
        );
    }
    target.check()?;
    let (mut start, mut finish) = (u32::MAX, u32::MAX);
    unsafe {
        SendMessageW(
            child,
            EM_GETSEL,
            Some(WPARAM(&mut start as *mut u32 as usize)),
            Some(LPARAM(&mut finish as *mut u32 as isize)),
        );
    }
    target.check()?;
    if start as usize != end || finish as usize != end || read_bound(target)? != before {
        return Err("native edit changed before append; input was not dispatched".into());
    }
    if via_clipboard {
        super::super::windows_clipboard::with_task_text(text, check, || {
            check()?;
            writable(child)?;
            unsafe { SendMessageW(child, WM_PASTE, None, None) };
            Ok(())
        })?;
    } else {
        check()?;
        writable(child)?;
        unsafe {
            SendMessageW(
                child,
                EM_REPLACESEL,
                Some(WPARAM(1)),
                Some(LPARAM(wide.as_ptr() as isize)),
            )
        };
    }
    verify(target, &expected)
}
