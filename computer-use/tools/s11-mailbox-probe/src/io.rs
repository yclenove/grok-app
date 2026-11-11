use std::ffi::CString;
use std::fs::File;
use std::io::{self, Read, Write};
use std::os::raw::c_char;
use std::os::unix::fs::MetadataExt;
use std::os::unix::io::{AsRawFd, FromRawFd};
use std::time::{Duration, Instant};
const fn pick(mac: i32, arm: i32, other: i32) -> i32 {
    if cfg!(target_os = "macos") {
        mac
    } else if cfg!(any(target_arch = "aarch64", target_arch = "arm")) {
        arm
    } else {
        other
    }
}
const NOFOLLOW: i32 = pick(0x100, 0x8000, 0x20000);
const DIRECTORY: i32 = pick(0x100000, 0x4000, 0x10000);
const CLOEXEC: i32 = pick(0x1000000, 0x80000, 0x80000);
const NONBLOCK: i32 = pick(4, 0x800, 0x800);
const WRONLY: i32 = 1;
const CREAT: i32 = pick(0x200, 0x40, 0x40);
const EXCL: i32 = pick(0x800, 0x80, 0x80);
const ELOOP: i32 = pick(62, 40, 40);
const ENOENT: i32 = 2;
const ENXIO: i32 = 6;
extern "C" {
    #[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
    fn openat(dirfd: i32, path: *const c_char, flags: i32, mode: i32) -> i32;
    fn renameat(olddir: i32, old: *const c_char, newdir: i32, newp: *const c_char) -> i32;
    fn unlinkat(dirfd: i32, path: *const c_char, flags: i32) -> i32;
    fn fsync(fd: i32) -> i32;
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    fn __error() -> *mut i32;
}
// macOS arm64 puts openat's variadic mode on the stack. The syscall keeps it in x3.
fn open_at(dirfd: i32, path: *const c_char, flags: i32, mode: i32) -> i32 {
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    unsafe {
        let (mut ret, mut bad) = (0isize, 0isize);
        std::arch::asm!(
            "svc #0x80", "cset {bad}, cs", in("x16") 0x20001cfisize, inout("x0") dirfd as isize => ret, in("x1") path, in("x2") flags as isize, in("x3") mode as isize, bad = lateout(reg) bad,
        );
        if bad != 0 {
            *__error() = ret as i32;
            -1
        } else {
            ret as i32
        }
    }
    #[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
    unsafe {
        openat(dirfd, path, flags, mode)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Snap {
    pub dev: u64,
    pub ino: u64,
    pub nlink: u64,
    pub size: u64,
    pub mtime: i64,
    pub mtime_nsec: i64,
    pub is_reg: bool,
}
#[derive(Debug, PartialEq, Eq)]
pub enum RdErr {
    Name,
    Absent,
    Symlink,
    NotFile,
    HardLink,
    Size,
    Replaced,
    Io,
}
pub struct Times {
    pub open_us: u64,
    pub write_us: u64,
    pub fsync_us: u64,
    pub rename_us: u64,
    pub dir_sync_us: Option<u64>,
}
pub struct Dir {
    file: File,
    dev: u64,
}
pub fn us(d: Duration) -> u64 {
    u64::try_from(d.as_micros()).unwrap_or(u64::MAX)
}
pub fn past(end: Instant) -> bool {
    Instant::now() >= end
}
fn name_ok(name: &str) -> bool {
    !name.is_empty() && name != "." && name != ".." && !name.contains('/') && !name.contains('\0') && name.is_ascii()
}
fn c_name(name: &str) -> Result<CString, RdErr> {
    if !name_ok(name) {
        return Err(RdErr::Name);
    }
    CString::new(name).map_err(|_| RdErr::Name)
}
fn snap_of(file: &File) -> io::Result<Snap> {
    let m = file.metadata()?;
    Ok(Snap { dev: m.dev(), ino: m.ino(), nlink: m.nlink(), size: m.size(), mtime: m.mtime(), mtime_nsec: m.mtime_nsec(), is_reg: m.mode() & 0o170000 == 0o100000 })
}
fn same(a: &Snap, b: &Snap) -> bool {
    a.dev == b.dev && a.ino == b.ino && a.nlink == b.nlink && a.size == b.size && a.mtime == b.mtime && a.mtime_nsec == b.mtime_nsec && a.is_reg == b.is_reg
}
fn map_open(err: io::Error) -> RdErr {
    match err.raw_os_error() {
        Some(ENOENT) => RdErr::Absent,
        Some(ELOOP) => RdErr::Symlink,
        Some(ENXIO) => RdErr::NotFile,
        _ => RdErr::Io,
    }
}
fn libc_at_fdcwd() -> i32 {
    if cfg!(target_os = "macos") {
        -2
    } else {
        -100
    }
}
fn open_dir(dirfd: i32, name: &str) -> Result<Dir, RdErr> {
    let c = CString::new(name).map_err(|_| RdErr::Name)?;
    let fd = open_at(dirfd, c.as_ptr(), DIRECTORY | NOFOLLOW | CLOEXEC, 0);
    if fd < 0 {
        return Err(map_open(io::Error::last_os_error()));
    }
    let file = unsafe { File::from_raw_fd(fd) };
    let s = snap_of(&file).map_err(|_| RdErr::Io)?;
    if s.is_reg {
        return Err(RdErr::NotFile);
    }
    Ok(Dir { file, dev: s.dev })
}
fn unlink_name(dirfd: i32, name: &CString) -> io::Result<()> {
    if unsafe { unlinkat(dirfd, name.as_ptr(), 0) } == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}
fn check_snap(s: &Snap, dev: u64, cap: usize) -> Result<(), RdErr> {
    if !s.is_reg {
        return Err(RdErr::NotFile);
    }
    if s.nlink == 0 {
        return Err(RdErr::Replaced);
    }
    if s.nlink != 1 {
        return Err(RdErr::HardLink);
    }
    if s.dev != dev {
        return Err(RdErr::Replaced);
    }
    if s.size as usize > cap {
        return Err(RdErr::Size);
    }
    Ok(())
}
fn create_synced(dir: &Dir, name: &CString, bytes: &[u8]) -> Result<(File, u64, u64, u64), RdErr> {
    let dirfd = dir.file.as_raw_fd();
    let t_open = Instant::now();
    let fd = open_at(dirfd, name.as_ptr(), WRONLY | CREAT | EXCL | NOFOLLOW | CLOEXEC, 0o600);
    if fd < 0 {
        return Err(RdErr::Io);
    }
    let mut file = unsafe { File::from_raw_fd(fd) };
    let open_us = us(t_open.elapsed());
    let t_w = Instant::now();
    if file.write_all(bytes).is_err() {
        let _ = unlink_name(dirfd, name);
        return Err(RdErr::Io);
    }
    let write_us = us(t_w.elapsed());
    let t_s = Instant::now();
    if unsafe { fsync(file.as_raw_fd()) } != 0 {
        let _ = unlink_name(dirfd, name);
        return Err(RdErr::Io);
    }
    let src = snap_of(&file).map_err(|_| RdErr::Io)?;
    if !src.is_reg || src.nlink != 1 || src.dev != dir.dev || src.size != bytes.len() as u64 {
        let _ = unlink_name(dirfd, name);
        return Err(RdErr::Replaced);
    }
    Ok((file, open_us, write_us, us(t_s.elapsed())))
}
impl Dir {
    pub fn open_path(path: &str) -> Result<Self, RdErr> {
        if !path.starts_with('/') || path.contains('\0') {
            return Err(RdErr::Name);
        }
        let mut current = open_dir(libc_at_fdcwd(), "/")?;
        for comp in path.split('/').filter(|s| !s.is_empty()) {
            if comp == "." || comp == ".." {
                return Err(RdErr::Name);
            }
            current = current.open_child_dir(comp)?;
        }
        Ok(current)
    }
    pub fn open_child_dir(&self, name: &str) -> Result<Self, RdErr> {
        if !name_ok(name) {
            return Err(RdErr::Name);
        }
        open_dir(self.file.as_raw_fd(), name)
    }
    pub fn dev(&self) -> u64 {
        self.dev
    }
    pub fn read_file(&self, name: &str, cap: usize) -> Result<(Vec<u8>, u64, u64), RdErr> {
        let c = c_name(name)?;
        let t0 = Instant::now();
        let fd = open_at(self.file.as_raw_fd(), c.as_ptr(), NOFOLLOW | NONBLOCK | CLOEXEC, 0);
        if fd < 0 {
            return Err(map_open(io::Error::last_os_error()));
        }
        let mut file = unsafe { File::from_raw_fd(fd) };
        let open_us = us(t0.elapsed());
        let before = snap_of(&file).map_err(|_| RdErr::Io)?;
        check_snap(&before, self.dev, cap)?;
        let t1 = Instant::now();
        let mut buf = vec![0u8; cap + 1];
        let mut filled = 0usize;
        while filled <= cap {
            match file.read(&mut buf[filled..]) {
                Ok(0) => break,
                Ok(n) => filled += n,
                Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
                Err(_) => return Err(RdErr::Io),
            }
        }
        buf.truncate(filled);
        let read_us = us(t1.elapsed());
        if filled > cap {
            return Err(RdErr::Size);
        }
        let after = snap_of(&file).map_err(|_| RdErr::Io)?;
        if !same(&before, &after) {
            return Err(RdErr::Replaced);
        }
        Ok((buf, open_us, read_us))
    }
    pub fn publish(&self, tmp: &str, final_name: &str, bytes: &[u8], sync_dir: bool) -> Result<Times, RdErr> {
        let tc = c_name(tmp)?;
        let fc = c_name(final_name)?;
        let (file, open_us, write_us, fsync_us) = create_synced(self, &tc, bytes)?;
        let src = snap_of(&file).map_err(|_| RdErr::Io)?;
        let t_r = Instant::now();
        let rc = unsafe { renameat(self.file.as_raw_fd(), tc.as_ptr(), self.file.as_raw_fd(), fc.as_ptr()) };
        if rc != 0 {
            let _ = unlink_name(self.file.as_raw_fd(), &tc);
            return Err(RdErr::Io);
        }
        let rename_us = us(t_r.elapsed());
        let moved = snap_of(&file).map_err(|_| RdErr::Io)?;
        if moved.dev != src.dev || moved.ino != src.ino || !moved.is_reg || moved.nlink != 1 {
            return Err(RdErr::Replaced);
        }
        let named = self.read_file(final_name, bytes.len().saturating_add(1))?;
        if named.0 != bytes {
            return Err(RdErr::Replaced);
        }
        let mut dir_sync_us = None;
        if sync_dir {
            let t_d = Instant::now();
            if unsafe { fsync(self.file.as_raw_fd()) } != 0 {
                return Err(RdErr::Io);
            }
            dir_sync_us = Some(us(t_d.elapsed()));
        }
        Ok(Times { open_us, write_us, fsync_us, rename_us, dir_sync_us })
    }
    pub fn write_fsync(&self, name: &str, bytes: &[u8]) -> Result<Times, RdErr> {
        let c = c_name(name)?;
        let (_file, open_us, write_us, fsync_us) = create_synced(self, &c, bytes)?;
        Ok(Times { open_us, write_us, fsync_us, rename_us: 0, dir_sync_us: None })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;
    use std::fs;
    use std::os::raw::c_char;
    use std::os::unix::fs::PermissionsExt;
    use std::sync::atomic::{AtomicU64, Ordering};

    extern "C" {
        fn mkfifo(path: *const c_char, mode: u32) -> i32;
    }

    fn scratch() -> std::path::PathBuf {
        static N: AtomicU64 = AtomicU64::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let p = std::env::temp_dir().join(format!("s11m-io-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir(&p).unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o700)).unwrap();
        // /tmp and /var are symlinks on macOS. O_NOFOLLOW rejects those components.
        fs::canonicalize(&p).unwrap()
    }

    #[test]
    fn rejects_symlink_fifo_hardlink_and_oversize() {
        let root = scratch();
        let dir = Dir::open_path(root.to_str().unwrap()).unwrap();
        fs::write(root.join("big"), vec![b'x'; 4097]).unwrap();
        assert_eq!(dir.read_file("big", 4096).unwrap_err(), RdErr::Size);
        fs::write(root.join("reg"), b"hello").unwrap();
        fs::hard_link(root.join("reg"), root.join("hard")).unwrap();
        assert_eq!(dir.read_file("hard", 4096).unwrap_err(), RdErr::HardLink);
        std::os::unix::fs::symlink("reg", root.join("link")).unwrap();
        assert_eq!(dir.read_file("link", 4096).unwrap_err(), RdErr::Symlink);
        let fifo = root.join("fifo");
        let c = CString::new(fifo.to_str().unwrap()).unwrap();
        assert_eq!(unsafe { mkfifo(c.as_ptr(), 0o600) }, 0);
        let err = dir.read_file("fifo", 4096).unwrap_err();
        assert!(matches!(err, RdErr::NotFile | RdErr::Io), "fifo opened as a regular file: {err:?}");
        let _ = fs::remove_dir_all(&root);
    }
}
