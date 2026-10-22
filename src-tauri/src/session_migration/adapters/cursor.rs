//! Cursor transcript, Composer/legacy Chat SQLite, and Store DB adapters.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use rusqlite::Connection;
use serde_json::Value;

#[path = "cursor_transcript_identity.rs"]
mod transcript_identity;
pub(crate) use native_identity::declared_family;
pub(crate) use transcript_identity::TranscriptIdentity;
#[path = "cursor_native_identity.rs"]
mod native_identity;
#[path = "cursor_transcript_wrappers.rs"]
mod transcript_wrappers;
pub(crate) use native_identity::{database_identity, store_identity_scoped};

use crate::session_migration::detect::current_platform;
use crate::session_migration::errors::{ErrorCode, ImportError};
#[cfg(test)]
use crate::session_migration::hash::sha256_bytes;
use crate::session_migration::jsonl::open_source_jsonl;
#[cfg(test)]
use crate::session_migration::sqlite_ro::{classify_query_err, table_names_at};
use crate::session_migration::sqlite_ro::{open_readonly, table_names, ReadonlySnapshot};
#[cfg(test)]
use crate::session_migration::types::SourceCandidate;
use crate::session_migration::types::{
    AdapterOutput, ArtifactEvidence, ContentBlock, EventBase, EventRelation, ImportIssue,
    ImportOutcome, ImportedEvent, Inventory, MessageRole, NormalizedGraph, Provider,
    RelationConfidence, RelationKind, SourceEvidence, SourceSpan, TimeOrigin, ToolInput,
    ToolOutcome, NORMALIZER_VERSION,
};

#[cfg(test)]
pub fn discover(root: &Path) -> Result<Vec<SourceCandidate>, ImportError> {
    let out = discover_scan(root);
    if out.candidates.is_empty() && !out.issues.is_empty() {
        return Err(out.issues.into_iter().next().unwrap());
    }
    Ok(out.candidates)
}

#[cfg(test)]
pub fn discover_scan(root: &Path) -> super::DiscoverOutcome {
    let mut candidates = Vec::new();
    let mut issues = Vec::new();
    discover_transcripts(root, &mut candidates);
    discover_composer(root, &mut candidates, &mut issues);
    discover_store(root, &mut candidates);
    let (candidates, agg_issues) = aggregate_cursor_identities(candidates);
    issues.extend(agg_issues.into_iter().map(|i| {
        ImportError::new(
            ErrorCode::PartialHistory,
            format!("{}: {}", i.code, i.message),
        )
    }));
    super::DiscoverOutcome { candidates, issues }
}

#[cfg(test)]
fn discover_transcripts(root: &Path, out: &mut Vec<SourceCandidate>) {
    let projects = root.join("projects");
    let base = if projects.is_dir() {
        projects
    } else if root.join(".cursor").join("projects").is_dir() {
        root.join(".cursor").join("projects")
    } else {
        root.to_path_buf()
    };
    walk_named(&base, "agent-transcripts", &base, out, 0);
}

#[cfg(test)]
fn walk_named(dir: &Path, want: &str, root: &Path, out: &mut Vec<SourceCandidate>, depth: u32) {
    if depth > 8 {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for ent in rd.flatten() {
        let p = ent.path();
        if p.is_dir() {
            if p.file_name().and_then(|s| s.to_str()) == Some(want) {
                collect_jsonl(&p, root, out);
            }
            walk_named(&p, want, root, out, depth + 1);
        }
    }
}

#[cfg(test)]
fn collect_jsonl(dir: &Path, root: &Path, out: &mut Vec<SourceCandidate>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for ent in rd.flatten() {
        let p = ent.path();
        if p.is_dir() {
            collect_jsonl(&p, root, out);
            continue;
        }
        if p.extension().and_then(|s| s.to_str()) != Some("jsonl") {
            continue;
        }
        if let Ok(c) = transcript_candidate(&p, root) {
            out.push(c);
        }
    }
}

#[cfg(test)]
fn transcript_relative_id(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .ok()
        .filter(|p| !p.as_os_str().is_empty())
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| {
            path.file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default()
        })
}

