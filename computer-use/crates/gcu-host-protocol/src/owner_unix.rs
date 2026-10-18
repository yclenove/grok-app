//! Unix owner-only files. Included by `gcu-host-protocol` and, on Unix only,
//! by `gcu-driver-linux` (that crate cannot depend on this one).
use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

const DIR_MODE: u32 = 0o700;
const FILE_MODE: u32 = 0o600;

pub(crate) fn ensure_dir(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "owner-only directory is a symlink",
            ));
        }
        Ok(meta) if !meta.is_dir() => {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "owner-only path is not a directory",
            ));
        }
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            if let Some(parent) = path.parent() {
                if !parent.as_os_str().is_empty() {
                    fs::create_dir_all(parent)?;
                }
            }
            fs::DirBuilder::new().mode(DIR_MODE).create(path)?;
        }
        Err(error) => return Err(error),
    }
    restrict(path)?;
    verify_dir(path)
}

pub(crate) fn restrict(path: &Path) -> io::Result<()> {
    let meta = fs::symlink_metadata(path)?;
    if meta.file_type().is_symlink() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "owner-only path is a symlink",
        ));
    }
    let directory = meta.is_dir();
    let file = open_nofollow(path, directory)?;
    fchmod(&file, if directory { DIR_MODE } else { FILE_MODE })?;
    verify_fd(&file, directory)
}

pub(crate) fn create_new(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    options.mode(FILE_MODE);
    options.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    let file = options.open(path)?;
    if let Err(error) = fchmod(&file, FILE_MODE).and_then(|_| verify_fd(&file, false)) {
        let _ = fs::remove_file(path);
        return Err(error);
    }
    Ok(file)
}

pub(crate) fn atomic_write(path: &Path, bytes: &[u8], require_sync: bool) -> io::Result<()> {
    reject_symlink(path)?;
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "owner-only file has no parent")
    })?;
    let parent_meta = fs::symlink_metadata(parent)?;
    if parent_meta.file_type().is_symlink() || !parent_meta.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "owner-only parent is not a real directory",
        ));
    }
    let temp = unique_temp(path)?;
    let file = create_new(&temp)?;
    let inode = file.metadata()?.ino();
    let device = file.metadata()?.dev();
    if let Err(error) = write_synced(&file, bytes, require_sync) {
        drop(file);
        let _ = fs::remove_file(&temp);
        return Err(error);
    }
    drop(file);
    if let Err(error) = fs::rename(&temp, path) {
        let _ = fs::remove_file(&temp);
        return Err(error);
    }
    let opened = open_nofollow(path, false)?;
    let meta = opened.metadata()?;
    if meta.ino() != inode || meta.dev() != device {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "owner-only file was replaced during rename",
        ));
    }
    verify_fd(&opened, false)
}

pub(crate) fn read_no_follow(path: &Path, max: u64) -> io::Result<Vec<u8>> {
    let file = open_nofollow(path, false)?;
    verify_fd(&file, false)?;
    let len = file.metadata()?.len();
    if len > max {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "owner-only file exceeds the read limit",
        ));
    }
    let mut bytes = Vec::new();
    io::Read::take(file, len).read_to_end(&mut bytes)?;
    if bytes.len() as u64 != len {
        return Err(io::Error::other("owner-only file changed while reading"));
    }
    Ok(bytes)
}

pub(crate) fn verify_dir(path: &Path) -> io::Result<()> {
    let file = open_nofollow(path, true)?;
    verify_fd(&file, true)
}

pub(crate) fn warn_meta(category: &str, path: &Path, error: &io::Error) {
    let key = format!("{category}:{}", path.display());
    let mut set = warned_set()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if !set.insert(key) {
        return;
    }
    let _ = writeln!(
        io::stderr(),
        "WARN category={category} location={} kind={:?}",
        path.display(),
        error.kind()
    );
}

#[cfg(test)]
pub(crate) fn warned_path(category: &str, path: &Path) -> bool {
    let key = format!("{category}:{}", path.display());
    warned_set()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .contains(&key)
}

fn warned_set() -> &'static Mutex<HashSet<String>> {
    static SET: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    SET.get_or_init(|| Mutex::new(HashSet::new()))
}

pub(crate) struct DirLock {
    file: File,
}

impl Drop for DirLock {
    fn drop(&mut self) {
        // SAFETY: the descriptor is still owned by this lock. Unlock ignores errors.
        unsafe {
            libc::flock(self.file.as_raw_fd(), libc::LOCK_UN);
        }
    }
}

pub(crate) fn hold_dir_lock(dir: &Path) -> io::Result<DirLock> {
    let file = lock_file(dir)?;
    lock_exclusive(&file, true)?;
    Ok(DirLock { file })
}

