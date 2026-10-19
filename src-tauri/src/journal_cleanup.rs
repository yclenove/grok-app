//! Retry deletion of closed chat journals. Message contents are unchanged.
//!
//! A missing sessions index means no chat is trusted. A corrupt index is not
//! treated as empty, so a parse failure does not delete every session directory.

use std::collections::HashSet;
use std::io;
use std::path::Path;
#[cfg(test)]
use std::path::PathBuf;

use gcu_host_protocol::owner_only::{self, SyncPolicy};

const CLOSED: &str = ".closed";
const RETRY: &str = ".purge-retry";

pub fn sweep() {
    let root = crate::paths::app_data_root().join("sessions");
    sweep_root(&root, &crate::paths::sessions_index_file());
}

pub fn purge_session_dir(dir: &Path) {
    let meta = match std::fs::symlink_metadata(dir) {
        Ok(meta) => meta,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return,
        Err(error) => {
            owner_only::warn_meta("journal_purge", dir, &error);
            return;
        }
    };
    if meta.file_type().is_symlink() || !meta.is_dir() {
        owner_only::warn_meta(
            "journal_purge",
            dir,
            &io::Error::new(io::ErrorKind::PermissionDenied, "not a real directory"),
        );
        return;
    }
    if let Err(error) =
        owner_only::atomic_write(&dir.join(CLOSED), b"closed\n", SyncPolicy::BestEffort)
    {
        remember_retry(dir);
        owner_only::warn_meta("journal_marker", dir, &error);
    }
    remove_tree(dir);
}

fn sweep_root(root: &Path, index: &Path) {
    let entries = match std::fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return,
        Err(error) => {
            owner_only::warn_meta("journal_scan", root, &error);
            return;
        }
    };
    let trusted = trusted_ids(index);
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                owner_only::warn_meta("journal_scan", root, &error);
                continue;
            }
        };
        let name = entry.file_name();
        let text = name.to_string_lossy();
        let path = entry.path();
        if text.starts_with(".gcu-tmp-") {
            remove_tree(&path);
            continue;
        }
        if text.starts_with('.') {
            continue;
        }
        let meta = match std::fs::symlink_metadata(&path) {
            Ok(meta) => meta,
            Err(error) => {
                owner_only::warn_meta("journal_scan", &path, &error);
                continue;
            }
        };
        if meta.file_type().is_symlink() || !meta.is_dir() {
            if meta.file_type().is_symlink() {
                owner_only::warn_meta(
                    "journal_purge",
                    &path,
                    &io::Error::new(io::ErrorKind::PermissionDenied, "symlink"),
                );
            }
            continue;
        }
        let marked = std::fs::symlink_metadata(path.join(CLOSED)).is_ok();
        let orphan = trusted
            .as_ref()
            .is_some_and(|ids| !ids.contains(text.as_ref()));
        if marked || orphan {
            remove_tree(&path);
        }
    }
    if let Some(names) = retry_names(root) {
        for name in names {
            remove_tree(&root.join(name));
        }
    }
}

fn trusted_ids(index: &Path) -> Option<HashSet<String>> {
    // NotFound is an empty index. Any other failure, including a symlink or a
    // mode that is not owner-only, must not be treated as "delete everything".
    let bytes = match owner_only::read_no_follow(index, 8 * 1024 * 1024) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Some(HashSet::new()),
        Err(error) => {
            owner_only::warn_meta("journal_index", index, &error);
            return None;
        }
    };
    let values: Vec<serde_json::Value> = match serde_json::from_slice(&bytes) {
        Ok(values) => values,
        Err(error) => {
            owner_only::warn_meta(
                "journal_index",
                index,
                &io::Error::new(io::ErrorKind::InvalidData, error),
            );
            return None;
        }
    };
    let mut ids = HashSet::new();
    for value in values {
        if let Some(id) = value.get("id").and_then(|id| id.as_str()) {
            ids.insert(id.to_owned());
        }
    }
    Some(ids)
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
        owner_only::warn_meta("journal_marker", &path, &error);
    }
}

fn retry_names(root: &Path) -> Option<Vec<String>> {
    let bytes = owner_only::read_no_follow(&root.join(RETRY), 64 * 1024).ok()?;
    let text = std::str::from_utf8(&bytes).ok()?;
    Some(
        text.lines()
            .filter(|name| safe_name(name))
            .map(str::to_owned)
            .collect(),
    )
}

fn remove_tree(path: &Path) {
    let meta = match std::fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return,
        Err(error) => {
            owner_only::warn_meta("journal_delete", path, &error);
            return;
        }
    };
    if meta.file_type().is_symlink() {
        owner_only::warn_meta(
            "journal_purge",
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
            owner_only::warn_meta("journal_delete", path, &error);
        }
    }
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
    use std::fs;

    fn scratch() -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "gcu-journal-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn corrupt_index_does_not_delete_unmarked_sessions() {
        let root = scratch();
        let keep = root.join("keep-chat");
        fs::create_dir(&keep).unwrap();
        fs::write(keep.join("messages.json"), b"[]").unwrap();
        owner_only::atomic_write(
            &root.join("sessions_index.json"),
            b"{",
            SyncPolicy::BestEffort,
        )
        .unwrap();
        sweep_root(&root, &root.join("sessions_index.json"));
        assert!(keep.join("messages.json").is_file());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn missing_index_removes_orphan_directories_and_retries_a_failed_delete() {
        let root = scratch();
        let orphan = root.join("old-chat");
        fs::create_dir(&orphan).unwrap();
        fs::write(orphan.join("messages.json"), b"[]").unwrap();
        sweep_root(&root, &root.join("missing-index.json"));
        assert!(!orphan.exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[cfg(unix)]
    #[test]
    fn journal_delete_failure_is_retried_on_the_next_sweep() {
        use std::os::unix::fs::PermissionsExt;
        let root = scratch();
        let dir = root.join("stuck-chat");
        fs::create_dir(&dir).unwrap();
        let nest = dir.join("nest");
        fs::create_dir(&nest).unwrap();
        fs::write(nest.join("messages.json"), b"[]").unwrap();
        fs::set_permissions(&nest, fs::Permissions::from_mode(0o000)).unwrap();
        owner_only::atomic_write(&root.join("index.json"), b"[]", SyncPolicy::BestEffort).unwrap();
        sweep_root(&root, &root.join("index.json"));
        assert!(dir.exists());
        fs::set_permissions(&nest, fs::Permissions::from_mode(0o700)).unwrap();
        sweep_root(&root, &root.join("index.json"));
        assert!(!dir.exists());
        let _ = fs::remove_dir_all(&root);
    }
}
