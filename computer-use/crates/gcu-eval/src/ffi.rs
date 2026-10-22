//! The only foreign calls in this crate.

/// Deliver SIGKILL to the process group whose id is `pid`.
///
/// `pid` must still be the live or zombie leader of a group this process
/// created. Callers drop the id after `wait` so a recycled pgid is not signaled.
/// Zero is ignored so this cannot signal the evaluator's own group.
#[cfg(unix)]
pub(crate) fn kill_process_group(pid: u32) {
    let Some(pgid) = i32::try_from(pid).ok().filter(|value| *value > 0) else {
        return;
    };
    // SAFETY: `kill` with a negative pid signals that process group and no
    // other. The id is the child we created, not zero and not this process.
    // SIGKILL does not share memory with the caller. Callers must not pass a
    // pid they have already reaped.
    let _ = unsafe { libc::kill(-pgid, libc::SIGKILL) };
}

/// Effective uid of this process.
#[cfg(unix)]
pub(crate) fn current_euid() -> u32 {
    // SAFETY: `geteuid` reads a process credential and does not touch caller memory.
    unsafe { libc::geteuid() }
}

/// Password-database home for the effective uid.
///
/// This is the OS account home, not the `HOME` environment variable. `None`
/// when the lookup fails. The returned path is an owned copy.
#[cfg(unix)]
pub(crate) fn os_home_dir() -> Option<std::path::PathBuf> {
    let uid = current_euid();
    let mut buf = vec![0u8; 16 * 1024];
    let mut pwd = std::mem::MaybeUninit::<libc::passwd>::zeroed();
    let mut result: *mut libc::passwd = std::ptr::null_mut();
    // SAFETY: `pwd` and `buf` are valid for the call. On success, `pw_dir`
    // points into `buf`. The C string is copied before `buf` is dropped.
    // `getpwuid_r` does not retain those pointers.
    let rc = unsafe {
        libc::getpwuid_r(
            uid,
            pwd.as_mut_ptr(),
            buf.as_mut_ptr().cast::<libc::c_char>(),
            buf.len(),
            &mut result,
        )
    };
    if rc != 0 || result.is_null() {
        return None;
    }
    // SAFETY: a zero `rc` and a non-null `result` mean `pwd` was fully written.
    let pwd = unsafe { pwd.assume_init() };
    if pwd.pw_dir.is_null() {
        return None;
    }
    // SAFETY: `pw_dir` is a NUL-terminated string inside `buf` for this call.
    let dir = unsafe { std::ffi::CStr::from_ptr(pwd.pw_dir) };
    Some(std::path::PathBuf::from(dir.to_string_lossy().into_owned()))
}

/// Windows profile directory from the known-folder API, not from `USERPROFILE`.
#[cfg(windows)]
pub(crate) fn os_home_dir() -> Option<std::path::PathBuf> {
    #[repr(C)]
    struct Guid {
        data1: u32,
        data2: u16,
        data3: u16,
        data4: [u8; 8],
    }
    // FOLDERID_Profile
    const PROFILE: Guid = Guid {
        data1: 0x5E6C_858F,
        data2: 0x0E22,
        data3: 0x4760,
        data4: [0x9A, 0xFE, 0xEA, 0x33, 0x17, 0xB6, 0x71, 0x73],
    };
    #[link(name = "shell32")]
    extern "system" {
        fn SHGetKnownFolderPath(
            rfid: *const Guid,
            flags: u32,
            token: *mut core::ffi::c_void,
            path: *mut *mut u16,
        ) -> i32;
    }
    #[link(name = "ole32")]
    extern "system" {
        fn CoTaskMemFree(ptr: *mut core::ffi::c_void);
    }
    let mut raw: *mut u16 = std::ptr::null_mut();
    // SAFETY: `PROFILE` is a static GUID. A null token asks for the current
    // user. On success `raw` is a CoTaskMemAlloc UTF-16 string we free below.
    let status = unsafe { SHGetKnownFolderPath(&PROFILE, 0, std::ptr::null_mut(), &mut raw) };
    if status != 0 || raw.is_null() {
        return None;
    }
    let mut len = 0usize;
    // SAFETY: `raw` is NUL-terminated. The scan stops at the terminator.
    while unsafe { *raw.add(len) } != 0 {
        len += 1;
        if len > 32_768 {
            // SAFETY: the pointer came from SHGetKnownFolderPath.
            unsafe { CoTaskMemFree(raw.cast()) };
            return None;
        }
    }
    // SAFETY: `len` wide units were read and are initialized.
    let slice = unsafe { std::slice::from_raw_parts(raw, len) };
    let path = String::from_utf16_lossy(slice);
    // SAFETY: `raw` is the allocation from this call and is not used again.
    unsafe { CoTaskMemFree(raw.cast()) };
    Some(std::path::PathBuf::from(path))
}