#[cfg(test)]
fn transcript_candidate(path: &Path, root: &Path) -> Result<SourceCandidate, ImportError> {
    let mut stream = open_source_jsonl(path)?;
    let id = transcript_relative_id(path, root);
    let subagent = path.components().any(|c| c.as_os_str() == "subagents");
    let mut title = None;
    for record in stream.by_ref() {
        let record = record?;
        if title.is_none() {
            title = transcript_title(&record.value);
        }
    }
    let prefix = stream.finish()?;
    let title = title.unwrap_or_else(|| id.clone());
    Ok(SourceCandidate {
        candidate_id: format!("cursor-transcript:{}", path.to_string_lossy()),
        provider: Provider::Cursor,
        native_session_id: id.clone(),
        title,
        cwd: None,
        started_at: None,
        updated_at: None,
        locator: path.to_string_lossy().into_owned(),
        source_revision: prefix.complete_prefix_sha256,
        archived: None,
        completeness: if subagent {
            "subagent".into()
        } else {
            "transcript".into()
        },
    })
}

/// Split a recognized DB-key suffix, preserving literal `#` in the file path.
/// Keep the key family so an old chat cannot accidentally resolve to a Composer.
pub fn split_composer_locator(locator: &str) -> (std::path::PathBuf, Option<&str>) {
    let suffix = ["#composerData:", "#chatData:"]
        .iter()
        .filter_map(|marker| locator.rfind(marker).map(|at| (at, marker.len())))
        .max_by_key(|(at, _)| *at);
    if let Some((at, marker_len)) = suffix {
        if locator.len() > at + marker_len {
            return (PathBuf::from(&locator[..at]), Some(&locator[at + 1..]));
        }
    }
    (PathBuf::from(locator), None)
}

pub fn normalize_transcript_file(path: &Path) -> Result<AdapterOutput, ImportError> {
    if path.extension().and_then(|s| s.to_str()) == Some("jsonl") {
        return normalize_transcript(path);
    }
    if path.file_name().and_then(|s| s.to_str()) == Some("store.db")
        || path.extension().and_then(|s| s.to_str()) == Some("vscdb")
        || path.extension().and_then(|s| s.to_str()) == Some("db")
    {
        return normalize_sqlite(path);
    }
    Err(ImportError::new(
        ErrorCode::FormatUnsupported,
        format!("unsupported Cursor artifact {}", path.display()),
    ))
}

pub fn normalize_transcript(path: &Path) -> Result<AdapterOutput, ImportError> {
    normalize_transcript_in_root(path, None)
}

pub(crate) fn normalize_transcript_in_root(
    path: &Path,
    root: Option<&Path>,
) -> Result<AdapterOutput, ImportError> {
    normalize_transcript_in_location(path, root, None)
}

