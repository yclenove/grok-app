//! Authenticated opaque page cursors.
//!
//! Envelope: `impcur1.<urlsafe-b64 canonical payload>.<urlsafe-b64 HMAC-SHA256>`.
//! The HMAC is verified before offset/mark/exp are read. The secret is
//! installation-scoped, persisted under the app imports dir, and never stored
//! inside an imported revision.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;

use super::errors::{ErrorCode, ImportError};

pub const CURSOR_VERSION: u32 = 1;
pub const CURSOR_TTL_SECS: i64 = 24 * 60 * 60;
const ENVELOPE_PREFIX: &str = "impcur1";
const SECRET_BYTES: usize = 32;
const INSTALLATION_MAC_PREFIX: &[u8] = b"grok-app-installation-mac-v1\0";

type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BoundCursor {
    pub v: u32,
    pub owner: String,
    pub snapshot: String,
    pub leaf: String,
    pub filter: String,
    pub offset: u32,
    pub exp: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mark: Option<String>,
}

#[derive(Debug, Clone, Copy)]
pub struct CursorScope<'a> {
    pub owner: &'a str,
    pub snapshot: &'a str,
    pub leaf: &'a str,
    pub filter: &'a str,
}

static SECRET_CACHE: Mutex<Option<(PathBuf, [u8; SECRET_BYTES])>> = Mutex::new(None);

fn stale() -> ImportError {
    ImportError::new(ErrorCode::PlanStale, "invalid or stale page cursor")
}

fn canonical_cursor_payload(cursor: &BoundCursor) -> String {
    super::hash::canonical_json(&serde_json::json!({
        "exp": cursor.exp,
        "filter": cursor.filter,
        "leaf": cursor.leaf,
        "mark": cursor.mark,
        "offset": cursor.offset,
        "owner": cursor.owner,
        "snapshot": cursor.snapshot,
        "v": cursor.v,
    }))
}

fn cursor_secret_path() -> Result<PathBuf, ImportError> {
    #[cfg(test)]
    if std::env::var_os("GROK_APP_HOME").is_none() {
        let dir = std::env::temp_dir().join("grok-app-test-cursor-hmac");
        fs::create_dir_all(&dir).map_err(|error| {
            ImportError::new(
                ErrorCode::SnapshotUnavailable,
                format!("cursor secret dir: {error}"),
            )
        })?;
        return Ok(dir.join(".cursor-hmac-secret"));
    }
    let dir = crate::paths::app_data_root().join("imports");
    fs::create_dir_all(&dir).map_err(|error| {
        ImportError::new(
            ErrorCode::SnapshotUnavailable,
            format!("cursor secret dir: {error}"),
        )
    })?;
    Ok(dir.join(".cursor-hmac-secret"))
}

fn random_secret() -> [u8; SECRET_BYTES] {
    let mut secret = [0u8; SECRET_BYTES];
    secret[..16].copy_from_slice(uuid::Uuid::new_v4().as_bytes());
    secret[16..].copy_from_slice(uuid::Uuid::new_v4().as_bytes());
    secret
}

fn require_regular_secret_file(path: &std::path::Path) -> Result<(), ImportError> {
    let meta = fs::symlink_metadata(path).map_err(|error| {
        ImportError::new(
            ErrorCode::SnapshotUnavailable,
            format!("stat cursor secret: {error}"),
        )
    })?;
    if !meta.file_type().is_file() {
        return Err(ImportError::new(
            ErrorCode::SnapshotUnavailable,
            "cursor secret must be a regular file",
        ));
    }
    Ok(())
}

fn read_cursor_secret_file(path: &std::path::Path) -> Result<[u8; SECRET_BYTES], ImportError> {
    require_regular_secret_file(path)?;
    let mut last_len = 0usize;
    for attempt in 0..16 {
        let bytes = fs::read(path).map_err(|error| {
            ImportError::new(
                ErrorCode::SnapshotUnavailable,
                format!("read cursor secret: {error}"),
            )
        })?;
        last_len = bytes.len();
        if bytes.len() == SECRET_BYTES {
            let mut secret = [0u8; SECRET_BYTES];
            secret.copy_from_slice(&bytes);
            return Ok(secret);
        }
        if bytes.len() > SECRET_BYTES || (!bytes.is_empty() && attempt >= 2) {
            break;
        }
        if attempt + 1 < 16 {
            std::thread::sleep(std::time::Duration::from_millis(10 * (attempt + 1) as u64));
        }
    }
    Err(ImportError::new(
        ErrorCode::SnapshotUnavailable,
        format!("cursor secret has the wrong length ({last_len})"),
    ))
}

