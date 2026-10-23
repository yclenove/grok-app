//! Session-owned files; paths and names never come from tool arguments.
use gcu_host_protocol::owner_only::{self, OwnerLock};
#[cfg(test)]
use std::sync::{Arc, Condvar};
use std::{
    io::{self, Write},
    path::{Path, PathBuf},
    sync::Mutex,
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
    files: Vec<(PathBuf, usize)>,
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
    /// Write to an unregistered name. The file is not exported until [`Self::commit`].
    ///
    /// Dropping the result deletes the file. May wait on the test pause, then on
    /// the area mutex. No timeout or cancellation of its own; the caller bounds
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
    /// Rename a staged file into the export ledger.
    ///
    /// The area mutex is held across the rename and the ledger insert. No await,
    /// timeout, or cancellation. On failure the staged file is left for its guard
    /// to delete and the ledger is unchanged.
    pub(crate) fn commit(&self, mut staged: StagedFile) -> io::Result<PathBuf> {
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
        area.files.push((path.clone(), staged.len));
        staged.committed = true;
        Ok(path)
    }
    /// Drop one file created by this session and its ledger entry.
    ///
    /// Memory plus one unlink. May wait on the area mutex. No timeout or
    /// cancellation. Missing paths and missing ledger entries are ignored.
    pub(crate) fn discard(&self, path: &Path) {
        let mut area = self.area.lock().unwrap_or_else(|error| error.into_inner());
        if let Some(index) = area.files.iter().position(|(saved, _)| saved == path) {
            let (_, size) = area.files.remove(index);
            area.bytes = area.bytes.saturating_sub(size);
        }
        drop(area);
        let _ = std::fs::remove_file(path);
    }
    pub(crate) fn export(&self) -> io::Result<Vec<(PathBuf, Vec<u8>)>> {
        let area = self.area.lock().unwrap_or_else(|error| error.into_inner());
        if area.directory.is_none() {
            return Err(io::Error::other("session closed"));
        }
        area.files
            .iter()
            .map(|(path, size)| {
                let bytes = owner_only::read_no_follow(path, *size as u64)?;
                if bytes.len() != *size {
                    return Err(io::Error::other("session file changed"));
                }
                Ok((path.clone(), bytes))
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
/// Later writes do not wait. No timeout or cancellation. Production never
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
            }),
            cv: Condvar::new(),
        })
    }

    fn arm(&self) {
        let mut guard = self.state.lock().unwrap_or_else(|err| err.into_inner());
        guard.armed = true;
        guard.released = false;
        guard.parked = false;
        guard.finished = false;
    }

    fn pause_once(&self) {
        let mut guard = self.state.lock().unwrap_or_else(|err| err.into_inner());
        if !guard.armed || guard.used {
            return;
        }
        guard.used = true;
        guard.parked = true;
        self.cv.notify_all();
        while !guard.released {
            guard = self.cv.wait(guard).unwrap_or_else(|err| err.into_inner());
        }
        guard.parked = false;
        guard.armed = false;
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
        let pause = FilePause::new();
        pause.arm();
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
        if let Some(gate) = &gate {
            gate.pause_once();
        }
        gate
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

/// Unregistered file. Dropping it deletes the bytes unless [`SessionFiles::commit`] ran.
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
