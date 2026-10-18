//! Closed-session gallery deletes that survive an IO failure.
//!
//! A `.closed` marker is written before removal. A failed remove leaves the
//! marker, and the next [`sweep`] retries it. Unmarked session directories are
//! kept: after a restart the in-memory index is empty and those files still
//! belong to chats.

use crate::owner_only::{self, SyncPolicy};
use std::io;
use std::path::Path;

const CLOSED: &str = ".closed";
const RETRY: &str = ".purge-retry";

pub(crate) fn ensure_private_dir(path: &Path) -> io::Result<()> {
    owner_only::ensure_dir(path)
}

pub(crate) fn write_private(path: &Path, bytes: &[u8]) -> io::Result<()> {
    owner_only::atomic_write(path, bytes, SyncPolicy::BestEffort)
}

/// Mark `dir` closed and delete it. A symlink is not followed.
pub(crate) fn purge_dir(dir: &Path) {
    let meta = match std::fs::symlink_metadata(dir) {
        Ok(meta) => meta,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return,
        Err(error) => {
            owner_only::warn_meta("gallery_purge", dir, &error);
            return;
        }
    };
    if meta.file_type().is_symlink() || !meta.is_dir() {
        owner_only::warn_meta(
            "gallery_purge",
            dir,
            &io::Error::new(io::ErrorKind::PermissionDenied, "not a real directory"),
        );
        return;
    }
    if let Err(error) =
        owner_only::atomic_write(&dir.join(CLOSED), b"closed\n", SyncPolicy::BestEffort)
    {
        remember_retry(dir);
        owner_only::warn_meta("gallery_marker", dir, &error);
    }
    remove_tree(dir);
}

/// Retry closed directories, retry-list names, and leftover temporary names.
pub(crate) fn sweep(root: &Path) {
    let entries = match std::fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return,
        Err(error) => {
            owner_only::warn_meta("gallery_scan", root, &error);
            return;
        }
    };
    let mut names = Vec::new();
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                owner_only::warn_meta("gallery_scan", root, &error);
                continue;
            }
        };
        names.push(entry.file_name());
    }
    for name in retry_names(root) {
        remove_tree(&root.join(name));
    }
    for name in names {
        let text = name.to_string_lossy();
        let path = root.join(&name);
        if text.starts_with(".gcu-tmp-") {
            remove_tree(&path);
            continue;
        }
        if text.starts_with('.') {
            continue;
        }
        let marker = path.join(CLOSED);
        if std::fs::symlink_metadata(&marker).is_ok() {
            remove_tree(&path);
        }
    }
}

pub(crate) fn forget_file(path: &Path) {
    if let Err(error) = std::fs::remove_file(path) {
        if error.kind() != io::ErrorKind::NotFound {
            owner_only::warn_meta("gallery_delete", path, &error);
        }
    }
}

fn remove_tree(path: &Path) {
    let meta = match std::fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return,
        Err(error) => {
            owner_only::warn_meta("gallery_delete", path, &error);
            return;
        }
    };
    if meta.file_type().is_symlink() {
        owner_only::warn_meta(
            "gallery_purge",
            path,
            &io::Error::new(io::ErrorKind::PermissionDenied, "symlink"),
        );
        return;
    }
    let result = if meta.is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    };
    if let Err(error) = result {
        if error.kind() != io::ErrorKind::NotFound {
            owner_only::warn_meta("gallery_delete", path, &error);
        }
    }
}

fn remember_retry(dir: &Path) {
    let Some(parent) = dir.parent() else {
        return;
    };
    let Some(name) = dir.file_name().and_then(|name| name.to_str()) else {
        return;
    };
    if !safe_name(name) {
        return;
    }
    let path = parent.join(RETRY);
    let mut body = owner_only::read_no_follow(&path, 64 * 1024).unwrap_or_default();
    let line = format!("{name}\n");
    if body
        .windows(line.len())
        .any(|window| window == line.as_bytes())
    {
        return;
    }
    body.extend(line.as_bytes());
    if let Err(error) = owner_only::atomic_write(&path, &body, SyncPolicy::BestEffort) {
        owner_only::warn_meta("gallery_marker", &path, &error);
    }
}

fn retry_names(root: &Path) -> Vec<String> {
    let Ok(bytes) = owner_only::read_no_follow(&root.join(RETRY), 64 * 1024) else {
        return Vec::new();
    };
    let Ok(text) = std::str::from_utf8(&bytes) else {
        return Vec::new();
    };
    text.lines()
        .filter(|name| safe_name(name))
        .map(str::to_owned)
        .collect()
}

fn safe_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains('/')
        && !name.contains('\\')
        && !name.contains('\0')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::owner_only;
    use std::fs;

    fn scratch() -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "gcu-gallery-{}-{:x}",
            std::process::id(),
            rand::random::<u64>()
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn orphan_closed_directory_is_removed_on_sweep() {
        let root = scratch();
        let dir = root.join("session-a");
        owner_only::ensure_dir(&dir).unwrap();
        fs::write(dir.join("shot.png"), b"png").unwrap();
        purge_dir(&dir);
        assert!(!dir.exists(), "a normal closed directory is removed");
        let again = root.join("session-b");
        owner_only::ensure_dir(&again).unwrap();
        fs::write(again.join(CLOSED), b"closed\n").unwrap();
        sweep(&root);
        assert!(!again.exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[cfg(unix)]
    #[test]
    fn purge_failure_warns_and_startup_sweep_retries() {
        use std::os::unix::fs::PermissionsExt;
        let root = scratch();
        owner_only::ensure_dir(&root).unwrap();
        let dir = root.join("session-stuck");
        owner_only::ensure_dir(&dir).unwrap();
        let nest = dir.join("nest");
        fs::create_dir(&nest).unwrap();
        fs::write(nest.join("shot.png"), b"png").unwrap();
        fs::set_permissions(&nest, fs::Permissions::from_mode(0o000)).unwrap();
        purge_dir(&dir);
        assert!(dir.exists());
        assert!(
            owner_only::warned_path("gallery_delete", &dir),
            "a failed delete must warn once"
        );
        fs::set_permissions(&nest, fs::Permissions::from_mode(0o700)).unwrap();
        sweep(&root);
        assert!(!dir.exists(), "the next sweep removes the marked directory");
        let _ = fs::remove_dir_all(&root);
    }
}
