//! Explicit isolated-copy audit.
//!
//! The default is refuse. Overlapping live profile trees, credential names, reparse points,
//! and network or device paths are rejected before their bytes are hashed.
//! Reports keep counts, roles, sizes, and digests. They do not keep file text.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read};
use std::path::{Component, Path, PathBuf};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use serde_json::Value;

use sha2::{Digest, Sha256};

use super::errors::ErrorCode;
use super::hash::{canonical_json, sha256_bytes};
use crate::process_util;

const MAX_DEPTH: u32 = 16;
const MAX_FILES: usize = 100_000;
const MAX_JSONL_LINE: usize = 256 * 1024;

const ALLOWED_JSON_KEYS: &[&str] = &[
    "cwd",
    "gitBranch",
    "isSidechain",
    "name",
    "parentUuid",
    "role",
    "sessionId",
    "session_id",
    "timestamp",
    "tool_use_id",
    "type",
    "version",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuardError {
    MissingSourceCopy,
    RelativePath,
    LiveProfile { kind: String },
    NetworkOrDevice,
    NotADirectory,
    CredentialFile { relative: String },
    SymlinkEscape { relative: String },
    SourceBusy { relative: String },
    SourceChanged,
    Io { code: ErrorCode },
}

impl std::fmt::Display for GuardError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingSourceCopy => write!(f, "SOURCE_COPY_REQUIRED"),
            Self::RelativePath => write!(f, "SOURCE_COPY_RELATIVE_REFUSED"),
            Self::LiveProfile { kind } => write!(f, "LIVE_PROFILE_REFUSED:{kind}"),
            Self::NetworkOrDevice => write!(f, "NETWORK_OR_DEVICE_REFUSED"),
            Self::NotADirectory => write!(f, "SOURCE_NOT_A_DIRECTORY"),
            Self::CredentialFile { relative } => {
                write!(f, "CREDENTIAL_FILE_REFUSED:{relative}")
            }
            Self::SymlinkEscape { relative } => write!(f, "SYMLINK_ESCAPE_REFUSED:{relative}"),
            Self::SourceBusy { relative } => write!(f, "SOURCE_BUSY:{relative}"),
            Self::SourceChanged => write!(f, "SOURCE_CHANGED"),
            Self::Io { code } => write!(f, "{}", code.as_str()),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FileRole {
    Database,
    Wal,
    Shm,
    RollbackJournal,
    CompanionDatabase,
    Jsonl,
    Other,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ManifestEntry {
    pub relative: String,
    pub role: FileRole,
    pub size: u64,
    pub mtime_secs: Option<i64>,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct FieldCoverage {
    pub jsonl_records: u64,
    pub jsonl_partial_tail: u64,
    pub oversize_records: u64,
    pub keys: BTreeMap<String, u64>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct CursorCoverage {
    pub database_files: u64,
    pub wal_files: u64,
    pub shm_files: u64,
    pub rollback_journals: u64,
    pub companion_databases: u64,
    pub wal_without_shm: u64,
    pub rollback_journal_present: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AuditReport {
    pub provider_hint: String,
    pub format_families: Vec<String>,
    pub file_count: u64,
    pub total_bytes: u64,
    pub manifest_sha256: String,
    pub source_unchanged: bool,
    pub cursor: CursorCoverage,
    pub field_coverage: FieldCoverage,
    pub error_codes: Vec<String>,
    pub duration_ms: u64,
    pub peak_working_set_bytes: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TreeSnapshot {
    entries: Vec<ManifestEntry>,
    manifest_sha256: String,
    total_bytes: u64,
    coverage: FieldCoverage,
    cursor: CursorCoverage,
    families: Vec<String>,
    error_codes: Vec<String>,
}

pub fn known_live_profile_roots() -> Vec<(&'static str, PathBuf)> {
    let home = process_util::user_home();
    let mut roots = vec![
        ("codex-home", home.join(".codex")),
        ("claude-config", home.join(".claude")),
        ("cursor-home", home.join(".cursor")),
        ("cursor-user", super::detect::cursor_user_dir()),
        ("grok-home", home.join(".grok")),
        ("grok-app-home", home.join(".grok-app")),
    ];
    if let Some(appdata) = std::env::var_os("APPDATA") {
        let appdata = PathBuf::from(appdata);
        roots.push(("cursor-roaming", appdata.join("Cursor")));
        roots.push(("grok-appdata", appdata.join("grok-app")));
        roots.push(("grok-appdata-id", appdata.join("com.grokapp.grok-app")));
    }
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        roots.push(("cursor-local", PathBuf::from(local).join("Cursor")));
    }
    for candidate in super::detect::default_root_candidates() {
        roots.push((candidate.kind, candidate.path));
    }
    if let Some(path) = env_path("CODEX_HOME") {
        roots.push(("codex-home-env", path));
    }
    if let Some(path) = env_path("CLAUDE_CONFIG_DIR") {
        roots.push(("claude-config-env", path));
    }
    if let Some(path) = env_path("GROK_HOME") {
        roots.push(("grok-home-env", path));
    }
    if let Some(path) = env_path("GROK_APP_HOME") {
        roots.push(("grok-app-home-env", path));
    }
    roots
}

pub fn classify_requested_root(path: &Path) -> Result<(), GuardError> {
    let roots = known_live_profile_roots();
    let borrowed: Vec<(&str, &Path)> = roots
        .iter()
        .map(|(kind, root)| (*kind, root.as_path()))
        .collect();
    classify_against(path, &borrowed)
}

pub fn classify_against(path: &Path, live_roots: &[(&str, &Path)]) -> Result<(), GuardError> {
    let text = path.to_string_lossy();
    if text.trim().is_empty() {
        return Err(GuardError::MissingSourceCopy);
    }
    if is_network_or_device(path) || file_name_is_reserved_device(path) {
        return Err(GuardError::NetworkOrDevice);
    }
    if !path.is_absolute() {
        return Err(GuardError::RelativePath);
    }
    if let Some(kind) = matching_live_kind(path, live_roots) {
        return Err(GuardError::LiveProfile {
            kind: kind.to_string(),
        });
    }
    Ok(())
}

pub fn audit_isolated_copy(root: &Path, provider_hint: &str) -> Result<AuditReport, GuardError> {
    audit_isolated_copy_between(root, provider_hint, || {})
}

fn audit_isolated_copy_between(
    root: &Path,
    provider_hint: &str,
    between: impl FnOnce(),
) -> Result<AuditReport, GuardError> {
    let started = Instant::now();
    classify_requested_root(root)?;
    if path_name_is_credential(root) {
        return Err(GuardError::CredentialFile {
            relative: file_name_string(root),
        });
    }
    refuse_reparse_ancestors(root)?;
    let meta = fs::symlink_metadata(root).map_err(map_io)?;
    if is_reparse(&meta) {
        return Err(GuardError::SymlinkEscape {
            relative: ".".to_string(),
        });
    }
    if !meta.is_dir() {
        return Err(GuardError::NotADirectory);
    }
    let before = snapshot_tree(root)?;
    between();
    let after = snapshot_tree(root)?;
    if before.manifest_sha256 != after.manifest_sha256 {
        return Err(GuardError::SourceChanged);
    }
    Ok(AuditReport {
        provider_hint: sanitize_hint(provider_hint),
        format_families: before.families,
        file_count: before.entries.len() as u64,
        total_bytes: before.total_bytes,
        manifest_sha256: before.manifest_sha256,
        source_unchanged: true,
        cursor: before.cursor,
        field_coverage: before.coverage,
        error_codes: before.error_codes,
        duration_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        peak_working_set_bytes: peak_working_set_bytes(),
    })
}

fn refuse_reparse_ancestors(root: &Path) -> Result<(), GuardError> {
    let ancestors: Vec<_> = root.ancestors().skip(1).collect();
    // Check outward parents first, without resolving through a linked ancestor.
    for ancestor in ancestors.into_iter().rev() {
        if is_reparse(&fs::symlink_metadata(ancestor).map_err(map_io)?) {
            return Err(GuardError::SymlinkEscape {
                relative: ".".to_string(),
            });
        }
    }
    Ok(())
}

pub fn redact_audit_text(text: &str) -> String {
    redact_user_dirs(&redact_emails(&redact_secrets(text)))
}

pub fn is_credential_file_name(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    const EXACT: &[&str] = &[
        "auth.json",
        "credentials",
        "credentials.json",
        "cookies",
        "cookies.json",
        "cookie",
        ".env",
        ".netrc",
        "id_rsa",
        "id_ed25519",
        "token.json",
        "tokens.json",
        "secrets.json",
        "secret.json",
        "api_key",
        "apikey",
        "access_token",
        "refresh_token",
    ];
    if EXACT.contains(&n.as_str()) || n.starts_with(".env.") {
        return true;
    }
    n.ends_with(".pem")
        || n.ends_with(".p12")
        || n.ends_with(".pfx")
        || n.ends_with(".key")
        || n.contains("api_key")
        || n.contains("access_token")
        || n.contains("refresh_token")
        || n.contains("client_secret")
}

fn snapshot_tree(root: &Path) -> Result<TreeSnapshot, GuardError> {
    let mut entries = walk_files(root)?;
    assign_roles(&mut entries);
    entries.sort_by(|a, b| a.relative.cmp(&b.relative));
    let total_bytes = entries.iter().map(|entry| entry.size).sum();
    let manifest_sha256 = manifest_digest(&entries);
    let coverage = census_jsonl_tree(root, &entries)?;
    let cursor = cursor_coverage(&entries);
    let mut families = BTreeSet::new();
    let mut error_codes = BTreeSet::new();
    if coverage.oversize_records > 0 {
        error_codes.insert(ErrorCode::ResourceLimit.as_str().to_string());
    }
    for entry in &entries {
        match entry.role {
            FileRole::Jsonl => {
                families.insert("jsonl".to_string());
            }
            FileRole::Database
            | FileRole::Wal
            | FileRole::Shm
            | FileRole::RollbackJournal
            | FileRole::CompanionDatabase => {
                families.insert("sqlite".to_string());
            }
            FileRole::Other => {
                families.insert("other".to_string());
            }
        }
    }
    Ok(TreeSnapshot {
        entries,
        manifest_sha256,
        total_bytes,
        coverage,
        cursor,
        families: families.into_iter().collect(),
        error_codes: error_codes.into_iter().collect(),
    })
}

fn walk_files(root: &Path) -> Result<Vec<ManifestEntry>, GuardError> {
    let mut entries = Vec::new();
    let mut stack = vec![(root.to_path_buf(), 0u32)];
    while let Some((dir, depth)) = stack.pop() {
        if depth > MAX_DEPTH {
            return Err(GuardError::Io {
                code: ErrorCode::ResourceLimit,
            });
        }
        let read = fs::read_dir(&dir).map_err(map_io)?;
        for item in read {
            let item = item.map_err(map_io)?;
            let path = item.path();
            let name = item.file_name();
            let name = name.to_string_lossy();
            if is_credential_file_name(&name) {
                return Err(GuardError::CredentialFile {
                    relative: relative_posix(root, &path)?,
                });
            }
            let meta = fs::symlink_metadata(&path).map_err(|err| {
                with_relative(map_io(err), relative_posix(root, &path).unwrap_or_default())
            })?;
            if is_reparse(&meta) {
                return Err(GuardError::SymlinkEscape {
                    relative: relative_posix(root, &path).unwrap_or_else(|_| "reparse".to_string()),
                });
            }
            if meta.is_dir() {
                stack.push((path, depth + 1));
                continue;
            }
            if !meta.is_file() {
                continue;
            }
            if entries.len() >= MAX_FILES {
                return Err(GuardError::Io {
                    code: ErrorCode::ResourceLimit,
                });
            }
            let relative = relative_posix(root, &path)?;
            let (sha256, size, mtime_secs) =
                hash_file(&path).map_err(|err| with_relative(err, relative.clone()))?;
            entries.push(ManifestEntry {
                relative,
                role: FileRole::Other,
                size,
                mtime_secs,
                sha256,
            });
        }
    }
    Ok(entries)
}

fn census_jsonl_tree(root: &Path, entries: &[ManifestEntry]) -> Result<FieldCoverage, GuardError> {
    let mut coverage = FieldCoverage::default();
    for entry in entries {
        if entry.role != FileRole::Jsonl {
            continue;
        }
        let path = root.join(entry.relative.replace('/', std::path::MAIN_SEPARATOR_STR));
        census_jsonl_file(&path, &mut coverage)
            .map_err(|err| with_relative(err, entry.relative.clone()))?;
    }
    Ok(coverage)
}

fn census_jsonl_file(path: &Path, coverage: &mut FieldCoverage) -> Result<(), GuardError> {
    let file = File::open(path).map_err(map_io)?;
    let mut reader = BufReader::new(file);
    let mut last_partial = false;
    loop {
        match read_line_capped(&mut reader, MAX_JSONL_LINE).map_err(map_io)? {
            LineRead::Eof => break,
            LineRead::Oversize => {
                coverage.oversize_records += 1;
                drain_until_newline(&mut reader).map_err(map_io)?;
                last_partial = false;
            }
            LineRead::Line(mut line) => {
                if line.last() == Some(&b'\n') {
                    line.pop();
                    if line.last() == Some(&b'\r') {
                        line.pop();
                    }
                }
                if line.is_empty() {
                    last_partial = false;
                    continue;
                }
                match serde_json::from_slice::<Value>(&line) {
                    Ok(value) => {
                        last_partial = false;
                        coverage.jsonl_records += 1;
                        if let Some(object) = value.as_object() {
                            for key in ALLOWED_JSON_KEYS {
                                if object.contains_key(*key) {
                                    *coverage.keys.entry((*key).to_string()).or_insert(0) += 1;
                                }
                            }
                        }
                    }
                    Err(_) => last_partial = true,
                }
            }
        }
    }
    if last_partial {
        coverage.jsonl_partial_tail += 1;
    }
    Ok(())
}

enum LineRead {
    Eof,
    Line(Vec<u8>),
    Oversize,
}

fn read_line_capped(reader: &mut impl BufRead, cap: usize) -> std::io::Result<LineRead> {
    let mut buf = Vec::new();
    loop {
        let step = {
            let available = reader.fill_buf()?;
            if available.is_empty() {
                return Ok(if buf.is_empty() {
                    LineRead::Eof
                } else {
                    LineRead::Line(buf)
                });
            }
            if let Some(pos) = available.iter().position(|byte| *byte == b'\n') {
                buf.extend_from_slice(&available[..=pos]);
                (pos + 1, true, false)
            } else if buf.len().saturating_add(available.len()) > cap {
                (available.len(), false, true)
            } else {
                let n = available.len();
                buf.extend_from_slice(available);
                (n, false, false)
            }
        };
        reader.consume(step.0);
        if step.1 {
            return Ok(LineRead::Line(buf));
        }
        if step.2 {
            return Ok(LineRead::Oversize);
        }
    }
}

fn drain_until_newline(reader: &mut impl BufRead) -> std::io::Result<()> {
    loop {
        let step = {
            let available = reader.fill_buf()?;
            if available.is_empty() {
                return Ok(());
            }
            if let Some(pos) = available.iter().position(|byte| *byte == b'\n') {
                (pos + 1, true)
            } else {
                (available.len(), false)
            }
        };
        reader.consume(step.0);
        if step.1 {
            return Ok(());
        }
    }
}

fn assign_roles(entries: &mut [ManifestEntry]) {
    let has_workspace_db = entries.iter().any(|entry| {
        let lower = entry.relative.to_ascii_lowercase();
        lower.contains("workspacestorage/") && is_database_name(&lower)
    });
    for entry in entries.iter_mut() {
        entry.role = classify_role(&entry.relative, has_workspace_db);
    }
}

fn classify_role(relative: &str, has_workspace_db: bool) -> FileRole {
    let name = relative.rsplit('/').next().unwrap_or(relative);
    let lower_name = name.to_ascii_lowercase();
    let lower_rel = relative.to_ascii_lowercase();
    if lower_name.ends_with("-journal") {
        return FileRole::RollbackJournal;
    }
    if lower_name.ends_with("-wal") {
        return FileRole::Wal;
    }
    if lower_name.ends_with("-shm") {
        return FileRole::Shm;
    }
    if lower_name.ends_with(".jsonl") {
        return FileRole::Jsonl;
    }
    if is_database_name(&lower_name)
        && has_workspace_db
        && lower_rel.contains("globalstorage/")
        && lower_name == "state.vscdb"
    {
        return FileRole::CompanionDatabase;
    }
    if is_database_name(&lower_name) {
        return FileRole::Database;
    }
    FileRole::Other
}

fn is_database_name(lower_name: &str) -> bool {
    let base = lower_name
        .trim_end_matches("-journal")
        .trim_end_matches("-wal")
        .trim_end_matches("-shm");
    base.ends_with(".vscdb")
        || base.ends_with(".sqlite")
        || base.ends_with(".sqlite3")
        || base.ends_with(".db")
}

fn cursor_coverage(entries: &[ManifestEntry]) -> CursorCoverage {
    let mut coverage = CursorCoverage::default();
    let shm: BTreeSet<&str> = entries
        .iter()
        .filter(|entry| entry.role == FileRole::Shm)
        .map(|entry| entry.relative.as_str())
        .collect();
    for entry in entries {
        match entry.role {
            FileRole::Database => coverage.database_files += 1,
            FileRole::Wal => {
                coverage.wal_files += 1;
                let expected = format!("{}-shm", entry.relative.trim_end_matches("-wal"));
                if !shm.contains(expected.as_str()) {
                    coverage.wal_without_shm += 1;
                }
            }
            FileRole::Shm => coverage.shm_files += 1,
            FileRole::RollbackJournal => coverage.rollback_journals += 1,
            FileRole::CompanionDatabase => coverage.companion_databases += 1,
            FileRole::Jsonl | FileRole::Other => {}
        }
    }
    coverage.rollback_journal_present = coverage.rollback_journals > 0;
    coverage
}

fn manifest_digest(entries: &[ManifestEntry]) -> String {
    let value = serde_json::to_value(entries).expect("manifest entries serialize");
    sha256_bytes(canonical_json(&value).as_bytes())
}

fn hash_file(path: &Path) -> Result<(String, u64, Option<i64>), GuardError> {
    let mut file = File::open(path).map_err(map_io)?;
    let meta = file.metadata().map_err(map_io)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    let mut size = 0u64;
    loop {
        let n = file.read(&mut buffer).map_err(map_io)?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
        size += n as u64;
    }
    let mtime_secs = meta.modified().ok().and_then(mtime_secs);
    Ok((hex::encode(hasher.finalize()), size, mtime_secs))
}

fn mtime_secs(time: SystemTime) -> Option<i64> {
    time.duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_secs()).ok())
}

fn relative_posix(root: &Path, path: &Path) -> Result<String, GuardError> {
    let rel = path
        .strip_prefix(root)
        .map_err(|_| GuardError::SymlinkEscape {
            relative: "outside".to_string(),
        })?;
    let mut parts = Vec::new();
    for component in rel.components() {
        match component {
            Component::Normal(part) => parts.push(part.to_string_lossy().into_owned()),
            _ => {
                return Err(GuardError::SymlinkEscape {
                    relative: "outside".to_string(),
                });
            }
        }
    }
    Ok(parts.join("/"))
}

fn matching_live_kind<'a>(path: &Path, live_roots: &[(&'a str, &Path)]) -> Option<&'a str> {
    let mut ranked: Vec<(&str, &Path)> = live_roots.to_vec();
    ranked.sort_by_key(|(_, root)| std::cmp::Reverse(normalize_key(root).len()));
    ranked
        .into_iter()
        // Auditing a parent would recursively enter a live profile as well.
        // Refuse either overlap direction before any filesystem enumeration.
        .find(|(_, root)| is_same_or_child(root, path) || is_same_or_child(path, root))
        .map(|(kind, _)| kind)
}

