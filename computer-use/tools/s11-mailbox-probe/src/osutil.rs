//! Unix openat / fstatfs helpers. No third-party crate: libc is the platform C library.

use std::ffi::CString;
use std::fs::{File, OpenOptions};
use std::io;
use std::os::raw::c_char;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::os::unix::io::{AsRawFd, FromRawFd};
use std::path::Path;

pub fn flag_nofollow() -> i32 {
    // Linux UAPI is not uniform: x86_64 uses the generic values, arm64 overrides them.
    #[cfg(all(target_os = "linux", any(target_arch = "aarch64", target_arch = "arm")))]
    {
        0x8000
    }
    #[cfg(all(
        target_os = "linux",
        not(any(target_arch = "aarch64", target_arch = "arm"))
    ))]
    {
        0x20000
    }
    #[cfg(target_os = "macos")]
    {
        0x100
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        0
    }
}

pub fn flag_cloexec() -> i32 {
    #[cfg(target_os = "linux")]
    {
        0x80000
    }
    #[cfg(target_os = "macos")]
    {
        0x1000000
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        0
    }
}

pub fn flag_nonblock() -> i32 {
    #[cfg(target_os = "linux")]
    {
        0x800
    }
    #[cfg(target_os = "macos")]
    {
        0x4
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        0
    }
}

pub fn flag_directory() -> i32 {
    #[cfg(all(target_os = "linux", any(target_arch = "aarch64", target_arch = "arm")))]
    {
        0x4000
    }
    #[cfg(all(
        target_os = "linux",
        not(any(target_arch = "aarch64", target_arch = "arm"))
    ))]
    {
        0x10000
    }
    #[cfg(target_os = "macos")]
    {
        0x100000
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        0
    }
}

pub fn flag_creat() -> i32 {
    #[cfg(target_os = "linux")]
    {
        0x40
    }
    #[cfg(target_os = "macos")]
    {
        0x200
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        0
    }
}

pub fn flag_excl() -> i32 {
    #[cfg(target_os = "linux")]
    {
        0x80
    }
    #[cfg(target_os = "macos")]
    {
        0x800
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        0
    }
}

pub fn flag_wronly() -> i32 {
    1
}

fn fsid_offset() -> usize {
    #[cfg(target_os = "linux")]
    {
        56
    }
    #[cfg(target_os = "macos")]
    {
        48
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        0
    }
}

extern "C" {
    // Variadic on purpose. Apple arm64 puts unnamed args on the stack; a fixed
    // fourth parameter never becomes the create mode, so the file is born with
    // stack garbage and a later read returns EACCES.
    fn openat(dirfd: i32, path: *const c_char, flags: i32, ...) -> i32;
    fn fchmod(fd: i32, mode: u32) -> i32;
    fn unlinkat(dirfd: i32, path: *const c_char, flags: i32) -> i32;
    fn fstatfs(fd: i32, buf: *mut u8) -> i32;
    fn geteuid() -> u32;
    fn getegid() -> u32;
    fn chown(path: *const c_char, uid: u32, gid: u32) -> i32;
    fn setpgid(pid: i32, pgid: i32) -> i32;
    fn kill(pid: i32, sig: i32) -> i32;
}

#[cfg(test)]
extern "C" {
    fn mkfifo(path: *const c_char, mode: u32) -> i32;
}

pub const SIGTERM: i32 = 15;
pub const SIGKILL: i32 = 9;

pub fn euid() -> u32 {
    unsafe { geteuid() }
}

/// `chown` the path to a different uid. `denied` is the expected non-elevated result.
pub fn chown_other(path: &Path) -> Result<&'static str, &'static str> {
    let c = CString::new(path.as_os_str().as_bytes()).map_err(|_| "chown")?;
    let uid = euid().wrapping_add(1);
    let gid = unsafe { getegid() };
    let rc = unsafe { chown(c.as_ptr(), uid, gid) };
    if rc == 0 {
        return Ok("allowed");
    }
    match io::Error::last_os_error().raw_os_error() {
        Some(1) | Some(13) => Ok("denied"),
        _ => Ok("error"),
    }
}

pub fn component_ok(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains('/')
        && !name.contains('\0')
        && name.is_ascii()
}

