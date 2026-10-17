//! Atomic policy file. A failed write leaves the previous file in place.
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::messages::Policy;

/// Read a policy snapshot. Corrupt or missing files return None (fail closed).
pub fn read_policy(path: &Path) -> Option<Policy> {
    let bytes = std::fs::read(path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// Replace `path` with `policy`. Parent directories are created. Returns false
/// on any IO or serialization failure. Nonblocking aside from the write itself;
/// callers are already off the approval wait.
pub fn write_policy(path: &Path, policy: &Policy) -> bool {
    let Some(parent) = path.parent() else {
        return false;
    };
    if parent.as_os_str().is_empty() || std::fs::create_dir_all(parent).is_err() {
        return false;
    }
    #[cfg(unix)]
    {
        let _ =
            std::fs::set_permissions(parent, std::os::unix::fs::PermissionsExt::from_mode(0o700));
    }
    let bytes = match serde_json::to_vec(policy) {
        Ok(bytes) => bytes,
        Err(_) => return false,
    };
    let tmp = temp_path(path);
    if !write_private(&tmp, &bytes) {
        let _ = std::fs::remove_file(&tmp);
        return false;
    }
    if std::fs::rename(&tmp, path).is_err() {
        let _ = std::fs::remove_file(&tmp);
        return false;
    }
    true
}

fn temp_path(path: &Path) -> PathBuf {
    let mut name = path
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_default();
    name.push(".tmp");
    path.with_file_name(name)
}

fn write_private(path: &Path, bytes: &[u8]) -> bool {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = match options.open(path) {
        Ok(file) => file,
        Err(_) => return false,
    };
    file.write_all(bytes).is_ok() && file.sync_all().is_ok()
}
