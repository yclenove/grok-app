//! Read-only native artifact lookup; self-test worlds are never native evidence.

use super::{RawTrial, SCHEMA_VERSION};
use crate::Error;
use std::io::Read;
use std::path::{Component, Path, PathBuf};

const MAX_ARTIFACT_BYTES: u64 = 1_048_576;

/// Trusted bundle reader. Injected implementations are for deterministic tests.
pub trait ArtifactSource {
    /// Load one raw trial; missing/unusable artifacts return None.
    /// May perform bounded read-only IO. No internal timeout/cancellation.
    fn load(&self, path: &Path) -> Result<Option<RawTrial>, Error>;
}

/// Read-only lookup below a trusted native bundle's `artifacts/` directory.
/// Rejects traversal, symlinks, absolute paths, empty or oversized files.
pub struct FileArtifacts {
    root: PathBuf,
}

impl FileArtifacts {
    /// Bind a trusted report bundle root. Pure; no IO, timeout or cancellation.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }
}

impl ArtifactSource for FileArtifacts {
    fn load(&self, path: &Path) -> Result<Option<RawTrial>, Error> {
        let mut parts = path.components();
        if parts.next() != Some(Component::Normal(std::ffi::OsStr::new("artifacts")))
            || path.extension().and_then(|s| s.to_str()) != Some("json")
        {
            return Ok(None);
        }
        let mut full = self.root.clone();
        for component in path.components() {
            let Component::Normal(part) = component else {
                return Ok(None);
            };
            full.push(part);
            let metadata = match std::fs::symlink_metadata(&full) {
                Ok(metadata) => metadata,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                Err(_) => return Err(Error::msg("could not inspect browser artifact")),
            };
            if metadata.file_type().is_symlink() {
                return Ok(None);
            }
        }
        let metadata = std::fs::metadata(&full)
            .map_err(|_| Error::msg("could not inspect browser artifact"))?;
        if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_ARTIFACT_BYTES {
            return Ok(None);
        }
        let file =
            std::fs::File::open(full).map_err(|_| Error::msg("could not open browser artifact"))?;
        let mut bytes = Vec::new();
        file.take(MAX_ARTIFACT_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| Error::msg("could not read browser artifact"))?;
        if bytes.len() as u64 > MAX_ARTIFACT_BYTES {
            return Ok(None);
        }
        // Malformed/missing platform artifacts are unavailable evidence, not
        // silently accepted rows or a passing synthetic fallback.
        let Ok(raw) = serde_json::from_slice::<RawTrial>(&bytes) else {
            return Ok(None);
        };
        Ok((raw.schema_version == SCHEMA_VERSION).then_some(raw))
    }
}
