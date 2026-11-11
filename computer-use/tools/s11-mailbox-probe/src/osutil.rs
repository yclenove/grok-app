//! Directory-fd helpers. Layout of `statfs` is asserted, not read from a raw offset.

use std::ffi::CString;
use std::fs::File;
use std::io::{self, Read, Write};
use std::mem::{size_of, MaybeUninit};
use std::os::raw::c_char;
use std::os::unix::fs::MetadataExt;
use std::os::unix::io::{AsRawFd, FromRawFd, RawFd};
use std::path::{Component, Path};

pub const SIGTERM: i32 = 15;
pub const SIGKILL: i32 = 9;

#[cfg(all(target_os = "linux", any(target_arch = "aarch64", target_arch = "arm")))]
mod bits {
    pub const NOFOLLOW: i32 = 0x8000;
    pub const DIRECTORY: i32 = 0x4000;
    pub const CLOEXEC: i32 = 0x80000;
    pub const NONBLOCK: i32 = 0x800;
    pub const CREAT: i32 = 0x40;
    pub const EXCL: i32 = 0x80;
    pub const AT_FDCWD: i32 = -100;
    pub const AT_REMOVEDIR: i32 = 0x200;
    pub const ELOOP: i32 = 40;
}
#[cfg(all(
    target_os = "linux",
    not(any(target_arch = "aarch64", target_arch = "arm"))
))]
mod bits {
    pub const NOFOLLOW: i32 = 0x20000;
    pub const DIRECTORY: i32 = 0x10000;
    pub const CLOEXEC: i32 = 0x80000;
    pub const NONBLOCK: i32 = 0x800;
    pub const CREAT: i32 = 0x40;
    pub const EXCL: i32 = 0x80;
    pub const AT_FDCWD: i32 = -100;
    pub const AT_REMOVEDIR: i32 = 0x200;
    pub const ELOOP: i32 = 40;
}
#[cfg(target_os = "macos")]
mod bits {
    pub const NOFOLLOW: i32 = 0x100;
    pub const DIRECTORY: i32 = 0x100000;
    pub const CLOEXEC: i32 = 0x1000000;
    pub const NONBLOCK: i32 = 0x4;
    pub const CREAT: i32 = 0x200;
    pub const EXCL: i32 = 0x800;
    pub const AT_FDCWD: i32 = -2;
    pub const AT_REMOVEDIR: i32 = 0x80;
    pub const ELOOP: i32 = 62;
}
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
mod bits {
    pub const NOFOLLOW: i32 = 0;
    pub const DIRECTORY: i32 = 0;
    pub const CLOEXEC: i32 = 0;
    pub const NONBLOCK: i32 = 0;
    pub const CREAT: i32 = 0;
    pub const EXCL: i32 = 0;
    pub const AT_FDCWD: i32 = -100;
    pub const AT_REMOVEDIR: i32 = 0x200;
    pub const ELOOP: i32 = 40;
}

pub fn flag_nofollow() -> i32 {
    bits::NOFOLLOW
}
pub fn flag_cloexec() -> i32 {
    bits::CLOEXEC
}
pub fn flag_nonblock() -> i32 {
    bits::NONBLOCK
}
pub fn flag_directory() -> i32 {
    bits::DIRECTORY
}
pub fn flag_creat() -> i32 {
    bits::CREAT
}
pub fn flag_excl() -> i32 {
    bits::EXCL
}

fn flag_rdonly() -> i32 {
    0
}

pub fn at_fdcwd() -> i32 {
    bits::AT_FDCWD
}

pub fn at_removedir() -> i32 {
    bits::AT_REMOVEDIR
}

fn errno_is(err: &io::Error, n: i32) -> bool {
    err.raw_os_error() == Some(n)
}

pub fn is_eloop(err: &io::Error) -> bool {
    errno_is(err, bits::ELOOP)
}

pub fn is_enoent(err: &io::Error) -> bool {
    errno_is(err, 2)
}

pub fn is_enxio(err: &io::Error) -> bool {
    errno_is(err, 6)
}

pub fn is_eacces(err: &io::Error) -> bool {
    errno_is(err, 13) || errno_is(err, 1)
}

