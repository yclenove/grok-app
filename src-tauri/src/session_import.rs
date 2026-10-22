//! Import conversation transcripts into a new App session journal.
//!
//! Parsing is shared with the session-migration generic adapter so fenced
//! `User:` lines and illegal JSON objects behave the same.

use chrono::Utc;
use uuid::Uuid;

use crate::store::{self, ChatMessageStored, SessionMeta};

/// Parse free-form transcript text into ordered (role, content) pairs.
pub fn parse_transcript(raw: &str) -> Result<Vec<(String, String)>, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("empty transcript".into());
    }
    crate::session_migration::adapters::generic::parse_legacy_pairs(raw)
}

/// Create a new App session and write imported messages to its journal.
pub fn import_transcript_as_session(
    raw: &str,
    title: Option<String>,
    project_id: Option<String>,
) -> Result<SessionMeta, String> {
    let pairs = parse_transcript(raw)?;
    let title = title
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| {
            pairs.iter().find(|(r, _)| r == "user").map(|(_, c)| {
                let line = c.lines().next().unwrap_or(c).trim();
                let mut t: String = line.chars().take(40).collect();
                if line.chars().count() > 40 {
                    t.push('…');
                }
                t
            })
        })
        .unwrap_or_else(|| "Imported chat".into());

    let mut meta = store::create_session(project_id, Some(title), false)?;
    let now = Utc::now();
    let msgs: Vec<ChatMessageStored> = pairs
        .into_iter()
        .map(|(role, content)| ChatMessageStored {
            id: Uuid::new_v4().to_string(),
            role,
            content,
            thought: None,
            created_at: now,
            is_error: false,
            attachments: None,
            marker: None,
        })
        .collect();
    store::save_messages(&meta.id, &msgs)?;
    meta.updated_at = now;
    let _ = store::update_session_meta(&meta);
    Ok(meta)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_markdown_roles() {
        let raw = r#"
## User
hello world
## Assistant
hi there
## User
second
## Assistant
ok
"#;
        let pairs = parse_transcript(raw).unwrap();
        assert_eq!(pairs.len(), 4);
        assert_eq!(pairs[0].0, "user");
        assert_eq!(pairs[0].1, "hello world");
        assert_eq!(pairs[1].0, "assistant");
    }

    #[test]
    fn parses_json_array() {
        let raw = r#"[
          {"role":"user","content":"a"},
          {"role":"assistant","content":"b"}
        ]"#;
        let pairs = parse_transcript(raw).unwrap();
        assert_eq!(pairs.len(), 2);
        assert_eq!(pairs[1].1, "b");
    }

    #[test]
    fn parses_user_colon_style() {
        let raw = "User: ping\nAssistant: pong\n";
        let pairs = parse_transcript(raw).unwrap();
        assert_eq!(pairs.len(), 2);
    }

    #[test]
    fn fenced_user_is_not_a_role() {
        let raw = "## User\nhello\n```\nUser: not a turn\n```\n## Assistant\nok\n";
        let pairs = parse_transcript(raw).unwrap();
        assert_eq!(pairs.len(), 2);
        assert!(pairs[0].1.contains("User: not a turn"));
        assert_eq!(pairs[1].1.trim(), "ok");
    }

    #[test]
    fn illegal_json_object_does_not_become_markdown() {
        let raw = r#"{"foo":"User: hello"}"#;
        assert!(parse_transcript(raw).is_err());
    }
}
