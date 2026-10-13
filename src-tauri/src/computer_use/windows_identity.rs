//! HWND identity stamps and host-window exclusion.
//!
//! `target_id` is `win:{pid}:{hwnd_hex}:{stamp}:{process_birth}`. A live process
//! creation time and per-HWND property fence PID and window reuse separately.
//! These checks do not make Win32 callbacks atomic. Host (`grok-app`) windows
//! and explicitly marked surfaces are never listed or acted on.

#![cfg(target_os = "windows")]

use std::sync::atomic::{AtomicU64, Ordering};

use windows::core::{w, PWSTR};
use windows::Win32::Foundation::{CloseHandle, FILETIME, HANDLE, HWND, WAIT_TIMEOUT};
use windows::Win32::Security::{
    GetSidSubAuthority, GetSidSubAuthorityCount, GetTokenInformation, TokenElevation,
    TokenIntegrityLevel, TOKEN_ELEVATION, TOKEN_MANDATORY_LABEL, TOKEN_QUERY,
};
use windows::Win32::System::StationsAndDesktops::{
    CloseDesktop, OpenInputDesktop, DESKTOP_CONTROL_FLAGS, DESKTOP_SWITCHDESKTOP,
};
use windows::Win32::System::Threading::{
    GetCurrentProcess, GetProcessTimes, OpenProcess, OpenProcessToken, QueryFullProcessImageNameW,
    WaitForSingleObject, PROCESS_NAME_FORMAT, PROCESS_QUERY_LIMITED_INFORMATION,
    PROCESS_SYNCHRONIZE,
};
use windows::Win32::UI::WindowsAndMessaging::{GetPropW, RemovePropW, SetPropW};

const PROP_STAMP: windows::core::PCWSTR = w!("GrokCuStamp");
const PROP_PROTECTED: windows::core::PCWSTR = w!("GrokCuProtected");

static STAMP: AtomicU64 = AtomicU64::new(1);

fn handle_from_u64(v: u64) -> HANDLE {
    HANDLE(v as usize as *mut core::ffi::c_void)
}

fn u64_from_handle(h: HANDLE) -> u64 {
    h.0 as usize as u64
}

pub fn stamp_window(hwnd: HWND) -> u64 {
    unsafe {
        let existing = GetPropW(hwnd, PROP_STAMP);
        if !existing.is_invalid() && !existing.0.is_null() {
            return u64_from_handle(existing);
        }
        let stamp = STAMP.fetch_add(1, Ordering::Relaxed);
        let _ = SetPropW(hwnd, PROP_STAMP, Some(handle_from_u64(stamp)));
        stamp
    }
}

pub fn read_stamp(hwnd: HWND) -> Option<u64> {
    unsafe {
        let h = GetPropW(hwnd, PROP_STAMP);
        if h.is_invalid() || h.0.is_null() {
            None
        } else {
            Some(u64_from_handle(h))
        }
    }
}

#[cfg(feature = "computer-use-probe")]
pub fn clear_stamp(hwnd: HWND) {
    unsafe {
        let _ = RemovePropW(hwnd, PROP_STAMP);
    }
}

#[cfg(feature = "computer-use-probe")]
pub fn mark_protected(hwnd: HWND) {
    unsafe {
        let _ = SetPropW(hwnd, PROP_PROTECTED, Some(handle_from_u64(1)));
    }
}

pub fn is_marked_protected(hwnd: HWND) -> bool {
    unsafe {
        let h = GetPropW(hwnd, PROP_PROTECTED);
        !h.is_invalid() && !h.0.is_null()
    }
}

pub fn format_target_id(pid: u32, hwnd: HWND, stamp: u64) -> String {
    process_birth(pid)
        .map(|birth| format!("win:{pid}:{:x}:{stamp}:{birth}", hwnd.0 as usize))
        .unwrap_or_default()
}

#[cfg(any(test, feature = "computer-use-probe"))]
pub fn parse_target_id(target_id: &str) -> Option<(u32, HWND, u64)> {
    parse_target_identity(target_id).map(|(pid, hwnd, stamp, _)| (pid, hwnd, stamp))
}

/// Decode without renewing authority. Execution must separately match birth.
pub(super) fn parse_target_identity(target_id: &str) -> Option<(u32, HWND, u64, u64)> {
    let rest = target_id.strip_prefix("win:")?;
    let mut parts = rest.split(':');
    let pid: u32 = parts.next()?.parse().ok()?;
    let hwnd_u = usize::from_str_radix(parts.next()?, 16).ok()?;
    let stamp: u64 = parts.next()?.parse().ok()?;
    let birth: u64 = parts.next()?.parse().ok()?;
    if pid == 0 || hwnd_u == 0 || stamp == 0 || birth == 0 || parts.next().is_some() {
        return None;
    }
    Some((pid, HWND(hwnd_u as *mut _), stamp, birth))
}

/// Query one exact live process. Inaccessible/exited processes have no witness;
/// names and PID alone are never a fallback. Every opened handle is closed.
pub(super) fn process_birth(pid: u32) -> Option<u64> {
    if pid == 0 {
        return None;
    }
    unsafe {
        let process = OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
            false,
            pid,
        )
        .ok()?;
        let mut creation = FILETIME::default();
        let mut exit = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        let read = GetProcessTimes(process, &mut creation, &mut exit, &mut kernel, &mut user);
        let live = WaitForSingleObject(process, 0) == WAIT_TIMEOUT;
        let _ = CloseHandle(process);
        read.ok()?;
        let birth = (u64::from(creation.dwHighDateTime) << 32) | u64::from(creation.dwLowDateTime);
        (live && birth != 0).then_some(birth)
    }
}

