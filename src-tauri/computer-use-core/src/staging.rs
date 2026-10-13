//! Run-owned download staging. Model-supplied filesystem paths are never the save target.

use std::path::{Path, PathBuf};

use crate::error::BrokerError;
use sha2::{Digest, Sha256};

pub const MAX_DOWNLOAD_BYTES: u64 = 8 * 1024 * 1024;

pub fn reject_model_path(model_path: Option<&str>) -> Result<(), BrokerError> {
    match model_path.map(str::trim).filter(|s| !s.is_empty()) {
        None => Ok(()),
        Some(_) => Err(BrokerError::Schema(
            "model-supplied filesystem path is not trusted".into(),
        )),
    }
}

pub fn sanitize_filename(name: &str) -> Result<String, BrokerError> {
    let raw = name.trim();
    if raw.is_empty()
        || raw.contains("..")
        || raw.contains('/')
        || raw.contains('\\')
        || raw.contains('\0')
        || raw.contains(':')
    {
        return Err(BrokerError::Schema("invalid download filename".into()));
    }
    let base = Path::new(raw)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .trim();
    if base.is_empty() || base == "." || base != raw {
        return Err(BrokerError::Schema("invalid download filename".into()));
    }
    let stem = base.split('.').next().unwrap_or(base).to_ascii_uppercase();
    if matches!(
        stem.as_str(),
        "CON"
            | "PRN"
            | "AUX"
            | "NUL"
            | "COM1"
            | "COM2"
            | "COM3"
            | "COM4"
            | "COM5"
            | "COM6"
            | "COM7"
            | "COM8"
            | "COM9"
            | "LPT1"
            | "LPT2"
            | "LPT3"
            | "LPT4"
            | "LPT5"
            | "LPT6"
            | "LPT7"
            | "LPT8"
            | "LPT9"
    ) {
        return Err(BrokerError::Schema("invalid download filename".into()));
    }
    Ok(base.chars().take(80).collect())
}

pub fn staging_file(root: &Path, run_id: &str, filename: &str) -> Result<PathBuf, BrokerError> {
    reject_model_path(None)?;
    let run = run_id.trim();
    if run.is_empty() || run.contains("..") || run.contains('/') || run.contains('\\') {
        return Err(BrokerError::IdentityMismatch("runId"));
    }
    let name = sanitize_filename(filename)?;
    let dir = root.join("computer-use-staging").join(run);
    std::fs::create_dir_all(&dir).map_err(|e| BrokerError::Adapter(e.to_string()))?;
    Ok(dir.join(name))
}

/// Allocate a fresh, action-private download directory under the run staging
/// root.  Keeping the requested basename inside a random directory avoids
/// overwriting an earlier staged file while preserving a useful filename.
pub fn staging_download_file(
    root: &Path,
    run_id: &str,
    filename: &str,
) -> Result<PathBuf, BrokerError> {
    let run = run_id.trim();
    if run.is_empty() || run.contains("..") || run.contains('/') || run.contains('\\') {
        return Err(BrokerError::IdentityMismatch("runId"));
    }
    let name = sanitize_filename(filename)?;
    let run_root = root
        .join("computer-use-staging")
        .join(download_run_component(run));
    std::fs::create_dir_all(&run_root).map_err(|e| BrokerError::Adapter(e.to_string()))?;
    let directory = run_root
        .join("downloads")
        .join(uuid::Uuid::new_v4().to_string());
    std::fs::create_dir_all(&directory).map_err(|e| BrokerError::Adapter(e.to_string()))?;

    let canonical_run = std::fs::canonicalize(&run_root)
        .map_err(|e| BrokerError::Adapter(format!("cannot resolve run staging root: {e}")))?;
    let canonical_directory = std::fs::canonicalize(&directory)
        .map_err(|e| BrokerError::Adapter(format!("cannot resolve download staging: {e}")))?;
    if !path_is_under(&canonical_directory, &canonical_run)
        || canonical_directory == canonical_run
        || std::fs::symlink_metadata(&directory)
            .map(|metadata| metadata.file_type().is_symlink())
            .unwrap_or(true)
    {
        return Err(BrokerError::Schema(
            "download staging is outside the run-owned root".into(),
        ));
    }
    Ok(canonical_directory.join(name))
}