fn load_or_create_cursor_secret() -> Result<[u8; SECRET_BYTES], ImportError> {
    let path = cursor_secret_path()?;
    {
        let cache = SECRET_CACHE.lock().map_err(|_| {
            ImportError::new(
                ErrorCode::SnapshotUnavailable,
                "cursor secret lock poisoned",
            )
        })?;
        if let Some((cached_path, secret)) = cache.as_ref() {
            if cached_path == &path {
                return Ok(*secret);
            }
        }
    }
    if path.exists() {
        let secret = read_cursor_secret_file(&path)?;
        let mut cache = SECRET_CACHE.lock().map_err(|_| {
            ImportError::new(
                ErrorCode::SnapshotUnavailable,
                "cursor secret lock poisoned",
            )
        })?;
        *cache = Some((path, secret));
        return Ok(secret);
    }
    let tmp = path.with_file_name(format!(".cursor-hmac-secret.{}.tmp", uuid::Uuid::new_v4()));
    let mut opts = OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let secret = match opts.open(&tmp) {
        Ok(mut file) => {
            #[cfg(test)]
            if let Ok(ms) = std::env::var("GROK_TEST_SECRET_CREATE_PAUSE_MS") {
                if let Ok(n) = ms.parse::<u64>() {
                    std::thread::sleep(std::time::Duration::from_millis(n));
                }
            }
            let secret = random_secret();
            let write = (|| {
                file.write_all(&secret).map_err(|error| {
                    ImportError::new(
                        ErrorCode::SnapshotUnavailable,
                        format!("write cursor secret: {error}"),
                    )
                })?;
                file.sync_all().map_err(|error| {
                    ImportError::new(
                        ErrorCode::SnapshotUnavailable,
                        format!("sync cursor secret: {error}"),
                    )
                })?;
                Ok(())
            })();
            if let Err(error) = write {
                let _ = fs::remove_file(&tmp);
                return Err(error);
            }
            drop(file);
            match fs::rename(&tmp, &path) {
                Ok(()) => secret,
                Err(_) => {
                    let _ = fs::remove_file(&tmp);
                    read_cursor_secret_file(&path)?
                }
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            let _ = fs::remove_file(&tmp);
            read_cursor_secret_file(&path)?
        }
        Err(error) => {
            let _ = fs::remove_file(&tmp);
            return Err(ImportError::new(
                ErrorCode::SnapshotUnavailable,
                format!("open cursor secret: {error}"),
            ));
        }
    };
    let mut cache = SECRET_CACHE.lock().map_err(|_| {
        ImportError::new(
            ErrorCode::SnapshotUnavailable,
            "cursor secret lock poisoned",
        )
    })?;
    *cache = Some((path, secret));
    Ok(secret)
}

fn sign_payload(payload: &[u8]) -> Result<[u8; 32], ImportError> {
    let secret = load_or_create_cursor_secret()?;
    let mut mac = HmacSha256::new_from_slice(&secret).map_err(|_| {
        ImportError::new(ErrorCode::SnapshotUnavailable, "cursor HMAC key rejected")
    })?;
    mac.update(payload);
    let bytes = mac.finalize().into_bytes();
    let mut out = [0u8; 32];
    out.copy_from_slice(&bytes);
    Ok(out)
}

fn verify_payload_mac(payload: &[u8], presented: &[u8]) -> Result<(), ImportError> {
    let secret = load_or_create_cursor_secret()?;
    let mut mac = HmacSha256::new_from_slice(&secret).map_err(|_| stale())?;
    mac.update(payload);
    mac.verify_slice(presented).map_err(|_| stale())
}

/// Sign installation-local durable metadata without reusing the cursor message
/// domain. The installation secret never leaves this module or the imports
/// directory.
pub(crate) fn sign_installation_payload(
    domain: &str,
    payload: &[u8],
) -> Result<[u8; 32], ImportError> {
    Ok(InstallationAuthenticator::load()?.sign(domain, payload))
}

/// Constant-time verification for installation-local durable metadata.
pub(crate) fn verify_installation_payload(
    domain: &str,
    payload: &[u8],
    presented: &[u8],
) -> Result<bool, ImportError> {
    Ok(InstallationAuthenticator::load()?.verify(domain, payload, presented))
}

/// Opaque crypto material only: no SQL acceptance facts or reusable writer
/// grant. Load before a catalog writer; sign/verify then perform no filesystem
/// I/O. Deliberately not Debug, Clone or serializable. Callers must still check
/// their current installation/job/input bindings in their own transaction.
pub(crate) struct InstallationAuthenticator {
    mac: HmacSha256,
    codec_context: [u8; 32],
}

impl InstallationAuthenticator {
    pub(crate) fn load() -> Result<Self, ImportError> {
        let secret = load_or_create_cursor_secret()?;
        let mac = HmacSha256::new_from_slice(&secret).map_err(|_| {
            ImportError::new(
                ErrorCode::SnapshotUnavailable,
                "installation HMAC key rejected",
            )
        })?;
        let mut authenticator = Self {
            mac,
            codec_context: [0; 32],
        };
        authenticator.codec_context = authenticator.sign("session-import-codec-context-v1", b"");
        Ok(authenticator)
    }

    /// Pure codec memoization must distinguish keys without retaining/exporting
    /// the key itself. This domain-separated tag grants no SQL permissions.
    pub(crate) fn codec_context(&self) -> [u8; 32] {
        self.codec_context
    }

    fn message(&self, domain: &str, payload: &[u8]) -> HmacSha256 {
        let mut mac = self.mac.clone();
        mac.update(INSTALLATION_MAC_PREFIX);
        mac.update(domain.as_bytes());
        mac.update(&[0]);
        mac.update(payload);
        mac
    }

    pub(crate) fn sign(&self, domain: &str, payload: &[u8]) -> [u8; 32] {
        let bytes = self.message(domain, payload).finalize().into_bytes();
        let mut out = [0u8; 32];
        out.copy_from_slice(&bytes);
        out
    }

    pub(crate) fn verify(&self, domain: &str, payload: &[u8], presented: &[u8]) -> bool {
        self.message(domain, payload)
            .verify_slice(presented)
            .is_ok()
    }
}

#[cfg(test)]
#[path = "installation_authenticator_tests.rs"]
mod installation_authenticator_tests;

pub fn encode_bound_cursor(
    scope: CursorScope<'_>,
    offset: u32,
    mark: Option<String>,
) -> Result<String, ImportError> {
    encode_bound_cursor_at(
        scope,
        offset,
        mark,
        chrono::Utc::now().timestamp() + CURSOR_TTL_SECS,
    )
}

pub(crate) fn encode_bound_cursor_at(
    scope: CursorScope<'_>,
    offset: u32,
    mark: Option<String>,
    exp: i64,
) -> Result<String, ImportError> {
    let cursor = BoundCursor {
        v: CURSOR_VERSION,
        owner: scope.owner.to_string(),
        snapshot: scope.snapshot.to_string(),
        leaf: scope.leaf.to_string(),
        filter: scope.filter.to_string(),
        offset,
        exp,
        mark,
    };
    let payload = canonical_cursor_payload(&cursor);
    let mac = sign_payload(payload.as_bytes())?;
    Ok(format!(
        "{ENVELOPE_PREFIX}.{}.{}",
        URL_SAFE_NO_PAD.encode(payload.as_bytes()),
        URL_SAFE_NO_PAD.encode(mac)
    ))
}

fn reject_unauthenticated(raw: &str) -> Result<(), ImportError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(stale());
    }
    if trimmed.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(ImportError::new(
            ErrorCode::PlanStale,
            "bare numeric cursors are not accepted",
        ));
    }
    if serde_json::from_str::<u64>(trimmed).is_ok() {
        return Err(ImportError::new(
            ErrorCode::PlanStale,
            "bare numeric cursors are not accepted",
        ));
    }
    let first = trimmed.as_bytes()[0];
    if first == b'{' || first == b'[' || first == b'"' {
        return Err(ImportError::new(
            ErrorCode::PlanStale,
            "unauthenticated cursor JSON is not accepted",
        ));
    }
    Ok(())
}

