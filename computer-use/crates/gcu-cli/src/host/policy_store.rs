//! Persistent always-grants. The engine reads them; only this Host writes them.
use crate::private;
use gcu_host_api::{AppIdentity, GrantProof, Platform, PolicyConfig, PolicyStore, SignatureState};
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_BYTES: u64 = 256 * 1024;

/// Why a grant file was not changed.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum StoreFault {
    /// Valid permissions, but the JSON is not a grant map. The file is left as it was.
    #[error("approvals file is corrupt and was left unchanged")]
    Corrupt,
    /// Symlink, unexpected type, or unsafe permissions. The path was not followed.
    #[error("approvals file is missing or has unsafe permissions")]
    Unsafe,
    /// The directory could not be updated.
    #[error("approvals file could not be written")]
    Io,
}

enum Class {
    Missing,
    Ready(Map<String, Value>),
    Corrupt,
    Unsafe,
}

/// In-memory policy plus the optional `approvals.json` path.
pub(crate) struct HostPolicy {
    path: Option<PathBuf>,
    inner: Mutex<PolicyConfig>,
}

impl HostPolicy {
    pub(crate) fn memory(config: PolicyConfig) -> Self {
        Self {
            path: None,
            inner: Mutex::new(config),
        }
    }

    /// Missing file means no grants. A bad file clears memory and does not abort startup.
    pub(crate) fn open(mut config: PolicyConfig, path: PathBuf) -> Self {
        match load_grants(&path) {
            Ok(grants) => config.always = grants,
            Err(_) => config.always.clear(),
        }
        Self {
            path: Some(path),
            inner: Mutex::new(config),
        }
    }

    pub(crate) fn approvals_path(&self) -> Option<PathBuf> {
        self.path.clone()
    }

    pub(crate) fn remember(&self, identity: &AppIdentity) {
        let mut config = lock(&self.inner);
        // Dedup is not authorization. An identical observation is stored once.
        // Only grant_eq may allow a later call.
        if !config
            .always
            .iter()
            .any(|grant| grant.grant_eq(identity) || grant == identity)
        {
            config.always.push(identity.clone());
        }
    }
}

impl PolicyStore for HostPolicy {
    fn load(&self) -> PolicyConfig {
        lock(&self.inner).clone()
    }

    fn always_allowed(&self, id: &AppIdentity) -> bool {
        self.load().always.iter().any(|grant| grant.grant_eq(id))
    }

    fn refresh(&self) {
        let Some(path) = &self.path else {
            return;
        };
        let mut config = lock(&self.inner);
        match load_grants(path) {
            Ok(grants) => config.always = grants,
            Err(_) => config.always.clear(),
        }
    }
}

fn lock(mutex: &Mutex<PolicyConfig>) -> std::sync::MutexGuard<'_, PolicyConfig> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Whether `identity` satisfies a grant in `path`. Errors and a missing file are false.
pub fn allows(path: &Path, identity: &AppIdentity) -> bool {
    load_grants(path).is_ok_and(|grants| grants.iter().any(|grant| grant.grant_eq(identity)))
}

/// Write one always-grant. Corrupt and unsafe files are not replaced.
///
/// A JSON object whose proof field has the wrong type is corrupt. It is not
/// parsed as a partial grant and it is not overwritten.
pub fn grant(path: &Path, identity: &AppIdentity) -> Result<(), StoreFault> {
    let mut map = match classify(path) {
        Class::Missing => Map::new(),
        Class::Ready(map) => map,
        Class::Corrupt => return Err(StoreFault::Corrupt),
        Class::Unsafe => return Err(StoreFault::Unsafe),
    };
    parse_map(&map)?;
    map.insert(storage_key(identity), record(identity));
    write_map(path, &map)
}

/// Remove the storage key printed by [`list_text`]. `Ok(false)` means it was not present.
pub fn revoke(path: &Path, key: &str) -> Result<bool, StoreFault> {
    let mut map = match classify(path) {
        Class::Missing => return Ok(false),
        Class::Ready(map) => map,
        Class::Corrupt => return Err(StoreFault::Corrupt),
        Class::Unsafe => return Err(StoreFault::Unsafe),
    };
    parse_map(&map)?;
    if map.remove(key).is_none() {
        return Ok(false);
    }
    if map.is_empty() {
        return match std::fs::remove_file(path) {
            Ok(()) => Ok(true),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(true),
            Err(_) => Err(StoreFault::Io),
        };
    }
    write_map(path, &map).map(|()| true)
}

