//! `openat` / `fstat` / `renameat` for one mailbox call. FDs are not kept.
//!
//! `f_fsid` is compared as eight raw bytes. Linux and macOS spell that field
//! differently, and both keep it private.

#[cfg(test)]
use std::cell::RefCell;
use std::ffi::CStr;
use std::io::Error;
use std::mem::MaybeUninit;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::ptr;

const _: () = assert!(size_of::<libc::fsid_t>() == 8);

/// Failure of one libc call. `EINTR` is retried and is not a variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SysErr {
    /// Any errno that is not a slot-shape failure.
    Io,
    /// `ENOENT`.
    NoEnt,
    /// `EEXIST`.
    Exist,
    /// `ELOOP`. The path was not followed.
    Loop,
    /// `ENOTDIR`.
    NotDir,
    /// `EAGAIN`, `ENXIO`, `ENODEV`, or `EISDIR`.
    Special,
}

/// Kind of one `fstat` result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    /// Regular file.
    File,
    /// Directory.
    Dir,
    /// Symlink. Not followed.
    Link,
    /// FIFO, socket, device, or anything else.
    Other,
}

/// Identity compared across a read. `atime` is not part of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct FileId {
    /// `st_dev`, widened without changing bits.
    pub dev: u64,
    /// `st_ino`.
    pub ino: u64,
    /// `st_nlink`.
    pub nlink: u64,
    /// `st_size` when it is non-negative.
    pub size: u64,
    /// `st_mtime` seconds.
    pub mtime_sec: i64,
    /// `st_mtime` nanoseconds.
    pub mtime_nsec: i64,
    /// `st_ctime` seconds.
    pub ctime_sec: i64,
    /// `st_ctime` nanoseconds.
    pub ctime_nsec: i64,
    /// File type bits.
    pub kind: Kind,
}

/// `fstat` on an owned or borrowed descriptor.
pub(super) fn fstat(fd: i32) -> Result<FileId, SysErr> {
    let mut buf = MaybeUninit::<libc::stat>::uninit();
    // Safety: `buf` is a writable `stat`. On success the kernel fills it.
    retry_int(|| unsafe { libc::fstat(fd, buf.as_mut_ptr()) })?;
    // Safety: `fstat` returned 0, so `buf` holds a complete `stat`.
    id_of(&unsafe { buf.assume_init() })
}

