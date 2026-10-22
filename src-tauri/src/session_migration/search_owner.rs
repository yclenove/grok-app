//! One owner per revision across threads and processes. Never unlink the lock:
//! a new inode could otherwise admit another owner while the old one is held.
//! Order: preview lifecycle -> revision owner -> index SQLite. Release ownership
//! before catalog projection; owned callbacks must not reacquire this lock.

use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex, MutexGuard, TryLockError, Weak};
use std::time::{Duration, Instant};

use super::{index_backup_path, index_error, index_path, sqlite_open, INDEX_SCHEMA_VERSION};
use crate::session_migration::errors::{ErrorCode, ImportError};
use rusqlite::OpenFlags;

type Owners = HashMap<PathBuf, Weak<Mutex<()>>>;
static OWNERS: LazyLock<Mutex<Owners>> = LazyLock::new(|| Mutex::new(HashMap::new()));
pub(crate) const LOCK_NAME: &str = "event-index.lock";

struct FileOwner(File);

impl Drop for FileOwner {
    fn drop(&mut self) {
        let _ = fs2::FileExt::unlock(&self.0);
    }
}

fn cancelled(cancel: &dyn Fn() -> bool) -> Result<(), ImportError> {
    if cancel() {
        Err(ImportError::new(
            ErrorCode::PlanStale,
            "index rebuild cancelled",
        ))
    } else {
        Ok(())
    }
}

fn busy() -> ImportError {
    ImportError::new(ErrorCode::SourceBusy, "event index is being rebuilt")
}

fn wait<'a, T>(
    mutex: &'a Mutex<T>,
    deadline: Instant,
    cancel: &dyn Fn() -> bool,
) -> Result<MutexGuard<'a, T>, ImportError> {
    loop {
        cancelled(cancel)?;
        match mutex.try_lock() {
            Ok(guard) => return Ok(guard),
            Err(TryLockError::WouldBlock) if Instant::now() < deadline => {
                // Armed only while acquiring the revision mutex. The OWNERS
                // registry wait leaves the flag clear, so it cannot release the fixture.
                #[cfg(test)]
                revision_owner_wait::contended_if_armed();
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(TryLockError::WouldBlock) => return Err(busy()),
            Err(TryLockError::Poisoned(_)) => return Err(index_error("index owner lock poisoned")),
        }
    }
}

pub(super) fn with<T>(
    dir: &Path,
    cancel: &dyn Fn() -> bool,
    action: impl FnOnce() -> Result<T, ImportError>,
) -> Result<T, ImportError> {
    let deadline = Instant::now() + Duration::from_secs(3);
    cancelled(cancel)?;
    let key = std::fs::canonicalize(dir).map_err(index_error)?;
    let process = {
        let mut owners = wait(&OWNERS, deadline, cancel)?;
        owners.retain(|_, value| value.strong_count() > 0);
        if let Some(owner) = owners.get(&key).and_then(Weak::upgrade) {
            owner
        } else {
            let owner = Arc::new(Mutex::new(()));
            owners.insert(key.clone(), Arc::downgrade(&owner));
            owner
        }
    };
    // Same wait() as the registry lock above. The arm covers only this call.
    let _process = {
        #[cfg(test)]
        let _arm = revision_owner_wait::arm();
        wait(&process, deadline, cancel)?
    };
    let path = key.join(LOCK_NAME);
    let exists = match std::fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.file_type().is_file() => true,
        Ok(_) => return Err(index_error("event index owner is not a regular file")),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(error) => return Err(index_error(error)),
    };
    // Existing future/locked artifacts must be rejected without even creating
    // a sentinel. Mutating callers repeat validation after gaining ownership.
    if !exists {
        validate_artifacts(dir)?;
    }
    cancelled(cancel)?;
    let file = OpenOptions::new()
        .read(true)
        // Ownership changes the OS lock state, not existing sentinel contents.
        .write(!exists)
        .create(!exists)
        .truncate(false)
        .open(path)
        .map_err(index_error)?;
    loop {
        cancelled(cancel)?;
        match fs2::FileExt::try_lock_exclusive(&file) {
            Ok(()) => break,
            Err(error) if crate::session_migration::preview_lifecycle::busy(&error) => {
                if Instant::now() >= deadline {
                    return Err(busy());
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(error) => return Err(index_error(error)),
        }
    }
    let _file = FileOwner(file);
    cancelled(cancel)?;
    action()
}

pub(super) fn validate_artifacts(dir: &Path) -> Result<(), ImportError> {
    let live = index_path(dir);
    let tmp = dir.join("event-index.sqlite.tmp");
    let backup = index_backup_path(&live);
    // Read only schema versions: a future format need not have current tables.
    for path in [&live, &tmp, &backup] {
        if !path.try_exists().map_err(index_error)? {
            continue;
        }
        let version = sqlite_open::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .and_then(|conn| conn.query_row("PRAGMA user_version", [], |row| row.get::<_, i32>(0)));
        match version {
            Ok(version) if version > INDEX_SCHEMA_VERSION => {
                return Err(ImportError::new(
                    ErrorCode::ImportSchemaTooNew,
                    format!("event index schema {version} is newer than this app"),
                ));
            }
            Ok(_) => {}
            Err(error)
                if matches!(
                    error.sqlite_error_code(),
                    Some(rusqlite::ErrorCode::DatabaseCorrupt | rusqlite::ErrorCode::NotADatabase)
                ) => {}
            Err(error) => return Err(index_error(error)),
        }
    }
    Ok(())
}

#[cfg(test)]
pub(crate) use revision_owner_wait::observe as observe_revision_owner_wait;

#[cfg(test)]
mod revision_owner_wait {
    use std::cell::{Cell, RefCell};
    use std::sync::mpsc::{Receiver, Sender};

    struct Probe {
        reached: Sender<()>,
        release: Receiver<()>,
    }

    thread_local! {
        static PROBE: RefCell<Option<Probe>> = const { RefCell::new(None) };
        static ARMED: Cell<bool> = const { Cell::new(false) };
    }

    struct Clear;

    impl Drop for Clear {
        fn drop(&mut self) {
            PROBE.with(|slot| {
                slot.borrow_mut().take();
            });
        }
    }

    pub(super) struct Arm;

    impl Drop for Arm {
        fn drop(&mut self) {
            ARMED.with(|flag| flag.set(false));
        }
    }

    pub(super) fn arm() -> Arm {
        // Construct the guard first so a panic still clears the flag.
        let guard = Arm;
        ARMED.with(|flag| flag.set(true));
        guard
    }

    pub(crate) fn observe<T>(
        reached: Sender<()>,
        release: Receiver<()>,
        action: impl FnOnce() -> T,
    ) -> T {
        PROBE.with(|slot| {
            *slot.borrow_mut() = Some(Probe { reached, release });
        });
        let _clear = Clear;
        action()
    }

    pub(super) fn contended_if_armed() {
        if !ARMED.with(|flag| flag.get()) {
            return;
        }
        let probe = PROBE.with(|slot| slot.borrow_mut().take());
        if let Some(probe) = probe {
            let _ = probe.reached.send(());
            // The caller arms cancellation, then sends this release.
            // Dropping the sender also unblocks a failed test.
            let _ = probe.release.recv();
        }
    }
}
