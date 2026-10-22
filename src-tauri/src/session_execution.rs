//! Credential-free execution provenance. Desired provider choices never select history.
use crate::{paths, store::SessionMeta};
use serde::{Deserialize, Deserializer, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub(crate) enum ExecutionSource {
    LegacyShared,
    LegacyIndependent,
    AppProfile {
        owner_session_id: String,
        generation: String,
    },
    /// Server-owned: never search a local directory for this agent id.
    Remote,
    /// Auxiliary/unsupported sources are not a license to read global history.
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ExecutionReceipt {
    pub schema_version: u16,
    pub agent_session_id: String,
    pub source: ExecutionSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub process_id: Option<String>,
}

/// Preserve the session row even when a newer/invalid receipt is unreadable.
/// Writers reject the marker before replacing the index, preserving raw bytes;
/// readers fail closed without quarantining an otherwise valid sessions index.
pub(crate) fn deserialize_receipt<'de, D: Deserializer<'de>>(
    de: D,
) -> Result<Option<ExecutionReceipt>, D::Error> {
    let raw = Option::<serde_json::Value>::deserialize(de)?;
    Ok(raw.map(|value| {
        serde_json::from_value(value).unwrap_or_else(|_| ExecutionReceipt {
            schema_version: 0,
            agent_session_id: String::new(),
            source: ExecutionSource::Unavailable,
            provider_id: None,
            model_id: None,
            process_id: None,
        })
    }))
}

fn valid_uuid(raw: &str) -> bool {
    uuid::Uuid::parse_str(raw).is_ok_and(|id| id.to_string() == raw)
}

impl ExecutionReceipt {
    pub(crate) fn validate(&self, agent: Option<&str>) -> Result<(), String> {
        if self.schema_version != 1
            || agent != Some(self.agent_session_id.as_str())
            || crate::cli_sessions::validate_agent_session_id(&self.agent_session_id).is_err()
            || self.agent_session_id.trim() != self.agent_session_id
        {
            return Err("execution receipt is invalid or no longer owns this agent".into());
        }
        if let ExecutionSource::AppProfile {
            owner_session_id,
            generation,
        } = &self.source
        {
            if !valid_uuid(owner_session_id) || !valid_uuid(generation) {
                return Err("execution profile identity is invalid".into());
            }
        }
        if [&self.provider_id, &self.model_id, &self.process_id]
            .iter()
            .any(|value| value.as_deref().is_some_and(|id| id.trim().is_empty()))
        {
            return Err("execution receipt identity is empty".into());
        }
        Ok(())
    }
}

impl ExecutionSource {
    /// Recognize only the owned two-UUID namespace. A malformed/missing marker
    /// inside that namespace is an error, never a reason to select global config.
    /// Auxiliary overrides and remote transports skip this main-home resolver.
    pub(crate) fn from_app_profile_home(home: &Path) -> Result<Option<Self>, String> {
        let namespace = paths::app_data_root().join("execution-profiles");
        let Ok(relative) = home.strip_prefix(&namespace) else {
            return Ok(None);
        };
        let components = relative.components().collect::<Vec<_>>();
        let [std::path::Component::Normal(owner), std::path::Component::Normal(generation)] =
            components.as_slice()
        else {
            return Err("execution profile identity is invalid".into());
        };
        let source = Self::AppProfile {
            owner_session_id: owner
                .to_str()
                .ok_or("execution profile identity is invalid")?
                .into(),
            generation: generation
                .to_str()
                .ok_or("execution profile identity is invalid")?
                .into(),
        };
        if source.home()?.as_deref() != Some(home) {
            return Err("execution profile redirects its owned namespace".into());
        }
        Ok(Some(source))
    }

    pub(crate) fn home(&self) -> Result<Option<PathBuf>, String> {
        Ok(match self {
            Self::LegacyShared => Some(crate::process_util::user_home().join(".grok")),
            Self::LegacyIndependent => Some(paths::agent_home_dir()),
            Self::Remote | Self::Unavailable => None,
            Self::AppProfile {
                owner_session_id,
                generation,
            } => {
                if !valid_uuid(owner_session_id) || !valid_uuid(generation) {
                    return Err("execution profile identity is invalid".into());
                }
                let base = paths::app_data_root();
                let home = base
                    .join("execution-profiles")
                    .join(owner_session_id)
                    .join(generation);
                let actual = home
                    .canonicalize()
                    .map_err(|_| "execution profile is unavailable")?;
                let base = base
                    .canonicalize()
                    .map_err(|_| "execution profile base is unavailable")?;
                let expected = base
                    .join("execution-profiles")
                    .join(owner_session_id)
                    .join(generation);
                if actual != expected {
                    return Err("execution profile redirects its owned namespace".into());
                }
                let marker = actual.join("execution-profile.json");
                let canonical = marker
                    .canonicalize()
                    .map_err(|_| "execution profile marker is unavailable")?;
                let metadata = fs::metadata(&canonical)
                    .map_err(|_| "execution profile marker is unavailable")?;
                if canonical != marker || !metadata.is_file() || metadata.len() > 16 * 1024 {
                    return Err("execution profile marker is invalid".into());
                }
                let mut bytes = Vec::new();
                fs::File::open(canonical)
                    .map_err(|_| "execution profile marker is unavailable")?
                    .take(16 * 1024 + 1)
                    .read_to_end(&mut bytes)
                    .map_err(|_| "execution profile marker is unavailable")?;
                if bytes.len() > 16 * 1024 {
                    return Err("execution profile marker is invalid".into());
                }
                let value: serde_json::Value = serde_json::from_slice(&bytes)
                    .map_err(|_| "execution profile marker is invalid")?;
                if value["schemaVersion"] != 1
                    || value["ownerSessionId"] != *owner_session_id
                    || value["generation"] != *generation
                {
                    return Err("execution profile ownership does not match".into());
                }
                verify_config_seal(&actual, &value)?;
                Some(home)
            }
        })
    }
}

fn verify_config_seal(home: &Path, marker: &serde_json::Value) -> Result<(), String> {
    let Some(seal) = marker.get("configurationSha256") else {
        // Existing version-1 markers did not bind configuration bytes.
        return Ok(());
    };
    let seal = seal
        .as_str()
        .filter(|seal| {
            seal.len() == 64
                && seal
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        })
        .ok_or("execution profile configuration seal is invalid")?;
    let path = home.join("config.toml");
    let canonical = path
        .canonicalize()
        .map_err(|_| "execution profile configuration is unavailable")?;
    let metadata = fs::symlink_metadata(&path)
        .map_err(|_| "execution profile configuration is unavailable")?;
    if canonical != path
        || !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() > 16 * 1024 * 1024
    {
        return Err("execution profile configuration is invalid".into());
    }
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(|_| "execution profile configuration is unavailable")?
        .take(16 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "execution profile configuration is unavailable")?;
    if bytes.len() > 16 * 1024 * 1024 || format!("{:x}", Sha256::digest(bytes)) != seal {
        return Err("execution profile configuration changed after publication".into());
    }
    Ok(())
}

pub(crate) fn find_session_dir(
    meta: &SessionMeta,
    agent: &str,
    cwd: Option<&str>,
    legacy_mode: &str,
) -> Result<Option<PathBuf>, String> {
    if meta.agent_session_id.as_deref() != Some(agent) {
        return Err("agent session does not belong to this App session".into());
    }
    let home = if let Some(receipt) = &meta.agent_execution {
        receipt.validate(meta.agent_session_id.as_deref())?;
        receipt.source.home()?
    } else if legacy_mode.trim().eq_ignore_ascii_case("shared") {
        Some(crate::process_util::user_home().join(".grok"))
    } else {
        Some(paths::agent_home_dir())
    };
    // Local connect gives a linked worktree precedence over the project root.
    // Every history consumer must use that same directory, including native forks
    // whose receipt legitimately belongs to the parent App session.
    let cwd = meta
        .worktree_path
        .as_deref()
        .filter(|path| !path.is_empty())
        .or(cwd);
    Ok(home.and_then(|home| find_in_home(&home, agent, cwd)))
}

pub(crate) fn verify_profile_config_capture(home: &Path, text: &str) -> Result<(), String> {
    let path = home.join("execution-profile.json");
    if path
        .canonicalize()
        .map_err(|_| "execution profile marker is unavailable")?
        != home
            .canonicalize()
            .map_err(|_| "execution profile is unavailable")?
            .join("execution-profile.json")
    {
        return Err("execution profile marker redirects its namespace".into());
    }
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(|_| "execution profile marker is unavailable")?
        .take(16 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "execution profile marker is unavailable")?;
    if bytes.len() > 16 * 1024 {
        return Err("execution profile marker is invalid".into());
    }
    let marker: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|_| "execution profile marker is invalid")?;
    if let Some(seal) = marker.get("configurationSha256") {
        if seal.as_str() != Some(format!("{:x}", Sha256::digest(text.as_bytes())).as_str()) {
            return Err("captured execution configuration does not match its publication".into());
        }
    }
    Ok(())
}

pub(crate) fn namespace_paths_match(actual: &Path, expected: &Path) -> bool {
    if actual == expected {
        return true;
    }
    #[cfg(windows)]
    {
        actual
            .to_str()
            .zip(expected.to_str())
            .is_some_and(|(a, b)| a.eq_ignore_ascii_case(b))
    }
    #[cfg(not(windows))]
    {
        false
    }
}

pub(crate) fn find_in_home(home: &Path, agent: &str, cwd: Option<&str>) -> Option<PathBuf> {
    crate::cli_sessions::validate_agent_session_id(agent).ok()?;
    let sessions = home.join("sessions");
    let boundary = sessions.canonicalize().ok()?;
    if !namespace_paths_match(&boundary, &home.canonicalize().ok()?.join("sessions")) {
        return None;
    }
    let contained = |path: PathBuf| {
        let expected = boundary.join(path.parent()?.file_name()?).join(agent);
        path.is_dir()
            .then(|| path.canonicalize().ok())
            .flatten()
            .filter(|actual| namespace_paths_match(actual, &expected))
            .map(|_| path)
    };
    let cwd = cwd.filter(|cwd| !cwd.is_empty());
    if let Some(cwd) = cwd {
        let path = sessions
            .join(paths::percent_encode_path_component(cwd))
            .join(agent);
        if let Some(path) = contained(path) {
            return Some(path);
        }
    }
    let mut found = None;
    for entry in fs::read_dir(&sessions).ok()? {
        let directory = entry.ok()?.path();
        let Some(path) = contained(directory.join(agent)) else {
            continue;
        };
        if let Some(requested) = cwd {
            let Some(recorded) = workspace_cwd(&directory) else {
                continue;
            };
            if !workspace_paths_match(&recorded, requested) {
                continue;
            }
        }
        // Without an exact folder, a repeated agent id is ambiguous. A caller's
        // explicit workspace must never become permission to read a different one.
        if found.is_some() {
            return None;
        }
        found = Some(path);
    }
    found
}

/// Build encodes short absolute cwd paths in the directory name. Long paths use
/// a slug/hash directory and a .cwd file; retain that physical name for migration
/// rather than duplicating Build's hashing algorithm or trusting summary fields.
fn workspace_cwd(directory: &Path) -> Option<String> {
    let name = directory.file_name()?.to_str()?;
    let decoded = percent_encoding::percent_decode_str(name)
        .decode_utf8()
        .ok()?;
    if Path::new(decoded.as_ref()).is_absolute() || decoded.starts_with('/') {
        return Some(decoded.into_owned());
    }
    const MAX_CWD_BYTES: u64 = 128 * 1024;
    let marker = directory.join(".cwd");
    let canonical = marker.canonicalize().ok()?;
    if canonical != directory.canonicalize().ok()?.join(".cwd") {
        return None;
    }
    let metadata = fs::metadata(&canonical).ok()?;
    if !metadata.is_file() || metadata.len() > MAX_CWD_BYTES {
        return None;
    }
    let mut bytes = Vec::new();
    fs::File::open(canonical)
        .ok()?
        .take(MAX_CWD_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() as u64 > MAX_CWD_BYTES {
        return None;
    }
    String::from_utf8(bytes).ok()
}

pub(crate) fn workspace_paths_match(recorded: &str, requested: &str) -> bool {
    // Match purely: deleted/offline workspaces still have readable history.
    // Unix case and literal backslashes must not inherit Windows normalization.
    #[cfg(windows)]
    let matches = |a: &str, b: &str| {
        let normalize = |path: &str| {
            let path = path.replace('\\', "/").to_ascii_lowercase();
            if let Some(unc) = path.strip_prefix("//?/unc/") {
                format!("//{unc}")
            } else {
                path.strip_prefix("//?/").unwrap_or(&path).to_owned()
            }
        };
        Path::new(&normalize(a)) == Path::new(&normalize(b))
    };
    #[cfg(not(windows))]
    let matches = |a: &str, b: &str| Path::new(a) == Path::new(b);
    matches(recorded, requested) || matches(recorded.trim_end_matches(['\r', '\n']), requested)
}

pub(crate) fn validate_rows(rows: &[SessionMeta]) -> Result<(), String> {
    for row in rows {
        if let Some(receipt) = &row.agent_execution {
            receipt.validate(row.agent_session_id.as_deref())?;
        }
    }
    Ok(())
}
