//! Strict workspace reads and one lock covering the entire JSON transaction.
//! Never recover a corrupt/future file as an empty store and overwrite it.

use std::{collections::HashSet, fs::File, io::Read, path::Path};

use serde::de::DeserializeOwned;

use super::{WorkspaceStoreFile, WORKSPACE_STORE_VERSION};

const MAX_STORE_BYTES: u64 = 16 * 1024 * 1024;

pub(super) fn read_optional<T: DeserializeOwned>(path: &Path) -> Result<Option<T>, String> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("read {}: {error}", path.display())),
    };
    let metadata = file.metadata().map_err(|error| error.to_string())?;
    if !metadata.is_file() || metadata.len() > MAX_STORE_BYTES {
        return Err("workspace store must be a regular file within 16 MiB".into());
    }
    let mut bytes = Vec::new();
    file.take(MAX_STORE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() as u64 > MAX_STORE_BYTES {
        return Err("workspace store exceeds 16 MiB".into());
    }
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|error| format!("invalid {}: {error}", path.display()))
}

pub(super) fn read(path: &Path) -> Result<WorkspaceStoreFile, String> {
    let file: WorkspaceStoreFile = read_optional(path)?.unwrap_or_default();
    validate(&file)?;
    Ok(file)
}

fn validate(file: &WorkspaceStoreFile) -> Result<(), String> {
    if file.version != WORKSPACE_STORE_VERSION {
        return Err(format!(
            "unsupported workspace store version: {}",
            file.version
        ));
    }
    let mut ids = HashSet::new();
    if file
        .workspaces
        .iter()
        .any(|row| row.id.is_empty() || !ids.insert(&row.id))
    {
        return Err("workspace store has empty or duplicate identities".into());
    }
    Ok(())
}

pub(super) fn update<T>(
    path: &Path,
    body: impl FnOnce(&mut WorkspaceStoreFile) -> Result<T, String>,
) -> Result<T, String> {
    {
        // Project removal/relocation and workspace publication share the same
        // order. Callers recheck their reviewed project targets under this lock.
        let _locks = crate::store_lock::lock_exclusive_many(&[
            crate::paths::projects_file(),
            crate::paths::sessions_index_file(),
            path.to_path_buf(),
        ])?;
        let mut file = read(path)?;
        let before = file.clone();
        let result = body(&mut file)?;
        if file != before {
            validate(&file)?;
            let bytes = serde_json::to_vec_pretty(&file).map_err(|error| error.to_string())?;
            if bytes.len() as u64 > MAX_STORE_BYTES {
                return Err("workspace store exceeds 16 MiB".into());
            }
            crate::store_lock::write_bytes_replace(path, &bytes)?;
        }
        Ok(result)
    }
}
