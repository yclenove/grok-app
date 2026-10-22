//! Independent input versions. Receipt schema stays untouched.
use super::super::errors::{ErrorCode, ImportError};
use serde::{Deserialize, Deserializer};

pub(super) const ORDINARY_VERSION: u32 = 2;
pub(super) const ASSOCIATION_VERSION: u32 = 3;
pub(super) const ASSOCIATION_BYTES: usize = 16 * 1024 * 1024;

#[derive(Default)]
struct Present(bool);

impl<'de> Deserialize<'de> for Present {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        serde::de::IgnoredAny::deserialize(deserializer)?;
        Ok(Self(true))
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Header {
    version: u32,
    #[serde(default)]
    identity_association: Present,
}

pub(super) fn unsupported(error: impl std::fmt::Display) -> ImportError {
    ImportError::new(
        ErrorCode::SchemaUnsupported,
        format!("unsupported legacy or invalid durable job input; create a new preview: {error}"),
    )
}

pub(super) fn validate_version(version: u32, present: bool) -> Result<(), ImportError> {
    if !matches!(
        (version, present),
        (ORDINARY_VERSION, false) | (ASSOCIATION_VERSION, true)
    ) {
        return Err(unsupported(
            "input version and association presence disagree",
        ));
    }
    Ok(())
}

/// Presence and duplicate keys are checked without allocating the adapter body.
/// The caller then deserializes the original JSON with deny_unknown_fields.
pub(super) fn inspect(json: &str) -> Result<(), ImportError> {
    let header: Header = serde_json::from_str(json).map_err(unsupported)?;
    require_header(header, json.len())
}

pub(super) fn inspect_reader(reader: impl std::io::Read, bytes: usize) -> Result<(), ImportError> {
    let header: Header = serde_json::from_reader(super::input_reader::HeaderReader::new(reader))
        .map_err(unsupported)?;
    require_header(header, bytes)
}

fn require_header(header: Header, bytes: usize) -> Result<(), ImportError> {
    validate_version(header.version, header.identity_association.0)?;
    if header.version == ASSOCIATION_VERSION && bytes > ASSOCIATION_BYTES {
        return Err(ImportError::new(
            ErrorCode::ResourceLimit,
            "association input exceeds record budget",
        ));
    }
    Ok(())
}
