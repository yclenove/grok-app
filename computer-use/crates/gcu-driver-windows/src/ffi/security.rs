//! Token integrity, process instance and interactive input-desktop probes.
use super::{native, string, Handle};
use crate::gate::{IntegrityLevel, ProcessSecurity};
use ::windows::{
    core::PWSTR,
    Win32::{
        Foundation::{FILETIME, HANDLE},
        Security::*,
        System::{RemoteDesktop::*, StationsAndDesktops::*, Threading::*},
    },
};
use gcu_core::driver::{DriverError, Result};

fn open_process(pid: u32) -> Result<Handle> {
    // SAFETY: no borrowed pointers; owned handle requests read-only limited rights.
    unsafe {
        OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid)
            .map(Handle)
            .map_err(|_| DriverError::PermissionMissing {
                permission: "process query".into(),
            })
    }
}
fn created(handle: HANDLE) -> Result<u64> {
    let (mut creation, mut exit, mut kernel, mut user) = (
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
    );
    // SAFETY: valid owned process handle and four writable FILETIME outputs.
    unsafe {
        GetProcessTimes(handle, &mut creation, &mut exit, &mut kernel, &mut user)
            .map_err(native)?;
    }
    Ok((u64::from(creation.dwHighDateTime) << 32) | u64::from(creation.dwLowDateTime))
}
pub(crate) fn process_created(pid: u32) -> Result<u64> {
    created(open_process(pid)?.0)
}
pub(super) fn process_path(pid: u32) -> Result<String> {
    let process = open_process(pid)?;
    let mut buffer = vec![0u16; 32768];
    let mut size = buffer.len() as u32;
    // SAFETY: process handle lives through the call; buffer and size are writable.
    unsafe {
        QueryFullProcessImageNameW(
            process.0,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &mut size,
        )
        .map_err(native)?;
    }
    Ok(string(&buffer[..size as usize]))
}
pub(super) fn process_aumid(pid: u32) -> Result<Option<String>> {
    use ::windows::Win32::{
        Foundation::{ERROR_INSUFFICIENT_BUFFER, ERROR_SUCCESS},
        Storage::Packaging::Appx::GetApplicationUserModelId,
    };
    let process = open_process(pid)?;
    let mut length = 0;
    // SAFETY: owned process handle and writable size; no buffer in sizing query.
    let status = unsafe { GetApplicationUserModelId(process.0, &mut length, None) };
    if status == ::windows::Win32::Foundation::APPMODEL_ERROR_NO_APPLICATION {
        return Ok(None);
    }
    if status != ERROR_INSUFFICIENT_BUFFER || length == 0 || length > 32768 {
        return Err(native("process AUMID size"));
    }
    let mut buffer = vec![0u16; length as usize];
    // SAFETY: correctly sized writable buffer; process handle remains live.
    let status = unsafe {
        GetApplicationUserModelId(process.0, &mut length, Some(PWSTR(buffer.as_mut_ptr())))
    };
    if status != ERROR_SUCCESS {
        return Err(native("process AUMID"));
    }
    Ok(Some(string(&buffer)))
}
fn token_info(token: HANDLE, class: TOKEN_INFORMATION_CLASS) -> Result<Vec<usize>> {
    let mut size = 0;
    // SAFETY: first call only obtains required size, so its expected error is checked below.
    unsafe {
        let _ = GetTokenInformation(token, class, None, 0, &mut size);
    }
    if size == 0 || size > 16384 {
        return Err(native("token size"));
    }
    let mut buffer = vec![0usize; (size as usize).div_ceil(std::mem::size_of::<usize>())];
    // SAFETY: pointer-aligned buffer is at least size bytes, token is valid.
    unsafe {
        GetTokenInformation(
            token,
            class,
            Some(buffer.as_mut_ptr().cast()),
            size,
            &mut size,
        )
        .map_err(native)?;
    }
    Ok(buffer)
}
fn secure_desktop() -> bool {
    // SAFETY: read-only desktop access; an inaccessible input desktop is treated as secure.
    let desktop =
        match unsafe { OpenInputDesktop(DESKTOP_CONTROL_FLAGS(0), false, DESKTOP_READOBJECTS) } {
            Ok(d) => d,
            Err(_) => return true,
        };
    let mut name = vec![0u16; 256];
    // SAFETY: valid desktop and fixed writable name buffer, with byte length.
    let result = unsafe {
        GetUserObjectInformationW(
            HANDLE(desktop.0),
            UOI_NAME,
            Some(name.as_mut_ptr().cast()),
            (name.len() * 2) as u32,
            None,
        )
    };
    // SAFETY: owned desktop handle, never used after this close.
    unsafe {
        let _ = CloseDesktop(desktop);
    }
    result.is_err() || !string(&name).eq_ignore_ascii_case("Default")
}
fn interactive(session_id: u32) -> Result<bool> {
    let mut buffer = PWSTR::null();
    let mut bytes = 0;
    // SAFETY: WTS allocates output storage; copied and freed before returning.
    unsafe {
        WTSQuerySessionInformationW(None, session_id, WTSConnectState, &mut buffer, &mut bytes)
            .map_err(native)?;
    }
    let active =
        if bytes >= std::mem::size_of::<WTS_CONNECTSTATE_CLASS>() as u32 && !buffer.is_null() {
            // SAFETY: WTSConnectState returns one aligned WTS_CONNECTSTATE_CLASS.
            unsafe { *buffer.0.cast::<WTS_CONNECTSTATE_CLASS>() == WTSActive }
        } else {
            false
        };
    // SAFETY: WTS allocation freed exactly once, null is accepted.
    unsafe {
        WTSFreeMemory(buffer.0.cast());
    }
    let mut flags = USEROBJECTFLAGS::default();
    // SAFETY: current process station is borrowed; flags buffer is correct size.
    unsafe {
        GetUserObjectInformationW(
            HANDLE(GetProcessWindowStation().map_err(native)?.0),
            UOI_FLAGS,
            Some((&mut flags as *mut USEROBJECTFLAGS).cast()),
            std::mem::size_of::<USEROBJECTFLAGS>() as u32,
            None,
        )
        .map_err(native)?;
    }
    Ok(active
        && flags.dwFlags & ::windows::Win32::UI::WindowsAndMessaging::WSF_VISIBLE as u32 != 0
        && session_id != 0)
}
pub(super) fn security(pid: u32) -> Result<ProcessSecurity> {
    let process = open_process(pid)?;
    let mut token = HANDLE::default();
    // SAFETY: valid process and writable token output; only query rights requested.
    unsafe {
        OpenProcessToken(process.0, TOKEN_QUERY, &mut token).map_err(|_| {
            DriverError::PermissionMissing {
                permission: "process token query".into(),
            }
        })?;
    }
    let token = Handle(token);
    let integrity = token_info(token.0, TokenIntegrityLevel)?;
    // SAFETY: TokenIntegrityLevel returned a pointer-aligned TOKEN_MANDATORY_LABEL,
    // including its SID inside the same live allocation.
    let label = unsafe { &*integrity.as_ptr().cast::<TOKEN_MANDATORY_LABEL>() };
    // SAFETY: the OS supplied a valid SID; count and authority are checked.
    let rid = unsafe {
        if !IsValidSid(label.Label.Sid).as_bool() {
            return Err(native("invalid SID"));
        }
        let count = *GetSidSubAuthorityCount(label.Label.Sid);
        if count == 0 {
            return Err(native("empty SID"));
        }
        *GetSidSubAuthority(label.Label.Sid, u32::from(count - 1))
    };
    let integrity = match rid {
        0..=0xfff => IntegrityLevel::Untrusted,
        0x1000..=0x1fff => IntegrityLevel::Low,
        0x2000..=0x2fff => IntegrityLevel::Medium,
        0x3000..=0x3fff => IntegrityLevel::High,
        _ => IntegrityLevel::System,
    };
    let ui_access = token_info(token.0, TokenUIAccess)?[0] & 0xffff_ffff != 0;
    // Refuse a known elevated engine before desktop/session probes can mask the
    // reason with an unrelated WTS failure (for example in a System service).
    // SAFETY: no pointers/handles; current process ID is nonblocking.
    if pid == unsafe { GetCurrentProcessId() } && (integrity >= IntegrityLevel::High || ui_access) {
        return Err(DriverError::ElevationRequired);
    }
    let mut session_id = 0;
    // SAFETY: writable session-id output, pid checked when opening process.
    unsafe {
        ProcessIdToSessionId(pid, &mut session_id).map_err(native)?;
    }
    Ok(ProcessSecurity {
        integrity,
        ui_access,
        session_id,
        interactive: interactive(session_id)?,
        secure_desktop: secure_desktop(),
        created: created(process.0)?,
    })
}