pub fn decode_bound_cursor(raw: &str, scope: CursorScope<'_>) -> Result<BoundCursor, ImportError> {
    let trimmed = raw.trim();
    reject_unauthenticated(trimmed)?;
    let mut parts = trimmed.split('.');
    let prefix = parts.next().ok_or_else(stale)?;
    let payload_b64 = parts.next().ok_or_else(stale)?;
    let mac_b64 = parts.next().ok_or_else(stale)?;
    if parts.next().is_some() || prefix != ENVELOPE_PREFIX {
        return Err(stale());
    }
    let payload = URL_SAFE_NO_PAD.decode(payload_b64).map_err(|_| stale())?;
    let mac = URL_SAFE_NO_PAD.decode(mac_b64).map_err(|_| stale())?;
    verify_payload_mac(&payload, &mac)?;
    let cursor: BoundCursor = serde_json::from_slice(&payload).map_err(|_| stale())?;
    if canonical_cursor_payload(&cursor).as_bytes() != payload.as_slice() {
        return Err(stale());
    }
    if cursor.v != CURSOR_VERSION {
        return Err(stale());
    }
    if cursor.owner != scope.owner
        || cursor.snapshot != scope.snapshot
        || cursor.leaf != scope.leaf
        || cursor.filter != scope.filter
    {
        return Err(ImportError::new(
            ErrorCode::PlanStale,
            "cursor belongs to another owner, snapshot, leaf, or filter",
        ));
    }
    if cursor.exp < chrono::Utc::now().timestamp() {
        return Err(ImportError::new(
            ErrorCode::PlanStale,
            "page cursor has expired",
        ));
    }
    Ok(cursor)
}