pub(crate) fn reap_unlocked_children(parent: &Path, prefix: &str, keep: &Path) -> io::Result<()> {
    let entries = match fs::read_dir(parent) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                warn_meta("orphan_scan", parent, &error);
                continue;
            }
        };
        let name = entry.file_name();
        let text = name.to_string_lossy();
        if !text.starts_with(prefix) && !text.starts_with(".gcu-tmp-") {
            continue;
        }
        let path = entry.path();
        if path == keep {
            continue;
        }
        let meta = match fs::symlink_metadata(&path) {
            Ok(meta) => meta,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => {
                warn_meta("orphan_scan", &path, &error);
                continue;
            }
        };
        if meta.file_type().is_symlink() {
            warn_meta(
                "orphan_symlink",
                &path,
                &io::Error::new(io::ErrorKind::PermissionDenied, "symlink"),
            );
            continue;
        }
        if text.starts_with(".gcu-tmp-") {
            remove_any(&path);
            continue;
        }
        if !meta.is_dir() {
            continue;
        }
        match lock_file(&path).and_then(|file| lock_exclusive(&file, false).map(|_| file)) {
            Ok(file) => {
                let trash = unique_temp(&path)?;
                if let Err(error) = fs::rename(&path, &trash) {
                    drop(file);
                    if error.kind() != io::ErrorKind::NotFound {
                        warn_meta("orphan_rename", &path, &error);
                    }
                    continue;
                }
                drop(file);
                remove_any(&trash);
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => warn_meta("orphan_lock", &path, &error),
        }
    }
    Ok(())
}

fn remove_any(path: &Path) {
    let meta = match fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return,
        Err(error) => {
            warn_meta("orphan_delete", path, &error);
            return;
        }
    };
    if meta.file_type().is_symlink() {
        warn_meta(
            "orphan_symlink",
            path,
            &io::Error::new(io::ErrorKind::PermissionDenied, "symlink"),
        );
        return;
    }
    let result = if meta.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    };
    if let Err(error) = result {
        if error.kind() != io::ErrorKind::NotFound {
            warn_meta("orphan_delete", path, &error);
        }
    }
}

fn lock_file(dir: &Path) -> io::Result<File> {
    let path = dir.join(".owner-lock");
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true);
    options.mode(FILE_MODE);
    options.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    let file = options.open(&path)?;
    fchmod(&file, FILE_MODE)?;
    Ok(file)
}

fn lock_exclusive(file: &File, block: bool) -> io::Result<()> {
    let flags = if block {
        libc::LOCK_EX
    } else {
        libc::LOCK_EX | libc::LOCK_NB
    };
    // SAFETY: `file` owns a live descriptor. flock does not consume it.
    let rc = unsafe { libc::flock(file.as_raw_fd(), flags) };
    if rc == 0 {
        return Ok(());
    }
    let error = io::Error::last_os_error();
    if matches!(error.raw_os_error(), Some(code) if code == libc::EWOULDBLOCK || code == libc::EAGAIN)
    {
        Err(io::Error::new(
            io::ErrorKind::WouldBlock,
            "directory lock is held",
        ))
    } else {
        Err(error)
    }
}

fn reject_symlink(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "owner-only path is a symlink",
        )),
        Ok(_) | Err(_) => Ok(()),
    }
}

fn unique_temp(path: &Path) -> io::Result<PathBuf> {
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "owner-only file has no parent")
    })?;
    let base = path.file_name().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "owner-only file has no name")
    })?;
    for _ in 0..8 {
        let mut name = std::ffi::OsString::from(".gcu-tmp-");
        name.push(base);
        name.push(format!(
            "-{}-{:x}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let candidate = parent.join(&name);
        if fs::symlink_metadata(&candidate).is_err() {
            return Ok(candidate);
        }
    }
    Err(io::Error::other("owner-only temporary name collided"))
}

fn write_synced(mut file: &File, bytes: &[u8], require_sync: bool) -> io::Result<()> {
    file.write_all(bytes)?;
    match file.sync_all() {
        Ok(()) => Ok(()),
        Err(error) if !require_sync => {
            warn_meta("owner_sync", Path::new("fd"), &error);
            Ok(())
        }
        Err(error) => Err(error),
    }
}

fn open_nofollow(path: &Path, directory: bool) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true);
    let mut flags = libc::O_NOFOLLOW | libc::O_CLOEXEC;
    if directory {
        flags |= libc::O_DIRECTORY;
    }
    options.custom_flags(flags);
    options.open(path)
}

fn fchmod(file: &File, mode: u32) -> io::Result<()> {
    // SAFETY: `file` owns the descriptor. `mode` is only the permission bits we set.
    let rc = unsafe { libc::fchmod(file.as_raw_fd(), mode as libc::mode_t) };
    if rc == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

fn verify_fd(file: &File, directory: bool) -> io::Result<()> {
    let meta = file.metadata()?;
    if meta.file_type().is_symlink()
        || directory != meta.is_dir()
        || (!directory && !meta.is_file())
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "owner-only path has an unexpected type",
        ));
    }
    // SAFETY: geteuid has no arguments and cannot fail.
    let uid = unsafe { libc::geteuid() };
    let mode = meta.mode() & 0o7777;
    let expect = if directory { DIR_MODE } else { FILE_MODE };
    if meta.uid() != uid || mode != expect || (!directory && meta.nlink() != 1) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "owner-only owner, mode, or link count does not match",
        ));
    }
    Ok(())
}
