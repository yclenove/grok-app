//! Session-owned files; paths and names never come from tool arguments.
use crate::lease_view::{visible, Epoch};
use crate::retire::{CallbackSlot, Hold, Sweep};
use gcu_core::scope::ScopeLease;
use gcu_host_protocol::owner_only::{self, OwnerLock};
use std::{
    io::{self, Write},
    ops::{Deref, DerefMut},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard, TryLockError},
    time::Duration,
};
use tempfile::TempDir;

#[cfg(test)]
#[path = "file_gate.rs"]
mod file_gate;
#[cfg(test)]
pub(crate) use file_gate::FilePause;

const MAX_SESSION_BYTES: usize = 32 * 1024 * 1024;
pub(crate) struct SessionFiles {
    sweep_id: u64,
    callbacks: CallbackSlot,
    /// Tickets a publish left because this mutex was busy or already held here.
    sweep: Sweep,
    area: Mutex<Area>,
    /// Signals once, before the sweep takes `area`. Test only.
    #[cfg(test)]
    before_sweep: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
    /// Test pause installed by [`SessionFiles::arm_file_gate`]. Absent in production.
    #[cfg(test)]
    gate: Mutex<Option<Arc<FilePause>>>,
}
struct Area {
    lock: Option<OwnerLock>,
    /// Kept alive across unlink so `close` cannot free the name first.
    directory: Option<Arc<TempDir>>,
    next: u64,
    stage_seq: u64,
    bytes: usize,
    files: Vec<StoredFile>,
    lease: Option<ScopeLease>,
}
struct StaleBatch {
    paths: Vec<PathBuf>,
    /// Holds the directory inode until the unlinks below finish.
    _directory: Option<Arc<TempDir>>,
}

/// Area mutex plus a deferred sweep after it unlocks.
///
/// A publish that cannot `try_lock` this mutex leaves a [`Sweep`] ticket.
/// Drop unlocks first, then catches up, so revoke does not wait on a holder
/// that is itself waiting for revoke.
struct AreaLock<'a> {
    store: &'a SessionFiles,
    guard: Option<MutexGuard<'a, Area>>,
}

impl<'a> AreaLock<'a> {
    fn new(store: &'a SessionFiles) -> Self {
        Self {
            store,
            guard: Some(store.area.lock().unwrap_or_else(|error| error.into_inner())),
        }
    }
}

impl Deref for AreaLock<'_> {
    type Target = Area;
    fn deref(&self) -> &Area {
        match self.guard.as_ref() {
            Some(area) => area,
            None => unreachable!("area lock used after drop"),
        }
    }
}

impl DerefMut for AreaLock<'_> {
    fn deref_mut(&mut self) -> &mut Area {
        match self.guard.as_mut() {
            Some(area) => area,
            None => unreachable!("area lock used after drop"),
        }
    }
}

impl Drop for AreaLock<'_> {
    fn drop(&mut self) {
        self.guard.take();
        self.store.flush_retired();
    }
}
struct StoredFile {
    path: PathBuf,
    len: usize,
    epoch: Option<Epoch>,
}
/// Validated ledger insertion. Its lock is acquired before the lease lock.
pub(crate) struct FileAdmission<'a> {
    files: &'a SessionFiles,
    hold: Option<Hold>,
    area: Option<MutexGuard<'a, Area>>,
    rows: Vec<StoredFile>,
    epoch: Option<Epoch>,
}
impl FileAdmission<'_> {
    /// Generation copied from the authority pairs inside the commit closure.
    ///
    /// Memory only. No await, timeout, or cancellation. `None` is unscoped.
    pub(crate) fn stamp(&mut self, epoch: Option<Epoch>) {
        self.epoch = epoch;
    }

    /// Memory-only, infallible registration of the batch validated by `admission`.
    ///
    /// No timeout or cancellation. A closed admission registers nothing.
    pub(crate) fn commit(&mut self, staged: &mut [StagedFile]) {
        let Some(area) = self.area.as_mut() else {
            return;
        };
        let epoch = self.epoch.clone();
        for mut row in self.rows.drain(..) {
            area.bytes += row.len;
            row.epoch = epoch.clone();
            area.files.push(row);
        }
        for file in staged {
            file.committed = true;
        }
    }
}

