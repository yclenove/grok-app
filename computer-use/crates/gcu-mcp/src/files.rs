//! Session-owned files; paths and names never come from tool arguments.
use crate::lease_view::{visible, Epoch};
use gcu_core::scope::ScopeLease;
use gcu_host_protocol::owner_only::{self, OwnerLock};
#[cfg(test)]
use std::sync::Arc;
use std::{
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{Mutex, MutexGuard},
    time::Duration,
};

#[cfg(test)]
mod hooks;
#[cfg(test)]
pub(crate) use hooks::FilePause;
use tempfile::TempDir;

const MAX_SESSION_BYTES: usize = 32 * 1024 * 1024;
/// Observes close/drop while the owner lock is still held. Test only.
#[cfg(test)]
type PurgeProbe = Box<dyn Fn(bool, bool) + Send + Sync>;
pub(crate) struct SessionFiles {
    area: Mutex<Area>,
    /// Runs once, before [`Self::export`] takes the area mutex. Test only.
    #[cfg(test)]
    before_export: Mutex<Option<Box<dyn Fn() + Send + Sync>>>,
    /// Observes [`purge`] while the owner lock is still held. Test only.
    #[cfg(test)]
    purge_probe: Mutex<Option<PurgeProbe>>,
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
            before_export: Mutex::new(None),
            #[cfg(test)]
            purge_probe: Mutex::new(None),
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
    /// Copy ledger rows that are still the bound lease's generation.
    ///
    /// The area mutex is held across the visibility check and the reads. Rows
    /// from any other generation are removed in that same critical section and
    /// unlinked only after the mutex is released. A closed area returns an
    /// error. Unlink errors are ignored. May block on the area mutex and on
    /// the reads. No timeout or cancellation.
    pub(crate) fn export(&self) -> io::Result<Vec<(PathBuf, Vec<u8>)>> {
        #[cfg(test)]
        if let Some(hook) = self
            .before_export
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take()
        {
            hook();
        }
        let mut area = self.area.lock().unwrap_or_else(|error| error.into_inner());
        if area.directory.is_none() {
            return Err(io::Error::other("session closed"));
        }
        let stale = take_stale(&mut area);
        let rows = area
            .files
            .iter()
            .map(|row| {
                let bytes = owner_only::read_no_follow(&row.path, row.len as u64)?;
                if bytes.len() != row.len {
                    return Err(io::Error::other("session file changed"));
                }
                Ok((row.path.clone(), bytes))
            })
            .collect();
        drop(area);
        for path in stale {
            let _ = std::fs::remove_file(path);
        }
        rows
    }
    /// Delete every registered file and the session directory.
    ///
    /// Artifact unlinks run while the owner lock and the directory object are
    /// both still held. The lock is released only after those unlinks, and on
    /// Unix the directory itself is removed before the lock. A second call is
    /// a no-op. Blocking: the area mutex and the deletes. No timeout or
    /// cancellation. Unlink errors are ignored; a directory-removal error is
    /// returned.
    pub(crate) fn close(&self) -> io::Result<()> {
        #[cfg(test)]
        let probe = self
            .purge_probe
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take();
        let mut area = self.area.lock().unwrap_or_else(|error| error.into_inner());
        purge(
            &mut area,
            #[cfg(test)]
            probe,
        )
    }
}

impl Drop for SessionFiles {
    fn drop(&mut self) {
        #[cfg(test)]
        let probe = self
            .purge_probe
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take();
        let mut area = self.area.lock().unwrap_or_else(|error| error.into_inner());
        let _ = purge(
            &mut area,
            #[cfg(test)]
            probe,
        );
    }
}

/// Remove invisible rows. Caller unlinks `paths` after releasing the area mutex.
fn take_stale(area: &mut Area) -> Vec<PathBuf> {
    let lease = area.lease.clone();
    let mut kept = Vec::with_capacity(area.files.len());
    let mut stale = Vec::new();
    let mut stale_len = 0usize;
    for row in area.files.drain(..) {
        if visible(lease.as_ref(), row.epoch.as_ref()) {
            kept.push(row);
        } else {
            stale_len = stale_len.saturating_add(row.len);
            stale.push(row.path);
        }
    }
    area.bytes = area.bytes.saturating_sub(stale_len);
    area.files = kept;
    stale
}

/// Delete registered files, then the directory. See [`SessionFiles::close`].
fn purge(area: &mut Area, #[cfg(test)] probe: Option<PurgeProbe>) -> io::Result<()> {
    let paths: Vec<PathBuf> = area.files.drain(..).map(|row| row.path).collect();
    area.bytes = 0;
    for path in &paths {
        let _ = std::fs::remove_file(path);
    }
    #[cfg(test)]
    if let Some(probe) = probe {
        let lock_held = area.lock.is_some() && area.directory.is_some();
        let files_gone = paths.iter().all(|path| !path.exists());
        probe(lock_held, files_gone);
    }
    let Some(directory) = area.directory.take() else {
        area.lock.take();
        return Ok(());
    };
    let root = directory.path().to_path_buf();
    // The owner lock is still inside `area`. Removing the directory while that
    // file is open succeeds on Unix and keeps the name out of the orphan scan.
    match std::fs::remove_dir_all(&root) {
        Ok(()) => {
            // The directory is already gone. Disarm TempDir so its destructor
            // does not try the removal again.
            let _kept = directory.keep();
            area.lock.take();
            Ok(())
        }
        Err(error) => {
            // Windows cannot delete `.owner-lock` while this handle is open.
            // Registered files are already unlinked above.
            area.lock.take();
            match directory.close() {
                Ok(()) => Ok(()),
                Err(_) => Err(error),
            }
        }
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