pub(crate) fn normalize_transcript_in_location(
    path: &Path,
    root: Option<&Path>,
    location_scope: Option<&str>,
) -> Result<AdapterOutput, ImportError> {
    let mut stream = open_source_jsonl(path)?;
    let locator = path.to_string_lossy().into_owned();
    let mut identity = TranscriptIdentity::new_scoped(path, root, location_scope)?;
    let mut events = Vec::new();
    let mut relations = Vec::new();
    let mut issues = Vec::new();
    let mut prev = None;
    let mut ordinal = 0u32;
    let mut cwd = None;
    let mut title = None;
    for rec in stream.by_ref() {
        let rec = rec?;
        identity.observe(&rec.value)?;
        if title.is_none() {
            title = transcript_title(&rec.value);
        }
        if cwd.is_none() {
            cwd = rec
                .value
                .get("cwd")
                .or_else(|| rec.value.pointer("/message/cwd"))
                .and_then(Value::as_str)
                .map(|s| s.to_string());
        }
        let ts = rec
            .value
            .get("timestamp")
            .or_else(|| rec.value.get("createdAt"))
            .and_then(Value::as_str)
            .map(|s| s.to_string());
        let role = rec.value.get("role").and_then(|x| x.as_str()).unwrap_or("");
        let record_id = rec
            .value
            .get("id")
            .and_then(Value::as_str)
            .map(|s| format!("cursor:{s}"))
            .unwrap_or_else(|| format!("cursor:tx:{}", rec.index));
        if role.is_empty() && rec.value.get("type").and_then(Value::as_str) == Some("turn_ended") {
            // Cursor 3.x turn boundary; the status is kept, it is not a missing role.
            let status: Option<String> = rec
                .value
                .get("status")
                .and_then(Value::as_str)
                .map(|s| {
                    s.chars()
                        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
                        .take(32)
                        .collect()
                })
                .filter(|s: &String| !s.is_empty());
            events.push(ImportedEvent::Metadata {
                base: ev_base(
                    &format!("{record_id}:turn-ended"),
                    ordinal,
                    ts,
                    &locator,
                    rec.index,
                    None,
                ),
                source_type: status
                    .map_or_else(|| "turn_ended".into(), |s| format!("turn_ended:{s}")),
                payload: None,
            });
            ordinal += 1;
            continue;
        }
        if role == "tool" {
            let call_id = rec
                .value
                .get("tool_call_id")
                .or_else(|| rec.value.get("toolCallId"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let text = transcript_text(&rec.value, role).unwrap_or_default();
            let id = format!("{record_id}:result");
            events.push(ImportedEvent::ToolResult {
                base: ev_base(&id, ordinal, ts, &locator, rec.index, None),
                call_id,
                blocks: vec![ContentBlock::text(text)],
                outcome: ToolOutcome::Success,
            });
            link_prev(&mut relations, &id, &mut prev);
            ordinal += 1;
            continue;
        }
        let mrole = match role {
            "user" | "human" => MessageRole::User,
            "assistant" | "ai" => MessageRole::Assistant,
            "" => {
                issues.push(
                    ImportIssue::new("unknown-role", "cursor record missing role")
                        .with_span(format!("record:{}", rec.index)),
                );
                continue;
            }
            other => {
                issues.push(
                    ImportIssue::new("unknown-role", format!("cursor role '{other}'"))
                        .with_span(format!("record:{}", rec.index)),
                );
                continue;
            }
        };
        let parts = transcript_parts(&rec.value);
        if parts.is_empty() {
            continue;
        }
        for (block, part) in parts.into_iter().enumerate() {
            let id = if block == 0 {
                record_id.clone()
            } else {
                format!("{record_id}:block:{block}")
            };
            let event = match part {
                TranscriptPart::Text(text) => {
                    let query = (mrole == MessageRole::User)
                        .then(|| transcript_wrappers::unwrap_user_query(&text))
                        .flatten();
                    let (text, stamp) = match query {
                        Some(query) => (query.text, query.timestamp),
                        None => (text, None),
                    };
                    ImportedEvent::Message {
                        base: ev_base(
                            &id,
                            ordinal,
                            ts.clone().or(stamp),
                            &locator,
                            rec.index,
                            Some(block as u32),
                        ),
                        role: mrole,
                        blocks: vec![ContentBlock::text(text)],
                        complete: true,
                    }
                }
                TranscriptPart::ToolUse {
                    call_id,
                    name,
                    input,
                } => ImportedEvent::ToolCall {
                    base: ev_base(
                        &id,
                        ordinal,
                        ts.clone(),
                        &locator,
                        rec.index,
                        Some(block as u32),
                    ),
                    call_id,
                    name,
                    input: ToolInput::Json { value: input },
                    parent_message_id: prev.clone(),
                },
                TranscriptPart::ToolResult { call_id, text } => ImportedEvent::ToolResult {
                    base: ev_base(
                        &id,
                        ordinal,
                        ts.clone(),
                        &locator,
                        rec.index,
                        Some(block as u32),
                    ),
                    call_id,
                    blocks: vec![ContentBlock::text(text)],
                    outcome: ToolOutcome::Success,
                },
                TranscriptPart::Unknown(source_type) => {
                    issues.push(
                        ImportIssue::new("unknown-block", format!("cursor block {source_type}"))
                            .with_span(format!("record:{}:block:{block}", rec.index)),
                    );
                    ImportedEvent::Unsupported {
                        base: ev_base(
                            &id,
                            ordinal,
                            ts.clone(),
                            &locator,
                            rec.index,
                            Some(block as u32),
                        ),
                        source_type,
                        payload: None,
                    }
                }
            };
            events.push(event);
            link_prev(&mut relations, &id, &mut prev);
            ordinal += 1;
        }
    }
    if events.is_empty() {
        return Err(ImportError::new(
            ErrorCode::FormatUnsupported,
            "Cursor transcript had no messages",
        ));
    }
    let prefix = stream.finish()?;
    issues.extend(prefix.issues);
    let mut out = finish_cursor(
        events,
        relations,
        issues,
        identity.finish(),
        locator,
        prefix.byte_length,
        prefix.file_sha256,
        Some(prefix.complete_prefix_sha256),
        "cursor-transcript-jsonl-v1",
        if prefix.trailing_incomplete {
            ImportOutcome::Partial
        } else {
            ImportOutcome::Complete
        },
    )?;
    out.graph.cwd = cwd;
    // Display names are not session identity. Keep the same readable filename
    // fallback as discovery instead of exposing a native ID or locator hash.
    out.graph.title = Some(title.unwrap_or_else(|| {
        path.file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned()
    }));
    Ok(out)
}

pub(crate) fn transcript_title(record: &Value) -> Option<String> {
    if !matches!(
        record.get("role").and_then(Value::as_str),
        Some("user" | "human")
    ) || record.get("isSidechain").and_then(Value::as_bool) == Some(true)
        || record.get("isMeta").and_then(Value::as_bool) == Some(true)
    {
        return None;
    }
    let content = record
        .pointer("/message/content")
        .or_else(|| record.get("content"))?;
    let single = match content {
        Value::String(text) => Some(text.as_str()),
        Value::Array(blocks) if blocks.len() == 1 => blocks[0].get("text").and_then(Value::as_str),
        _ => None,
    };
    match single.and_then(transcript_wrappers::unwrap_user_query) {
        Some(query) => super::titles::prompt_title(&Value::String(query.text)),
        None => super::titles::prompt_title(content),
    }
}

fn link_prev(relations: &mut Vec<EventRelation>, id: &str, prev: &mut Option<String>) {
    if let Some(p) = prev.take() {
        relations.push(EventRelation {
            kind: RelationKind::Parent,
            from: id.into(),
            to: p,
            confidence: RelationConfidence::Inferred,
        });
    }
    *prev = Some(id.into());
}

fn ev_base(
    id: &str,
    ordinal: u32,
    ts: Option<String>,
    locator: &str,
    index: u32,
    block: Option<u32>,
) -> EventBase {
    EventBase {
        id: id.into(),
        ordinal,
        spans: vec![SourceSpan {
            artifact_id: locator.into(),
            record: format!("record:{index}"),
            block,
        }],
        timestamp: ts.clone(),
        time_origin: if ts.is_some() {
            TimeOrigin::Source
        } else {
            TimeOrigin::Unknown
        },
    }
}

enum TranscriptPart {
    Text(String),
    ToolUse {
        call_id: String,
        name: String,
        input: Value,
    },
    ToolResult {
        call_id: String,
        text: String,
    },
    Unknown(String),
}

fn transcript_parts(v: &Value) -> Vec<TranscriptPart> {
    if let Some(s) = v.pointer("/message/content").and_then(Value::as_str) {
        return nonempty_text(s);
    }
    if let Some(arr) = v.pointer("/message/content").and_then(Value::as_array) {
        return arr.iter().filter_map(parse_transcript_block).collect();
    }
    if let Some(s) = v.get("content").and_then(Value::as_str) {
        return nonempty_text(s);
    }
    if let Some(arr) = v.get("content").and_then(Value::as_array) {
        return arr.iter().filter_map(parse_transcript_block).collect();
    }
    Vec::new()
}

fn nonempty_text(s: &str) -> Vec<TranscriptPart> {
    if s.is_empty() {
        Vec::new()
    } else {
        vec![TranscriptPart::Text(s.to_string())]
    }
}

fn parse_transcript_block(p: &Value) -> Option<TranscriptPart> {
    let ty = p.get("type").and_then(Value::as_str).unwrap_or("text");
    match ty {
        "text" => p
            .get("text")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(|s| TranscriptPart::Text(s.to_string())),
        "tool_use" | "tool_call" => Some(TranscriptPart::ToolUse {
            call_id: p
                .get("id")
                .or_else(|| p.get("toolCallId"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            name: p
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("tool")
                .to_string(),
            input: p.get("input").cloned().unwrap_or(Value::Null),
        }),
        "tool_result" => Some(TranscriptPart::ToolResult {
            call_id: p
                .get("tool_use_id")
                .or_else(|| p.get("toolCallId"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            text: p
                .get("content")
                .and_then(Value::as_str)
                .or_else(|| p.get("text").and_then(Value::as_str))
                .unwrap_or("")
                .to_string(),
        }),
        other => Some(TranscriptPart::Unknown(other.into())),
    }
}

fn transcript_text(v: &Value, _role: &str) -> Option<String> {
    if let Some(s) = v.pointer("/message/content").and_then(|x| x.as_str()) {
        return Some(s.to_string());
    }
    if let Some(arr) = v.pointer("/message/content").and_then(|x| x.as_array()) {
        let t = arr
            .iter()
            .filter_map(|p| p.get("text").and_then(|x| x.as_str()))
            .collect::<Vec<_>>()
            .join("\n");
        if !t.is_empty() {
            return Some(t);
        }
    }
    v.get("content")
        .and_then(|x| x.as_str())
        .map(|s| s.to_string())
}

#[cfg(test)]
fn discover_composer(root: &Path, out: &mut Vec<SourceCandidate>, issues: &mut Vec<ImportError>) {
    let user = if root.join("workspaceStorage").is_dir() {
        root.to_path_buf()
    } else if root.join("User").join("workspaceStorage").is_dir() {
        root.join("User")
    } else {
        return;
    };
    let ws = user.join("workspaceStorage");
    if let Ok(rd) = std::fs::read_dir(&ws) {
        for ent in rd.flatten() {
            let db = ent.path().join("state.vscdb");
            if db.is_file() {
                match list_composer_from_db(&db, Some(&ent.path())) {
                    Ok(list) => out.extend(list),
                    Err(e) => issues.push(e),
                }
            }
        }
    }
    let global = user.join("globalStorage").join("state.vscdb");
    if global.is_file() {
        match list_composer_from_db(&global, None) {
            Ok(list) => out.extend(list),
            Err(e) => issues.push(e),
        }
    }
}

#[cfg(test)]
pub(crate) fn list_composer_from_db(
    db: &Path,
    workspace_dir: Option<&Path>,
) -> Result<Vec<SourceCandidate>, ImportError> {
    let conn = match open_readonly(db) {
        Ok(c) => c,
        Err(e) if e.code == ErrorCode::SourceNotFound => return Ok(vec![]),
        Err(e) => return Err(e),
    };
    let tables = table_names_at(&conn, db)?;
    let source_revision = conn.source_revision();
    if !tables
        .iter()
        .any(|t| t == "ItemTable" || t == "cursorDiskKV")
    {
        return Err(ImportError::new(
            ErrorCode::SchemaUnsupported,
            format!("unrecognized composer schema in {}", db.display()),
        ));
    }
    let mut stmt = conn
        .prepare("SELECT key FROM ItemTable")
        .or_else(|_| conn.prepare("SELECT key FROM cursorDiskKV"))
        .map_err(|e| classify_query_err(db, &e))?;
    let rows = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|e| classify_query_err(db, &e))?;
    let mut out = Vec::new();
    for key in rows {
        let key = key.map_err(|e| classify_query_err(db, &e))?;
        let (prefix, completeness) = if key.starts_with("composerData:") {
            ("composerData:", "composer")
        } else if key.starts_with("chatData:") {
            ("chatData:", "legacy-chat")
        } else {
            continue;
        };
        if let Some(id) = key.strip_prefix(prefix) {
            out.push(SourceCandidate {
                candidate_id: format!("cursor-{completeness}:{id}"),
                provider: Provider::Cursor,
                native_session_id: id.to_string(),
                title: id.to_string(),
                cwd: workspace_dir.and_then(read_workspace_path),
                started_at: None,
                updated_at: None,
                locator: format!("{}#{prefix}{id}", db.display()),
                source_revision: source_revision.clone(),
                archived: None,
                completeness: completeness.into(),
            });
        }
    }
    Ok(out)
}

pub(crate) fn read_workspace_path(dir: &Path) -> Option<String> {
    let p = dir.join("workspace.json");
    let text = std::fs::read_to_string(p).ok()?;
    let v: Value = serde_json::from_str(&text).ok()?;
    v.get("folder")
        .and_then(|x| x.as_str())
        .map(|s| s.trim_start_matches("file://").to_string())
}

#[cfg(test)]
fn discover_store(root: &Path, out: &mut Vec<SourceCandidate>) {
    let chats = if root.join("chats").is_dir() {
        root.join("chats")
    } else if root.join(".cursor").join("chats").is_dir() {
        root.join(".cursor").join("chats")
    } else {
        return;
    };
    walk_store(&chats, out, 0);
    let acp = root.join("acp-sessions");
    if acp.is_dir() {
        walk_store(&acp, out, 0);
    }
}

#[cfg(test)]
fn walk_store(dir: &Path, out: &mut Vec<SourceCandidate>, depth: u32) {
    if depth > 6 {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for ent in rd.flatten() {
        let p = ent.path();
        if p.is_dir() {
            let db = p.join("store.db");
            if db.is_file() {
                let meta = p.join("meta.json");
                let id = p
                    .file_name()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let title = std::fs::read_to_string(&meta)
                    .ok()
                    .and_then(|t| serde_json::from_str::<Value>(&t).ok())
                    .and_then(|v| {
                        v.get("name")
                            .or_else(|| v.get("title"))
                            .and_then(|x| x.as_str())
                            .map(|s| s.to_string())
                    })
                    .unwrap_or_else(|| id.clone());
                out.push(SourceCandidate {
                    candidate_id: format!("cursor-store:{}", p.display()),
                    provider: Provider::Cursor,
                    native_session_id: id,
                    title,
                    cwd: None,
                    started_at: None,
                    updated_at: None,
                    locator: db.to_string_lossy().into_owned(),
                    source_revision: sha256_bytes(&std::fs::read(&db).unwrap_or_default()),
                    archived: None,
                    completeness: "store".into(),
                });
            }
            walk_store(&p, out, depth + 1);
        }
    }
}

#[path = "cursor_store.rs"]
mod cursor_store;
pub use cursor_store::*;

#[path = "cursor_store_wire.rs"]
mod cursor_store_wire;

#[cfg(test)]
mod tests {
    use super::*;
    include!("cursor_tests.rs");
}

#[cfg(test)]
#[path = "cursor_transcript_323_tests.rs"]
mod transcript_323_tests;