impl Drop for FileAdmission<'_> {
    fn drop(&mut self) {
        self.area.take();
        self.hold.take();
        self.files.flush_retired();
    }
}
impl SessionFiles {
    pub(crate) fn new() -> io::Result<Self> {
        let directory = tempfile::Builder::new().prefix("gcu-session-").tempdir()?;
        owner_only::ensure_dir(directory.path())?;
        let lock = owner_only::hold_dir_lock(directory.path())?;
        if let Some(parent) = directory.path().parent() {
            if let Err(error) =
                owner_only::reap_unlocked_children(parent, "gcu-session-", directory.path())
            {
                owner_only::warn_meta("orphan_scan", parent, &error);
            }
        }
        Ok(Self {
            sweep_id: crate::retire::next_sweep_id(),
            callbacks: CallbackSlot::new(),
            sweep: Sweep::new(),
            area: Mutex::new(Area {
                lock: Some(lock),
                directory: Some(Arc::new(directory)),
                next: 0,
                stage_seq: 0,
                bytes: 0,
                files: Vec::new(),
                lease: None,
            }),
            #[cfg(test)]
            before_sweep: Mutex::new(None),
            #[cfg(test)]
            gate: Mutex::new(None),
        })
    }
    /// Write and register in one step. Prefer [`Self::stage`] when a later check
    /// must still be able to drop the bytes before they become exportable.
    ///
    /// Test helper. Production registers only after the commit-boundary check.
    #[cfg(test)]
    pub(crate) fn write(&self, bytes: &[u8], extension: &str) -> io::Result<PathBuf> {
        let staged = self.stage(bytes, extension)?;
        self.commit(staged)
    }
    /// Write to an unregistered name. Export requires a committed [`Self::admission`].
    ///
    /// Dropping the result deletes the file. May wait on the test pause, then on
    /// the area mutex. Test pauses are bounded; the caller bounds
    /// the blocking task. A closed area or a full budget returns an error and
    /// leaves the ledger unchanged.
    pub(crate) fn stage(&self, bytes: &[u8], extension: &str) -> io::Result<StagedFile> {
        #[cfg(test)]
        let gate = self.pause_for_test();
        let mut area = AreaLock::new(self);
        if area.bytes.saturating_add(bytes.len()) > MAX_SESSION_BYTES {
            return Err(io::Error::other("session file budget reached"));
        }
        let root = area
            .directory
            .as_ref()
            .ok_or_else(|| io::Error::other("session closed"))?
            .path()
            .to_path_buf();
        let seq = area.stage_seq;
        area.stage_seq = area.stage_seq.saturating_add(1);
        let path = root.join(format!(".part-{seq}.{}", sanitize_extension(extension)));
        let mut file = owner_only::create_new(&path)?;
        if let Err(error) = file.write_all(bytes) {
            drop(file);
            let _ = std::fs::remove_file(&path);
            return Err(error);
        }
        drop(file);
        drop(area);
        Ok(StagedFile {
            path,
            len: bytes.len(),
            extension: sanitize_extension(extension).to_owned(),
            committed: false,
            #[cfg(test)]
            gate,
        })
    }
    /// Rename a staged file to its export name without registering it.
    ///
    /// The ledger and [`Self::export`] ignore it until its admission is committed.
    /// Dropping it still deletes the bytes. May wait on the area mutex. No
    /// await, timeout, or cancellation. A closed area leaves the staged name
    /// in place for its guard to delete.
    pub(crate) fn seal(&self, staged: &mut StagedFile) -> io::Result<PathBuf> {
        let mut area = AreaLock::new(self);
        let directory = area
            .directory
            .as_ref()
            .ok_or_else(|| io::Error::other("session closed"))?;
        let path = directory
            .path()
            .join(format!("{}.{}", area.next, staged.extension));
        std::fs::rename(&staged.path, &path)?;
        area.next = area.next.saturating_add(1);
        drop(area);
        staged.path = path.clone();
        Ok(path)
    }
    /// Prepare to register already-sealed files. All of them, or none.
    ///
    /// Memory only aside from the area mutex. No rename, await, timeout, or
    /// cancellation. Acquires the ledger lock and reserves space before the
    /// lease commit point. Failure leaves the ledger unchanged and the caller
    /// still owns all staged guards. Dropping the admission registers nothing.
    pub(crate) fn admission(&self, staged: &[StagedFile]) -> io::Result<FileAdmission<'_>> {
        let hold = Hold::enter(self.sweep_id);
        let mut area = self.area.lock().unwrap_or_else(|error| error.into_inner());
        if area.directory.is_none() {
            drop(area);
            drop(hold);
            self.flush_retired();
            return Err(io::Error::other("session closed"));
        }
        let extra = staged
            .iter()
            .try_fold(0usize, |sum, file| sum.checked_add(file.len));
        let Some(extra) = extra else {
            drop(area);
            drop(hold);
            self.flush_retired();
            return Err(io::Error::other("session file budget reached"));
        };
        if area.bytes.saturating_add(extra) > MAX_SESSION_BYTES {
            drop(area);
            drop(hold);
            self.flush_retired();
            return Err(io::Error::other("session file budget reached"));
        }
        let rows = staged
            .iter()
            .map(|file| StoredFile {
                path: file.path.clone(),
                len: file.len,
                epoch: None,
            })
            .collect();
        area.files.reserve(staged.len());
        Ok(FileAdmission {
            files: self,
            hold: Some(hold),
            area: Some(area),
            rows,
            epoch: None,
        })
    }

    /// Remember the lease that later exports compare, and retire on invalidation.
    ///
    /// Registers a weak callback on `lease`. Revoke, rebind, and a lease read
    /// that publishes expiry run that callback on the calling thread. Dropping
    /// this store drops the callback. A second bind arms the new lease and
    /// drops the previous hook. Expiry is not watched: if nobody reads the
    /// lease, rows stay until that read. Disk unlink stays outside both locks.
    /// No timeout or cancellation.
    pub(crate) fn bind_lease(self: &Arc<Self>, lease: ScopeLease) {
        {
            let mut area = AreaLock::new(self);
            area.lease = Some(lease.clone());
        }
        let files = Arc::downgrade(self);
        let callback: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
            if let Some(files) = files.upgrade() {
                files.release_stale();
            }
        });
        self.callbacks.arm(&lease, callback);
        self.release_stale();
    }

    /// Remove ledger rows that are no longer visible and delete their files.
    ///
    /// The area lock and the lease lock are both dropped before unlink. The
    /// ticket is stored before `try_lock`. A busy mutex, or `Hold` already set
    /// on this thread, returns without waiting: the holder's guard catches up
    /// after it unlocks. Same-thread `try_lock` is unspecified, so this thread
    /// never tries it while `Hold` is set. No timeout or cancellation. A
    /// closed area leaves the disk to [`Self::close`]. Unlink errors are
    /// ignored. At most eight passes; a still-open ticket waits for the next
    /// unlock instead of spinning.
    pub(crate) fn release_stale(&self) {
        if Hold::contains(self.sweep_id) {
            self.sweep.request();
            return;
        }
        #[cfg(test)]
        self.fire_before_sweep();
        self.sweep.request();
        self.drive();
    }

    fn flush_retired(&self) {
        if Hold::contains(self.sweep_id) {
            self.sweep.request();
            return;
        }
        self.drive();
    }

    fn drive(&self) {
        for _ in 0..8 {
            let target = self.sweep.requested();
            if self.sweep.finished() >= target {
                return;
            }
            let Some(batch) = self.extract() else {
                return;
            };
            for path in &batch.paths {
                let _ = std::fs::remove_file(path);
            }
        }
    }

    /// `None` means another thread holds the area lock.
    fn extract(&self) -> Option<StaleBatch> {
        let _hold = Hold::enter(self.sweep_id);
        let mut area = match self.area.try_lock() {
            Ok(guard) => guard,
            Err(TryLockError::Poisoned(err)) => err.into_inner(),
            Err(TryLockError::WouldBlock) => return None,
        };
        // Sample before the lease read. A publish during `visible` opens a
        // newer ticket that this pass must not mark finished.
        let seen = self.sweep.requested();
        if area.directory.is_none() {
            self.sweep.mark(seen);
            return Some(StaleBatch {
                paths: Vec::new(),
                _directory: None,
            });
        }
        let directory = area.directory.clone();
        let lease = area.lease.clone();
        let mut stale_len = 0usize;
        let mut paths = Vec::new();
        area.files.retain(|row| {
            if visible(lease.as_ref(), row.epoch.as_ref()) {
                true
            } else {
                stale_len = stale_len.saturating_add(row.len);
                paths.push(row.path.clone());
                false
            }
        });
        area.bytes = area.bytes.saturating_sub(stale_len);
        self.sweep.mark(seen);
        Some(StaleBatch {
            paths,
            _directory: directory,
        })
    }
    /// Rename a staged file into the export ledger.
    ///
    /// The area mutex is held across the rename and the ledger insert. No await,
    /// timeout, or cancellation. On failure the staged file is left for its guard
    /// to delete and the ledger is unchanged.
    #[cfg(test)]
    fn commit(&self, mut staged: StagedFile) -> io::Result<PathBuf> {
        let mut area = AreaLock::new(self);
        if area.bytes.saturating_add(staged.len) > MAX_SESSION_BYTES {
            return Err(io::Error::other("session file budget reached"));
        }
        let directory = area
            .directory
            .as_ref()
            .ok_or_else(|| io::Error::other("session closed"))?;
        let path = directory
            .path()
            .join(format!("{}.{}", area.next, staged.extension));
        std::fs::rename(&staged.path, &path)?;
        staged.path = path.clone();
        area.next = area.next.saturating_add(1);
        area.bytes += staged.len;
        area.files.push(StoredFile {
            path: path.clone(),
            len: staged.len,
            epoch: None,
        });
        staged.committed = true;
        Ok(path)
    }
    pub(crate) fn export(&self) -> io::Result<Vec<(PathBuf, Vec<u8>)>> {
        self.release_stale();
        let area = AreaLock::new(self);
        if area.directory.is_none() {
            return Err(io::Error::other("session closed"));
        }
        area.files
            .iter()
            .map(|row| {
                let bytes = owner_only::read_no_follow(&row.path, row.len as u64)?;
                if bytes.len() != row.len {
                    return Err(io::Error::other("session file changed"));
                }
                Ok((row.path.clone(), bytes))
            })
            .collect()
    }
    /// Drop the invalidation callback, then delete the session directory.
    ///
    /// An unlink that already cloned the directory keeps that inode until it
    /// finishes, so this call cannot hand the name to a later store first.
    /// Blocking: the area mutex and the directory delete. Timeout: none.
    /// Cancellation: none. A second call is a no-op.
    pub(crate) fn close(&self) -> io::Result<()> {
        self.callbacks.clear();
        let directory = {
            let mut area = AreaLock::new(self);
            area.lock.take();
            area.directory.take()
        };
        match directory {
            Some(directory) => match Arc::try_unwrap(directory) {
                Ok(directory) => directory.close(),
                Err(shared) => {
                    drop(shared);
                    Ok(())
                }
            },
            None => Ok(()),
        }
    }
}

