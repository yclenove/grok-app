//! Descriptor-relative Unix I/O: no writable operation resolves a named parent.

use std::ffi::{CStr, CString, OsStr};
use std::fs::File;
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::ffi::OsStrExt;

use super::writer::{error, MAX_DIRECTORY_ENTRIES};

fn name(value: &OsStr) -> Result<CString, String> {
    let bytes = value.as_bytes();
    if bytes.is_empty() || bytes == b".." || bytes.contains(&b'/') {
        return Err("telemetry_invalid_path".into());
    }
    CString::new(bytes).map_err(error)
}

fn open_at(parent: &File, name: &CStr, flags: libc::c_int) -> Result<File, String> {
    // SAFETY: parent owns a live fd and name is NUL-terminated. On success
    // ownership of the new fd is transferred exactly once into File.
    let fd = unsafe { libc::openat(parent.as_raw_fd(), name.as_ptr(), flags, 0o600) };
    if fd < 0 {
        return Err(error(std::io::Error::last_os_error()));
    }
    Ok(unsafe { File::from_raw_fd(fd) })
}

pub(super) fn open_directory(parent: &File, child: &OsStr) -> Result<File, String> {
    open_at(
        parent,
        &name(child)?,
        libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
    )
}

pub(super) fn create_directory(parent: &File, child: &OsStr) -> Result<File, String> {
    let name = name(child)?;
    // SAFETY: the directory descriptor and C string remain live for this call.
    let result = unsafe { libc::mkdirat(parent.as_raw_fd(), name.as_ptr(), 0o700) };
    if result != 0 {
        let io_error = std::io::Error::last_os_error();
        if io_error.kind() != std::io::ErrorKind::AlreadyExists {
            return Err(error(io_error));
        }
    }
    open_directory(parent, child)
}

pub(super) fn open_file(parent: &File, child: &OsStr, create_new: bool) -> Result<File, String> {
    let flags = if create_new {
        libc::O_EXCL | libc::O_APPEND
    } else {
        0
    };
    open_at(
        parent,
        &name(child)?,
        libc::O_RDWR | libc::O_CREAT | libc::O_NOFOLLOW | libc::O_CLOEXEC | flags,
    )
}

pub(super) fn metadata(parent: &File, child: &OsStr) -> Result<libc::stat, String> {
    let name = name(child)?;
    // SAFETY: stat is an initialized output buffer with the correct size;
    // AT_SYMLINK_NOFOLLOW describes a link itself, never its target.
    let mut stat: libc::stat = unsafe { std::mem::zeroed() };
    let result = unsafe {
        libc::fstatat(
            parent.as_raw_fd(),
            name.as_ptr(),
            &mut stat,
            libc::AT_SYMLINK_NOFOLLOW,
        )
    };
    if result != 0 {
        return Err(error(std::io::Error::last_os_error()));
    }
    Ok(stat)
}

pub(super) fn regular_size(stat: &libc::stat) -> Result<u64, String> {
    if stat.st_mode & libc::S_IFMT != libc::S_IFREG || stat.st_size < 0 {
        return Err("telemetry_unsafe_file".into());
    }
    Ok(stat.st_size as u64)
}

struct DirectoryStream(*mut libc::DIR);

impl Drop for DirectoryStream {
    fn drop(&mut self) {
        // SAFETY: this stream exclusively owns the successful fdopendir result.
        unsafe { libc::closedir(self.0) };
    }
}

// readdir returns null for both EOF and an error. Reset and inspect errno so a
// failed scan cannot masquerade as an empty directory and bypass the disk cap.
fn errno_pointer() -> *mut libc::c_int {
    #[cfg(any(target_os = "linux", target_os = "hurd"))]
    {
        unsafe { libc::__errno_location() }
    }
    #[cfg(any(target_os = "macos", target_os = "ios", target_os = "freebsd"))]
    {
        unsafe { libc::__error() }
    }
    #[cfg(target_os = "android")]
    {
        unsafe { libc::__errno() }
    }
}

pub(super) fn directory_bytes(parent: &File) -> Result<u64, String> {
    use std::os::fd::IntoRawFd;
    // A fresh open description, not dup: scans must not share/reuse offsets.
    let opened = open_directory(parent, OsStr::new("."))?;
    let fd = opened.into_raw_fd();
    // SAFETY: fd is freshly owned, and fdopendir consumes it only on success.
    let stream = unsafe { libc::fdopendir(fd) };
    if stream.is_null() {
        let io_error = std::io::Error::last_os_error();
        drop(unsafe { File::from_raw_fd(fd) });
        return Err(error(io_error));
    }
    let stream = DirectoryStream(stream);
    let mut count = 0;
    let mut bytes = 0u64;
    loop {
        // SAFETY: errno is thread-local; stream is live and exclusively used.
        unsafe { *errno_pointer() = 0 };
        let entry = unsafe { libc::readdir(stream.0) };
        if entry.is_null() {
            if unsafe { *errno_pointer() } != 0 {
                return Err(error(std::io::Error::last_os_error()));
            }
            break;
        }
        // SAFETY: readdir supplies a terminated d_name until its next call.
        let filename = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) }.to_bytes();
        if filename == b"." || filename == b".." {
            continue;
        }
        count += 1;
        if count > MAX_DIRECTORY_ENTRIES {
            return Err("telemetry_directory_limit".into());
        }
        let filename = OsStr::from_bytes(filename);
        if !super::writer::own_filename(std::path::Path::new(filename)) {
            continue;
        }
        bytes = bytes
            .checked_add(regular_size(&metadata(parent, filename)?)?)
            .ok_or("telemetry_disk_cap")?;
    }
    Ok(bytes)
}
