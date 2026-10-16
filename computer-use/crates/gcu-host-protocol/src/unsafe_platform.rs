//! All descriptor ownership and native process metadata operations.
use std::io;

#[cfg(target_os = "linux")]
pub(crate) fn started(pid: u32) -> io::Result<u64> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat"))?;
    let fields = stat
        .rsplit_once(')')
        .ok_or_else(|| io::Error::other("invalid process metadata"))?
        .1;
    fields
        .split_whitespace()
        .nth(19)
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| io::Error::other("invalid process start time"))
}
#[cfg(target_os = "macos")]
pub(crate) fn started(pid: u32) -> io::Result<u64> {
    let mut info = std::mem::MaybeUninit::<libc::proc_bsdinfo>::uninit();
    // SAFETY: writable output buffer is sized for PROC_PIDTBSDINFO; no pointers escape.
    let n = unsafe {
        libc::proc_pidinfo(
            pid as i32,
            libc::PROC_PIDTBSDINFO,
            0,
            info.as_mut_ptr().cast(),
            std::mem::size_of::<libc::proc_bsdinfo>() as i32,
        )
    };
    if n as usize != std::mem::size_of::<libc::proc_bsdinfo>() {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: successful proc_pidinfo filled the complete struct above.
    let info = unsafe { info.assume_init() };
    Ok(info
        .pbi_start_tvsec
        .saturating_mul(1_000_000)
        .saturating_add(info.pbi_start_tvusec))
}
#[cfg(windows)]
pub(crate) fn started(pid: u32) -> io::Result<u64> {
    use windows::Win32::{
        Foundation::{CloseHandle, FILETIME},
        System::Threading::{GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION},
    };
    // SAFETY: query-only handle; successful acquisition is closed below on every path.
    let h = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }
        .map_err(io::Error::other)?;
    let mut times = [FILETIME::default(); 4];
    let [creation, exit, kernel, user] = &mut times;
    // SAFETY: live process handle and four writable outputs with correct sizes.
    let result =
        unsafe { GetProcessTimes(h, creation, exit, kernel, user) }.map_err(io::Error::other);
    // SAFETY: h is owned by this function and is no longer used.
    let _ = unsafe { CloseHandle(h) };
    result?;
    Ok((u64::from(times[0].dwHighDateTime) << 32) | u64::from(times[0].dwLowDateTime))
}
#[cfg(unix)]
pub(crate) fn duplicate_socket(fd: usize) -> io::Result<tokio::net::UnixStream> {
    let stream = std::os::unix::net::UnixStream::from(duplicate_descriptor(fd)?);
    stream.set_nonblocking(true)?;
    tokio::net::UnixStream::from_std(stream)
}
/// Import an inherited duplex Windows named-pipe client handle. The App creates
/// the pair before launch, with an owner-only ACL; the child receives no name.
#[cfg(windows)]
pub(crate) fn duplicate_socket(
    fd: usize,
) -> io::Result<tokio::net::windows::named_pipe::NamedPipeClient> {
    if fd == 0 || fd == usize::MAX {
        return Err(io::Error::from(io::ErrorKind::InvalidInput));
    }
    use std::os::windows::io::IntoRawHandle;
    let handle = duplicate_handle(fd)?;
    // SAFETY: the owned duplicate is a pipe handle from the trusted launcher;
    // tokio assumes ownership and validates registration with IOCP.
    unsafe {
        tokio::net::windows::named_pipe::NamedPipeClient::from_raw_handle(handle.into_raw_handle())
    }
}
#[cfg(unix)]
pub(crate) fn duplicate_file(fd: usize) -> io::Result<std::fs::File> {
    Ok(std::fs::File::from(duplicate_descriptor(fd)?))
}
#[cfg(unix)]
fn duplicate_descriptor(fd: usize) -> io::Result<std::os::fd::OwnedFd> {
    if fd > i32::MAX as usize {
        return Err(io::Error::from(io::ErrorKind::InvalidInput));
    }
    use std::os::fd::FromRawFd;
    // SAFETY: fcntl validates the integer before changing its descriptor flags.
    let flags = unsafe { libc::fcntl(fd as i32, libc::F_GETFD) };
    if flags < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: preserve the validated descriptor flags while preventing its
    // inherited original from leaking into any engine-launched subprocess.
    if unsafe { libc::fcntl(fd as i32, libc::F_SETFD, flags | libc::FD_CLOEXEC) } < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: fcntl accepts an arbitrary integer and rejects closed/nonexistent
    // descriptors. No BorrowedFd is constructed before the OS validates it.
    let duplicate = unsafe { libc::fcntl(fd as i32, libc::F_DUPFD_CLOEXEC, 0) };
    if duplicate < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: successful fcntl returned a new owned live descriptor.
    Ok(unsafe { std::os::fd::OwnedFd::from_raw_fd(duplicate) })
}
#[cfg(windows)]
pub(crate) fn duplicate_file(fd: usize) -> io::Result<std::fs::File> {
    Ok(std::fs::File::from(duplicate_handle(fd)?))
}
#[cfg(windows)]
fn duplicate_handle(fd: usize) -> io::Result<std::os::windows::io::OwnedHandle> {
    if fd == 0 || fd == usize::MAX {
        return Err(io::Error::from(io::ErrorKind::InvalidInput));
    }
    use std::os::windows::io::FromRawHandle;
    use windows::Win32::{
        Foundation::{
            DuplicateHandle, SetHandleInformation, DUPLICATE_SAME_ACCESS, HANDLE, HANDLE_FLAGS,
            HANDLE_FLAG_INHERIT,
        },
        System::Threading::GetCurrentProcess,
    };
    let mut duplicate = HANDLE::default();
    // SAFETY: the OS validates the raw input; clearing inheritance prevents the
    // launcher's original handle from leaking into engine-launched subprocesses.
    unsafe { SetHandleInformation(HANDLE(fd as *mut _), HANDLE_FLAG_INHERIT.0, HANDLE_FLAGS(0)) }
        .map_err(io::Error::other)?;
    // SAFETY: current-process handles are valid; DuplicateHandle checks the raw
    // input and writes a new owned handle. It is deliberately not inheritable.
    unsafe {
        let process = GetCurrentProcess();
        DuplicateHandle(
            process,
            HANDLE(fd as *mut _),
            process,
            &mut duplicate,
            0,
            false,
            DUPLICATE_SAME_ACCESS,
        )
    }
    .map_err(io::Error::other)?;
    // SAFETY: successful DuplicateHandle returned a live independently owned handle.
    Ok(unsafe { std::os::windows::io::OwnedHandle::from_raw_handle(duplicate.0) })
}
#[cfg(unix)]
pub(crate) fn uid() -> u32 {
    // SAFETY: geteuid has no memory/argument preconditions.
    unsafe { libc::geteuid() }
}
#[cfg(unix)]
pub(crate) fn parent() -> u32 {
    // SAFETY: getppid has no memory/argument preconditions.
    unsafe { libc::getppid() as u32 }
}
#[cfg(windows)]
pub(crate) fn parent() -> u32 {
    0
} // Windows inherited capability and proof bind the launcher; no self-reported parent is trusted.
#[cfg(windows)]
pub(crate) fn pipe_descriptor(
) -> io::Result<interprocess::os::windows::security_descriptor::SecurityDescriptor> {
    use interprocess::os::windows::security_descriptor::{
        AsSecurityDescriptorExt, BorrowedSecurityDescriptor,
    };
    use windows::{
        core::PCWSTR,
        Win32::{
            Foundation::{LocalFree, HLOCAL},
            Security::{
                Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW,
                PSECURITY_DESCRIPTOR,
            },
        },
    };
    let wide: Vec<u16> = format!("D:P(A;;GA;;;{})", current_user_sid()?)
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let mut sd = PSECURITY_DESCRIPTOR::default();
    // SAFETY: terminated UTF-16 input and writable output; API allocates sd.
    unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            PCWSTR(wide.as_ptr()),
            1,
            &mut sd,
            None,
        )
    }
    .map_err(io::Error::other)?;
    // SAFETY: successful API returned a live valid security descriptor.
    let result = unsafe { BorrowedSecurityDescriptor::from_ptr(sd.0) }.to_owned_sd();
    // SAFETY: sd belongs to LocalAlloc; the returned descriptor owns a deep copy.
    unsafe {
        LocalFree(Some(HLOCAL(sd.0)));
    }
    result
}