/// Ledger lock held by a test thread. Drop unlocks, then flushes a deferred sweep.
#[cfg(test)]
pub(crate) struct AreaHold<'a> {
    files: &'a SessionFiles,
    hold: Option<Hold>,
    guard: Option<MutexGuard<'a, Area>>,
}

#[cfg(test)]
impl Drop for AreaHold<'_> {
    fn drop(&mut self) {
        self.guard.take();
        self.hold.take();
        self.files.flush_retired();
    }
}

impl SessionFiles {
    /// Park the next file write until the returned gate is released.
    ///
    /// Test only. Memory only. No timeout or cancellation. The pause happens
    /// before the file-area mutex is taken, so export can run while it waits.
    /// Only the first write parks.
    #[cfg(test)]
    pub(crate) fn arm_file_gate(&self) -> Arc<FilePause> {
        self.arm_file_gate_at(1)
    }

    /// Park the `nth` file write (1-based). Earlier writes do not wait.
    ///
    /// Test only. Memory only. No timeout or cancellation. The pause happens
    /// before the file-area mutex is taken. `1` matches [`Self::arm_file_gate`].
    #[cfg(test)]
    pub(crate) fn arm_file_gate_at(&self, nth: u32) -> Arc<FilePause> {
        let pause = FilePause::new();
        pause.arm_skip(nth.saturating_sub(1));
        *self.gate.lock().unwrap_or_else(|err| err.into_inner()) = Some(Arc::clone(&pause));
        pause
    }