/// `fstatat` with `AT_SYMLINK_NOFOLLOW`.
pub(super) fn fstat_at(dir: i32, name: &CStr) -> Result<FileId, SysErr> {
    let mut buf = MaybeUninit::<libc::stat>::uninit();
    // Safety: `name` is a C string. `buf` is a writable `stat`.
    retry_int(|| unsafe {
        libc::fstatat(
            dir,
            name.as_ptr(),
            buf.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    })?;
    // Safety: `fstatat` returned 0, so `buf` holds a complete `stat`.
    id_of(&unsafe { buf.assume_init() })
}

/// Eight-byte `f_fsid` of the descriptor's filesystem.
pub(super) fn fsid(fd: i32) -> Result<[u8; 8], SysErr> {
    let mut buf = MaybeUninit::<libc::statfs>::uninit();
    // Safety: `buf` is a writable `statfs`. On success the kernel fills it.
    retry_int(|| unsafe { libc::fstatfs(fd, buf.as_mut_ptr()) })?;
    // Safety: `fstatfs` returned 0, so `f_fsid` is initialized.
    let filled = unsafe { buf.assume_init() };
    Ok(fsid_bytes(&filled.f_fsid))
}

/// Open one directory segment. Does not follow a symlink.
pub(super) fn open_dir(dir: i32, name: &CStr) -> Result<OwnedFd, SysErr> {
    open_at(
        dir,
        name,
        libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        0,
    )
}

/// Open a slot for reading. `O_NONBLOCK` so a FIFO cannot park this thread.
pub(super) fn open_read(dir: i32, name: &CStr) -> Result<OwnedFd, SysErr> {
    open_at(
        dir,
        name,
        libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
        0,
    )
}

/// Create a new file. `O_EXCL` refuses a name that already exists.
pub(super) fn open_create(dir: i32, name: &CStr) -> Result<OwnedFd, SysErr> {
    open_at(
        dir,
        name,
        libc::O_CREAT | libc::O_EXCL | libc::O_WRONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        mode_of(0o600),
    )
}

/// Create a directory. An existing name is [`SysErr::Exist`].
pub(super) fn mkdir_excl(dir: i32, name: &CStr) -> Result<(), SysErr> {
    // Safety: `name` is a single path segment, not a walk.
    retry_int(|| unsafe { libc::mkdirat(dir, name.as_ptr(), mode_of(0o700)) })?;
    Ok(())
}

/// Read at most `cap` bytes. A short read at EOF stops.
///
/// On error the second value is how many bytes were already read, so the
/// caller can still charge them.
pub(super) fn read_capped(fd: i32, cap: usize) -> Result<Vec<u8>, (SysErr, usize)> {
    #[cfg(test)]
    if let Some(partial) = take_forced_fail() {
        return Err((SysErr::Io, partial.min(cap)));
    }
    #[cfg(test)]
    if let Some(forced) = take_forced_read() {
        return Ok(forced);
    }
    let mut out = vec![0u8; cap];
    let mut filled = 0usize;
    while filled < cap {
        let rest = out.get_mut(filled..).ok_or((SysErr::Io, filled))?;
        // Safety: `rest` is initialized storage of `rest.len()` bytes.
        let n = retry_isize(|| unsafe { libc::read(fd, rest.as_mut_ptr().cast(), rest.len()) })
            .map_err(|err| (err, filled))?;
        if n == 0 {
            break;
        }
        let step = usize::try_from(n).map_err(|_| (SysErr::Io, filled))?;
        filled = filled.checked_add(step).ok_or((SysErr::Io, filled))?;
    }
    out.truncate(filled);
    Ok(out)
}

/// Write every byte. A zero-length write is an error.
pub(super) fn write_all(fd: i32, mut bytes: &[u8]) -> Result<(), SysErr> {
    while !bytes.is_empty() {
        // Safety: `bytes` is an initialized readable buffer.
        let n = retry_isize(|| unsafe { libc::write(fd, bytes.as_ptr().cast(), bytes.len()) })?;
        if n == 0 {
            return Err(SysErr::Io);
        }
        let step = usize::try_from(n).map_err(|_| SysErr::Io)?;
        bytes = bytes.get(step..).ok_or(SysErr::Io)?;
    }
    Ok(())
}

/// `fsync` one file or directory descriptor.
pub(super) fn fsync_fd(fd: i32) -> Result<(), SysErr> {
    // Safety: `fd` is a live descriptor owned by the caller for this call.
    retry_int(|| unsafe { libc::fsync(fd) })?;
    Ok(())
}

/// Names other than `.` and `..`, in directory order, stopping at `limit`.
///
/// The caller passes the whitelist length plus one. This does not sort, recurse,
/// or read again to see a stable picture.
pub(super) fn list_limited(dir: i32, limit: usize) -> Result<Vec<Vec<u8>>, SysErr> {
    if limit == 0 {
        return Ok(Vec::new());
    }
    let duped = dup_cloexec(dir)?;
    let handle = match adopt_dir(duped) {
        Ok(handle) => handle,
        Err(err) => {
            // `fdopendir` leaves the descriptor open when it fails.
            let _ = unsafe { libc::close(duped) };
            return Err(err);
        }
    };
    let mut names = Vec::new();
    loop {
        clear_errno();
        // Safety: `handle` is a live `DIR`. The returned entry stays valid
        // until the next `readdir` or `closedir` on this pointer.
        let ent = unsafe { libc::readdir(handle.0) };
        if ent.is_null() {
            match Error::last_os_error().raw_os_error() {
                None | Some(0) => break,
                Some(libc::EINTR) => continue,
                _ => return Err(map_last()),
            }
        }
        let name = entry_name(ent)?;
        if name == b"." || name == b".." {
            continue;
        }
        names.push(name);
        if names.len() == limit {
            break;
        }
    }
    Ok(names)
}

/// `renameat` from one parent to another. Replaces the destination name.
pub(super) fn rename_at(from_dir: i32, from: &CStr, to_dir: i32, to: &CStr) -> Result<(), SysErr> {
    // Safety: both names are single segments. The call does not follow `to`.
    retry_int(|| unsafe { libc::renameat(from_dir, from.as_ptr(), to_dir, to.as_ptr()) })?;
    Ok(())
}

/// Borrow the raw descriptor. The [`OwnedFd`] still closes it.
pub(super) fn raw_fd(fd: &OwnedFd) -> i32 {
    fd.as_raw_fd()
}

/// Open a directory by path. Tests use this so the harness file stays free of `unsafe`.
#[cfg(test)]
pub(super) fn open_root(path: &CStr) -> Result<OwnedFd, SysErr> {
    // Safety: `path` is NUL-terminated. The flags do not follow a final symlink.
    let rc = retry_int(|| unsafe {
        libc::open(
            path.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    })?;
    // Safety: `open` returned a new descriptor. This owner closes it.
    Ok(unsafe { OwnedFd::from_raw_fd(rc) })
}

/// One-shot stand-in for the next [`read_capped`]. Metadata calls stay real.
#[cfg(test)]
pub(super) fn force_read(bytes: Vec<u8>) -> ReadForce {
    READ_FORCED.with(|cell| *cell.borrow_mut() = Some(bytes));
    ReadForce
}

/// Clears a [`force_read`] override on drop.
#[cfg(test)]
#[derive(Debug)]
pub(super) struct ReadForce;

#[cfg(test)]
impl Drop for ReadForce {
    fn drop(&mut self) {
        READ_FORCED.with(|cell| *cell.borrow_mut() = None);
    }
}

#[cfg(test)]
thread_local! {
    static READ_FORCED: RefCell<Option<Vec<u8>>> = const { RefCell::new(None) };
}

#[cfg(test)]
fn take_forced_read() -> Option<Vec<u8>> {
    READ_FORCED.with(|cell| cell.borrow_mut().take())
}

/// One-shot failure of the next [`read_capped`] after `partial` bytes.
#[cfg(test)]
pub(super) fn force_read_fail(partial: usize) -> ReadFail {
    READ_FAIL.with(|cell| *cell.borrow_mut() = Some(partial));
    ReadFail
}

/// Clears a [`force_read_fail`] override on drop.
#[cfg(test)]
#[derive(Debug)]
pub(super) struct ReadFail;

#[cfg(test)]
impl Drop for ReadFail {
    fn drop(&mut self) {
        READ_FAIL.with(|cell| *cell.borrow_mut() = None);
    }
}

#[cfg(test)]
thread_local! {
    static READ_FAIL: RefCell<Option<usize>> = const { RefCell::new(None) };
}

#[cfg(test)]
fn take_forced_fail() -> Option<usize> {
    READ_FAIL.with(|cell| cell.borrow_mut().take())
}

/// Create a FIFO. Tests use it to prove a slot read does not block.
#[cfg(test)]
pub(super) fn make_fifo(path: &CStr) -> Result<(), SysErr> {
    // Safety: `path` is NUL-terminated. `mkfifo` creates the node and does not follow it.
    retry_int(|| unsafe { libc::mkfifo(path.as_ptr(), mode_of(0o600)) })?;
    Ok(())
}

struct DirHandle(*mut libc::DIR);

impl Drop for DirHandle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // Safety: `fdopendir` returned this pointer and it has not been closed.
            let _ = unsafe { libc::closedir(self.0) };
            self.0 = ptr::null_mut();
        }
    }
}

