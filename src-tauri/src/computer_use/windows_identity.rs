//! HWND identity stamps and host-window exclusion.
//!
//! `target_id` is `win:{pid}:{hwnd_hex}:{stamp}`. Stamp is a per-HWND property
//! so a recycled handle cannot satisfy an old id. Host (`grok-app`) windows
//! and explicitly marked surfaces are never listed or acted on.

#![cfg(target_os = "windows")]

use std::sync::atomic::{AtomicU64, Ordering};

use windows::core::{w, PWSTR};
use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND};
use windows::Win32::Security::{
    GetSidSubAuthority, GetSidSubAuthorityCount, GetTokenInformation, TokenElevation,
    TokenIntegrityLevel, TOKEN_ELEVATION, TOKEN_MANDATORY_LABEL, TOKEN_QUERY,
};
use windows::Win32::System::StationsAndDesktops::{
    CloseDesktop, OpenInputDesktop, DESKTOP_CONTROL_FLAGS, DESKTOP_SWITCHDESKTOP,
};
use windows::Win32::System::Threading::{
    GetCurrentProcess, OpenProcess, OpenProcessToken, QueryFullProcessImageNameW,
    PROCESS_NAME_FORMAT, PROCESS_QUERY_LIMITED_INFORMATION,
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
    format!("win:{pid}:{:x}:{stamp}", hwnd.0 as usize)
}

pub fn parse_target_id(target_id: &str) -> Option<(u32, HWND, u64)> {
    let rest = target_id.strip_prefix("win:")?;
    let mut parts = rest.split(':');
    let pid: u32 = parts.next()?.parse().ok()?;
    let hwnd_u = usize::from_str_radix(parts.next()?, 16).ok()?;
    let stamp: u64 = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some((pid, HWND(hwnd_u as *mut _), stamp))
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
