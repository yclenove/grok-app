//! Compile-owned resource ceilings, shared with the read-only bundle audit.
//! Locks/manifests describe exact content; they cannot raise these ceilings.

use crate::playwright_materialize::MaterializeError;
use serde::Deserialize;
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TreeLimits {
    pub max_entries: u32,
    pub max_files: u32,
    pub max_file_bytes: u64,
    pub max_total_bytes: u64,
    pub max_path_chars: usize,
    pub max_archive_bytes: u64,
    pub max_compression_ratio: f64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Contract {
    schema_version: u32,
    chromium: TreeLimits,
    playwright: TreeLimits,
    node: TreeLimits,
}

fn contract() -> &'static Contract {
    static CONTRACT: OnceLock<Contract> = OnceLock::new();
    CONTRACT.get_or_init(|| {
        let value: Contract = serde_json::from_str(include_str!(
            "../../../scripts/computer-use-runtime-limits.json"
        ))
        .expect("compile-owned runtime limits must be valid JSON");
        assert_eq!(value.schema_version, 1, "unsupported runtime limit schema");
        for limit in [value.chromium, value.playwright, value.node] {
            assert!(limit.max_files > 0 && limit.max_entries >= limit.max_files);
            assert!(limit.max_file_bytes > 0 && limit.max_total_bytes >= limit.max_file_bytes);
            assert!(limit.max_archive_bytes > 0 && limit.max_path_chars > 0);
            assert!(limit.max_compression_ratio.is_finite() && limit.max_compression_ratio >= 1.0);
        }
        value
    })
}

pub fn chromium() -> TreeLimits {
    contract().chromium
}

pub fn playwright() -> TreeLimits {
    contract().playwright
}

pub fn node() -> TreeLimits {
    contract().node
}

pub(crate) fn error(code: &str, message: impl Into<String>) -> MaterializeError {
    MaterializeError {
        code: code.into(),
        message: message.into(),
    }
}

#[derive(Default)]
pub(crate) struct TreeBudget {
    pub entries: u32,
    pub files: u32,
    pub bytes: u64,
}

impl TreeBudget {
    /// Root directories count as entries; links count as files and payload bytes.
    pub fn add(&mut self, limits: TreeLimits, size: Option<u64>) -> Result<(), MaterializeError> {
        self.entries = self
            .entries
            .checked_add(1)
            .ok_or_else(|| error("oversize", "runtime entry count overflow"))?;
        if self.entries > limits.max_entries {
            return Err(error("oversize", "too many runtime entries"));
        }
        if let Some(size) = size {
            self.files = self
                .files
                .checked_add(1)
                .ok_or_else(|| error("oversize", "runtime file count overflow"))?;
            self.bytes = self
                .bytes
                .checked_add(size)
                .ok_or_else(|| error("oversize", "runtime byte count overflow"))?;
            if self.files > limits.max_files
                || size > limits.max_file_bytes
                || self.bytes > limits.max_total_bytes
            {
                return Err(error("oversize", "runtime tree exceeds resource limits"));
            }
        }
        Ok(())
    }
}

pub(crate) fn validate_path(path: &str, limit: usize) -> Result<(), MaterializeError> {
    if path.chars().count() > limit
        || path.contains(['\0', '\t', '\n', '\r', ':'])
        || path.starts_with('/')
        || path
            .split('/')
            .any(|part| part == "." || part == ".." || part.is_empty())
    {
        return Err(error("path", "invalid runtime tree path"));
    }
    Ok(())
}

#[cfg(test)]
#[path = "runtime_limits_tests.rs"]
mod tests;