/// Hard-link count of an open file, from the same handle the caller will read.
///
/// `1` means the file has no extra hard link. This uses
/// `GetFileInformationByHandle` because `MetadataExt::number_of_links` is an
/// unstable `windows_by_handle` API on the Windows toolchain.
#[cfg(windows)]
pub(crate) fn file_link_count(file: &std::fs::File) -> std::io::Result<u32> {
    use std::os::windows::io::AsRawHandle;
    // Fields exist for the ABI layout. Only `number_of_links` is read.
    #[repr(C)]
    #[allow(dead_code)]
    struct Filetime {
        low: u32,
        high: u32,
    }
    #[repr(C)]
    #[allow(dead_code)]
    struct ByHandleFileInformation {
        file_attributes: u32,
        creation_time: Filetime,
        last_access_time: Filetime,
        last_write_time: Filetime,
        volume_serial_number: u32,
        file_size_high: u32,
        file_size_low: u32,
        number_of_links: u32,
        file_index_high: u32,
        file_index_low: u32,
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn GetFileInformationByHandle(
            file: *mut core::ffi::c_void,
            info: *mut ByHandleFileInformation,
        ) -> i32;
    }
    let mut info = std::mem::MaybeUninit::<ByHandleFileInformation>::uninit();
    // SAFETY: `file` is an open handle owned by the caller. `info` is writable
    // for this call. On success the struct is fully written. The call does
    // not retain either pointer.
    let ok = unsafe { GetFileInformationByHandle(file.as_raw_handle(), info.as_mut_ptr()) };
    if ok == 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: a non-zero BOOL means `GetFileInformationByHandle` initialized `info`.
    let info = unsafe { info.assume_init() };
    Ok(info.number_of_links)
}

/// Fill `buf` from the system RNG. Failure is an error; there is no weak fallback.
#[cfg(windows)]
pub(crate) fn random_bytes(buf: &mut [u8]) -> Result<(), crate::Error> {
    #[link(name = "bcrypt")]
    extern "system" {
        fn BCryptGenRandom(
            algorithm: *mut core::ffi::c_void,
            buffer: *mut u8,
            length: u32,
            flags: u32,
        ) -> i32;
    }
    const USE_SYSTEM_PREFERRED_RNG: u32 = 0x0000_0002;
    let length =
        u32::try_from(buf.len()).map_err(|_| crate::Error::msg("could not read randomness"))?;
    // SAFETY: a null algorithm plus BCRYPT_USE_SYSTEM_PREFERRED_RNG writes
    // exactly `length` bytes into `buf` and does not retain the pointer.
    let status = unsafe {
        BCryptGenRandom(
            core::ptr::null_mut(),
            buf.as_mut_ptr(),
            length,
            USE_SYSTEM_PREFERRED_RNG,
        )
    };
    if status == 0 {
        Ok(())
    } else {
        Err(crate::Error::msg("could not read randomness"))
    }
}