/// Stored records for operators. JSON is data, not translated prose.
pub fn list_json(path: &Path) -> Result<Vec<Value>, StoreFault> {
    let map = match classify(path) {
        Class::Missing => return Ok(Vec::new()),
        Class::Ready(map) => map,
        Class::Corrupt => return Err(StoreFault::Corrupt),
        Class::Unsafe => return Err(StoreFault::Unsafe),
    };
    parse_map(&map)?;
    let mut rows: Vec<Value> = map
        .into_iter()
        .map(|(key, value)| serde_json::json!({"key": key, "record": value}))
        .collect();
    rows.sort_by(|left, right| {
        left["key"]
            .as_str()
            .unwrap_or("")
            .cmp(right["key"].as_str().unwrap_or(""))
    });
    Ok(rows)
}

/// One storage key per line, or the localized empty sentence.
pub fn list_text(path: &Path, locale: &str) -> Result<String, StoreFault> {
    let rows = list_json(path)?;
    if rows.is_empty() {
        return Ok(gcu_mcp::text(locale, "approvals.empty"));
    }
    let mut lines = Vec::new();
    for row in rows {
        if let Some(key) = row.get("key").and_then(Value::as_str) {
            lines.push(key.to_string());
        }
    }
    Ok(lines.join("\n"))
}

/// `platform:` plus the app key. The split is the first colon, so `C:\` paths stay intact.
pub fn storage_key(identity: &AppIdentity) -> String {
    format!("{}:{}", platform_name(identity.platform), identity.key)
}

fn load_grants(path: &Path) -> Result<Vec<AppIdentity>, StoreFault> {
    match classify(path) {
        Class::Missing => Ok(Vec::new()),
        Class::Ready(map) => parse_map(&map),
        Class::Corrupt => Err(StoreFault::Corrupt),
        Class::Unsafe => Err(StoreFault::Unsafe),
    }
}

fn classify(path: &Path) -> Class {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Class::Missing,
        Err(_) => return Class::Unsafe,
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() > MAX_BYTES {
        return if metadata.file_type().is_symlink() || !metadata.is_file() {
            Class::Unsafe
        } else {
            Class::Corrupt
        };
    }
    if private::check_file(path).is_err() {
        return Class::Unsafe;
    }
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(_) => return Class::Unsafe,
    };
    match serde_json::from_slice::<Value>(&bytes) {
        Ok(Value::Object(map)) => Class::Ready(map),
        _ => Class::Corrupt,
    }
}

fn parse_map(map: &Map<String, Value>) -> Result<Vec<AppIdentity>, StoreFault> {
    let mut grants = Vec::new();
    for (raw_key, value) in map {
        grants.push(parse_entry(raw_key, value)?);
    }
    Ok(grants)
}

fn parse_entry(raw_key: &str, value: &Value) -> Result<AppIdentity, StoreFault> {
    let (platform_name, key) = raw_key.split_once(':').ok_or(StoreFault::Corrupt)?;
    if key.is_empty() {
        return Err(StoreFault::Corrupt);
    }
    let platform = parse_platform(platform_name).ok_or(StoreFault::Corrupt)?;
    let object = value.as_object().ok_or(StoreFault::Corrupt)?;
    if object.get("decision").and_then(Value::as_str) != Some("always") {
        return Err(StoreFault::Corrupt);
    }
    let granted_at = object
        .get("granted_at")
        .and_then(Value::as_str)
        .ok_or(StoreFault::Corrupt)?;
    if granted_at.len() < 20 {
        return Err(StoreFault::Corrupt);
    }
    let ident = object
        .get("identity")
        .and_then(Value::as_object)
        .ok_or(StoreFault::Corrupt)?;
    if ident.get("platform").and_then(Value::as_str) != Some(platform_name)
        || ident.get("key").and_then(Value::as_str) != Some(key)
    {
        return Err(StoreFault::Corrupt);
    }
    let signature = match ident.get("signature").and_then(Value::as_str) {
        Some("valid") => SignatureState::Valid,
        Some("invalid") => SignatureState::Invalid,
        Some("unsigned") => SignatureState::Unsigned,
        Some("unknown") | Some(_) => SignatureState::Unknown,
        None => return Err(StoreFault::Corrupt),
    };
    Ok(AppIdentity {
        platform,
        key: key.to_string(),
        team_id: opt_string(ident, "team_id")?,
        publisher: opt_string(ident, "publisher")?,
        sha256: path_hash(ident)?,
        signature,
        image_path: required_string(ident, "image_path")?,
        proof: parse_proof(ident)?,
    })
}