pub fn open_at(dir: &File, name: &str, flags: i32, mode: u32) -> io::Result<File> {
    if !component_ok(name) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "component"));
    }
    let c = CString::new(name).map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "nul"))?;
    let flags = flags | flag_cloexec();
    // `va_arg(..., int)` — pass a 32-bit int, not a mode_t-sized value.
    let fd = unsafe { openat(dir.as_raw_fd(), c.as_ptr(), flags, mode as i32) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    let file = unsafe { File::from_raw_fd(fd) };
    if flags & flag_creat() != 0 {
        // Exact 0600, independent of umask. A failed fchmod must not leave the name.
        let rc = unsafe { fchmod(file.as_raw_fd(), 0o600) };
        if rc != 0 {
            let err = io::Error::last_os_error();
            drop(file);
            unsafe { unlinkat(dir.as_raw_fd(), c.as_ptr(), 0) };
            return Err(err);
        }
    }
    Ok(file)
}

pub fn open_dir_nofollow(path: &Path) -> io::Result<File> {
    let mut opts = OpenOptions::new();
    opts.read(true);
    opts.custom_flags(flag_directory() | flag_nofollow() | flag_cloexec());
    opts.open(path)
}

pub fn fsid_of(file: &File) -> Result<[u8; 8], ()> {
    if fsid_offset() == 0 {
        return Err(());
    }
    let mut buf = [0u8; 4096];
    let rc = unsafe { fstatfs(file.as_raw_fd(), buf.as_mut_ptr()) };
    if rc != 0 {
        return Err(());
    }
    let off = fsid_offset();
    let mut id = [0u8; 8];
    id.copy_from_slice(&buf[off..off + 8]);
    if id == [0u8; 8] {
        return Err(());
    }
    Ok(id)
}

pub fn mkdir_0700(path: &Path) -> io::Result<()> {
    std::fs::create_dir(path)?;
    let perms = std::fs::Permissions::from_mode(0o700);
    std::fs::set_permissions(path, perms)?;
    let meta = std::fs::symlink_metadata(path)?;
    if meta.file_type().is_symlink() || !meta.is_dir() {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "mkdir"));
    }
    let mode = meta.permissions().mode() & 0o777;
    if mode != 0o700 {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "mode"));
    }
    Ok(())
}

pub fn file_0600(path: &Path) -> io::Result<File> {
    let mut opts = OpenOptions::new();
    opts.write(true)
        .create_new(true)
        .custom_flags(flag_nofollow() | flag_cloexec());
    let file = opts.open(path)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    Ok(file)
}

/// Own process group so a timeout can signal the child without touching pid 1.
pub fn own_pgroup() -> io::Result<()> {
    let rc = unsafe { setpgid(0, 0) };
    if rc != 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

pub fn signal_group(pid: i32, sig: i32) -> Result<(), &'static str> {
    if pid <= 1 {
        return Err("bad-pid");
    }
    let rc = unsafe { kill(-pid, sig) };
    if rc != 0 {
        Err("signal")
    } else {
        Ok(())
    }
}

pub fn set_own_group(pid: i32) -> Result<(), &'static str> {
    if pid <= 1 {
        return Err("bad-pid");
    }
    let rc = unsafe { setpgid(pid, pid) };
    if rc != 0 {
        Err("setpgid")
    } else {
        Ok(())
    }
}

#[cfg(test)]
pub fn make_fifo(path: &Path) -> io::Result<()> {
    let c = CString::new(path.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "nul"))?;
    let rc = unsafe { mkfifo(c.as_ptr(), 0o600) };
    if rc != 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::MetadataExt;

    #[test]
    fn fsid_matches_inside_one_directory() {
        let dir = std::env::temp_dir().join(format!("s11-fsid-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        mkdir_0700(&dir).unwrap();
        let a = dir.join("a");
        let b = dir.join("b");
        std::fs::write(&a, b"a").unwrap();
        std::fs::write(&b, b"b").unwrap();
        let fa = File::open(&a).unwrap();
        let fb = File::open(&b).unwrap();
        let ida = fsid_of(&fa).expect("fsid");
        let idb = fsid_of(&fb).expect("fsid");
        assert_eq!(ida, idb);
        assert_eq!(fa.metadata().unwrap().dev(), fb.metadata().unwrap().dev());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn refuses_bad_signal_targets() {
        assert_eq!(signal_group(0, SIGTERM), Err("bad-pid"));
        assert_eq!(signal_group(1, SIGKILL), Err("bad-pid"));
        assert_eq!(signal_group(-3, SIGTERM), Err("bad-pid"));
    }
}