extern "C" {
    fn openat(dirfd: i32, path: *const c_char, flags: i32, ...) -> i32;
    fn fchmod(fd: i32, mode: u32) -> i32;
    fn unlinkat(dirfd: i32, path: *const c_char, flags: i32) -> i32;
    fn mkdirat(dirfd: i32, path: *const c_char, mode: u32) -> i32;
    fn renameat(olddirfd: i32, old: *const c_char, newdirfd: i32, newp: *const c_char) -> i32;
    fn fsync(fd: i32) -> i32;
    fn geteuid() -> u32;
    fn setpgid(pid: i32, pgid: i32) -> i32;
    fn kill(pid: i32, sig: i32) -> i32;
    fn fcntl(fd: i32, cmd: i32, ...) -> i32;
}

pub fn euid() -> u32 {
    unsafe { geteuid() }
}

pub fn component_ok(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains('/')
        && !name.contains('\0')
        && name.is_ascii()
}

fn c_name(name: &str) -> io::Result<CString> {
    if !component_ok(name) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "component"));
    }
    CString::new(name).map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "nul"))
}

pub fn open_at(dir: &File, name: &str, flags: i32, mode: i32) -> io::Result<File> {
    let c = c_name(name)?;
    let fd = unsafe { openat(dir.as_raw_fd(), c.as_ptr(), flags | flag_cloexec(), mode) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(unsafe { File::from_raw_fd(fd) })
}

fn open_cwd(name: &str, flags: i32) -> io::Result<File> {
    let c = CString::new(name).map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "nul"))?;
    let fd = unsafe { openat(at_fdcwd(), c.as_ptr(), flags | flag_cloexec(), 0) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(unsafe { File::from_raw_fd(fd) })
}

pub fn fsync_fd(file: &File) -> io::Result<()> {
    let rc = unsafe { fsync(file.as_raw_fd()) };
    if rc != 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

pub fn set_nonblock(fd: RawFd) -> io::Result<()> {
    let flags = unsafe { fcntl(fd, 3) };
    if flags < 0 {
        return Err(io::Error::last_os_error());
    }
    let rc = unsafe { fcntl(fd, 4, flags | flag_nonblock()) };
    if rc < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

pub fn own_pgroup() -> io::Result<()> {
    if unsafe { setpgid(0, 0) } != 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

pub fn set_own_group(pid: i32) -> Result<(), &'static str> {
    if pid <= 1 {
        return Err("bad-pid");
    }
    if unsafe { setpgid(pid, pid) } != 0 {
        Err("setpgid")
    } else {
        Ok(())
    }
}

pub fn signal_group(pid: i32, sig: i32) -> Result<(), &'static str> {
    if pid <= 1 {
        return Err("bad-pid");
    }
    if unsafe { kill(-pid, sig) } != 0 {
        Err("signal")
    } else {
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NodeFact {
    pub uid: u32,
    pub mode: u32,
    pub is_dir: bool,
    pub acl_write: bool,
}

/// Every ancestor is checked. A different owner does not stop the walk early.
pub fn walk_facts(facts: &[NodeFact], euid: u32) -> Result<(), &'static str> {
    if euid == 0 || facts.is_empty() {
        return Err("private-root");
    }
    let (leaf, ancestors) = facts.split_last().unwrap();
    if !leaf.is_dir || leaf.uid != euid || (leaf.mode & 0o777) != 0o700 || leaf.acl_write {
        return Err("private-root");
    }
    for anc in ancestors {
        if !anc.is_dir || (anc.uid != 0 && anc.uid != euid) {
            return Err("private-root");
        }
        if anc.mode & 0o022 != 0 || anc.acl_write {
            return Err("private-root");
        }
    }
    Ok(())
}

pub struct Held {
    pub file: File,
    pub dev: u64,
    pub ino: u64,
    pub fsid: Option<[i32; 2]>,
}

fn fact_of(file: &File) -> io::Result<NodeFact> {
    let meta = file.metadata()?;
    let acl_write = acl_has_write(file.as_raw_fd())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "acl"))?;
    Ok(NodeFact {
        uid: meta.uid(),
        mode: meta.mode(),
        is_dir: meta.is_dir(),
        acl_write,
    })
}

pub fn held_of(file: File) -> io::Result<Held> {
    let meta = file.metadata()?;
    let fsid = fsid_of(&file);
    Ok(Held {
        file,
        dev: meta.dev(),
        ino: meta.ino(),
        fsid,
    })
}