fn is_same_or_child(root: &Path, candidate: &Path) -> bool {
    let root = normalize_key(root);
    let candidate = normalize_key(candidate);
    if candidate == root {
        return true;
    }
    let root_parts: Vec<_> = Path::new(&root).components().collect();
    let candidate_parts: Vec<_> = Path::new(&candidate).components().collect();
    candidate_parts.len() > root_parts.len()
        && candidate_parts
            .iter()
            .zip(root_parts.iter())
            .all(|(left, right)| left == right)
}

fn normalize_key(path: &Path) -> String {
    let stripped = strip_verbatim(&path.to_string_lossy());
    let mut out = PathBuf::new();
    for component in Path::new(&stripped).components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    if cfg!(windows) {
        out.to_string_lossy().to_ascii_lowercase()
    } else {
        out.to_string_lossy().into_owned()
    }
}

fn strip_verbatim(text: &str) -> String {
    let trimmed = text.trim();
    if let Some(rest) = trimmed.strip_prefix(r"\\?\UNC\") {
        return format!(r"\\{rest}");
    }
    if let Some(rest) = trimmed.strip_prefix(r"\\?\") {
        return rest.to_string();
    }
    trimmed.to_string()
}

fn is_network_or_device(path: &Path) -> bool {
    let text = path.to_string_lossy();
    let trimmed = text.trim();
    if trimmed.starts_with(r"\\.\") || trimmed.starts_with("//./") {
        return true;
    }
    let lower = trimmed.to_ascii_lowercase();
    if lower.starts_with(r"\\?\unc\") {
        return true;
    }
    if trimmed.starts_with(r"\\") && !trimmed.starts_with(r"\\?\") {
        return true;
    }
    trimmed.starts_with("//") && !trimmed.starts_with("//?/")
}

