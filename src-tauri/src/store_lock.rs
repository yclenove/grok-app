//! Advisory file locks for App JSON store (E06 shared / multi-instance).
//!
//! CLI and a second App window may touch the same data root in `shared` mode.
//! We take an exclusive lock around read-modify-write of index files so the
//! index is not half-written, and use temp+rename for atomic replace.

use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, TryLockError};
use std::thread;
use std::time::{Duration, Instant};

use fs2::FileExt;

/// How long to wait for a lock before failing with a clear error.
const LOCK_WAIT: Duration = Duration::from_secs(3);
const LOCK_POLL: Duration = Duration::from_millis(40);

/// `fs2` locks coordinate separate processes, but some platforms treat a
/// second descriptor in the same process as re-entrant.  Keep an in-process
/// guard as well so two threads can never interleave a read-modify-write
/// transaction (the common path for stream/journal updates).
static PROCESS_STORE_LOCK: Mutex<()> = Mutex::new(());

/// Sidecar lock path for `foo.json` → `foo.json.lock`.
pub fn lock_path_for(target: &Path) -> PathBuf {
    let mut s = target.as_os_str().to_os_string();
    s.push(".lock");
    PathBuf::from(s)
}

/// Holds an exclusive lock until dropped.
pub struct ExclusiveLock {
    // The guard must live for the full file-lock lifetime.  A single process
    // mutex is intentionally conservative: store transactions are short and
    // correctness matters more than parallel JSON writes.
    _process_guard: MutexGuard<'static, ()>,
    files: Vec<File>,
}

impl Drop for ExclusiveLock {
    fn drop(&mut self) {
        // Best-effort unlock; File drop also releases the lock on most OSes.
        for file in &self.files {
            let _ = file.unlock();
        }
    }
}

/// Acquire exclusive lock for `target` (creates `target.lock`).
pub fn lock_exclusive(target: &Path) -> Result<ExclusiveLock, String> {
    lock_exclusive_many(&[target.to_path_buf()])
}

/// Acquire related stores in a deterministic order with one process guard.
/// Callbacks must use write_bytes_replace and must not acquire store locks again.
pub fn lock_exclusive_many(targets: &[PathBuf]) -> Result<ExclusiveLock, String> {
    lock_exclusive_many_in(targets, &|| Ok(()), true)
}

/// Like [`lock_exclusive_many`], but `guard` runs after the process lock is held
/// and before any lock file is opened. A guard error creates nothing.
/// `guard` may run more than once and must not acquire store or runtime locks.
/// Missing parent directories are not created: a deleted session directory must
/// not come back just so its sidecar can be locked.
pub fn lock_exclusive_many_guarded(
    targets: &[PathBuf],
    guard: &dyn Fn() -> Result<(), String>,
) -> Result<ExclusiveLock, String> {
    lock_exclusive_many_in(targets, guard, false)
}

fn lock_exclusive_many_in(
    targets: &[PathBuf],
    guard: &dyn Fn() -> Result<(), String>,
    create_parent: bool,
) -> Result<ExclusiveLock, String> {
    let process_guard = PROCESS_STORE_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    guard()?;
    let mut targets = targets.to_vec();
    targets.sort();
    targets.dedup();
    let mut files = Vec::new();
    for target in targets {
        guard()?;
        files.push(lock_file_checked(&target, None, &|| Ok(()), create_parent)?);
    }
    Ok(ExclusiveLock {
        _process_guard: process_guard,
        files,
    })
}