/// Open `path` one component at a time with `O_DIRECTORY|O_NOFOLLOW`.
pub fn walk_dir(path: &Path) -> io::Result<(File, Vec<NodeFact>)> {
    if !path.is_absolute() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "abs"));
    }
    let mut fd = open_cwd("/", flag_directory() | flag_nofollow() | flag_rdonly())?;
    let mut facts = vec![fact_of(&fd)?];
    for comp in path.components() {
        match comp {
            Component::RootDir => {}
            Component::Normal(os) => {
                let name = os
                    .to_str()
                    .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "utf8"))?;
                if !component_ok(name) {
                    return Err(io::Error::new(io::ErrorKind::InvalidInput, "component"));
                }
                fd = open_at(
                    &fd,
                    name,
                    flag_directory() | flag_nofollow() | flag_rdonly(),
                    0,
                )?;
                facts.push(fact_of(&fd)?);
            }
            _ => return Err(io::Error::new(io::ErrorKind::InvalidInput, "component")),
        }
    }
    Ok((fd, facts))
}

pub fn verify_private_leaf(path: &Path) -> Result<Held, &'static str> {
    if euid() == 0 {
        return Err("private-root");
    }
    let (fd, facts) = walk_dir(path).map_err(|_| "private-root")?;
    walk_facts(&facts, euid())?;
    held_of(fd).map_err(|_| "private-root")
}

pub fn mkdir_0700_at(parent: &File, name: &str) -> io::Result<File> {
    let c = c_name(name)?;
    let rc = unsafe { mkdirat(parent.as_raw_fd(), c.as_ptr(), 0o700) };
    if rc != 0 {
        return Err(io::Error::last_os_error());
    }
    let file = open_at(
        parent,
        name,
        flag_directory() | flag_nofollow() | flag_rdonly(),
        0,
    )?;
    if unsafe { fchmod(file.as_raw_fd(), 0o700) } != 0 {
        let err = io::Error::last_os_error();
        let _ = unlink_dir_at(parent, name);
        return Err(err);
    }
    let mode = file.metadata()?.mode() & 0o777;
    if mode != 0o700 || !file.metadata()?.is_dir() {
        let _ = unlink_dir_at(parent, name);
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "mode"));
    }
    Ok(file)
}

/// `O_CREAT|O_EXCL` without `O_NOFOLLOW` (Darwin rejects that pair). `O_EXCL`
/// still refuses an existing symlink. Mode is forced with `fchmod`.
pub fn create_0600_at(parent: &File, name: &str) -> io::Result<File> {
    let c = c_name(name)?;
    let file = open_at(parent, name, flag_creat() | flag_excl() | 1, 0o600)?;
    if unsafe { fchmod(file.as_raw_fd(), 0o600) } != 0 {
        let err = io::Error::last_os_error();
        unsafe { unlinkat(parent.as_raw_fd(), c.as_ptr(), 0) };
        return Err(err);
    }
    Ok(file)
}

