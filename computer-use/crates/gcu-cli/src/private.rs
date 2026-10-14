//! Private filesystem areas; symlinks and unknown owners/permissions fail closed.
pub(crate) mod unsafe_platform;
use crate::exit::Failure;
use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

pub(crate) fn runtime_root() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
}
pub(crate) fn state_dir() -> Result<PathBuf, Failure> {
    #[cfg(windows)]
    let root = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
    #[cfg(target_os = "macos")]
    let root =
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support"));
    #[cfg(not(any(windows, target_os = "macos")))]
    let root = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")));
    let dir = root
        .ok_or_else(|| Failure::arguments("Cannot resolve persistent state directory"))?
        .join("grok-computer-use");
    #[cfg(any(windows, target_os = "macos"))]
    let dir = dir.join("state");
    let parent = dir
        .parent()
        .ok_or_else(|| Failure::arguments("Invalid state directory"))?;
    fs::create_dir_all(parent).map_err(|_| Failure::arguments("Cannot create state parent"))?;
    match fs::symlink_metadata(&dir) {
        Ok(_) => check_dir(&dir)?,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                fs::DirBuilder::new()
                    .mode(0o700)
                    .create(&dir)
                    .map_err(|_| Failure::arguments("Cannot create private state directory"))?;
            }
            #[cfg(windows)]
            {
                fs::create_dir(&dir)
                    .map_err(|_| Failure::arguments("Cannot create private state directory"))?;
                unsafe_platform::secure(&dir)
                    .map_err(|_| Failure::arguments("Cannot secure state directory"))?;
            }
        }
        Err(_) => return Err(Failure::arguments("Cannot inspect state directory")),
    }
    Ok(dir)
}
pub(crate) fn check_dir(path: &Path) -> Result<(), Failure> {
    check(path, true)
}
pub(crate) fn check_file(path: &Path) -> Result<(), Failure> {
    check(path, false)
}
fn check(path: &Path, directory: bool) -> Result<(), Failure> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|_| Failure::arguments("Private control path is unavailable"))?;
    if metadata.file_type().is_symlink()
        || (directory && !metadata.is_dir())
        || (!directory && !metadata.is_file())
    {
        return Err(Failure::arguments(
            "Private control path has an unexpected type",
        ));
    }
    unsafe_platform::check(path, &metadata, directory).map_err(|error| {
        // The detail is an OS error or an ACL/mode description, never file contents or secrets.
        Failure::arguments(format!(
            "Private control path has an unknown owner or unsafe permissions ({error})"
        ))
    })
}
pub(crate) fn session_directory(prefix: &str) -> io::Result<tempfile::TempDir> {
    let dir = tempfile::Builder::new()
        .prefix(prefix)
        .tempdir_in(runtime_root())?;
    unsafe_platform::secure(dir.path())?;
    Ok(dir)
}
pub(crate) fn write_secret(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("missing private parent"))?;
    let mut f = tempfile::NamedTempFile::new_in(parent)?;
    unsafe_platform::secure(f.path())?;
    f.write_all(bytes)?;
    f.as_file().sync_all()?;
    f.persist(path).map_err(|e| e.error)?;
    Ok(())
}

pub(crate) async fn work<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, Failure> + Send + 'static,
) -> Result<T, Failure> {
    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        tokio::task::spawn_blocking(f),
    )
    .await
    .map_err(|_| Failure::internal("Private file work timed out"))?
    .map_err(|_| Failure::internal("Private file worker failed"))?
}
