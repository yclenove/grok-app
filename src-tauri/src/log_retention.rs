//! CU log retention: 7 days and 5 files of 5MB.
//!
//! IO failures are returned and reported once as a metadata warning
//! (category and location only). File contents are never included.

use gcu_host_protocol::log_policy::{LOG_FILE_MAX_BYTES, LOG_FILE_MAX_COUNT, RETENTION};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, TryLockError};
use std::time::Duration;

static WARNED: AtomicBool = AtomicBool::new(false);
static IO: Mutex<()> = Mutex::new(());

/// Bounds for one retention pass. Production uses 7 days and 5×5MB.
pub struct Limits {
    /// Maximum size of one file.
    pub max_bytes: u64,
    /// Maximum number of files in one family (`app.log*` or `panic.log*`).
    pub max_files: usize,
    /// Files older than this are deleted.
    pub max_age: Duration,
}

/// Architecture limits: 7 days, 5 files, 5MB each.
pub fn production_limits() -> Limits {
    Limits {
        max_bytes: LOG_FILE_MAX_BYTES,
        max_files: LOG_FILE_MAX_COUNT,
        max_age: RETENTION,
    }
}

/// Metadata warning. Later failures in this process do not repeat it.
pub fn warn_once(category: &str, location: &str) {
    if WARNED.swap(true, Ordering::Relaxed) {
        return;
    }
    let _ = writeln!(io::stderr(), "WARN category={category} location={location}");
}

/// Whether [`warn_once`] has fired. Tests use this as the observable signal.
#[cfg(test)]
fn warned() -> bool {
    WARNED.load(Ordering::Relaxed)
}

#[cfg(test)]
fn reset_warned() {
    WARNED.store(false, Ordering::Relaxed);
}

/// Create the log directory, tighten Unix permissions, and prune both families.
pub fn enforce(dir: &Path, limits: &Limits) -> io::Result<()> {
    fs::create_dir_all(dir).map_err(|error| fail("log_dir", dir, error))?;
    #[cfg(unix)]
    owner_dir(dir).map_err(|error| fail("log_dir_mode", dir, error))?;
    prune(dir, "app.log", limits).map_err(|error| fail("app_log", dir, error))?;
    prune(dir, "panic.log", limits).map_err(|error| fail("panic_log", dir, error))?;
    Ok(())
}

/// Daily app log that rotates at the size cap and prunes on each roll.
pub struct CuLogFile {
    dir: PathBuf,
    day: String,
    file: File,
    len: u64,
    limits: Limits,
}

impl CuLogFile {
    /// Open today's app log after enforcing production limits.
    pub fn open(dir: &Path) -> io::Result<Self> {
        Self::open_limited(dir, production_limits())
    }

    fn open_limited(dir: &Path, limits: Limits) -> io::Result<Self> {
        let _guard = lock();
        enforce(dir, &limits)?;
        let day = today();
        let path = dir.join(format!("app.log.{day}"));
        let file = open_append(&path)?;
        let len = file.metadata()?.len();
        Ok(Self {
            dir: dir.to_path_buf(),
            day,
            file,
            len,
            limits,
        })
    }
}

impl Write for CuLogFile {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let _guard = lock();
        self.write_locked(buf)
            .map_err(|error| fail("app_log", &self.dir, error))
    }

    fn flush(&mut self) -> io::Result<()> {
        let _guard = lock();
        self.file
            .flush()
            .map_err(|error| fail("app_log", &self.dir, error))
    }
}

impl CuLogFile {
    fn write_locked(&mut self, buf: &[u8]) -> io::Result<usize> {
        let day = today();
        if day != self.day {
            self.day = day;
            self.reopen()?;
        }
        if self.len > 0 && self.len.saturating_add(buf.len() as u64) > self.limits.max_bytes {
            self.rotate_today()?;
        }
        let written = self.file.write(buf)?;
        self.len = self.len.saturating_add(written as u64);
        Ok(written)
    }