pub fn decode_optional_offset(
    raw: Option<&str>,
    scope: CursorScope<'_>,
) -> Result<u32, ImportError> {
    match raw.filter(|value| !value.is_empty()) {
        None => Ok(0),
        Some(value) => Ok(decode_bound_cursor(value, scope)?.offset),
    }
}

#[cfg(test)]
pub(crate) fn reset_cursor_secret_cache() {
    if let Ok(mut cache) = SECRET_CACHE.lock() {
        *cache = None;
    }
}

#[cfg(test)]
pub(crate) fn tamper_cursor_field(raw: &str, field: &str, value: serde_json::Value) -> String {
    let mut parts = raw.split('.');
    let prefix = parts.next().expect("envelope prefix");
    let payload_b64 = parts.next().expect("envelope payload");
    let mac_b64 = parts.next().expect("envelope mac");
    assert!(parts.next().is_none());
    let payload = URL_SAFE_NO_PAD.decode(payload_b64).unwrap();
    let mut obj: serde_json::Value = serde_json::from_slice(&payload).unwrap();
    obj[field] = value;
    let new_payload = super::hash::canonical_json(&obj);
    format!(
        "{prefix}.{}.{mac_b64}",
        URL_SAFE_NO_PAD.encode(new_payload.as_bytes())
    )
}