pub fn unlink_at(parent: &File, name: &str) -> io::Result<()> {
    let c = c_name(name)?;
    if unsafe { unlinkat(parent.as_raw_fd(), c.as_ptr(), 0) } != 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

pub fn unlink_dir_at(parent: &File, name: &str) -> io::Result<()> {
    let c = c_name(name)?;
    if unsafe { unlinkat(parent.as_raw_fd(), c.as_ptr(), at_removedir()) } != 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

pub fn rename_at(from_dir: &File, from: &str, to_dir: &File, to: &str) -> io::Result<()> {
    let a = c_name(from)?;
    let b = c_name(to)?;
    if unsafe {
        renameat(
            from_dir.as_raw_fd(),
            a.as_ptr(),
            to_dir.as_raw_fd(),
            b.as_ptr(),
        )
    } != 0
    {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Id {
    pub dev: u64,
    pub ino: u64,
    pub mode: u32,
    pub nlink: u64,
    pub size: u64,
    pub mtime: i64,
    pub mtime_nsec: i64,
    pub ctime: i64,
    pub ctime_nsec: i64,
    pub is_dir: bool,
    pub is_reg: bool,
}

impl Id {
    pub fn same_file(&self, other: &Id) -> bool {
        self.dev == other.dev && self.ino == other.ino
    }
}

pub fn id_of(file: &File) -> io::Result<Id> {
    let m = file.metadata()?;
    Ok(Id {
        dev: m.dev(),
        ino: m.ino(),
        mode: m.mode(),
        nlink: m.nlink(),
        size: m.size(),
        mtime: m.mtime(),
        mtime_nsec: m.mtime_nsec(),
        ctime: m.ctime(),
        ctime_nsec: m.ctime_nsec(),
        is_dir: m.is_dir(),
        is_reg: m.is_file(),
    })
}

pub fn open_child(parent: &File, name: &str) -> io::Result<File> {
    open_at(
        parent,
        name,
        flag_nofollow() | flag_nonblock() | flag_rdonly(),
        0,
    )
}

pub fn open_child_dir(parent: &File, name: &str) -> io::Result<File> {
    open_at(
        parent,
        name,
        flag_directory() | flag_nofollow() | flag_rdonly(),
        0,
    )
}

pub fn read_fd_capped(file: &mut File, cap: usize) -> io::Result<Vec<u8>> {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 8192];
    while buf.len() <= cap {
        match file.read(&mut tmp)? {
            0 => return Ok(buf),
            n => {
                let room = cap.saturating_add(1).saturating_sub(buf.len());
                let take = n.min(room);
                buf.extend_from_slice(&tmp[..take]);
                if buf.len() > cap {
                    return Err(io::Error::new(io::ErrorKind::InvalidData, "size"));
                }
            }
        }
    }
    Err(io::Error::new(io::ErrorKind::InvalidData, "size"))
}

pub fn write_all_fd(file: &mut File, bytes: &[u8]) -> io::Result<()> {
    file.write_all(bytes)
}

// Linux aarch64 LP64 statfs: seven 8-byte words, then fsid at offset 56. Size 120.
// Measured with the system compiler (size 120, align 8, fsid 56).
#[cfg(target_os = "linux")]
#[repr(C)]
struct StatFsL {
    f_type: i64,
    f_bsize: i64,
    f_blocks: i64,
    f_bfree: i64,
    f_bavail: i64,
    f_files: i64,
    f_ffree: i64,
    f_fsid: [i32; 2],
    f_namelen: i64,
    f_frsize: i64,
    f_flags: i64,
    f_spare: [i64; 4],
}

#[cfg(target_os = "linux")]
const _: () = assert!(size_of::<StatFsL>() == 120);
#[cfg(target_os = "linux")]
const _: () = assert!(std::mem::align_of::<StatFsL>() == 8);
#[cfg(target_os = "linux")]
const _: () = assert!(std::mem::offset_of!(StatFsL, f_fsid) == 56);

// Darwin LP64 statfs. fsid at 48, full size 2168 (libc apple layout).
#[cfg(target_os = "macos")]
#[repr(C)]
struct StatFsM {
    f_bsize: u32,
    f_iosize: i32,
    f_blocks: u64,
    f_bfree: u64,
    f_bavail: u64,
    f_files: u64,
    f_ffree: u64,
    f_fsid: [i32; 2],
    f_owner: u32,
    f_type: u32,
    f_flags: u32,
    f_fssubtype: u32,
    f_fstypename: [u8; 16],
    f_mntonname: [u8; 1024],
    f_mntfromname: [u8; 1024],
    f_flags_ext: u32,
    f_reserved: [u32; 7],
}

#[cfg(target_os = "macos")]
const _: () = assert!(size_of::<StatFsM>() == 2168);
#[cfg(target_os = "macos")]
const _: () = assert!(std::mem::align_of::<StatFsM>() == 8);
#[cfg(target_os = "macos")]
const _: () = assert!(std::mem::offset_of!(StatFsM, f_fsid) == 48);

#[cfg(target_os = "linux")]
extern "C" {
    fn fstatfs(fd: i32, buf: *mut StatFsL) -> i32;
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
extern "C" {
    fn fstatfs(fd: i32, buf: *mut StatFsM) -> i32;
}

#[cfg(all(target_os = "macos", not(target_arch = "aarch64")))]
extern "C" {
    #[link_name = "fstatfs$INODE64"]
    fn fstatfs(fd: i32, buf: *mut StatFsM) -> i32;
}

macro_rules! read_fsid {
    ($fd:expr, $ty:ty) => {{
        let mut buf = MaybeUninit::<$ty>::uninit();
        let rc = unsafe { fstatfs($fd, buf.as_mut_ptr()) };
        if rc != 0 {
            None
        } else {
            let id = unsafe { buf.assume_init() }.f_fsid;
            if id == [0, 0] {
                None
            } else {
                Some(id)
            }
        }
    }};
}

/// `None` when the call fails or `f_fsid` is all zeros. That is not a pass.
#[cfg(target_os = "linux")]
pub fn fsid_of(file: &File) -> Option<[i32; 2]> {
    read_fsid!(file.as_raw_fd(), StatFsL)
}

#[cfg(target_os = "macos")]
pub fn fsid_of(file: &File) -> Option<[i32; 2]> {
    read_fsid!(file.as_raw_fd(), StatFsM)
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub fn fsid_of(_file: &File) -> Option<[i32; 2]> {
    None
}

#[cfg(target_os = "linux")]
extern "C" {
    fn fgetxattr(fd: i32, name: *const c_char, value: *mut u8, size: usize) -> isize;
}

/// POSIX ACL xattr. `Ok(true)` means some entry other than `USER_OBJ` has
/// effective write. Missing ACL (`ENODATA` / `EOPNOTSUPP`) is `Ok(false)`.
#[cfg(target_os = "linux")]
pub fn acl_has_write(fd: i32) -> Result<bool, &'static str> {
    let name = c"system.posix_acl_access";
    let mut buf = vec![0u8; 256];
    let n = unsafe { fgetxattr(fd, name.as_ptr(), buf.as_mut_ptr(), buf.len()) };
    if n < 0 {
        let err = io::Error::last_os_error();
        let raw = err.raw_os_error();
        if raw == Some(61) || raw == Some(95) {
            return Ok(false);
        }
        return Err("acl");
    }
    if n == 0 {
        return Ok(false);
    }
    buf.truncate(n as usize);
    posix_acl_write_grant(&buf)
}

#[cfg(target_os = "linux")]
pub fn posix_acl_write_grant(bytes: &[u8]) -> Result<bool, &'static str> {
    if bytes.len() < 4 || !(bytes.len() - 4).is_multiple_of(8) {
        return Err("acl");
    }
    let ver = u32::from_le_bytes(bytes[0..4].try_into().unwrap());
    if ver != 2 {
        return Err("acl");
    }
    const USER_OBJ: u16 = 1;
    const MASK: u16 = 0x10;
    const OTHER: u16 = 0x20;
    const WRITE: u16 = 2;
    let mut mask: u16 = 0xffff;
    let mut saw_mask = false;
    let mut entries = Vec::new();
    let mut i = 4;
    while i + 8 <= bytes.len() {
        let tag = u16::from_le_bytes(bytes[i..i + 2].try_into().unwrap());
        let perm = u16::from_le_bytes(bytes[i + 2..i + 4].try_into().unwrap());
        if tag == MASK {
            mask = perm;
            saw_mask = true;
        }
        entries.push((tag, perm));
        i += 8;
    }
    if !saw_mask {
        mask = 0xffff;
    }
    for (tag, perm) in entries {
        if tag == MASK || tag == USER_OBJ {
            continue;
        }
        let eff = if tag == OTHER { perm } else { perm & mask };
        if eff & WRITE != 0 {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(target_os = "macos")]
const ACL_WRITE_PERMS: [u32; 8] = [
    4,    // WRITE_DATA
    32,   // APPEND_DATA
    16,   // DELETE
    64,   // DELETE_CHILD
    256,  // WRITE_ATTRIBUTES
    1024, // WRITE_EXTATTRIBUTES
    4096, // WRITE_SECURITY
    8192, // CHANGE_OWNER
];

#[cfg(target_os = "macos")]
extern "C" {
    fn acl_get_fd(fd: i32) -> *mut std::ffi::c_void;
    fn acl_get_entry(
        acl: *mut std::ffi::c_void,
        entry_id: i32,
        entry: *mut *mut std::ffi::c_void,
    ) -> i32;
    fn acl_get_tag_type(entry: *mut std::ffi::c_void, tag: *mut i32) -> i32;
    fn acl_get_permset(entry: *mut std::ffi::c_void, permset: *mut *mut std::ffi::c_void) -> i32;
    fn acl_get_perm_np(permset: *mut std::ffi::c_void, perm: u32) -> i32;
    fn acl_free(obj: *mut std::ffi::c_void) -> i32;
}

#[cfg(target_os = "macos")]
pub fn acl_has_write(fd: i32) -> Result<bool, &'static str> {
    // Measured on the Mini: no ACL is NULL + ENOENT (2). A later
    // `acl_get_entry` of -1 + EINVAL (22) means the previous entry was last.
    // Success is 0. ALLOW is tag 1; DENY is tag 2 and is not a grant.
    const ENOENT: i32 = 2;
    const EINVAL: i32 = 22;
    const ALLOW: i32 = 1;
    const DENY: i32 = 2;
    let acl = unsafe { acl_get_fd(fd) };
    if acl.is_null() {
        let raw = io::Error::last_os_error().raw_os_error();
        if raw == Some(ENOENT) {
            return Ok(false);
        }
        return Err("acl");
    }
    let mut entry = std::ptr::null_mut();
    let mut which = 0i32;
    let mut grant = false;
    let mut saw = false;
    let result = loop {
        let rc = unsafe { acl_get_entry(acl, which, &mut entry) };
        if rc != 0 {
            if !saw {
                break Err("acl");
            }
            let raw = io::Error::last_os_error().raw_os_error();
            if raw == Some(EINVAL) {
                break Ok(grant);
            }
            break Err("acl");
        }
        saw = true;
        which = -1;
        let mut tag = 0i32;
        if unsafe { acl_get_tag_type(entry, &mut tag) } != 0 {
            break Err("acl");
        }
        if tag == DENY {
            continue;
        }
        if tag != ALLOW {
            break Err("acl");
        }
        let mut perms = std::ptr::null_mut();
        if unsafe { acl_get_permset(entry, &mut perms) } != 0 {
            break Err("acl");
        }
        let mut bad = false;
        for perm in ACL_WRITE_PERMS {
            let g = unsafe { acl_get_perm_np(perms, perm) };
            if g < 0 {
                bad = true;
                break;
            }
            if g == 1 {
                grant = true;
            }
        }
        if bad {
            break Err("acl");
        }
    };
    unsafe { acl_free(acl) };
    result
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub fn acl_has_write(_fd: i32) -> Result<bool, &'static str> {
    Err("acl")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn fact(uid: u32, mode: u32, acl: bool) -> NodeFact {
        NodeFact {
            uid,
            mode,
            is_dir: true,
            acl_write: acl,
        }
    }

    #[test]
    fn walk_facts_checks_every_ancestor() {
        let me = 1001u32;
        assert!(walk_facts(&[fact(0, 0o755, false), fact(me, 0o700, false)], me).is_ok());
        assert!(walk_facts(
            &[
                fact(0, 0o755, false),
                fact(me, 0o755, false),
                fact(me, 0o700, false)
            ],
            me
        )
        .is_ok());
        // Different-uid ancestor used to return Ok after one other-write check.
        assert!(walk_facts(&[fact(50, 0o755, false), fact(me, 0o700, false)], me).is_err());
        assert!(walk_facts(&[fact(me, 0o775, false), fact(me, 0o700, false)], me).is_err());
        assert!(walk_facts(
            &[
                fact(0, 0o755, false),
                fact(0, 0o777, false),
                fact(me, 0o700, false)
            ],
            me
        )
        .is_err());
        assert!(walk_facts(&[fact(0, 0o1777, false), fact(me, 0o700, false)], me).is_err());
        assert!(walk_facts(&[fact(0, 0o755, true), fact(me, 0o700, false)], me).is_err());
        assert!(walk_facts(&[fact(0, 0o755, false), fact(me, 0o755, false)], me).is_err());
        assert!(walk_facts(&[fact(0, 0o755, false), fact(me, 0o700, false)], 0).is_err());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn posix_acl_parser_rejects_effective_write() {
        fn ent(tag: u16, perm: u16) -> [u8; 8] {
            let mut e = [0u8; 8];
            e[0..2].copy_from_slice(&tag.to_le_bytes());
            e[2..4].copy_from_slice(&perm.to_le_bytes());
            e
        }
        let mut ok = vec![2u8, 0, 0, 0];
        ok.extend_from_slice(&ent(1, 7));
        ok.extend_from_slice(&ent(0x20, 5));
        assert!(!posix_acl_write_grant(&ok).unwrap());
        let mut other = vec![2u8, 0, 0, 0];
        other.extend_from_slice(&ent(1, 7));
        other.extend_from_slice(&ent(0x20, 2));
        assert!(posix_acl_write_grant(&other).unwrap());
        let mut masked = vec![2u8, 0, 0, 0];
        masked.extend_from_slice(&ent(4, 7));
        masked.extend_from_slice(&ent(0x10, 5));
        assert!(!posix_acl_write_grant(&masked).unwrap());
        let mut granted = vec![2u8, 0, 0, 0];
        granted.extend_from_slice(&ent(4, 2));
        granted.extend_from_slice(&ent(0x10, 2));
        assert!(posix_acl_write_grant(&granted).unwrap());
        assert!(posix_acl_write_grant(&[2, 0, 0]).is_err());
        assert!(posix_acl_write_grant(&[1, 0, 0, 0]).is_err());
    }

    #[test]
    fn live_private_root_and_fsid() {
        if euid() == 0 {
            return;
        }
        let base = Path::new(&std::env::var("HOME").unwrap())
            .join(format!(".s11m2a-os-{}", std::process::id()));
        let _ = std::fs::remove_dir(&base);
        std::fs::create_dir(&base).unwrap();
        std::fs::set_permissions(&base, std::fs::Permissions::from_mode(0o700)).unwrap();
        let held = verify_private_leaf(&base).expect("private");
        assert!(held.fsid.is_some());
        let wide = base.join("wide");
        std::fs::create_dir(&wide).unwrap();
        std::fs::set_permissions(&wide, std::fs::Permissions::from_mode(0o775)).unwrap();
        let leaf = wide.join("leaf");
        std::fs::create_dir(&leaf).unwrap();
        std::fs::set_permissions(&leaf, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert!(verify_private_leaf(&leaf).is_err());
        let link = base.join("link");
        std::os::unix::fs::symlink("/tmp", &link).unwrap();
        let through = link.join("nope");
        assert!(verify_private_leaf(&through).is_err());
        assert!(verify_private_leaf(Path::new("/tmp")).is_err());
        let _ = std::fs::remove_file(&link);
        let _ = std::fs::remove_dir(&leaf);
        let _ = std::fs::remove_dir(&wide);
        let _ = std::fs::remove_dir(&base);
    }

    /// `deny delete` is not a write grant. `allow write` is, and the leaf is rejected.
    #[cfg(target_os = "macos")]
    #[test]
    fn macos_acl_deny_is_not_a_grant() {
        fn apply(fd: i32, text: &str) {
            extern "C" {
                fn acl_from_text(buf: *const std::os::raw::c_char) -> *mut std::ffi::c_void;
                fn acl_set_fd(fd: i32, acl: *mut std::ffi::c_void) -> i32;
                fn acl_free(obj: *mut std::ffi::c_void) -> i32;
            }
            let c = std::ffi::CString::new(text).unwrap();
            let acl = unsafe { acl_from_text(c.as_ptr()) };
            assert!(!acl.is_null());
            let rc = unsafe { acl_set_fd(fd, acl) };
            unsafe { acl_free(acl) };
            assert_eq!(rc, 0);
        }
        if euid() == 0 {
            return;
        }
        let base = Path::new(&std::env::var("HOME").unwrap())
            .join(format!(".s11m2a-acl-{}", std::process::id()));
        let _ = std::fs::remove_dir(&base);
        std::fs::create_dir(&base).unwrap();
        std::fs::set_permissions(&base, std::fs::Permissions::from_mode(0o700)).unwrap();
        let held = verify_private_leaf(&base).expect("plain");
        assert_eq!(acl_has_write(held.file.as_raw_fd()), Ok(false));
        apply(
            held.file.as_raw_fd(),
            "!#acl 1\ngroup:ABCDEFAB-CDEF-ABCD-EFAB-CDEF0000000C:everyone:12:deny:delete\n",
        );
        assert_eq!(acl_has_write(held.file.as_raw_fd()), Ok(false));
        assert!(verify_private_leaf(&base).is_ok());
        apply(
            held.file.as_raw_fd(),
            "!#acl 1\ngroup:ABCDEFAB-CDEF-ABCD-EFAB-CDEF0000000C:everyone:12:allow:write\n",
        );
        assert_eq!(acl_has_write(held.file.as_raw_fd()), Ok(true));
        assert!(verify_private_leaf(&base).is_err());
        drop(held);
        let _ = std::fs::remove_dir(&base);
    }

    #[test]
    fn refuses_bad_signal_targets() {
        assert_eq!(signal_group(0, SIGTERM), Err("bad-pid"));
        assert_eq!(signal_group(1, SIGKILL), Err("bad-pid"));
        assert_eq!(signal_group(-3, SIGTERM), Err("bad-pid"));
    }
}