    /// Hold the ledger lock until the guard drops.
    ///
    /// The drop flushes a sweep that was deferred because this thread held the
    /// lock. Test only. Blocking: the area mutex. No timeout or cancellation.
    #[cfg(test)]
    pub(crate) fn lock_area_for_test(&self) -> AreaHold<'_> {
        let hold = Hold::enter(self.sweep_id);
        let guard = self.area.lock().unwrap_or_else(|error| error.into_inner());
        AreaHold {
            files: self,
            hold: Some(hold),
            guard: Some(guard),
        }
    }

    /// Run `hook` once, on the next sweep, before it takes the ledger lock.
    ///
    /// Test only. Memory only. No timeout or cancellation.
    #[cfg(test)]
    pub(crate) fn set_before_sweep(&self, hook: Arc<dyn Fn() + Send + Sync>) {
        *self
            .before_sweep
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some(hook);
    }

    #[cfg(test)]
    fn fire_before_sweep(&self) {
        let hook = self
            .before_sweep
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take();
        if let Some(hook) = hook {
            hook();
        }
    }

    #[cfg(test)]
    fn pause_for_test(&self) -> Option<Arc<FilePause>> {
        let gate = self
            .gate
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clone();
        gate.filter(|gate| gate.pause_once())
    }

    /// Whether any file in this session directory contains `needle`.
    ///
    /// Includes names that are not in the export ledger. A sweep ticket left
    /// by a busy publish is finished after the mutex drops and before the
    /// directory is read. Test only. May wait on the area mutex. No timeout
    /// or cancellation.
    #[cfg(test)]
    pub(crate) fn contains_bytes(&self, needle: &[u8]) -> bool {
        if needle.is_empty() {
            return false;
        }
        let root = {
            let area = self.area.lock().unwrap_or_else(|err| err.into_inner());
            area.directory.as_ref().map(|dir| dir.path().to_path_buf())
        };
        self.flush_retired();
        let Some(root) = root else {
            return false;
        };
        let Ok(entries) = std::fs::read_dir(root) else {
            return false;
        };
        for entry in entries.flatten() {
            let Ok(bytes) = std::fs::read(entry.path()) else {
                continue;
            };
            if bytes.windows(needle.len()).any(|window| window == needle) {
                return true;
            }
        }
        false
    }
}

/// Unregistered file. Drop deletes its bytes until its admission is committed.
pub(crate) struct StagedFile {
    path: PathBuf,
    len: usize,
    extension: String,
    committed: bool,
    #[cfg(test)]
    gate: Option<Arc<FilePause>>,
}

impl StagedFile {
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for StagedFile {
    fn drop(&mut self) {
        if !self.committed {
            let _ = std::fs::remove_file(&self.path);
        }
        #[cfg(test)]
        if let Some(gate) = self.gate.take() {
            gate.note_finished();
        }
    }
}

fn sanitize_extension(extension: &str) -> &str {
    if !extension.is_empty() && extension.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
        extension
    } else {
        "bin"
    }
}

pub(crate) async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> io::Result<T> + Send + 'static,
) -> io::Result<T> {
    tokio::time::timeout(Duration::from_secs(5), tokio::task::spawn_blocking(work))
        .await
        .map_err(|_| io::Error::other("file/image work timed out"))?
        .map_err(|_| io::Error::other("file/image worker failed"))?
}