#[cfg(test)]
pub(crate) fn tamper_cursor_signature(raw: &str) -> String {
    let mut parts = raw.split('.');
    let prefix = parts.next().unwrap();
    let payload = parts.next().unwrap();
    let mac_b64 = parts.next().unwrap();
    let mut mac = URL_SAFE_NO_PAD.decode(mac_b64).unwrap();
    mac[0] ^= 0xff;
    format!("{prefix}.{payload}.{}", URL_SAFE_NO_PAD.encode(mac))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scope() -> CursorScope<'static> {
        CursorScope {
            owner: "imp:owner",
            snapshot: "rev-a",
            leaf: "leaf-a",
            filter: "events",
        }
    }

    #[test]
    fn round_trip_and_reject_bare_tamper_cross_owner_and_stale() {
        let _home = super::super::test_home::IsolatedAppHome::acquire("cur-roundtrip");
        let encoded = encode_bound_cursor(scope(), 20, None).unwrap();
        let decoded = decode_bound_cursor(&encoded, scope()).unwrap();
        assert_eq!(decoded.offset, 20);
        assert!(
            encoded.starts_with("impcur1."),
            "cursor must be an authenticated envelope, got {encoded}"
        );
        assert!(decode_bound_cursor("20", scope()).is_err());
        assert!(decode_bound_cursor("0", scope()).is_err());
        assert!(decode_optional_offset(Some("99"), scope()).is_err());
        assert!(decode_bound_cursor(&encoded[..10], scope()).is_err());
        let other = CursorScope {
            owner: "imp:other",
            ..scope()
        };
        assert!(decode_bound_cursor(&encoded, other).is_err());
        let stale_raw = encode_bound_cursor_at(scope(), 20, None, 1).unwrap();
        let err = decode_bound_cursor(&stale_raw, scope()).unwrap_err();
        assert_eq!(err.code, ErrorCode::PlanStale);
        assert!(decode_bound_cursor(&serde_json::to_string(&decoded).unwrap(), scope()).is_err());
        assert!(decode_optional_offset(None, scope()).unwrap() == 0);
    }

    #[test]
    fn field_and_envelope_tamper_is_rejected_before_offset() {
        // Signing and verification must use the same installation even while
        // other tests change the process-wide GROK_APP_HOME under its lock.
        let _home = super::super::test_home::IsolatedAppHome::acquire("cur-tamper");
        let encoded = encode_bound_cursor(scope(), 20, Some("mark-a".into())).unwrap();
        assert_eq!(decode_bound_cursor(&encoded, scope()).unwrap().offset, 20);

        let cases = [
            ("v", serde_json::json!(2)),
            ("owner", serde_json::json!("imp:other")),
            ("snapshot", serde_json::json!("rev-b")),
            ("leaf", serde_json::json!("leaf-b")),
            ("filter", serde_json::json!("other")),
            ("offset", serde_json::json!(99)),
            ("mark", serde_json::json!("mark-b")),
            (
                "exp",
                serde_json::json!(chrono::Utc::now().timestamp() + 99_999),
            ),
        ];
        for (field, value) in cases {
            let tampered = tamper_cursor_field(&encoded, field, value);
            let err = decode_bound_cursor(&tampered, scope()).unwrap_err();
            assert_eq!(
                err.code,
                ErrorCode::PlanStale,
                "field {field} must fail closed"
            );
        }
        assert!(decode_bound_cursor(&tamper_cursor_signature(&encoded), scope()).is_err());

        assert!(decode_bound_cursor("[1,2,3]", scope()).is_err());
        assert!(decode_bound_cursor(&encoded[..encoded.len() / 2], scope()).is_err());
        assert!(decode_bound_cursor("not-a-cursor-at-all", scope()).is_err());
        assert!(decode_bound_cursor(
            &[0u8; 32]
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>(),
            scope()
        )
        .is_err());

        let cross_rev = CursorScope {
            snapshot: "rev-b",
            ..scope()
        };
        assert!(decode_bound_cursor(&encoded, cross_rev).is_err());
        let cross_leaf = CursorScope {
            leaf: "leaf-b",
            ..scope()
        };
        assert!(decode_bound_cursor(&encoded, cross_leaf).is_err());
        let cross_filter = CursorScope {
            filter: "search",
            ..scope()
        };
        assert!(decode_bound_cursor(&encoded, cross_filter).is_err());
        let replay = encode_bound_cursor_at(scope(), 20, None, 1).unwrap();
        assert!(decode_bound_cursor(&replay, scope()).is_err());
    }

    fn production_src(src: &str) -> String {
        let lines: Vec<&str> = src.lines().collect();
        let mut out = String::new();
        let mut i = 0usize;
        while i < lines.len() {
            if lines[i].trim_start().starts_with("#[cfg(test)]") {
                i += 1;
                while i < lines.len() && lines[i].trim_start().starts_with("#[") {
                    i += 1;
                }
                if i >= lines.len() {
                    break;
                }
                let item = lines[i].trim_start();
                if item.starts_with("use ") || (item.starts_with("mod ") && item.ends_with(';')) {
                    i += 1;
                    continue;
                }
                let mut depth = 0i32;
                let mut started = false;
                while i < lines.len() {
                    for ch in lines[i].chars() {
                        if ch == '{' {
                            depth += 1;
                            started = true;
                        }
                        if ch == '}' {
                            depth -= 1;
                        }
                    }
                    i += 1;
                    if started && depth <= 0 {
                        break;
                    }
                }
                continue;
            }
            out.push_str(lines[i]);
            out.push('\n');
            i += 1;
        }
        out
    }

    #[test]
    fn production_cursors_use_bound_encoders() {
        let files = [
            ("scan.rs", include_str!("scan.rs")),
            ("engine.rs", include_str!("engine.rs")),
            ("search.rs", include_str!("search.rs")),
            ("catalog.rs", include_str!("catalog.rs")),
            ("mcp_stdio.rs", include_str!("mcp_stdio.rs")),
            (
                "commands/session_migration.rs",
                include_str!("../commands/session_migration.rs"),
            ),
        ];
        let allowed = [
            "encode_bound_cursor",
            "encode_page_cursor",
            "encode_search_cursor",
            "encode_read_cursor",
            "encode_preview_cursor",
        ];
        for (name, src) in files {
            let prod = production_src(src);
            assert!(
                !prod.contains("serde_json::to_string(&("),
                "{name} encodes a tuple as a cursor string"
            );
            assert!(
                !prod.contains("(String, i64, String)"),
                "{name} still uses a search tuple cursor type"
            );
            assert!(
                !prod.contains("(scope, ordinal, id)"),
                "{name} still mentions the old search tuple"
            );
            for line in prod.lines() {
                let trimmed = line.trim();
                if !trimmed.contains("next") || !trimmed.contains("cursor") {
                    continue;
                }
                if trimmed.starts_with("//") {
                    continue;
                }
                let uses_allowed = allowed.iter().any(|enc| trimmed.contains(enc));
                let assigns_option = trimmed.contains("Some(") && trimmed.contains("encode_");
                if trimmed.contains("to_string(&(") {
                    panic!("{name}: production next cursor uses a JSON tuple: {trimmed}");
                }
                let _ = (uses_allowed, assigns_option);
            }
        }
    }

    #[test]
    fn encode_page_cursor_does_not_abort_when_secret_is_a_directory() {
        let home = crate::session_migration::test_home::IsolatedAppHome::acquire("cur-dir");
        let secret = home.path.join("imports").join(".cursor-hmac-secret");
        let _ = std::fs::remove_file(&secret);
        std::fs::create_dir_all(&secret).unwrap();
        let rev = home.path.join("rev");
        std::fs::create_dir_all(&rev).unwrap();
        let result = crate::session_migration::search::encode_page_cursor(&rev, 0);
        assert!(
            result.is_err(),
            "encode_page_cursor must return a stable error when the HMAC secret is not a file, got {result:?}"
        );
    }

    #[test]
    fn encode_page_cursor_does_not_abort_when_secret_has_the_wrong_length() {
        let home = crate::session_migration::test_home::IsolatedAppHome::acquire("cur-len");
        let secret = home.path.join("imports").join(".cursor-hmac-secret");
        std::fs::create_dir_all(secret.parent().unwrap()).unwrap();
        std::fs::write(&secret, [7u8; 4]).unwrap();
        let rev = home.path.join("rev");
        std::fs::create_dir_all(&rev).unwrap();
        let result = crate::session_migration::search::encode_page_cursor(&rev, 0);
        assert!(
            result.is_err(),
            "encode_page_cursor must return a stable error when the HMAC secret length is wrong, got {result:?}"
        );
    }

    #[test]
    fn cursor_secret_is_not_visible_until_complete_length() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;
        let home = crate::session_migration::test_home::IsolatedAppHome::acquire("cur-atomic");
        reset_cursor_secret_cache();
        let secret = home.path.join("imports").join(".cursor-hmac-secret");
        let _ = std::fs::remove_file(&secret);
        std::fs::create_dir_all(secret.parent().unwrap()).unwrap();
        std::env::set_var("GROK_TEST_SECRET_CREATE_PAUSE_MS", "80");
        let seen = Arc::new(AtomicUsize::new(usize::MAX));
        let seen_t = seen.clone();
        let path = secret.clone();
        let watcher = std::thread::spawn(move || {
            let start = std::time::Instant::now();
            while start.elapsed() < std::time::Duration::from_millis(500) {
                if let Ok(bytes) = std::fs::read(&path) {
                    seen_t.store(bytes.len(), Ordering::SeqCst);
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
        });
        let encoded = encode_bound_cursor(scope(), 1, None);
        watcher.join().unwrap();
        std::env::remove_var("GROK_TEST_SECRET_CREATE_PAUSE_MS");
        assert!(
            encoded.is_ok(),
            "create must still succeed, got {encoded:?}"
        );
        let first = seen.load(Ordering::SeqCst);
        assert_eq!(
            first, SECRET_BYTES,
            "target secret name must not be visible until the full {SECRET_BYTES} bytes are in place, first observed {first}"
        );
    }

    #[test]
    fn encode_bound_cursor_rejects_directory_and_wrong_length_secrets() {
        let home = crate::session_migration::test_home::IsolatedAppHome::acquire("cur-bound");
        let secret = home.path.join("imports").join(".cursor-hmac-secret");
        let _ = std::fs::remove_file(&secret);
        std::fs::create_dir_all(&secret).unwrap();
        assert!(encode_bound_cursor(scope(), 1, None).is_err());
        std::fs::remove_dir_all(&secret).unwrap();
        std::fs::write(&secret, [1u8; 8]).unwrap();
        assert!(encode_bound_cursor(scope(), 1, None).is_err());
    }
}
