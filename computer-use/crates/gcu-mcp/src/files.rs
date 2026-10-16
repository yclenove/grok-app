//! Session-owned files; paths and names never come from tool arguments.
use std::{
    fs::OpenOptions,
    io::{self, Read, Write},
    path::PathBuf,
    sync::Mutex,
    time::Duration,
};
use tempfile::TempDir;

const MAX_SESSION_BYTES: usize = 32 * 1024 * 1024;
pub(crate) struct SessionFiles(Mutex<Area>);
struct Area {
    directory: Option<TempDir>,
    next: u64,
    bytes: usize,
    files: Vec<(PathBuf, usize)>,
}
impl SessionFiles {
    pub(crate) fn new() -> io::Result<Self> {
        let directory = tempfile::Builder::new().prefix("gcu-session-").tempdir()?;
        // tempfile creates Unix directories with 0700. Set it explicitly too;
        // descendants are inaccessible to other users from their first write.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700))?;
        }
        Ok(Self(Mutex::new(Area {
            directory: Some(directory),
            next: 0,
            bytes: 0,
            files: Vec::new(),
        })))
    }
    pub(crate) fn write(&self, bytes: &[u8], extension: &str) -> io::Result<PathBuf> {
        let mut area = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if area.bytes.saturating_add(bytes.len()) > MAX_SESSION_BYTES {
            return Err(io::Error::other("session file budget reached"));
        }
        let directory = area
            .directory
            .as_ref()
            .ok_or_else(|| io::Error::other("session closed"))?;
        let path = directory.path().join(format!("{}.{extension}", area.next));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&path)?;
        file.write_all(bytes)?;
        area.next += 1;
        area.bytes += bytes.len();
        area.files.push((path.clone(), bytes.len()));
        Ok(path)
    }
    pub(crate) fn export(&self) -> io::Result<Vec<(PathBuf, Vec<u8>)>> {
        let area = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if area.directory.is_none() {
            return Err(io::Error::other("session closed"));
        }
        area.files
            .iter()
            .map(|(path, size)| {
                if !std::fs::symlink_metadata(path)?.is_file() {
                    return Err(io::Error::other("session file replaced"));
                }
                let mut bytes = Vec::new();
                std::fs::File::open(path)?
                    .take(*size as u64 + 1)
                    .read_to_end(&mut bytes)?;
                if bytes.len() != *size {
                    return Err(io::Error::other("session file changed"));
                }
                Ok((path.clone(), bytes))
            })
            .collect()
    }
    pub(crate) fn close(&self) -> io::Result<()> {
        let mut area = self.0.lock().unwrap_or_else(|e| e.into_inner());
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