#[cfg(unix)]
pub(crate) fn inherit(command: &mut std::process::Command, fds: Vec<i32>) {
    use std::os::unix::process::CommandExt;
    // SAFETY: the post-fork hook only invokes async-signal-safe fcntl. The owned
    // descriptors outlive spawn; no allocation, locks or shared state is used.
    unsafe {
        command.pre_exec(move || {
            for fd in &fds {
                if libc::fcntl(*fd, libc::F_SETFD, 0) < 0 {
                    return Err(io::Error::last_os_error());
                }
            }
            Ok(())
        });
    }
}
#[cfg(unix)]
pub(crate) fn nonblocking(file: &std::fs::File) -> io::Result<()> {
    use std::os::fd::AsRawFd;
    // SAFETY: borrowed live fd; fcntl only reads/sets flags on that descriptor.
    let flags = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETFL) };
    if flags < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: same live borrowed fd, preserving all other status flags.
    if unsafe { libc::fcntl(file.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}
#[cfg(windows)]
pub(crate) fn pipe_available(file: &std::fs::File) -> io::Result<u32> {
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::{Foundation::HANDLE, System::Pipes::PeekNamedPipe};
    let mut available = 0;
    // SAFETY: live inherited pipe handle, no data read, correctly sized output.
    unsafe {
        PeekNamedPipe(
            HANDLE(file.as_raw_handle()),
            None,
            0,
            None,
            Some(&mut available),
            None,
        )
    }
    .map_err(io::Error::other)?;
    Ok(available)
}
#[cfg(windows)]
fn current_user_sid() -> io::Result<String> {
    use windows::{
        core::PWSTR,
        Win32::{
            Foundation::{CloseHandle, LocalFree, HANDLE, HLOCAL},
            Security::{
                Authorization::ConvertSidToStringSidW, GetTokenInformation, TokenUser, TOKEN_QUERY,
                TOKEN_USER,
            },
            System::Threading::{GetCurrentProcess, OpenProcessToken},
        },
    };
    let mut token = HANDLE::default();
    // SAFETY: current process pseudo-handle is valid; token output is writable.
    unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) }
        .map_err(io::Error::other)?;
    let result = (|| {
        let mut size = 0;
        // SAFETY: zero-size query obtains the required token buffer size.
        let _ = unsafe { GetTokenInformation(token, TokenUser, None, 0, &mut size) };
        if size == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut storage = vec![0u64; (size as usize).div_ceil(8)];
        // SAFETY: aligned writable buffer holds at least size bytes.
        unsafe {
            GetTokenInformation(
                token,
                TokenUser,
                Some(storage.as_mut_ptr().cast()),
                size,
                &mut size,
            )
        }
        .map_err(io::Error::other)?;
        // SAFETY: successful query initialized a TOKEN_USER at the aligned start.
        let user = unsafe { &*storage.as_ptr().cast::<TOKEN_USER>() };
        let mut text = PWSTR::null();
        // SAFETY: token SID remains alive in storage, output pointer is writable.
        unsafe { ConvertSidToStringSidW(user.User.Sid, &mut text) }.map_err(io::Error::other)?;
        // SAFETY: successful conversion returns terminated allocated UTF-16.
        let result = unsafe { text.to_string() }.map_err(io::Error::other);
        // SAFETY: conversion uses LocalAlloc and returned ownership to us.
        unsafe {
            LocalFree(Some(HLOCAL(text.0.cast())));
        }
        result
    })();
    // SAFETY: query-only token handle is owned and no longer used.
    let _ = unsafe { CloseHandle(token) };
    result
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::fd::AsRawFd;
    #[test]
    fn inherited_secret_original_and_duplicate_cannot_leak_to_descendants() {
        let (original, _other) = std::os::unix::net::UnixStream::pair().unwrap();
        // SAFETY: the owned socket is live and fcntl only changes descriptor flags.
        assert_eq!(
            unsafe { libc::fcntl(original.as_raw_fd(), libc::F_SETFD, 0) },
            0
        );
        let duplicate = duplicate_file(original.as_raw_fd() as usize).unwrap();
        for fd in [original.as_raw_fd(), duplicate.as_raw_fd()] {
            // SAFETY: both owned descriptors remain live throughout the check.
            assert_ne!(
                unsafe { libc::fcntl(fd, libc::F_GETFD) } & libc::FD_CLOEXEC,
                0
            );
        }
    }
}
