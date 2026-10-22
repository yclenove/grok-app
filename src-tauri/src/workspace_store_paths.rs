//! Paths here belong to this host. Imported source paths must first be mapped.

use std::path::{Component, Path, PathBuf};

pub fn canonicalize_workspace_root(raw: &str) -> Result<(String, bool), String> {
    if raw.is_empty() || raw.len() > 8192 || raw.chars().any(char::is_control) {
        return Err("path empty or invalid".into());
    }
    let path = PathBuf::from(raw);
    if !path.is_absolute() {
        return Err("path must be absolute on this computer".into());
    }
    if path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err("path must not contain ..".into());
    }
    match path.metadata() {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok((normalize_display_path(&path), false));
        }
        Err(error) => return Err(format!("inspect workspace root: {error}")),
        Ok(metadata) if !metadata.is_dir() => return Err("path is not a directory".into()),
        Ok(_) => {}
    }
    let canon = path
        .canonicalize()
        .map_err(|error| format!("canonicalize: {error}"))?;
    Ok((normalize_display_path(&canon), true))
}

fn normalize_display_path(path: &Path) -> String {
    let raw = path.to_string_lossy();
    #[cfg(windows)]
    {
        if let Some(tail) = raw.strip_prefix(r"\\?\UNC\") {
            return format!("//{}", tail.replace('\\', "/"));
        }
        raw.strip_prefix(r"\\?\").unwrap_or(&raw).replace('\\', "/")
    }
    #[cfg(not(windows))]
    raw.into_owned()
}

/// Canonical spellings preserve case-sensitive directories, also on Windows.
pub(super) fn paths_equal(a: &str, b: &str) -> bool {
    Path::new(a) == Path::new(b)
}