fn parse_proof(ident: &Map<String, Value>) -> Result<GrantProof, StoreFault> {
    Ok(GrantProof {
        cdhash: opt_string(ident, "cdhash")?,
        certificate_sha256: opt_string(ident, "certificate_sha256")?,
        product: opt_string(ident, "product")?,
        build: opt_string(ident, "build")?,
        file_dev: opt_u64(ident, "file_dev")?,
        file_inode: opt_u64(ident, "file_inode")?,
    })
}

fn opt_string(object: &Map<String, Value>, key: &str) -> Result<Option<String>, StoreFault> {
    match object.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) => Ok(Some(text.clone())),
        Some(_) => Err(StoreFault::Corrupt),
    }
}

fn required_string(object: &Map<String, Value>, key: &str) -> Result<String, StoreFault> {
    object
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or(StoreFault::Corrupt)
}

fn opt_u64(object: &Map<String, Value>, key: &str) -> Result<Option<u64>, StoreFault> {
    match object.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(number)) => number.as_u64().map(Some).ok_or(StoreFault::Corrupt),
        Some(_) => Err(StoreFault::Corrupt),
    }
}

fn path_hash(object: &Map<String, Value>) -> Result<Option<[u8; 32]>, StoreFault> {
    match object.get("path_hash") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) => hex_decode(text).map(Some).ok_or(StoreFault::Corrupt),
        Some(_) => Err(StoreFault::Corrupt),
    }
}

fn record(identity: &AppIdentity) -> Value {
    serde_json::json!({
        "decision": "always",
        "granted_at": format_granted_at(SystemTime::now()),
        "identity": {
            "team_id": identity.team_id,
            "publisher": identity.publisher,
            "path_hash": identity.sha256.map(hex_encode),
            "platform": platform_name(identity.platform),
            "key": identity.key,
            "signature": signature_name(identity.signature),
            "image_path": identity.image_path,
            "cdhash": identity.proof.cdhash,
            "certificate_sha256": identity.proof.certificate_sha256,
            "product": identity.proof.product,
            "build": identity.proof.build,
            "file_dev": identity.proof.file_dev,
            "file_inode": identity.proof.file_inode,
        }
    })
}

fn write_map(path: &Path, map: &Map<String, Value>) -> Result<(), StoreFault> {
    let mut body = serde_json::to_vec(map).map_err(|_| StoreFault::Io)?;
    body.push(b'\n');
    private::write_secret(path, &body).map_err(|_| StoreFault::Io)
}

fn platform_name(platform: Platform) -> &'static str {
    match platform {
        Platform::Macos => "macos",
        Platform::Windows => "windows",
        Platform::Linux => "linux",
    }
}

fn parse_platform(name: &str) -> Option<Platform> {
    match name {
        "macos" => Some(Platform::Macos),
        "windows" => Some(Platform::Windows),
        "linux" => Some(Platform::Linux),
        _ => None,
    }
}

fn signature_name(signature: SignatureState) -> &'static str {
    match signature {
        SignatureState::Valid => "valid",
        SignatureState::Invalid => "invalid",
        SignatureState::Unsigned => "unsigned",
        SignatureState::Unknown => "unknown",
    }
}

fn hex_encode(bytes: [u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(64);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0xf) as usize] as char);
    }
    out
}

fn hex_decode(text: &str) -> Option<[u8; 32]> {
    if text.len() != 64 {
        return None;
    }
    let bytes = text.as_bytes();
    let mut out = [0; 32];
    for index in 0..32 {
        let high = nibble(bytes[index * 2])?;
        let low = nibble(bytes[index * 2 + 1])?;
        out[index] = (high << 4) | low;
    }
    Some(out)
}

fn nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

pub(crate) fn format_granted_at(now: SystemTime) -> String {
    let secs = now
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0);
    let days = i64::try_from(secs / 86_400).unwrap_or(i64::MAX);
    let tod = (secs % 86_400) as u32;
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        tod / 3600,
        (tod % 3600) / 60,
        tod % 60
    )
}

/// Howard Hinnant civil_from_days, days since Unix epoch.
fn civil_from_days(days: i64) -> (i32, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let mut year = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    if month <= 2 {
        year += 1;
    }
    (year as i32, month as u32, day as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn unix_epoch_and_known_day_format_as_utc() {
        assert_eq!(format_granted_at(UNIX_EPOCH), "1970-01-01T00:00:00Z");
        let next = UNIX_EPOCH + Duration::from_secs(86_400);
        assert_eq!(format_granted_at(next), "1970-01-02T00:00:00Z");
        let day = UNIX_EPOCH + Duration::from_secs(1_792_108_800);
        assert_eq!(format_granted_at(day), "2026-10-16T00:00:00Z");
    }
}