/// Cancellable publication waits must never hold a runtime-map lock. One
/// deadline covers the process mutex AND all sidecars; ordinary writers keep
/// their existing locking contract above. `check` must not acquire store or
/// runtime locks (otherwise callers could reintroduce an ABBA lock inversion).
pub(crate) fn lock_exclusive_many_checked(
    targets: &[PathBuf],
    check: &dyn Fn() -> Result<(), String>,
) -> Result<ExclusiveLock, String> {
    let deadline = Instant::now() + LOCK_WAIT;
    let process_guard = loop {
        check()?;
        match PROCESS_STORE_LOCK.try_lock() {
            Ok(guard) => break guard,
            Err(TryLockError::Poisoned(error)) => break error.into_inner(),
            Err(TryLockError::WouldBlock) if Instant::now() < deadline => {
                thread::sleep(LOCK_POLL);
            }
            Err(TryLockError::WouldBlock) => {
                return Err("LOCK_BUSY: process store lock exceeded publication budget".into());
            }
        }
    };
    let mut targets = targets.to_vec();
    targets.sort();
    targets.dedup();
    let mut files = Vec::new();
    for target in targets {
        files.push(lock_file_checked(&target, Some(deadline), check, true)?);
    }
    Ok(ExclusiveLock {
        _process_guard: process_guard,
        files,
    })
}

fn lock_file_checked(
    target: &Path,
    deadline: Option<Instant>,
    check: &dyn Fn() -> Result<(), String>,
    create_parent: bool,
) -> Result<File, String> {
    check()?;
    let path = lock_path_for(target);
    if let Some(parent) = path.parent() {
        if create_parent {
            fs::create_dir_all(parent).map_err(|e| format!("lock dir: {e}"))?;
        } else {
            match parent.try_exists() {
                Ok(true) => {}
                Ok(false) => {
                    return Err(format!("lock dir missing: {}", parent.display()));
                }
                Err(error) => return Err(format!("lock dir: {error}")),
            }
        }
    }
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&path)
        .map_err(|e| format!("open lock {}: {e}", path.display()))?;

    // Ordinary sidecars retain their full wait after directory/open work.
    // Publication supplies a deadline shared with its process mutex and other
    // sidecars; never reset that deadline when a later file is opened.
    let deadline = deadline.unwrap_or_else(|| Instant::now() + LOCK_WAIT);
    loop {
        check()?;
        match file.try_lock_exclusive() {
            Ok(()) => {
                return Ok(file);
            }
            Err(_) if Instant::now() < deadline => {
                thread::sleep(LOCK_POLL);
            }
            Err(e) => {
                return Err(format!(
                    "LOCK_BUSY: could not lock {} within {}ms ({e})",
                    path.display(),
                    LOCK_WAIT.as_millis()
                ));
            }
        }
    }
}

/// Run `body` while holding an exclusive lock on `target`.
pub fn with_exclusive_lock<T>(
    target: &Path,
    body: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    let _lock = lock_exclusive(target)?;
    body()
}

/// Write bytes to `path` via temp file + rename (caller must hold the lock).
pub fn write_bytes_replace(path: &Path, bytes: &[u8]) -> Result<(), String> {
    write_bytes_replace_checked(path, bytes, &|| Ok(()))
}

/// Cancellation before replacement leaves the original bytes untouched. Once
/// rename succeeds the durable receipt is history, not a claim of runtime Ready;
/// the manager still checks the exact owner after IO before publishing Ready.
pub(crate) fn write_bytes_replace_checked(
    path: &Path,
    bytes: &[u8],
    check: &dyn Fn() -> Result<(), String>,
) -> Result<(), String> {
    write_bytes_replace_in(path, bytes, check, true)
}

/// Atomic replace that never creates `path`'s parent. `rename` does not create
/// missing directories either; refusing `create_dir_all` is what stops a late
/// draft write from resurrecting a session directory deleted underneath it.
pub(crate) fn write_bytes_replace_existing(path: &Path, bytes: &[u8]) -> Result<(), String> {
    write_bytes_replace_in(path, bytes, &|| Ok(()), false)
}