fn file_name_is_reserved_device(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
        return false;
    };
    let base = name.split('.').next().unwrap_or(name);
    matches!(
        base.to_ascii_uppercase().as_str(),
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
    )
}

fn path_name_is_credential(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(is_credential_file_name)
}

fn file_name_string(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "credential".to_string())
}

fn env_path(key: &str) -> Option<PathBuf> {
    std::env::var(key)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
}

fn sanitize_hint(hint: &str) -> String {
    let trimmed = hint.trim().to_ascii_lowercase();
    if !trimmed.is_empty()
        && trimmed.len() <= 32
        && trimmed
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
    {
        trimmed
    } else {
        "unspecified".to_string()
    }
}

fn map_io(err: std::io::Error) -> GuardError {
    if matches!(err.raw_os_error(), Some(32 | 33)) {
        return GuardError::SourceBusy {
            relative: String::new(),
        };
    }
    let code = match err.kind() {
        std::io::ErrorKind::NotFound => ErrorCode::SourceNotFound,
        std::io::ErrorKind::PermissionDenied => ErrorCode::SourcePermissionDenied,
        _ => ErrorCode::SnapshotUnavailable,
    };
    GuardError::Io { code }
}

fn with_relative(err: GuardError, relative: String) -> GuardError {
    match err {
        GuardError::SourceBusy { .. } => GuardError::SourceBusy { relative },
        other => other,
    }
}