    fn reopen(&mut self) -> io::Result<()> {
        let path = self.dir.join(format!("app.log.{}", self.day));
        self.file = open_append(&path)?;
        self.len = self.file.metadata()?.len();
        prune(&self.dir, "app.log", &self.limits)?;
        Ok(())
    }

    fn rotate_today(&mut self) -> io::Result<()> {
        self.file.flush()?;
        let active = format!("app.log.{}", self.day);
        shift(&self.dir, &active, self.limits.max_files)?;
        self.file = open_append(&self.dir.join(&active))?;
        self.len = 0;
        prune(&self.dir, "app.log", &self.limits)?;
        Ok(())
    }
}

/// Append one line to `name`, rotating that file when it would pass the size cap.
///
/// Uses a non-blocking lock so a panic hook cannot deadlock on the log writer.
pub fn try_append(dir: &Path, name: &str, line: &str, limits: &Limits) -> io::Result<()> {
    let _guard = match IO.try_lock() {
        Ok(guard) => guard,
        Err(TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
        Err(TryLockError::WouldBlock) => {
            warn_once("log_busy", &dir.display().to_string());
            return Err(io::Error::other("log retention busy"));
        }
    };
    append_locked(dir, name, line, limits).map_err(|error| fail("log_append", dir, error))
}

fn append_locked(dir: &Path, name: &str, line: &str, limits: &Limits) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    #[cfg(unix)]
    owner_dir(dir)?;
    let path = dir.join(name);
    if let Ok(metadata) = fs::metadata(&path) {
        if metadata.len() > 0 && metadata.len().saturating_add(line.len() as u64) > limits.max_bytes
        {
            shift(dir, name, limits.max_files)?;
        }
    }
    let mut file = open_append(&path)?;
    file.write_all(line.as_bytes())?;
    file.flush()?;
    prune(dir, family(name), limits)?;
    Ok(())
}

fn prune(dir: &Path, prefix: &str, limits: &Limits) -> io::Result<()> {
    let mut files = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if !matches_family(name, prefix) {
            continue;
        }
        let metadata = entry.metadata()?;
        if !metadata.is_file() {
            continue;
        }
        if older(&metadata, limits.max_age)? {
            fs::remove_file(entry.path())?;
            continue;
        }
        files.push((entry.path(), metadata.len(), metadata.modified()?));
    }
    files.sort_by_key(|(_, _, modified)| std::cmp::Reverse(*modified));
    while files.len() > limits.max_files {
        let Some((path, _, _)) = files.pop() else {
            break;
        };
        fs::remove_file(path)?;
    }
    for (path, len, _) in &files {
        if *len > limits.max_bytes {
            cap_tail(path, limits.max_bytes)?;
        }
    }
    Ok(())
}

fn matches_family(name: &str, prefix: &str) -> bool {
    name == prefix || name.starts_with(&format!("{prefix}."))
}

fn family(name: &str) -> &str {
    if name.starts_with("panic.log") {
        "panic.log"
    } else {
        "app.log"
    }
}

fn older(metadata: &fs::Metadata, max_age: Duration) -> io::Result<bool> {
    let modified = metadata.modified()?;
    Ok(modified.elapsed().map(|age| age > max_age).unwrap_or(false))
}

fn shift(dir: &Path, active: &str, max_files: usize) -> io::Result<()> {
    let keep = max_files.saturating_sub(1);
    if keep == 0 {
        match fs::remove_file(dir.join(active)) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        return Ok(());
    }
    for index in keep..(keep + 8) {
        remove_if_present(&dir.join(format!("{active}.{index}")))?;
    }
    for index in (1..keep).rev() {
        let from = dir.join(format!("{active}.{index}"));
        if from.exists() {
            fs::rename(&from, dir.join(format!("{active}.{}", index + 1)))?;
        }
    }
    let current = dir.join(active);
    if current.exists() {
        fs::rename(&current, dir.join(format!("{active}.1")))?;
    }
    Ok(())
}