fn write_bytes_replace_in(
    path: &Path,
    bytes: &[u8],
    check: &dyn Fn() -> Result<(), String>,
    create_parent: bool,
) -> Result<(), String> {
    check()?;
    if let Some(parent) = path.parent() {
        if create_parent {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        } else {
            match parent.try_exists() {
                Ok(true) => {}
                Ok(false) => {
                    return Err(format!("parent directory is gone: {}", parent.display()));
                }
                Err(error) => return Err(format!("parent directory: {error}")),
            }
        }
    }
    let tmp = {
        let mut p = path.as_os_str().to_os_string();
        p.push(".tmp");
        PathBuf::from(p)
    };
    fs::write(&tmp, bytes).map_err(|e| format!("write temp: {e}"))?;
    if let Err(error) = check() {
        let _ = fs::remove_file(&tmp);
        return Err(error);
    }
    let parent_gone = !create_parent && path.parent().is_some_and(|parent| !parent.exists());
    if parent_gone {
        let _ = fs::remove_file(&tmp);
        let parent = path
            .parent()
            .map(|parent| parent.display().to_string())
            .unwrap_or_default();
        return Err(format!("parent directory is gone: {parent}"));
    }
    fs::rename(&tmp, path).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        format!("rename into place: {e}")
    })?;
    Ok(())
}

/// Write bytes to `path` via temp file + rename under exclusive lock.
pub fn write_bytes_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    with_exclusive_lock(path, || write_bytes_replace(path, bytes))
}