fn is_reparse(meta: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }
    #[cfg(not(windows))]
    {
        meta.file_type().is_symlink()
    }
}

fn peak_working_set_bytes() -> Option<u64> {
    #[cfg(windows)]
    {
        windows_peak_working_set()
    }
    #[cfg(not(windows))]
    {
        None
    }
}

#[cfg(windows)]
fn windows_peak_working_set() -> Option<u64> {
    #[repr(C)]
    struct Counters {
        cb: u32,
        page_fault_count: u32,
        peak_working_set_size: usize,
        working_set_size: usize,
        quota_peak_paged_pool_usage: usize,
        quota_paged_pool_usage: usize,
        quota_peak_non_paged_pool_usage: usize,
        quota_non_paged_pool_usage: usize,
        pagefile_usage: usize,
        peak_pagefile_usage: usize,
    }
    #[link(name = "psapi")]
    extern "system" {
        fn GetProcessMemoryInfo(process: isize, counters: *mut Counters, cb: u32) -> i32;
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn GetCurrentProcess() -> isize;
    }
    // SAFETY: counters is zeroed, cb is the struct size, and the pointer stays live for the call.
    unsafe {
        let mut counters = std::mem::zeroed::<Counters>();
        counters.cb = u32::try_from(std::mem::size_of::<Counters>()).ok()?;
        if GetProcessMemoryInfo(GetCurrentProcess(), &mut counters, counters.cb) == 0 {
            None
        } else {
            Some(counters.peak_working_set_size as u64)
        }
    }
}