fn remove_if_present(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn cap_tail(path: &Path, max_bytes: u64) -> io::Result<()> {
    let mut input = File::open(path)?;
    let len = input.metadata()?.len();
    if len <= max_bytes {
        return Ok(());
    }
    input.seek(SeekFrom::Start(len - max_bytes))?;
    let mut buf = Vec::new();
    input.read_to_end(&mut buf)?;
    let mut output = File::create(path)?;
    output.write_all(&buf)?;
    output.flush()?;
    #[cfg(unix)]
    owner_file(path)?;
    Ok(())
}

fn open_append(path: &Path) -> io::Result<File> {
    let file = OpenOptions::new().create(true).append(true).open(path)?;
    #[cfg(unix)]
    owner_file(path)?;
    Ok(file)
}

fn today() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

fn lock() -> MutexGuard<'static, ()> {
    IO.lock().unwrap_or_else(|error| error.into_inner())
}

fn fail(category: &str, location: &Path, error: io::Error) -> io::Error {
    warn_once(category, &location.display().to_string());
    error
}

#[cfg(unix)]
fn owner_dir(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
}

#[cfg(unix)]
fn owner_file(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::SystemTime;

    fn temp_dir() -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "gcu-log-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn touch(path: &Path, age: Duration, bytes: &[u8]) {
        fs::write(path, bytes).unwrap();
        let file = File::options().write(true).open(path).unwrap();
        file.set_modified(SystemTime::now().checked_sub(age).unwrap())
            .unwrap();
    }

    fn small() -> Limits {
        Limits {
            max_bytes: 32,
            max_files: 5,
            max_age: Duration::from_secs(7 * 24 * 60 * 60),
        }
    }

    #[test]
    fn retention_deletes_files_older_than_seven_days() {
        let dir = temp_dir();
        touch(
            &dir.join("app.log.2000-01-01"),
            Duration::from_secs(8 * 24 * 60 * 60),
            b"old-secret-text",
        );
        touch(
            &dir.join("panic.log.9"),
            Duration::from_secs(8 * 24 * 60 * 60),
            b"old",
        );
        touch(&dir.join("app.log.keep"), Duration::from_secs(60), b"keep");
        enforce(&dir, &small()).unwrap();
        assert!(!dir.join("app.log.2000-01-01").exists());
        assert!(!dir.join("panic.log.9").exists());
        assert_eq!(fs::read(dir.join("app.log.keep")).unwrap(), b"keep");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn retention_rotates_when_over_five_files_or_five_megabytes() {
        let dir = temp_dir();
        let limits = small();
        for index in 0..6 {
            touch(
                &dir.join(format!("app.log.{index}")),
                Duration::from_secs(1000 - index),
                b"x",
            );
        }
        let fat = vec![b'z'; 80];
        touch(&dir.join("panic.log"), Duration::from_secs(10), &fat);
        enforce(&dir, &limits).unwrap();
        let app_files: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .filter(|name| name.starts_with("app.log"))
            .collect();
        assert_eq!(app_files.len(), 5);
        assert!(!dir.join("app.log.0").exists());
        assert_eq!(fs::metadata(dir.join("panic.log")).unwrap().len(), 32);
        let mut writer = CuLogFile::open_limited(&dir, limits).unwrap();
        let first = b"0123456789abcdef0123456789abcd";
        writer.write_all(first).unwrap();
        writer.write_all(b"NEW").unwrap();
        let day = today();
        let active = fs::read_to_string(dir.join(format!("app.log.{day}"))).unwrap();
        assert!(active.contains("NEW"));
        assert!(!active.contains("0123456789abcdef"));
        let archived = fs::read_to_string(dir.join(format!("app.log.{day}.1"))).unwrap();
        assert!(archived.contains("0123456789abcdef"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn retention_io_error_warns_once() {
        let dir = temp_dir();
        let blocker = dir.join("not-a-directory");
        fs::write(&blocker, b"secret-body").unwrap();
        reset_warned();
        let error = enforce(&blocker, &small());
        assert!(error.is_err());
        assert!(warned());
        assert!(!format!("{error:?}").contains("secret-body"));
        let _ = fs::remove_dir_all(&dir);
    }
}
