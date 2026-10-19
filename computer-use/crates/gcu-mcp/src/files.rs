//! Session-owned files; paths and names never come from tool arguments.
use gcu_host_protocol::owner_only::{self, OwnerLock};
use std::{
    io::{self, Write},
    path::PathBuf,
    sync::Mutex,
    time::Duration,
};
use tempfile::TempDir;

const MAX_SESSION_BYTES: usize = 32 * 1024 * 1024;
pub(crate) struct SessionFiles(Mutex<Area>);
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
        Ok(Self(Mutex::new(Area {
            lock: Some(lock),
            directory: Some(directory),
            next: 0,
            bytes: 0,
            files: Vec::new(),
        })))
    }
    pub(crate) fn write(&self, bytes: &[u8], extension: &str) -> io::Result<PathBuf> {
        let mut area = self.0.lock().unwrap_or_else(|error| error.into_inner());
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
    pub(crate) fn export(&self) -> io::Result<Vec<(PathBuf, Vec<u8>)>> {
        let area = self.0.lock().unwrap_or_else(|error| error.into_inner());
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
        let mut area = self.0.lock().unwrap_or_else(|error| error.into_inner());
        area.lock.take();
        if let Some(directory) = area.directory.take() {
            directory.close()?;
        }
        Ok(())
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