fn redact_secrets(text: &str) -> String {
    let lower = text.to_ascii_lowercase();
    const MARKERS: &[&str] = &[
        "refresh_token=",
        "access_token=",
        "api_key=",
        "api-key=",
        "bearer ",
        "token=",
        "sk-",
    ];
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut index = 0;
    while index < bytes.len() {
        if let Some(marker) = MARKERS
            .iter()
            .copied()
            .find(|marker| lower[index..].starts_with(marker))
        {
            out.push_str("[redacted-secret]");
            index += marker.len();
            while index < bytes.len() && !is_secret_end(bytes[index]) {
                index += 1;
            }
            continue;
        }
        let ch = text[index..].chars().next().expect("char boundary");
        out.push(ch);
        index += ch.len_utf8();
    }
    out
}

fn is_secret_end(byte: u8) -> bool {
    byte.is_ascii_whitespace() || matches!(byte, b'"' | b'\'' | b',' | b';' | b')' | b']')
}

fn redact_emails(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut index = 0;
    while index < chars.len() {
        if chars[index] == '@' {
            let mut left = index;
            while left > 0 && is_email_char(chars[left - 1]) {
                left -= 1;
            }
            let mut right = index + 1;
            while right < chars.len() && is_email_char(chars[right]) {
                right += 1;
            }
            let domain = &chars[index + 1..right];
            if left < index && domain.contains(&'.') {
                let drop_chars = index - left;
                let keep = out.chars().count().saturating_sub(drop_chars);
                out = out.chars().take(keep).collect();
                out.push_str("[redacted-email]");
                index = right;
                continue;
            }
        }
        out.push(chars[index]);
        index += 1;
    }
    out
}

fn is_email_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '%' | '+' | '-')
}

fn redact_user_dirs(text: &str) -> String {
    let mut output = text.to_string();
    for marker in ["\\Users\\", "/Users/", "\\users\\", "/users/"] {
        loop {
            let lower = output.to_ascii_lowercase();
            let Some(found) = lower.find(&marker.to_ascii_lowercase()) else {
                break;
            };
            let start = found + marker.len();
            let rest = &output[start..];
            let end_rel = rest.find(['\\', '/', ' ', '"', '\'']).unwrap_or(rest.len());
            output.replace_range(found..start + end_rel, "[redacted-user]");
        }
    }
    output
}

#[cfg(test)]
#[path = "source_copy_guard_tests.rs"]
mod tests;
