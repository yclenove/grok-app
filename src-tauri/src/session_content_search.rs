//! Search App journal message bodies (messages.json) for sidebar content hits.
//!
//! Pure match/snippet helpers are unit-tested; `search_sessions` scans on-disk
//! journals with hard caps so the palette stays responsive.

use std::collections::HashSet;
use std::fs;
use std::path::Path;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::paths::session_dir;
use crate::store::{self, ChatMessageStored, SessionMeta};

/// Max sessions to open on disk (most recently updated first).
const MAX_SESSIONS_SCAN: usize = 200;
/// Skip journals larger than this (bytes).
const MAX_MESSAGES_FILE_BYTES: u64 = 2 * 1024 * 1024;
/// Snippet half-window around the first match (chars).
const SNIPPET_RADIUS: usize = 48;
/// Max snippet length returned to the UI.
const SNIPPET_MAX: usize = 120;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionContentHit {
    pub id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    /// First matching excerpt with optional ellipsis.
    pub snippet: String,
    /// How many user/assistant messages contain the query.
    pub match_count: u32,
    pub updated_at: DateTime<Utc>,
    #[serde(default)]
    pub archived: bool,
    /// `session` (native journal) or `importedHistory` (catalog, `imp:` id).
    #[serde(default = "default_hit_kind")]
    pub kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionContentSearchResponse {
    pub hits: Vec<SessionContentHit>,
    pub imported_history_error: Option<crate::session_migration::errors::ErrorCode>,
}

fn default_hit_kind() -> String {
    "session".into()
}

/// Count matching user/assistant messages and build a snippet from the first hit.
/// Case-insensitive substring. Empty query → `None`.
pub fn match_messages<'a, I>(query: &str, messages: I) -> Option<(u32, String)>
where
    I: IntoIterator<Item = (&'a str, &'a str)>,
{
    let q = query.trim();
    if q.is_empty() {
        return None;
    }
    let q_lower = q.to_lowercase();
    let mut match_count = 0u32;
    let mut first_snippet: Option<String> = None;

    for (role, content) in messages {
        if role != "user" && role != "assistant" {
            continue;
        }
        if content.is_empty() {
            continue;
        }
        let lower = content.to_lowercase();
        if let Some(byte_idx) = lower.find(&q_lower) {
            match_count = match_count.saturating_add(1);
            if first_snippet.is_none() {
                first_snippet = Some(make_snippet(content, byte_idx, q.len()));
            }
        }
    }

    if match_count == 0 {
        None
    } else {
        Some((match_count, first_snippet.unwrap_or_default()))
    }
}

/// Build a single-line snippet around a UTF-8 byte offset into `content`.
pub fn make_snippet(content: &str, match_byte_idx: usize, match_len: usize) -> String {
    // Clamp to char boundaries.
    let start_byte = floor_char_boundary(content, match_byte_idx);
    let end_byte = ceil_char_boundary(
        content,
        match_byte_idx.saturating_add(match_len).min(content.len()),
    );

    let prefix = &content[..start_byte];
    let matched = &content[start_byte..end_byte];
    let suffix = &content[end_byte..];

    let prefix_chars: Vec<char> = prefix.chars().collect();
    let suffix_chars: Vec<char> = suffix.chars().collect();
    let matched_chars: Vec<char> = matched.chars().collect();

    let take_pre = SNIPPET_RADIUS.min(prefix_chars.len());
    let pre_slice = &prefix_chars[prefix_chars.len().saturating_sub(take_pre)..];
    let lead_ellipsis = prefix_chars.len() > take_pre;

    let mut out = String::new();
    if lead_ellipsis {
        out.push('…');
    }
    out.extend(pre_slice.iter().copied());
    out.extend(matched_chars.iter().copied());

    let room = SNIPPET_MAX.saturating_sub(out.chars().count());
    let take_suf = room.min(suffix_chars.len()).min(SNIPPET_RADIUS + 16);
    out.extend(suffix_chars.iter().take(take_suf).copied());
    if suffix_chars.len() > take_suf {
        out.push('…');
    }

    // Collapse whitespace for a single-line palette row.
    let collapsed: String = out.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() > SNIPPET_MAX {
        let trimmed: String = collapsed
            .chars()
            .take(SNIPPET_MAX.saturating_sub(1))
            .collect();
        format!("{trimmed}…")
    } else {
        collapsed
    }
}

fn floor_char_boundary(s: &str, i: usize) -> usize {
    if i >= s.len() {
        return s.len();
    }
    let mut i = i;
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

fn ceil_char_boundary(s: &str, i: usize) -> usize {
    if i >= s.len() {
        return s.len();
    }
    let mut i = i;
    while i < s.len() && !s.is_char_boundary(i) {
        i += 1;
    }
    i
}

/// Scan App journal sessions for `query`. Caps work; skips huge files.
pub fn search_sessions(query: &str, limit: usize) -> SessionContentSearchResponse {
    let q = query.trim();
    if q.is_empty() {
        return SessionContentSearchResponse {
            hits: Vec::new(),
            imported_history_error: None,
        };
    }
    let limit = limit.clamp(1, 50);
    let index = store::load_sessions_index();
    let mut native_hits = Vec::new();

    for meta in index.into_iter().take(MAX_SESSIONS_SCAN) {
        // Match sidebar default: hide archived from content hits.
        if meta.archived {
            continue;
        }
        if let Some(hit) = scan_session(&meta, q) {
            native_hits.push(hit);
            if native_hits.len() >= limit {
                break;
            }
        }
    }
    // Both stores are first-class search sources. Query them independently so
    // a full native page cannot hide imported matches or imported-store errors.
    let (imported_hits, imported_history_error) = search_imported_history(q, limit);
    let hits = merge_content_sources(native_hits, imported_hits, limit);
    SessionContentSearchResponse {
        hits,
        imported_history_error,
    }
}

fn merge_content_sources(
    native_hits: Vec<SessionContentHit>,
    imported_hits: Vec<SessionContentHit>,
    limit: usize,
) -> Vec<SessionContentHit> {
    let cap = limit.clamp(1, 50);
    let mut merged = Vec::with_capacity(cap);
    let mut seen = HashSet::with_capacity(cap);
    let mut native = native_hits.into_iter();
    let mut imported = imported_hits.into_iter();
    let mut native_done = false;
    let mut imported_done = false;

    while merged.len() < cap && !(native_done && imported_done) {
        if !native_done {
            match native.next() {
                Some(hit) => {
                    if seen.insert(hit.id.clone()) {
                        merged.push(hit);
                    }
                }
                None => native_done = true,
            }
        }
        if merged.len() >= cap {
            break;
        }
        if !imported_done {
            match imported.next() {
                Some(hit) => {
                    if seen.insert(hit.id.clone()) {
                        merged.push(hit);
                    }
                }
                None => imported_done = true,
            }
        }
    }
    merged
}

fn search_imported_history(
    query: &str,
    limit: usize,
) -> (
    Vec<SessionContentHit>,
    Option<crate::session_migration::errors::ErrorCode>,
) {
    let catalog = crate::session_migration::catalog::catalog_path();
    match std::fs::metadata(&catalog) {
        Ok(metadata) if metadata.is_file() => {}
        Ok(_) => {
            return (
                Vec::new(),
                Some(crate::session_migration::errors::ErrorCode::HistoryAccessUnavailable),
            )
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return (Vec::new(), None),
        Err(_) => {
            return (
                Vec::new(),
                Some(crate::session_migration::errors::ErrorCode::HistoryAccessUnavailable),
            )
        }
    }
    let conn = match crate::session_migration::catalog::open_catalog_readonly() {
        Ok(conn) => conn,
        Err(error) => return (Vec::new(), Some(error.code)),
    };
    search_imported_history_with_conn(&conn, query, limit)
}

fn imported_updated_at(entry: &crate::session_migration::catalog::CatalogEntry) -> DateTime<Utc> {
    entry
        .created_at
        .parse::<DateTime<Utc>>()
        .unwrap_or_default()
}

fn search_imported_history_with_conn(
    conn: &rusqlite::Connection,
    query: &str,
    limit: usize,
) -> (
    Vec<SessionContentHit>,
    Option<crate::session_migration::errors::ErrorCode>,
) {
    let cap = limit.clamp(1, 50);
    let mut hits = Vec::new();
    let mut error = None;
    match crate::session_migration::catalog::search_imported_titles(conn, query, cap) {
        Ok(title_hits) => {
            for entry in title_hits {
                if hits.len() >= cap {
                    break;
                }
                if !crate::session_migration::ids::is_import_id(&entry.import_id) {
                    continue;
                }
                let title = entry
                    .alias
                    .as_deref()
                    .filter(|s| !s.is_empty())
                    .unwrap_or(entry.title.as_str());
                hits.push(SessionContentHit {
                    updated_at: imported_updated_at(&entry),
                    id: entry.import_id,
                    title: title.to_string(),
                    project_id: None,
                    snippet: title.to_string(),
                    match_count: 1,
                    archived: false,
                    kind: "importedHistory".into(),
                });
            }
        }
        Err(query_error) => error = Some(query_error.code),
    }
    if hits.len() >= cap {
        return (hits, error);
    }
    let body_hits =
        match crate::session_migration::catalog::search_imported_bodies(conn, query, cap) {
            Ok(body_hits) => body_hits,
            Err(query_error) => {
                if error.is_none() {
                    error = Some(query_error.code);
                }
                return (hits, error);
            }
        };
    for (entry, text) in body_hits {
        if hits.len() >= cap {
            break;
        }
        if hits.iter().any(|h| h.id == entry.import_id) {
            continue;
        }
        if !crate::session_migration::ids::is_import_id(&entry.import_id) {
            continue;
        }
        let title = entry
            .alias
            .as_deref()
            .filter(|s| !s.is_empty())
            .unwrap_or(entry.title.as_str());
        let q = query.trim();
        let snippet = text
            .to_lowercase()
            .find(&q.to_lowercase())
            .map(|idx| make_snippet(&text, idx, q.len()))
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| title.to_string());
        hits.push(SessionContentHit {
            updated_at: imported_updated_at(&entry),
            id: entry.import_id,
            title: title.to_string(),
            project_id: None,
            snippet,
            match_count: 1,
            archived: false,
            kind: "importedHistory".into(),
        });
    }
    (hits, error)
}

fn scan_session(meta: &SessionMeta, query: &str) -> Option<SessionContentHit> {
    let path = session_dir(&meta.id).join("messages.json");
    if !path.is_file() {
        return None;
    }
    if file_too_large(&path) {
        return None;
    }
    let messages: Vec<ChatMessageStored> = match fs::read_to_string(&path) {
        Ok(s) if s.trim().is_empty() => return None,
        Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
        Err(_) => return None,
    };
    let iter = messages
        .iter()
        .map(|m| (m.role.as_str(), m.content.as_str()));
    let (match_count, snippet) = match_messages(query, iter)?;
    Some(SessionContentHit {
        id: meta.id.clone(),
        title: meta.title.clone(),
        project_id: meta.project_id.clone(),
        snippet,
        match_count,
        updated_at: meta.updated_at,
        archived: meta.archived,
        kind: default_hit_kind(),
    })
}

fn file_too_large(path: &Path) -> bool {
    match fs::metadata(path) {
        Ok(m) => m.len() > MAX_MESSAGES_FILE_BYTES,
        Err(_) => true,
    }
}

#[cfg(test)]
#[path = "session_content_search/tests.rs"]
mod tests;
