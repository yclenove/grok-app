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
                bytes: 0,
                files: Vec::new(),
            }),
            #[cfg(test)]
            gate: Mutex::new(None),
        })
    }
    pub(crate) fn write(&self, bytes: &[u8], extension: &str) -> io::Result<PathBuf> {
        #[cfg(test)]
        self.pause_for_test();
        let mut area = self.area.lock().unwrap_or_else(|error| error.into_inner());
        if area.bytes.saturating_add(bytes.len()) > MAX_SESSION_BYTES {
            return Err(io::Error::other("session file budget reached"));
        }
        let directory = area
            .directory
            .as_ref()
            .ok_or_else(|| io::Error::other("session closed"))?;
        let path = directory.path().join(format!("{}.{extension}", area.next));
        let mut file = owner_only::create_new(&path)?;
        if let Err(error) = file.write_all(bytes) {
            drop(file);
            let _ = std::fs::remove_file(&path);
            return Err(error);
        }
        area.next += 1;
        area.bytes += bytes.len();
        area.files.push((path.clone(), bytes.len()));
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
            }),
            cv: Condvar::new(),
        })
    }

    fn arm(&self) {
        let mut guard = self.state.lock().unwrap_or_else(|err| err.into_inner());
        guard.armed = true;
        guard.released = false;
        guard.parked = false;
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
    fn pause_for_test(&self) {
        let gate = self
            .gate
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clone();
        if let Some(gate) = gate {
            gate.pause_once();
        }
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