fn dup_cloexec(fd: i32) -> Result<i32, SysErr> {
    // Safety: `fd` is a live descriptor. The new one is closed by the caller.
    retry_int(|| unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 0) })
}

fn adopt_dir(fd: i32) -> Result<DirHandle, SysErr> {
    // Safety: `fd` is a directory descriptor we own. On success `DIR` closes it.
    let dir = unsafe { libc::fdopendir(fd) };
    if dir.is_null() {
        return Err(map_last());
    }
    Ok(DirHandle(dir))
}

fn entry_name(ent: *mut libc::dirent) -> Result<Vec<u8>, SysErr> {
    // Safety: `readdir` returned `ent`, and `d_name` is NUL-terminated.
    let name = unsafe { CStr::from_ptr((*ent).d_name.as_ptr()) };
    Ok(name.to_bytes().to_vec())
}

fn clear_errno() {
    // Safety: the pointer is the thread's errno cell.
    unsafe {
        *errno_location() = 0;
    }
}

fn errno_location() -> *mut libc::c_int {
    #[cfg(target_os = "macos")]
    // Safety: `__error` returns this thread's errno cell.
    unsafe {
        libc::__error()
    }
    #[cfg(not(target_os = "macos"))]
    // Safety: `__errno_location` returns this thread's errno cell.
    unsafe {
        libc::__errno_location()
    }
}

fn open_at(
    dir: i32,
    name: &CStr,
    flags: libc::c_int,
    mode: libc::mode_t,
) -> Result<OwnedFd, SysErr> {
    // `openat` is variadic. macOS `mode_t` is `u16` and cannot cross that ABI.
    let bits = libc::c_uint::from(mode);
    // Safety: `name` is a C string. `bits` is ignored unless `O_CREAT` is set.
    let rc = retry_int(|| unsafe { libc::openat(dir, name.as_ptr(), flags, bits) })?;
    // Safety: `openat` returned a new descriptor. This owner closes it.
    Ok(unsafe { OwnedFd::from_raw_fd(rc) })
}

fn id_of(st: &libc::stat) -> Result<FileId, SysErr> {
    Ok(FileId {
        dev: dev_wide(st.st_dev),
        ino: ino_wide(st.st_ino),
        nlink: nlink_wide(st.st_nlink),
        size: off_wide(st.st_size).ok_or(SysErr::Io)?,
        mtime_sec: stamp(st.st_mtime),
        mtime_nsec: stamp(st.st_mtime_nsec),
        ctime_sec: stamp(st.st_ctime),
        ctime_nsec: stamp(st.st_ctime_nsec),
        kind: kind_of(st.st_mode),
    })
}