pub fn process_image_stem(pid: u32) -> Option<String> {
    unsafe {
        let proc = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 520];
        let mut size = buf.len() as u32;
        let result = QueryFullProcessImageNameW(
            proc,
            PROCESS_NAME_FORMAT(0),
            PWSTR(buf.as_mut_ptr()),
            &mut size,
        );
        let _ = CloseHandle(proc);
        result.ok()?;
        let path = String::from_utf16_lossy(&buf[..size as usize]);
        std::path::Path::new(&path)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
    }
}

pub fn is_host_process(pid: u32) -> bool {
    process_image_stem(pid)
        .map(|stem| stem.eq_ignore_ascii_case("grok-app"))
        .unwrap_or(false)
}

pub fn is_protected_window(hwnd: HWND, pid: u32) -> bool {
    is_marked_protected(hwnd) || is_host_process(pid)
}

fn token_is_elevated(token: HANDLE) -> bool {
    unsafe {
        let mut elev = TOKEN_ELEVATION::default();
        let mut ret = 0u32;
        GetTokenInformation(
            token,
            TokenElevation,
            Some(std::ptr::from_mut(&mut elev).cast()),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut ret,
        )
        .ok()
        .map(|_| elev.TokenIsElevated != 0)
        .unwrap_or(false)
    }
}

fn process_is_elevated(process: HANDLE) -> Option<bool> {
    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(process, TOKEN_QUERY, &mut token).ok()?;
        let elevated = token_is_elevated(token);
        let _ = CloseHandle(token);
        Some(elevated)
    }
}

fn process_integrity_rid(process: HANDLE) -> Option<u32> {
    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(process, TOKEN_QUERY, &mut token).ok()?;
        let mut needed = 0u32;
        let _ = GetTokenInformation(token, TokenIntegrityLevel, None, 0, &mut needed);
        if needed == 0 {
            let _ = CloseHandle(token);
            return None;
        }
        let mut buf = vec![0u8; needed as usize];
        if GetTokenInformation(
            token,
            TokenIntegrityLevel,
            Some(buf.as_mut_ptr().cast()),
            needed,
            &mut needed,
        )
        .is_err()
        {
            let _ = CloseHandle(token);
            return None;
        }
        let label = buf.as_ptr().cast::<TOKEN_MANDATORY_LABEL>();
        let sid = (*label).Label.Sid;
        if sid.0.is_null() {
            let _ = CloseHandle(token);
            return None;
        }
        let count_ptr = GetSidSubAuthorityCount(sid);
        if count_ptr.is_null() || *count_ptr == 0 {
            let _ = CloseHandle(token);
            return None;
        }
        let rid_ptr = GetSidSubAuthority(sid, u32::from(*count_ptr) - 1);
        if rid_ptr.is_null() {
            let _ = CloseHandle(token);
            return None;
        }
        let rid = *rid_ptr;
        let _ = CloseHandle(token);
        Some(rid)
    }
}

pub fn current_process_is_elevated() -> bool {
    unsafe { process_is_elevated(GetCurrentProcess()) }.unwrap_or(false)
}

/// Fail closed on UIPI: inaccessible or higher-integrity targets are not controllable.
/// Never used to bypass UAC or enable UIAccess.
pub fn can_control_process(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    if pid == std::process::id() {
        return true;
    }
    unsafe {
        let Ok(proc) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
            return false;
        };
        let theirs = process_integrity_rid(proc);
        let ours = process_integrity_rid(GetCurrentProcess());
        let allowed = match (theirs, ours) {
            (Some(t), Some(o)) => t <= o,
            _ => match process_is_elevated(proc) {
                Some(true) if !current_process_is_elevated() => false,
                Some(_) => true,
                None => false,
            },
        };
        let _ = CloseHandle(proc);
        allowed
    }
}

/// False when the Winlogon secure desktop (lock screen) owns input.
pub fn session_input_available() -> bool {
    unsafe {
        match OpenInputDesktop(DESKTOP_CONTROL_FLAGS(0), false, DESKTOP_SWITCHDESKTOP) {
            Ok(h) => {
                let _ = CloseDesktop(h);
                true
            }
            Err(_) => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn process_birth_is_live_stable_and_not_available_for_pid_zero() {
        let before = process_birth(std::process::id()).expect("own live process birth");
        assert_eq!(process_birth(std::process::id()), Some(before));
        assert_eq!(process_birth(0), None);
    }

    #[test]
    fn root_identity_requires_process_and_window_witnesses() {
        assert!(parse_target_id("win:42:ab:3").is_none());
        for invalid in [
            "win:0:ab:3:4",
            "win:42:0:3:4",
            "win:42:ab:0:4",
            "win:42:ab:3:0",
            "win:42:ab:3:4:extra",
            "win:42:ab:3:bad",
        ] {
            assert!(parse_target_id(invalid).is_none(), "{invalid}");
        }
        let (pid, hwnd, stamp, birth) = parse_target_identity("win:42:ab:3:4").unwrap();
        assert_eq!((pid, hwnd.0 as usize, stamp, birth), (42, 0xab, 3, 4));
    }
}
