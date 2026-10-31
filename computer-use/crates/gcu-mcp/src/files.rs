//! Session-owned files; paths and names never come from tool arguments.
use crate::lease_view::{visible, Epoch};
use gcu_core::scope::ScopeLease;
use gcu_host_protocol::owner_only::{self, OwnerLock};
#[cfg(test)]
use std::sync::{Arc, Condvar};
use std::{
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{Mutex, MutexGuard},
    time::Duration,
};
use tempfile::TempDir;

const MAX_SESSION_BYTES: usize = 32 * 1024 * 1024;
pub(crate) struct SessionFiles {
    area: Mutex<Area>,
    /// Test pause installed by [`SessionFiles::arm_file_gate`]. Absent in production.
    #[cfg(test)]
    gate: Mutex<Option<Arc<FilePause>>>,
}
struct Area {
    lock: Option<OwnerLock>,
    directory: Option<TempDir>,
    next: u64,
    stage_seq: u64,
    bytes: usize,
    files: Vec<StoredFile>,
    lease: Option<ScopeLease>,
}
struct StoredFile {
    path: PathBuf,
    len: usize,
    epoch: Option<Epoch>,
}
/// Validated ledger insertion. Its lock is acquired before the lease lock.
pub(crate) struct FileAdmission<'a> {
    area: MutexGuard<'a, Area>,
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
    pub(crate) fn commit(&mut self, staged: &mut [StagedFile]) {
        let epoch = self.epoch.clone();
        for mut row in self.rows.drain(..) {
            self.area.bytes += row.len;
            row.epoch = epoch.clone();
            self.area.files.push(row);
        }
        for file in staged {
            file.committed = true;
        }
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
            area: Mutex::new(Area {
                lock: Some(lock),
                directory: Some(directory),
                next: 0,
                stage_seq: 0,
                bytes: 0,
                files: Vec::new(),
                lease: None,
            }),
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
        let mut area = self.area.lock().unwrap_or_else(|error| error.into_inner());
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
        let mut area = self.area.lock().unwrap_or_else(|error| error.into_inner());
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
        let mut area = self.area.lock().unwrap_or_else(|error| error.into_inner());
        if area.directory.is_none() {
            return Err(io::Error::other("session closed"));
        }
        let extra = staged
            .iter()
            .try_fold(0usize, |sum, file| sum.checked_add(file.len))
            .ok_or_else(|| io::Error::other("session file budget reached"))?;
        if area.bytes.saturating_add(extra) > MAX_SESSION_BYTES {
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
            area,
            rows,
            epoch: None,
        })
    }

    /// Remember the lease that later exports compare. Memory only.
    ///
    /// Call before taking the area lock for a commit. No timeout or cancellation.
    pub(crate) fn bind_lease(&self, lease: ScopeLease) {
        self.area
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .lease = Some(lease);
    }
    /// Rename a staged file into the export ledger.
    ///
    /// The area mutex is held across the rename and the ledger insert. No await,
    /// timeout, or cancellation. On failure the staged file is left for its guard
    /// to delete and the ledger is unchanged.
    #[cfg(test)]
    fn commit(&self, mut staged: StagedFile) -> io::Result<PathBuf> {
        let mut area = self.area.lock().unwrap_or_else(|error| error.into_inner());
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
        let mut area = self.area.lock().unwrap_or_else(|error| error.into_inner());
        if area.directory.is_none() {
            return Err(io::Error::other("session closed"));
        }
        let lease = area.lease.clone();
        let mut stale_len = 0usize;
        let mut stale_paths = Vec::new();
        area.files.retain(|row| {
            if visible(lease.as_ref(), row.epoch.as_ref()) {
                true
            } else {
                stale_len = stale_len.saturating_add(row.len);
                stale_paths.push(row.path.clone());
                false
            }
        });
        area.bytes = area.bytes.saturating_sub(stale_len);
        for path in &stale_paths {
            let _ = std::fs::remove_file(path);
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
    pub(crate) fn close(&self) -> io::Result<()> {
        let mut area = self.area.lock().unwrap_or_else(|error| error.into_inner());
        area.lock.take();
        if let Some(directory) = area.directory.take() {
            directory.close()?;
        }
        Ok(())
    }
}
/// One-shot pause around the first session-file write in a test.
///
/// Memory only. The first [`SessionFiles::write`] waits until [`Self::release`].
/// Later writes do not wait. Panics after 15 seconds. No cancellation. Production never
/// constructs this value.
#[cfg(test)]
pub(crate) struct FilePause {
    state: Mutex<PauseState>,
    cv: Condvar,
}

#[cfg(test)]
struct PauseState {
    armed: bool,
    used: bool,
    parked: bool,
    released: bool,
    finished: bool,
    /// Writes to ignore before the one that parks. Zero parks the next write.
    skip: u32,
}

#[cfg(test)]
impl FilePause {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(PauseState {
                armed: false,
                used: false,
                parked: false,
                released: false,
                finished: false,
                skip: 0,
            }),
            cv: Condvar::new(),
        })
    }

    fn arm_skip(&self, skip: u32) {
        let mut guard = self.state.lock().unwrap_or_else(|err| err.into_inner());
        guard.armed = true;
        guard.released = false;
        guard.parked = false;
        guard.finished = false;
        guard.used = false;
        guard.skip = skip;
    }

    fn pause_once(&self) -> bool {
        let mut guard = self.state.lock().unwrap_or_else(|err| err.into_inner());
        if !guard.armed || guard.used {
            return false;
        }
        if guard.skip > 0 {
            guard.skip -= 1;
            return false;
        }
        guard.used = true;
        guard.parked = true;
        self.cv.notify_all();
        let (mut guard, timeout) = self
            .cv
            .wait_timeout_while(guard, Duration::from_secs(15), |state| !state.released)
            .unwrap_or_else(|err| err.into_inner());
        let expired = timeout.timed_out() && !guard.released;
        guard.parked = false;
        guard.armed = false;
        drop(guard);
        assert!(
            !expired,
            "file test gate was not released within 15 seconds"
        );
        true
    }

    /// Unblock the write parked in this gate.
    ///
    /// Memory only. No timeout or cancellation.
    pub(crate) fn release(&self) {
        let mut guard = self.state.lock().unwrap_or_else(|err| err.into_inner());
        guard.released = true;
        guard.armed = false;
        self.cv.notify_all();
    }

    /// Whether the first write is waiting. Memory only. No timeout or cancellation.
    pub(crate) fn is_parked(&self) -> bool {
        self.state
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .parked
    }

    /// The staged file from this gate was dropped or committed.
    ///
    /// Memory only. No timeout or cancellation.
    pub(crate) fn has_finished(&self) -> bool {
        self.state
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .finished
    }

    fn note_finished(&self) {
        let mut guard = self.state.lock().unwrap_or_else(|err| err.into_inner());
        guard.finished = true;
        self.cv.notify_all();
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
    /// Includes names that are not in the export ledger. Test only. May wait on
    /// the area mutex and reads the directory. No timeout or cancellation.
    #[cfg(test)]
    pub(crate) fn contains_bytes(&self, needle: &[u8]) -> bool {
        if needle.is_empty() {
            return false;
        }
        let root = {
            let area = self.area.lock().unwrap_or_else(|err| err.into_inner());
            area.directory.as_ref().map(|dir| dir.path().to_path_buf())
        };
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
