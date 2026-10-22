//! Shared dispatch / exclusive review lease. No JSON lock crosses an await.
//! One stable file per receipt namespace; never unlink a potentially locked inode.
use fs2::FileExt;
use std::{
    collections::HashMap,
    fs::{File, OpenOptions},
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};
fn held() -> &'static Mutex<HashMap<PathBuf, (usize, bool)>> {
    static HELD: OnceLock<Mutex<HashMap<PathBuf, (usize, bool)>>> = OnceLock::new();
    HELD.get_or_init(|| Mutex::new(HashMap::new()))
}
pub(super) struct Lease {
    file: File,
    path: PathBuf,
}
impl Drop for Lease {
    fn drop(&mut self) {
        let mut keys = held().lock().unwrap_or_else(|p| p.into_inner());
        let _ = FileExt::unlock(&self.file);
        if let Some((count, _)) = keys.get_mut(&self.path) {
            *count -= 1;
            if *count == 0 {
                keys.remove(&self.path);
            }
        }
    }
}
pub(super) fn try_acquire(path: &Path, exclusive: bool) -> Result<Option<Lease>, String> {
    let parent = path
        .parent()
        .ok_or("receipt parent missing")?
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let name = path
        .file_name()
        .ok_or("receipt filename missing")?
        .to_string_lossy();
    let path = parent.join(format!("{name}.dispatch-lease"));
    let mut keys = held().lock().unwrap_or_else(|p| p.into_inner());
    if keys
        .get(&path)
        .is_some_and(|(_, writer)| exclusive || *writer)
    {
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
    let result = if exclusive {
        FileExt::try_lock_exclusive(&file)
    } else {
        FileExt::try_lock_shared(&file)
    };
    match result {
        Ok(()) => {
            keys.entry(path.clone())
                .and_modify(|v| v.0 += 1)
                .or_insert((1, exclusive));
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
