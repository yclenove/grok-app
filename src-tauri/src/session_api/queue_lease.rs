//! Dispatch-lifetime lease shared by cooperating Hosts and manual recovery.
//! This is not the short JSON store lock; it may live across an async dispatch.
use fs2::FileExt;
use std::{
    collections::HashSet,
    fs::{File, OpenOptions},
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};
fn held() -> &'static Mutex<HashSet<PathBuf>> {
    static HELD: OnceLock<Mutex<HashSet<PathBuf>>> = OnceLock::new();
    HELD.get_or_init(|| Mutex::new(HashSet::new()))
}
pub(super) struct Lease {
    file: File,
    path: PathBuf,
}
impl Drop for Lease {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.file);
        held()
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&self.path);
    }
}
pub(super) fn try_acquire(root: &Path) -> Result<Option<Lease>, String> {
    let path = root
        .canonicalize()
        .map_err(|e| e.to_string())?
        .join("session-api-queue.dispatch-lease");
    let mut keys = held().lock().unwrap_or_else(|p| p.into_inner());
    if keys.contains(&path) {
        return Ok(None);
    }
    let mut options = OpenOptions::new();
    options.create(true).truncate(false).read(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(&path).map_err(|e| e.to_string())?;
    match file.try_lock_exclusive() {
        Ok(()) => {
            keys.insert(path.clone());
            Ok(Some(Lease { file, path }))
        }
        Err(e)
            if e.kind() == std::io::ErrorKind::WouldBlock
                || e.raw_os_error() == fs2::lock_contended_error().raw_os_error() =>
        {
            Ok(None)
        }
        Err(e) => Err(e.to_string()),
    }
}