fn kind_of(mode: libc::mode_t) -> Kind {
    match mode & libc::S_IFMT {
        bits if bits == libc::S_IFREG => Kind::File,
        bits if bits == libc::S_IFDIR => Kind::Dir,
        bits if bits == libc::S_IFLNK => Kind::Link,
        _ => Kind::Other,
    }
}

fn fsid_bytes(id: &libc::fsid_t) -> [u8; 8] {
    let mut out = [0u8; 8];
    // Safety: `fsid_t` is eight bytes (asserted above) and `id` is aligned for it.
    unsafe { ptr::copy_nonoverlapping(ptr::from_ref(id).cast::<u8>(), out.as_mut_ptr(), 8) };
    out
}

fn retry_int(mut op: impl FnMut() -> libc::c_int) -> Result<libc::c_int, SysErr> {
    loop {
        let rc = op();
        if rc >= 0 {
            return Ok(rc);
        }
        let err = map_last();
        if err != SysErr::Io || Error::last_os_error().raw_os_error() != Some(libc::EINTR) {
            return Err(err);
        }
    }
}

fn retry_isize(mut op: impl FnMut() -> isize) -> Result<isize, SysErr> {
    loop {
        let rc = op();
        if rc >= 0 {
            return Ok(rc);
        }
        let err = map_last();
        if err != SysErr::Io || Error::last_os_error().raw_os_error() != Some(libc::EINTR) {
            return Err(err);
        }
    }
}

fn map_last() -> SysErr {
    match Error::last_os_error().raw_os_error() {
        Some(libc::EINTR) => SysErr::Io,
        Some(libc::ENOENT) => SysErr::NoEnt,
        Some(libc::EEXIST) => SysErr::Exist,
        Some(libc::ELOOP) => SysErr::Loop,
        Some(libc::ENOTDIR) => SysErr::NotDir,
        Some(code)
            if code == libc::EAGAIN
                || code == libc::ENXIO
                || code == libc::ENODEV
                || code == libc::EISDIR =>
        {
            SysErr::Special
        }
        _ => SysErr::Io,
    }
}

trait DevWide {
    fn dev_wide(self) -> u64;
}
impl DevWide for u64 {
    fn dev_wide(self) -> u64 {
        self
    }
}
impl DevWide for i32 {
    fn dev_wide(self) -> u64 {
        u64::from(u32::from_ne_bytes(self.to_ne_bytes()))
    }
}
fn dev_wide(dev: impl DevWide) -> u64 {
    dev.dev_wide()
}

trait InoWide {
    fn ino_wide(self) -> u64;
}
impl InoWide for u64 {
    fn ino_wide(self) -> u64 {
        self
    }
}
impl InoWide for u32 {
    fn ino_wide(self) -> u64 {
        u64::from(self)
    }
}
fn ino_wide(ino: impl InoWide) -> u64 {
    ino.ino_wide()
}

trait NlinkWide {
    fn nlink_wide(self) -> u64;
}
impl NlinkWide for u64 {
    fn nlink_wide(self) -> u64 {
        self
    }
}
impl NlinkWide for u32 {
    fn nlink_wide(self) -> u64 {
        u64::from(self)
    }
}
impl NlinkWide for u16 {
    fn nlink_wide(self) -> u64 {
        u64::from(self)
    }
}
fn nlink_wide(nlink: impl NlinkWide) -> u64 {
    nlink.nlink_wide()
}

trait OffWide {
    fn off_wide(self) -> Option<u64>;
}
impl OffWide for i64 {
    fn off_wide(self) -> Option<u64> {
        u64::try_from(self).ok()
    }
}
impl OffWide for i32 {
    fn off_wide(self) -> Option<u64> {
        u64::try_from(self).ok()
    }
}
fn off_wide(size: impl OffWide) -> Option<u64> {
    size.off_wide()
}

trait Stamp {
    fn stamp(self) -> i64;
}
impl Stamp for i64 {
    fn stamp(self) -> i64 {
        self
    }
}
impl Stamp for i32 {
    fn stamp(self) -> i64 {
        i64::from(self)
    }
}
fn stamp(value: impl Stamp) -> i64 {
    value.stamp()
}

trait ModeOf: Copy {
    fn mode_of(bits: u16) -> Self;
}
impl ModeOf for u16 {
    fn mode_of(bits: u16) -> Self {
        bits
    }
}
impl ModeOf for u32 {
    fn mode_of(bits: u16) -> Self {
        u32::from(bits)
    }
}
fn mode_of(bits: u16) -> libc::mode_t {
    ModeOf::mode_of(bits)
}
