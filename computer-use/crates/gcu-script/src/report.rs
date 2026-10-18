//! Map engine and host failures onto script status bytes.

use crate::{limits::OUTPUT_MAX_BYTES, ScriptStatus};

pub(crate) fn cap_output(text: &str) -> String {
    let mut end = text.len().min(OUTPUT_MAX_BYTES);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_owned()
}

pub(crate) fn classify_fail(fail: &str) -> (ScriptStatus, String, String) {
    let (code, message) = fail.split_once(':').unwrap_or(("internal", fail));
    let status = if matches!(
        code,
        "stopped_by_user"
            | "approval_denied"
            | "approval_timeout"
            | "app_denied"
            | "stale_element"
            | "stale_geometry"
            | "stale_screenshot"
            | "timeout"
            | "window_gone"
            | "window_not_found"
            | "window_minimized"
    ) {
        ScriptStatus::Stopped
    } else if code == "capability_unavailable"
        || message.to_ascii_lowercase().contains("stack")
        || message.to_ascii_lowercase().contains("memory")
    {
        ScriptStatus::Limit
    } else {
        ScriptStatus::Error
    };
    (status, code.to_owned(), message.to_owned())
}

pub(crate) fn classify_engine(error: &str) -> (ScriptStatus, String, String) {
    let lower = error.to_ascii_lowercase();
    if lower.contains("interrupted") {
        (
            ScriptStatus::Timeout,
            "timeout".to_owned(),
            "interrupted".to_owned(),
        )
    } else if lower.contains("out of memory") || lower.contains("memory") || lower.contains("stack")
    {
        (
            ScriptStatus::Limit,
            "capability_unavailable".to_owned(),
            "engine limit".to_owned(),
        )
    } else {
        (
            ScriptStatus::Error,
            "internal".to_owned(),
            engine_detail(error),
        )
    }
}

fn engine_detail(error: &str) -> String {
    let extra: String = error
        .chars()
        .filter(|ch| !ch.is_control())
        .take(160)
        .collect();
    if extra.is_empty() {
        "script failed".to_owned()
    } else {
        format!("script failed: {extra}")
    }
}

/// Finish-frame discriminant. `0` done, `1` limit, `2` timeout, `3` stopped.
pub(crate) fn status_byte(status: ScriptStatus) -> u8 {
    match status {
        ScriptStatus::Done => 0,
        ScriptStatus::Limit => 1,
        ScriptStatus::Timeout => 2,
        ScriptStatus::Stopped => 3,
        ScriptStatus::Error => 4,
    }
}

/// Map a finish-frame status byte back to [`ScriptStatus`].
///
/// Unknown bytes are [`ScriptStatus::Error`]. Blocking: none. Timeout: none.
/// Cancellation: none.
pub fn status_from_byte(byte: u8) -> ScriptStatus {
    match byte {
        0 => ScriptStatus::Done,
        1 => ScriptStatus::Limit,
        2 => ScriptStatus::Timeout,
        3 => ScriptStatus::Stopped,
        _ => ScriptStatus::Error,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_bytes_roundtrip_known_discriminants() {
        for status in [
            ScriptStatus::Done,
            ScriptStatus::Limit,
            ScriptStatus::Timeout,
            ScriptStatus::Stopped,
            ScriptStatus::Error,
        ] {
            assert_eq!(status_from_byte(status_byte(status)), status);
        }
        assert_eq!(status_from_byte(9), ScriptStatus::Error);
    }
}