/// True if error string is a lock contention failure.
#[allow(dead_code)]
pub fn is_lock_busy(err: &str) -> bool {
    err.contains("LOCK_BUSY")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Barrier};
    use std::thread;

    fn tmp_file(name: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!(
            "grok-store-lock-{}-{}-{}",
            name,
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        p
    }

    #[test]
    fn lock_path_suffix() {
        let p = PathBuf::from("/tmp/sessions_index.json");
        assert_eq!(
            lock_path_for(&p),
            PathBuf::from("/tmp/sessions_index.json.lock")
        );
    }

    #[test]
    fn atomic_write_roundtrip() {
        let path = tmp_file("atomic.json");
        write_bytes_atomic(&path, br#"{"ok":true}"#).unwrap();
        let s = fs::read_to_string(&path).unwrap();
        assert!(s.contains("ok"));
        let _ = fs::remove_file(&path);
        let _ = fs::remove_file(lock_path_for(&path));
    }

    #[test]
    fn exclusive_blocks_second_holder() {
        let path = tmp_file("block.json");
        let barrier = Arc::new(Barrier::new(2));
        let path2 = path.clone();
        let b2 = Arc::clone(&barrier);

        let t = thread::spawn(move || {
            let _lock = lock_exclusive(&path2).expect("first lock");
            b2.wait();
            // Hold long enough for the other thread to time out path.
            thread::sleep(Duration::from_millis(200));
        });

        barrier.wait();
        // Second lock should fail quickly if we shrink wait — use direct try.
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(lock_path_for(&path))
            .unwrap();
        let busy = file.try_lock_exclusive().is_err();
        assert!(busy, "second exclusive lock should be busy");
        t.join().unwrap();
        let _ = fs::remove_file(&path);
        let _ = fs::remove_file(lock_path_for(&path));
    }

    #[test]
    fn is_lock_busy_detects_prefix() {
        assert!(is_lock_busy("LOCK_BUSY: could not lock"));
        assert!(!is_lock_busy("write temp: disk full"));
    }

    fn contended_sidecar_after_preparation(
        deadline: Option<Instant>,
        preparation_delay: Duration,
    ) -> (Result<(), String>, usize) {
        use std::cell::{Cell, RefCell};

        let path = tmp_file("preparation.json");
        let holder = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(lock_path_for(&path))
            .unwrap();
        holder.try_lock_exclusive().unwrap();
        let holder = RefCell::new(Some(holder));
        let checks = Cell::new(0);
        let result = lock_file_checked(
            &path,
            deadline,
            &|| {
                let index = checks.get();
                checks.set(index + 1);
                if index == 0 {
                    // Deterministically model time spent before opening a sidecar.
                    // This is not a claim about measuring slow real filesystem IO.
                    thread::sleep(preparation_delay);
                } else if index == 2 {
                    // Release only after the first real fs2 acquisition failed.
                    drop(holder.borrow_mut().take());
                }
                Ok(())
            },
            true,
        );
        // Release every descriptor and remove only this owned synthetic sidecar
        // BEFORE asserting, including when the old implementation times out.
        drop(holder.borrow_mut().take());
        let result = result.map(drop);
        let _ = fs::remove_file(lock_path_for(&path));
        (result, checks.get())
    }

    #[test]
    fn ordinary_sidecar_budget_starts_after_preparation() {
        let (result, checks) = contended_sidecar_after_preparation(None, LOCK_WAIT + LOCK_POLL);
        assert!(
            result.is_ok(),
            "ordinary sidecar lost its wait budget: {result:?}"
        );
        assert_eq!(checks, 3);
    }

    #[test]
    fn publication_sidecar_keeps_its_original_shared_deadline() {
        let (result, checks) =
            contended_sidecar_after_preparation(Some(Instant::now() + LOCK_POLL), LOCK_POLL * 2);
        let error = result.expect_err("publication reset its deadline after opening the sidecar");
        assert!(is_lock_busy(&error), "{error}");
        assert_eq!(checks, 2);
    }

    #[test]
    fn cancellation_after_temp_write_preserves_original_and_removes_owned_temp() {
        let path = tmp_file("cancel-replace.json");
        write_bytes_atomic(&path, b"original").unwrap();
        let _held = lock_exclusive(&path).unwrap();
        let checks = std::sync::atomic::AtomicUsize::new(0);
        let result = write_bytes_replace_checked(&path, b"replacement", &|| {
            if checks.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
                Ok(())
            } else {
                Err("synthetic cancellation before rename".into())
            }
        });
        assert!(result.is_err());
        assert_eq!(fs::read(&path).unwrap(), b"original");
        let mut tmp = path.as_os_str().to_os_string();
        tmp.push(".tmp");
        assert!(!PathBuf::from(tmp).exists());
        drop(_held);
        let _ = fs::remove_file(&path);
        let _ = fs::remove_file(lock_path_for(&path));
    }

    #[test]
    fn guarded_lock_does_not_create_a_missing_parent() {
        let dir = tmp_file("guard-missing");
        let target = dir.join("import_continuation.json");
        let refused = match lock_exclusive_many_guarded(std::slice::from_ref(&target), &|| Ok(())) {
            Ok(_lock) => panic!("guarded lock created a missing parent"),
            Err(error) => error,
        };
        assert!(refused.starts_with("lock dir missing:"), "{refused}");
        assert!(!dir.exists());
        assert!(!lock_path_for(&target).exists());

        let blocked = lock_exclusive_many_guarded(&[target], &|| {
            Err("continuation session no longer exists".into())
        });
        let blocked = match blocked {
            Ok(_lock) => panic!("guard ran after the parent was already gone"),
            Err(error) => error,
        };
        assert_eq!(blocked, "continuation session no longer exists");
        assert!(!dir.exists());
    }

    #[test]
    fn replace_existing_does_not_recreate_a_removed_parent() {
        let dir = tmp_file("removed-parent");
        fs::create_dir(&dir).unwrap();
        let path = dir.join("handoff_draft.txt");
        fs::write(&path, b"old").unwrap();
        fs::remove_dir_all(&dir).unwrap();
        let error = write_bytes_replace_existing(&path, b"new").unwrap_err();
        assert!(error.contains("parent directory is gone"), "{error}");
        assert!(!dir.exists());
        assert!(!path.exists());
        let mut tmp = path.as_os_str().to_os_string();
        tmp.push(".tmp");
        assert!(!PathBuf::from(tmp).exists());
    }
}