fn download_run_component(run_id: &str) -> String {
    let digest = Sha256::digest(run_id.as_bytes());
    format!("run-{}", hex::encode(&digest[..16]))
}

pub fn path_is_under(child: &Path, root: &Path) -> bool {
    let child = child.components().collect::<Vec<_>>();
    let root = root.components().collect::<Vec<_>>();
    child.len() >= root.len() && child.starts_with(&root)
}

pub fn accept_staged_file(path: &Path, staging_root: &Path) -> Result<u64, BrokerError> {
    if !path.is_file() {
        return Err(BrokerError::Adapter("staged file missing".into()));
    }
    let canonical_path = std::fs::canonicalize(path)
        .map_err(|e| BrokerError::Adapter(format!("cannot resolve staged file: {e}")))?;
    let canonical_root = std::fs::canonicalize(staging_root)
        .map_err(|e| BrokerError::Adapter(format!("cannot resolve staging root: {e}")))?;
    if !path_is_under(&canonical_path, &canonical_root) || canonical_path == canonical_root {
        return Err(BrokerError::Schema(
            "staged file is outside the Host staging root".into(),
        ));
    }
    let len = canonical_path
        .metadata()
        .map_err(|e| BrokerError::Adapter(e.to_string()))?
        .len();
    if len == 0 || len > MAX_DOWNLOAD_BYTES {
        let _ = std::fs::remove_file(&canonical_path);
        return Err(BrokerError::Schema(
            "staged file size is not allowed".into(),
        ));
    }
    Ok(len)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_path_and_traversal_are_rejected() {
        assert!(reject_model_path(Some("C:\\\\Users\\\\x\\\\file.bin")).is_err());
        assert!(reject_model_path(Some("/tmp/out.bin")).is_err());
        assert!(reject_model_path(None).is_ok());
        assert!(sanitize_filename("../etc/passwd").is_err());
        assert!(sanitize_filename("a/b.bin").is_err());
        assert!(sanitize_filename("CON").is_err());
        assert!(sanitize_filename("nul.txt").is_err());
        let root = std::env::temp_dir().join(format!("cu-stage-{}", uuid::Uuid::new_v4()));
        let path = staging_file(&root, "run-a", "report.bin").unwrap();
        assert!(path.ends_with(
            Path::new("computer-use-staging")
                .join("run-a")
                .join("report.bin")
        ));
        assert!(!path.to_string_lossy().contains(".."));
        std::fs::write(&path, "abc").unwrap();
        assert_eq!(accept_staged_file(&path, &root).unwrap(), 3);
        let first = staging_download_file(&root, "run-a", "report.bin").unwrap();
        let second = staging_download_file(&root, "run-a", "report.bin").unwrap();
        assert_ne!(first, second);
        assert_eq!(
            first.file_name().and_then(|name| name.to_str()),
            Some("report.bin")
        );
        assert!(path_is_under(
            first.parent().unwrap(),
            &std::fs::canonicalize(
                root.join("computer-use-staging")
                    .join(download_run_component("run-a"))
            )
            .unwrap()
        ));
        let long_run = format!("cu-task-{}", "a".repeat(96));
        let compact = staging_download_file(&root, &long_run, "compact.bin").unwrap();
        assert!(!compact.to_string_lossy().contains(&long_run));
        assert_eq!(
            compact
                .parent()
                .and_then(Path::parent)
                .and_then(Path::parent)
                .and_then(Path::file_name)
                .and_then(|name| name.to_str())
                .map(str::len),
            Some(36)
        );
        let outside = std::env::temp_dir().join(format!("cu-out-{}", uuid::Uuid::new_v4()));
        std::fs::write(&outside, "nope").unwrap();
        assert!(accept_staged_file(&outside, &root).is_err());
        let _ = std::fs::remove_file(&outside);
        let _ = std::fs::remove_dir_all(root);
    }
}
