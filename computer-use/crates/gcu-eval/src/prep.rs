//! Temporary directory helpers for the local evaluation provider.

use crate::Error;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

pub(crate) struct Rm(pub(crate) PathBuf);
impl Drop for Rm {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub(crate) fn unique_root() -> PathBuf {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("gcu-ev-{}-{n}", std::process::id()))
}

pub(crate) fn remove_root(path: &Path) -> Result<bool, Error> {
    match std::fs::remove_dir_all(path) {
        Ok(()) => Ok(true),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(true),
        Err(err) => Err(Error::msg(format!("destroy: {err}"))),
    }
}

pub(crate) fn lock_dir(path: &Path) -> Result<(), Error> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
            .map_err(|err| Error::msg(format!("chmod: {err}")))
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Ok(())
    }
}

pub(crate) fn write_shims(dir: &Path) -> Result<(), Error> {
    for name in ["curl", "wget", "ssh", "nc", "mail"] {
        write_shim(dir, name)?;
    }
    Ok(())
}

pub(crate) fn write_shim(dir: &Path, name: &str) -> Result<(), Error> {
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let path = dir.join(name);
        let body = "#!/bin/sh\necho \"process $(basename \"$0\")\" >> \"$GCU_EVAL_SIDE_LOG\"\necho \"network $*\" >> \"$GCU_EVAL_SIDE_LOG\"\nexit 1\n";
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .mode(0o755)
            .open(&path)
            .map_err(|err| Error::msg(format!("shim: {err}")))?;
        file.write_all(body.as_bytes())
            .map_err(|err| Error::msg(format!("shim: {err}")))?;
        Ok(())
    }
    #[cfg(not(unix))]
    {
        let path = dir.join(format!("{name}.cmd"));
        let body = "@echo off\r\necho process %~n0>>\"%GCU_EVAL_SIDE_LOG%\"\r\necho network %*>>\"%GCU_EVAL_SIDE_LOG%\"\r\nexit /b 1\r\n";
        std::fs::write(path, body).map_err(|err| Error::msg(format!("shim: {err}")))
    }
}
