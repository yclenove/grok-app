//! Sole reader of `GCU_LOG_PAYLOADS`.
//!
//! Unset, empty, and every value other than the exact string `1` are
//! metadata-only: input text, screenshots, trees, error originals, and tokens
//! are not recorded. `1` may record a payload only after [`crate::inject::scrub`],
//! which still removes registered secrets, bearer tokens, `GCU_TOKEN`
//! assignments, and password/token field values. The flag never disables that
//! pipeline, and it never authorizes raw image bytes.

use std::time::Duration;

/// How long metadata summaries and CU log files may remain.
pub const RETENTION: Duration = Duration::from_secs(7 * 24 * 60 * 60);
/// Largest single CU log file (the `5` in 5×5MB).
pub const LOG_FILE_MAX_BYTES: u64 = 5 * 1024 * 1024;
/// How many files one CU log stream may keep (the other `5`).
pub const LOG_FILE_MAX_COUNT: usize = 5;

/// Whether `value` enables payload recording. Only the exact string `1` does.
pub fn payloads_enabled_value(value: Option<&str>) -> bool {
    value == Some("1")
}

/// The only read of the `GCU_LOG_PAYLOADS` environment variable.
pub fn payloads_enabled() -> bool {
    payloads_enabled_value(std::env::var("GCU_LOG_PAYLOADS").ok().as_deref())
}

/// Metadata line, or a scrubbed payload when `enabled` is true.
///
/// Screenshots and other raw buffers must use [`format_bytes`]; this function
/// is for text. Redaction runs whenever the body is included.
pub fn format_text(enabled: bool, kind: &str, body: &str) -> String {
    if !enabled {
        return format!("{kind} bytes={}", body.len());
    }
    format!("{kind} {}", crate::inject::scrub(body))
}

/// Log text using the process flag. Redaction still runs when the flag is on.
pub fn log_text(kind: &str, body: &str) -> String {
    format_text(payloads_enabled(), kind, body)
}

/// Image and buffer metadata. Raw bytes are never formatted, even when payloads are enabled.
pub fn format_bytes(kind: &str, len: usize) -> String {
    format!("{kind} bytes={len}")
}

/// Panic and error record: location and category only, never a message body.
pub fn error_record(location: &str, category: &str) -> String {
    format!(
        "category={} location={}",
        category_token(category),
        category_token(location)
    )
}

fn category_token(text: &str) -> String {
    let mut out = String::new();
    for ch in text.chars().take(160) {
        if ch.is_control() {
            continue;
        }
        out.push(ch);
    }
    if out.is_empty() {
        "unknown".into()
    } else {
        out
    }
}
