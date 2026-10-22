//! Cursor 3.x agent transcripts wrap each user turn as
//! `<timestamp>Thursday, Oct 22, 2026, 12:31 AM (UTC+8)</timestamp>\n<user_query>\n…\n</user_query>`.
//! Only that exact shape is unwrapped; any other wrapper keeps the full text.

use chrono::{DateTime, FixedOffset, NaiveDateTime};

pub(super) struct UserQuery {
    pub text: String,
    pub timestamp: Option<String>,
}

pub(super) fn unwrap_user_query(raw: &str) -> Option<UserQuery> {
    let mut rest = raw.trim();
    let mut stamp = None;
    if let Some(after) = rest.strip_prefix("<timestamp>") {
        let (inner, tail) = after.split_once("</timestamp>")?;
        stamp = Some(inner.trim());
        rest = tail.trim_start();
    }
    let body = rest
        .strip_prefix("<user_query>")?
        .strip_suffix("</user_query>")?;
    if body.contains("<user_query>") || body.contains("</user_query>") {
        return None;
    }
    let text = body.trim_matches(|c| c == '\n' || c == '\r');
    (!text.trim().is_empty()).then(|| UserQuery {
        text: text.into(),
        timestamp: stamp.and_then(source_time),
    })
}

/// `Thursday, Oct 22, 2026, 12:31 AM (UTC+8)` → `2026-10-22T00:31:00+08:00`.
/// The weekday must agree with the date; anything else stays unknown.
fn source_time(stamp: &str) -> Option<String> {
    let (local, zone) = stamp.rsplit_once(" (")?;
    let zone = zone.strip_suffix(')')?;
    let offset = parse_offset(zone)?;
    let naive = NaiveDateTime::parse_from_str(local, "%A, %b %d, %Y, %I:%M %p").ok()?;
    let at: DateTime<FixedOffset> = naive.and_local_timezone(offset).single()?;
    Some(at.format("%Y-%m-%dT%H:%M:%S%:z").to_string())
}

fn parse_offset(zone: &str) -> Option<FixedOffset> {
    let rest = zone.strip_prefix("UTC")?;
    if rest.is_empty() {
        return FixedOffset::east_opt(0);
    }
    let (sign, digits) = match rest.as_bytes().first()? {
        b'+' => (1, &rest[1..]),
        b'-' => (-1, &rest[1..]),
        _ => return None,
    };
    let (hours, minutes) = digits.split_once(':').unwrap_or((digits, "0"));
    let (hours, minutes): (i32, i32) = (hours.parse().ok()?, minutes.parse().ok()?);
    if !(0..=14).contains(&hours) || !(0..60).contains(&minutes) {
        return None;
    }
    FixedOffset::east_opt(sign * (hours * 3600 + minutes * 60))
}
