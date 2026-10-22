//! Operation-scoped protection against preview reclamation across app instances.
//!
//! Order: lifecycle -> lease file / SQLite. Cleanup only tries an exclusive
//! lock, so nested operations can share a guard without a waiting writer. Never
//! retain this guard in the preview cache: an idle cache must not inhibit GC.

use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::path::PathBuf;
use std::sync::{Arc, LazyLock, Mutex, MutexGuard, TryLockError, Weak};
use std::time::{Duration, Instant};

use super::errors::{ErrorCode, ImportError};

type Active = HashMap<PathBuf, Weak<LockedFile>>;
static ACTIVE: LazyLock<Mutex<Active>> = LazyLock::new(|| Mutex::new(HashMap::new()));

#[derive(Debug)]
struct LockedFile(File);

impl Drop for LockedFile {
    fn drop(&mut self) {
        let _ = fs2::FileExt::unlock(&self.0);
    }
}

/// Sendable and nestable, including when several worker threads overlap.
#[derive(Debug)]
pub(crate) struct OperationGuard {
    _file: Arc<LockedFile>,
}

pub(crate) struct CleanupGuard {
    // Unlock the file before admitting another in-process operation.
    _file: LockedFile,
    _active: MutexGuard<'static, Active>,
}

/// Evidence that the caller already owns the lifecycle boundary. In particular,
/// cleanup must not reacquire ACTIVE while its exclusive guard holds that mutex.
pub(super) enum Protection<'a> {
    Operation(&'a OperationGuard),
    Cleanup(&'a CleanupGuard),
}

impl Protection<'_> {
    pub(super) fn held(&self) {
        match self {
            Self::Operation(guard) => {
                let _ = guard;
            }
            Self::Cleanup(guard) => {
                let _ = guard;
            }
        }
    }
}

fn failure(error: impl std::fmt::Display) -> ImportError {
    ImportError::new(ErrorCode::SnapshotUnavailable, error.to_string())
}

fn lock_path() -> PathBuf {
    super::catalog::imports_dir().join("preview-lifecycle.lock")
}

fn open(path: &std::path::Path) -> Result<File, ImportError> {
    std::fs::create_dir_all(super::catalog::imports_dir()).map_err(failure)?;
    super::catalog::ensure_inside_imports(path)?;
    OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)
        .map_err(failure)
}

pub(crate) fn busy(error: &std::io::Error) -> bool {
    error.kind() == std::io::ErrorKind::WouldBlock
        || error.raw_os_error() == fs2::lock_contended_error().raw_os_error()
}

pub(super) fn wait_mutex<'a, T>(
    mutex: &'a Mutex<T>,
    deadline: Instant,
    message: &str,
) -> Result<MutexGuard<'a, T>, ImportError> {
    loop {
        match mutex.try_lock() {
            Ok(guard) => return Ok(guard),
            Err(TryLockError::WouldBlock) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(TryLockError::WouldBlock) => {
                return Err(ImportError::new(ErrorCode::SourceBusy, message));
            }
            Err(TryLockError::Poisoned(_)) => return Err(failure("preview lock poisoned")),
        }
    }
}

pub(crate) fn operation() -> Result<OperationGuard, ImportError> {
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut active = wait_mutex(&ACTIVE, deadline, "preview cleanup is busy")?;
    let path = lock_path();
    if let Some(file) = active.get(&path).and_then(Weak::upgrade) {
        return Ok(OperationGuard { _file: file });
    }
    let file = open(&path)?;
    loop {
        match fs2::FileExt::try_lock_shared(&file) {
            Ok(()) => break,
            Err(error) if busy(&error) && Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(error) if busy(&error) => {
                return Err(ImportError::new(
                    ErrorCode::SourceBusy,
                    "preview cleanup is busy",
                ));
            }
            Err(error) => return Err(failure(error)),
        }
    }
    let file = Arc::new(LockedFile(file));
    active.retain(|_, value| value.strong_count() > 0);
    active.insert(path, Arc::downgrade(&file));
    Ok(OperationGuard { _file: file })
}

/// Skipping one GC pass is safe; waiting behind an active operation is not.
pub(crate) fn try_cleanup() -> Result<Option<CleanupGuard>, ImportError> {
    let active = match ACTIVE.try_lock() {
        Ok(active) => active,
        Err(TryLockError::WouldBlock) => return Ok(None),
        Err(TryLockError::Poisoned(_)) => return Err(failure("preview lock poisoned")),
    };
    let path = lock_path();
    if active.get(&path).and_then(Weak::upgrade).is_some() {
        return Ok(None);
    }
    let file = open(&path)?;
    match fs2::FileExt::try_lock_exclusive(&file) {
        Ok(()) => Ok(Some(CleanupGuard {
            _file: LockedFile(file),
            _active: active,
        })),
        Err(error) if busy(&error) => Ok(None),
        Err(error) => Err(failure(error)),
    }
}
