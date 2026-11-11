#[cfg(test)]
use std::cell::RefCell;
use std::ffi::CString;
use std::fs::File;
use std::io::{self, Read, Write};
use std::os::raw::c_char;
use std::os::unix::fs::MetadataExt;
use std::os::unix::io::{AsRawFd, FromRawFd};
use std::time::{Duration, Instant};
#[cfg(test)]
thread_local! {
    static IO_HOOK: RefCell<Option<Box<dyn Fn() + Send>>> = RefCell::new(None);
}
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
    pub ctime: i64,
    pub ctime_nsec: i64,
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
    Deadline,
    Io,
}
#[derive(Debug)]
pub struct Times {
    pub open_us: u64,
    pub write_us: u64,
    pub fsync_us: u64,
    pub rename_us: u64,
    pub dir_sync_us: Option<u64>,
    pub slice_us: u64,
}
pub struct Dir {
    file: File,
    dev: u64,
    ino: u64,
    /// Parent fd this directory was opened through. -1 when unbound.
    /// Valid only while that parent `Dir` is still alive.
    parent_fd: i32,
    parent_name: String,
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
    Ok(Snap { dev: m.dev(), ino: m.ino(), nlink: m.nlink(), size: m.size(), mtime: m.mtime(), mtime_nsec: m.mtime_nsec(), ctime: m.ctime(), ctime_nsec: m.ctime_nsec(), is_reg: m.mode() & 0o170000 == 0o100000 })
}
fn same(a: &Snap, b: &Snap) -> bool {
    a.dev == b.dev && a.ino == b.ino && a.nlink == b.nlink && a.size == b.size && a.mtime == b.mtime && a.mtime_nsec == b.mtime_nsec && a.ctime == b.ctime && a.ctime_nsec == b.ctime_nsec && a.is_reg == b.is_reg
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
    Ok(Dir { file, dev: s.dev, ino: s.ino, parent_fd: -1, parent_name: String::new() })
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
fn gate(end: Option<Instant>) -> Result<(), RdErr> {
    if let Some(end) = end {
        if past(end) {
            return Err(RdErr::Deadline);
        }
    }
    Ok(())
}
#[cfg(test)]
pub fn test_set_hook(hook: Box<dyn Fn() + Send>) {
    set_hook(hook);
}
#[cfg(test)]
pub fn test_clear_hook() {
    clear_hook();
}
/// One-shot test seam. Production builds call an empty function.
fn io_hook() {
    #[cfg(test)]
    {
        let hook = IO_HOOK.with(|slot| slot.borrow_mut().take());
        if let Some(hook) = hook {
            hook();
        }
    }
}
#[cfg(test)]
fn set_hook(hook: Box<dyn Fn() + Send>) {
    IO_HOOK.with(|slot| *slot.borrow_mut() = Some(hook));
}
#[cfg(test)]
fn clear_hook() {
    IO_HOOK.with(|slot| *slot.borrow_mut() = None);
}
fn create_synced(dir: &Dir, name: &CString, bytes: &[u8], end: Option<Instant>) -> Result<(File, u64, u64, u64), RdErr> {
    gate(end)?;
    let dirfd = dir.file.as_raw_fd();
    let t_open = Instant::now();
    let fd = open_at(dirfd, name.as_ptr(), WRONLY | CREAT | EXCL | NOFOLLOW | CLOEXEC, 0o600);
    if fd < 0 {
        return Err(RdErr::Io);
    }
    let mut file = unsafe { File::from_raw_fd(fd) };
    let open_us = us(t_open.elapsed());
    gate(end)?;
    let t_w = Instant::now();
    if file.write_all(bytes).is_err() {
        let _ = unlink_name(dirfd, name);
        return Err(RdErr::Io);
    }
    let write_us = us(t_w.elapsed());
    gate(end)?;
    let t_s = Instant::now();
    if unsafe { fsync(file.as_raw_fd()) } != 0 {
        let _ = unlink_name(dirfd, name);
        return Err(RdErr::Io);
    }
    let fsync_us = us(t_s.elapsed());
    let src = snap_of(&file).map_err(|_| RdErr::Io)?;
    if !src.is_reg || src.nlink != 1 || src.dev != dir.dev || src.size != bytes.len() as u64 {
        let _ = unlink_name(dirfd, name);
        return Err(RdErr::Replaced);
    }
    gate(end)?;
    Ok((file, open_us, write_us, fsync_us))
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
            // Mount points along an absolute path may change devices. Bind only mailbox children.
            current = current.open_child(comp, false)?;
        }
        Ok(current)
    }
    fn open_child(&self, name: &str, bound: bool) -> Result<Self, RdErr> {
        if !name_ok(name) {
            return Err(RdErr::Name);
        }
        let mut dir = open_dir(self.file.as_raw_fd(), name)?;
        self.require_dir_id(name, dir.dev, dir.ino)?;
        if bound && dir.dev != self.dev {
            return Err(RdErr::Replaced);
        }
        if bound {
            dir.parent_fd = self.file.as_raw_fd();
            dir.parent_name = name.to_string();
        }
        Ok(dir)
    }
    pub fn open_child_dir(&self, name: &str) -> Result<Self, RdErr> {
        self.open_child(name, true)
    }
    pub fn dev(&self) -> u64 {
        self.dev
    }
    fn confirm_bind(&self) -> Result<(), RdErr> {
        if self.parent_fd < 0 {
            return Ok(());
        }
        let c = c_name(&self.parent_name)?;
        let fd = open_at(self.parent_fd, c.as_ptr(), DIRECTORY | NOFOLLOW | CLOEXEC, 0);
        if fd < 0 {
            return Err(RdErr::Replaced);
        }
        let file = unsafe { File::from_raw_fd(fd) };
        let s = snap_of(&file).map_err(|_| RdErr::Io)?;
        if s.dev != self.dev || s.ino != self.ino {
            return Err(RdErr::Replaced);
        }
        Ok(())
    }
    fn require_dir_id(&self, name: &str, dev: u64, ino: u64) -> Result<(), RdErr> {
        let c = c_name(name)?;
        let fd = open_at(self.file.as_raw_fd(), c.as_ptr(), DIRECTORY | NOFOLLOW | CLOEXEC, 0);
        if fd < 0 {
            return Err(RdErr::Replaced);
        }
        let file = unsafe { File::from_raw_fd(fd) };
        let s = snap_of(&file).map_err(|_| RdErr::Io)?;
        if s.dev != dev || s.ino != ino {
            Err(RdErr::Replaced)
        } else {
            Ok(())
        }
    }
    fn require_file_id(&self, name: &str, dev: u64, ino: u64) -> Result<(), RdErr> {
        let (got_dev, got_ino) = self.file_id(name)?;
        if got_dev != dev || got_ino != ino {
            Err(RdErr::Replaced)
        } else {
            Ok(())
        }
    }
    fn file_id(&self, name: &str) -> Result<(u64, u64), RdErr> {
        let c = c_name(name)?;
        let fd = open_at(self.file.as_raw_fd(), c.as_ptr(), NOFOLLOW | NONBLOCK | CLOEXEC, 0);
        if fd < 0 {
            return Err(map_open(io::Error::last_os_error()));
        }
        let file = unsafe { File::from_raw_fd(fd) };
        let s = snap_of(&file).map_err(|_| RdErr::Io)?;
        if !s.is_reg {
            return Err(RdErr::NotFile);
        }
        Ok((s.dev, s.ino))
    }
    pub fn read_file(&self, name: &str, cap: usize, end: Instant) -> Result<(Vec<u8>, u64, u64, u64), RdErr> {
        self.read_at(name, cap, Some(end))
    }
    fn read_at(&self, name: &str, cap: usize, end: Option<Instant>) -> Result<(Vec<u8>, u64, u64, u64), RdErr> {
        let slice = Instant::now();
        gate(end)?;
        self.confirm_bind()?;
        let c = c_name(name)?;
        let t0 = Instant::now();
        let fd = open_at(self.file.as_raw_fd(), c.as_ptr(), NOFOLLOW | NONBLOCK | CLOEXEC, 0);
        if fd < 0 {
            return Err(map_open(io::Error::last_os_error()));
        }
        let mut file = unsafe { File::from_raw_fd(fd) };
        let open_us = us(t0.elapsed());
        gate(end)?;
        let before = snap_of(&file).map_err(|_| RdErr::Io)?;
        check_snap(&before, self.dev, cap)?;
        let t1 = Instant::now();
        let mut buf = vec![0u8; cap + 1];
        let mut filled = 0usize;
        while filled <= cap {
            gate(end)?;
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
        io_hook();
        let after = snap_of(&file).map_err(|_| RdErr::Io)?;
        if !same(&before, &after) {
            return Err(RdErr::Replaced);
        }
        self.require_file_id(name, before.dev, before.ino)?;
        gate(end)?;
        Ok((buf, open_us, read_us, us(slice.elapsed())))
    }
    pub fn publish(&self, tmp: &str, final_name: &str, bytes: &[u8], sync_dir: bool, end: Instant) -> Result<Times, RdErr> {
        self.publish_at(tmp, final_name, bytes, sync_dir, Some(end))
    }
    /// Identity checks still run. `None` does not start a new deadline; only a non-success record uses it.
    pub fn publish_diag(&self, tmp: &str, final_name: &str, bytes: &[u8], sync_dir: bool) -> Result<Times, RdErr> {
        self.publish_at(tmp, final_name, bytes, sync_dir, None)
    }
    fn publish_at(&self, tmp: &str, final_name: &str, bytes: &[u8], sync_dir: bool, end: Option<Instant>) -> Result<Times, RdErr> {
        let slice = Instant::now();
        gate(end)?;
        self.confirm_bind()?;
        let tc = c_name(tmp)?;
        let fc = c_name(final_name)?;
        let (file, open_us, write_us, fsync_us) = create_synced(self, &tc, bytes, end)?;
        let src = match snap_of(&file) {
            Ok(src) => src,
            Err(_) => {
                let _ = unlink_name(self.file.as_raw_fd(), &tc);
                return Err(RdErr::Io);
            }
        };
        io_hook();
        if let Err(err) = self.require_file_id(tmp, src.dev, src.ino).and_then(|_| gate(end)) {
            let _ = unlink_name(self.file.as_raw_fd(), &tc);
            return Err(err);
        }
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
        io_hook();
        self.require_file_id(final_name, src.dev, src.ino)?;
        gate(end)?;
        let named = self.read_at(final_name, bytes.len().saturating_add(1), end)?;
        if named.0 != bytes {
            return Err(RdErr::Replaced);
        }
        let mut dir_sync_us = None;
        if sync_dir {
            gate(end)?;
            let t_d = Instant::now();
            if unsafe { fsync(self.file.as_raw_fd()) } != 0 {
                return Err(RdErr::Io);
            }
            dir_sync_us = Some(us(t_d.elapsed()));
        }
        gate(end)?;
        Ok(Times { open_us, write_us, fsync_us, rename_us, dir_sync_us, slice_us: us(slice.elapsed()) })
    }
    pub fn write_fsync(&self, name: &str, bytes: &[u8], end: Instant) -> Result<Times, RdErr> {
        self.write_at(name, bytes, Some(end))
    }
    fn write_at(&self, name: &str, bytes: &[u8], end: Option<Instant>) -> Result<Times, RdErr> {
        let slice = Instant::now();
        gate(end)?;
        self.confirm_bind()?;
        let c = c_name(name)?;
        let (file, open_us, write_us, fsync_us) = create_synced(self, &c, bytes, end)?;
        let src = match snap_of(&file) {
            Ok(src) => src,
            Err(_) => {
                let _ = unlink_name(self.file.as_raw_fd(), &c);
                return Err(RdErr::Io);
            }
        };
        io_hook();
        if let Err(err) = self.require_file_id(name, src.dev, src.ino).and_then(|_| gate(end)) {
            let _ = unlink_name(self.file.as_raw_fd(), &c);
            return Err(err);
        }
        Ok(Times { open_us, write_us, fsync_us, rename_us: 0, dir_sync_us: None, slice_us: us(slice.elapsed()) })
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

    fn end() -> Instant {
        Instant::now() + Duration::from_secs(2)
    }

    #[test]
    fn rejects_symlink_fifo_hardlink_and_oversize() {
        let root = scratch();
        let dir = Dir::open_path(root.to_str().unwrap()).unwrap();
        let stop = end();
        fs::write(root.join("big"), vec![b'x'; 4097]).unwrap();
        assert_eq!(dir.read_file("big", 4096, stop).unwrap_err(), RdErr::Size);
        fs::write(root.join("reg"), b"hello").unwrap();
        fs::hard_link(root.join("reg"), root.join("hard")).unwrap();
        assert_eq!(dir.read_file("hard", 4096, stop).unwrap_err(), RdErr::HardLink);
        std::os::unix::fs::symlink("reg", root.join("link")).unwrap();
        assert_eq!(dir.read_file("link", 4096, stop).unwrap_err(), RdErr::Symlink);
        let fifo = root.join("fifo");
        let c = CString::new(fifo.to_str().unwrap()).unwrap();
        assert_eq!(unsafe { mkfifo(c.as_ptr(), 0o600) }, 0);
        let err = dir.read_file("fifo", 4096, stop).unwrap_err();
        assert!(matches!(err, RdErr::NotFile | RdErr::Io), "fifo opened as a regular file: {err:?}");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn renamed_open_frame_and_replaced_directory_and_source_are_refused() {
        let root = scratch();
        let dir = Dir::open_path(root.to_str().unwrap()).unwrap();
        let stop = end();
        dir.publish("a.tmp", "a", b"old-bytes", false, stop).unwrap();
        let root_hook = root.clone();
        set_hook(Box::new(move || {
            let _ = fs::rename(root_hook.join("a"), root_hook.join("a.aside"));
            let _ = fs::write(root_hook.join("a"), b"torn-new");
        }));
        let err = dir.read_file("a", 64, stop).unwrap_err();
        assert_eq!(err, RdErr::Replaced, "opened fd must not outlive its name");
        clear_hook();
        let kept = fs::read(root.join("a.aside")).unwrap();
        assert_eq!(kept, b"old-bytes");

        fs::create_dir(root.join("child")).unwrap();
        fs::write(root.join("child/f"), b"inside").unwrap();
        let child = dir.open_child_dir("child").unwrap();
        fs::rename(root.join("child"), root.join("child.aside")).unwrap();
        fs::create_dir(root.join("child")).unwrap();
        fs::write(root.join("child/f"), b"other").unwrap();
        assert_eq!(child.read_file("f", 64, stop).unwrap_err(), RdErr::Replaced);
        assert_eq!(fs::read(root.join("child.aside/f")).unwrap(), b"inside");

        let root_hook = root.clone();
        set_hook(Box::new(move || {
            let _ = fs::rename(root_hook.join("src.tmp"), root_hook.join("src.aside"));
            let _ = fs::write(root_hook.join("src.tmp"), b"same-bytes");
        }));
        let err = dir.publish("src.tmp", "src", b"same-bytes", false, stop).unwrap_err();
        assert_eq!(err, RdErr::Replaced, "substitute inode must not count as the source");
        assert!(!root.join("src").exists(), "refused publish must not leave the final name");
        clear_hook();
        let _ = fs::remove_dir_all(&root);
    }
}
