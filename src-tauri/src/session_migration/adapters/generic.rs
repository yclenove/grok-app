//! Generic JSON / Markdown import. Unknown JSON does not fall back to Markdown.

use std::path::Path;

use serde_json::Value;

use crate::session_migration::budget::SESSION_BODY_MAX_BYTES;
use crate::session_migration::detect::current_platform;
#[cfg(test)]
use crate::session_migration::discovery;
use crate::session_migration::errors::{ErrorCode, ImportError};
use crate::session_migration::hash::sha256_bytes;
use crate::session_migration::jsonl;
#[cfg(test)]
use crate::session_migration::types::SourceCandidate;
use crate::session_migration::types::NORMALIZER_VERSION;
use crate::session_migration::types::{
    AdapterOutput, ArtifactEvidence, ContentBlock, EventBase, EventRelation, ImportIssue,
    ImportOutcome, ImportedEvent, Inventory, MessageRole, NormalizedGraph, Provider,
    RelationConfidence, RelationKind, SourceEvidence, SourceSpan, TimeOrigin,
};

#[path = "generic_spool.rs"]
mod generic_spool;
#[path = "generic_spool_raw.rs"]
mod generic_spool_raw;
#[path = "generic_spool_tokens.rs"]
mod generic_spool_tokens;
#[path = "generic_stream.rs"]
mod generic_stream;

#[cfg(test)]
pub fn discover(root: &Path) -> Result<Vec<SourceCandidate>, ImportError> {
    let mut out = Vec::new();
    if root.is_file() {
        if accepted_extension(root) {
            out.push(discovery::file_candidate(
                Provider::GenericFile,
                root,
                root,
            )?);
        }
        return Ok(out);
    }
    let rd = std::fs::read_dir(root).map_err(|e| {
        ImportError::new(
            ErrorCode::SourcePermissionDenied,
            format!("read {}: {e}", root.display()),
        )
    })?;
    for ent in rd.flatten() {
        let p = ent.path();
        if p.is_file() && accepted_extension(&p) {
            out.push(discovery::file_candidate(Provider::GenericFile, &p, root)?);
        }
    }
    Ok(out)
}

#[cfg(test)]
fn accepted_extension(path: &Path) -> bool {
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    matches!(ext.as_str(), "json" | "md" | "markdown" | "txt")
}

#[cfg(test)]
pub fn normalize_file(path: &Path) -> Result<AdapterOutput, ImportError> {
    normalize_file_in_root(path, None)
}

pub fn normalize_file_in_root(
    path: &Path,
    root: Option<&Path>,
) -> Result<AdapterOutput, ImportError> {
    let len = std::fs::metadata(path)
        .map_err(|e| {
            ImportError::new(
                ErrorCode::SourceNotFound,
                format!("{}: {e}", path.display()),
            )
        })?
        .len();
    ensure_body_budget(len)?;
    let locator = relative_locator(path, root);
    generic_stream::normalize_file(path, &locator, len)
}

pub(crate) fn prepare_file_in_root(
    path: &Path,
    root: Option<&Path>,
    native_session_id: &str,
    options: &serde_json::Value,
    preview_directory: Option<&str>,
) -> Result<
    (
        AdapterOutput,
        crate::session_migration::prepared_builder::PreparedRevisionOutput,
    ),
    ImportError,
> {
    let len = std::fs::metadata(path)
        .map_err(|error| {
            ImportError::new(
                ErrorCode::SourceNotFound,
                format!("{}: {error}", path.display()),
            )
        })?
        .len();
    ensure_body_budget(len)?;
    let locator = relative_locator(path, root);
    generic_spool::prepare_file(
        path,
        &locator,
        native_session_id,
        len,
        options,
        preview_directory,
    )
}

fn relative_locator(path: &Path, root: Option<&Path>) -> String {
    if let Some(root) = root {
        if let Ok(rel) = path.strip_prefix(root) {
            if !rel.as_os_str().is_empty() {
                return rel.to_string_lossy().into_owned();
            }
        }
    }
    path.file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

fn ensure_body_budget(len: u64) -> Result<(), ImportError> {
    if len > SESSION_BODY_MAX_BYTES {
        return Err(ImportError::new(
            ErrorCode::ResourceLimit,
            "file exceeds 1 GiB",
        ));
    }
    Ok(())
}

pub fn normalize_bytes(bytes: &[u8], locator: &str) -> Result<AdapterOutput, ImportError> {
    ensure_body_budget(bytes.len() as u64)?;
    let text = std::str::from_utf8(bytes)
        .map_err(|_| ImportError::new(ErrorCode::FormatUnsupported, "file is not UTF-8"))?;
    let trimmed = strip_bom(text).trim();
    if trimmed.is_empty() {
        return Err(ImportError::new(
            ErrorCode::FormatUnsupported,
            "empty transcript",
        ));
    }
    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        jsonl::check_depth(trimmed.as_bytes())?;
    }
    if trimmed.starts_with('{') {
        return parse_json_object(trimmed, locator, bytes);
    }
    if trimmed.starts_with('[') {
        return parse_json_array(trimmed, locator, bytes);
    }
    parse_markdown(trimmed, locator, bytes)
}

fn strip_bom(s: &str) -> &str {
    s.strip_prefix('\u{feff}').unwrap_or(s)
}

fn parse_json_object(
    text: &str,
    locator: &str,
    bytes: &[u8],
) -> Result<AdapterOutput, ImportError> {
    let v: Value = serde_json::from_str(text).map_err(|e| {
        ImportError::new(
            ErrorCode::FormatUnsupported,
            format!("invalid JSON object: {e}"),
        )
    })?;
    let arr = v
        .get("messages")
        .or_else(|| v.get("conversation"))
        .or_else(|| v.get("items"))
        .and_then(|x| x.as_array())
        .ok_or_else(|| {
            ImportError::new(
                ErrorCode::FormatUnsupported,
                "JSON object has no messages/conversation/items array",
            )
        })?;
    let mut out = messages_from_json_array(arr, locator, bytes, "json-object")?;
    out.inventory.source_tokens = source_tokens_from_json(&v);
    Ok(out)
}

fn source_tokens_from_json(v: &Value) -> u64 {
    fn as_u64(x: &Value) -> Option<u64> {
        x.as_u64()
            .or_else(|| x.as_i64().and_then(|n| u64::try_from(n).ok()))
            .or_else(|| {
                x.as_f64()
                    .and_then(|n| (n.is_finite() && n >= 0.0).then_some(n as u64))
            })
    }
    v.get("usage")
        .and_then(|u| {
            u.get("total_tokens")
                .or_else(|| u.get("totalTokens"))
                .and_then(as_u64)
        })
        .or_else(|| v.get("token_count").and_then(as_u64))
        .or_else(|| v.get("tokenCount").and_then(as_u64))
        .or_else(|| v.get("total_tokens").and_then(as_u64))
        .or_else(|| v.get("totalTokens").and_then(as_u64))
        .unwrap_or(0)
}

fn parse_json_array(text: &str, locator: &str, bytes: &[u8]) -> Result<AdapterOutput, ImportError> {
    let v: Value = serde_json::from_str(text).map_err(|e| {
        ImportError::new(
            ErrorCode::FormatUnsupported,
            format!("invalid JSON array: {e}"),
        )
    })?;
    let arr = v.as_array().ok_or_else(|| {
        ImportError::new(ErrorCode::FormatUnsupported, "JSON root is not an array")
    })?;
    messages_from_json_array(arr, locator, bytes, "json-array")
}

fn messages_from_json_array(
    arr: &[Value],
    locator: &str,
    bytes: &[u8],
    fingerprint: &str,
) -> Result<AdapterOutput, ImportError> {
    let mut events = Vec::new();
    let mut relations = Vec::new();
    let mut prev: Option<String> = None;
    let mut ordinal = 0u32;
    let mut issues = Vec::new();
    for (i, item) in arr.iter().enumerate() {
        let role_raw = item
            .get("role")
            .or_else(|| item.get("author"))
            .and_then(|x| x.as_str())
            .unwrap_or("");
        let Some(role) = normalize_role(role_raw) else {
            issues.push(
                ImportIssue::new("unknown-role", format!("row {i} has role '{role_raw}'"))
                    .with_span(format!("index:{i}")),
            );
            continue;
        };
        let text = extract_text(item);
        if text.trim().is_empty() {
            continue;
        }
        let ts = item
            .get("timestamp")
            .or_else(|| item.get("createdAt"))
            .or_else(|| item.get("created_at"))
            .and_then(|x| x.as_str())
            .map(|s| s.to_string());
        let id = item
            .get("id")
            .and_then(|x| x.as_str())
            .map(|s| format!("generic:{s}"))
            .unwrap_or_else(|| format!("generic:{i}"));
        let ev = ImportedEvent::Message {
            base: EventBase {
                id: id.clone(),
                ordinal,
                spans: vec![SourceSpan {
                    artifact_id: locator.into(),
                    record: format!("index:{i}"),
                    block: None,
                }],
                timestamp: ts.clone(),
                time_origin: if ts.is_some() {
                    TimeOrigin::Source
                } else {
                    TimeOrigin::Unknown
                },
            },
            role,
            blocks: vec![ContentBlock::text(text)],
            complete: true,
        };
        if let Some(p) = prev {
            relations.push(EventRelation {
                kind: RelationKind::Parent,
                from: id.clone(),
                to: p,
                confidence: RelationConfidence::Inferred,
            });
        }
        prev = Some(id);
        events.push(ev);
        ordinal += 1;
    }
    if events.is_empty() {
        return Err(ImportError::new(
            ErrorCode::FormatUnsupported,
            "JSON array had no user/assistant messages",
        ));
    }
    Ok(finish(
        events,
        relations,
        issues,
        locator,
        bytes,
        fingerprint,
        Path::new(locator)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned()),
    ))
}

fn extract_text(item: &Value) -> String {
    if let Some(s) = item.get("content").and_then(|x| x.as_str()) {
        return s.to_string();
    }
    if let Some(s) = item.get("text").and_then(|x| x.as_str()) {
        return s.to_string();
    }
    if let Some(s) = item.get("message").and_then(|x| x.as_str()) {
        return s.to_string();
    }
    if let Some(arr) = item.get("content").and_then(|x| x.as_array()) {
        return arr
            .iter()
            .filter_map(|p| {
                p.get("text")
                    .and_then(|t| t.as_str())
                    .or_else(|| p.as_str())
            })
            .collect::<Vec<_>>()
            .join("\n");
    }
    String::new()
}

fn normalize_role(role: &str) -> Option<MessageRole> {
    match role.trim().to_ascii_lowercase().as_str() {
        "user" | "human" | "me" | "prompt" => Some(MessageRole::User),
        "assistant" | "ai" | "bot" | "model" | "grok" | "agent" => Some(MessageRole::Assistant),
        "system" => Some(MessageRole::System),
        _ => None,
    }
}

fn parse_markdown(text: &str, locator: &str, bytes: &[u8]) -> Result<AdapterOutput, ImportError> {
    let mut events = Vec::new();
    let mut relations = Vec::new();
    let mut prev: Option<String> = None;
    let mut ordinal = 0u32;
    let mut cur_role: Option<MessageRole> = None;
    let mut buf: Vec<String> = Vec::new();
    let mut in_fence = false;
    let mut fence_mark: Option<String> = None;

    let flush = |role: Option<MessageRole>,
                 buf: &mut Vec<String>,
                 events: &mut Vec<ImportedEvent>,
                 relations: &mut Vec<EventRelation>,
                 prev: &mut Option<String>,
                 ordinal: &mut u32,
                 locator: &str| {
        let Some(role) = role else { return };
        let body = buf.join("\n").trim().to_string();
        buf.clear();
        if body.is_empty() {
            return;
        }
        let id = format!("generic:md:{ordinal}");
        events.push(ImportedEvent::Message {
            base: EventBase {
                id: id.clone(),
                ordinal: *ordinal,
                spans: vec![SourceSpan {
                    artifact_id: locator.into(),
                    record: format!("md:{ordinal}"),
                    block: None,
                }],
                timestamp: None,
                time_origin: TimeOrigin::Unknown,
            },
            role,
            blocks: vec![ContentBlock::text(body)],
            complete: true,
        });
        if let Some(p) = prev.take() {
            relations.push(EventRelation {
                kind: RelationKind::Parent,
                from: id.clone(),
                to: p,
                confidence: RelationConfidence::Inferred,
            });
        }
        *prev = Some(id);
        *ordinal += 1;
    };

    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(mark) = fence_open(trimmed) {
            if in_fence && fence_mark.as_deref() == Some(mark) {
                in_fence = false;
                fence_mark = None;
            } else if !in_fence {
                in_fence = true;
                fence_mark = Some(mark.to_string());
            }
            if cur_role.is_some() {
                buf.push(line.to_string());
            }
            continue;
        }
        if in_fence {
            if cur_role.is_some() {
                buf.push(line.to_string());
            }
            continue;
        }
        if let Some(role) = heading_role(trimmed).or_else(|| prefix_role(trimmed)) {
            flush(
                cur_role,
                &mut buf,
                &mut events,
                &mut relations,
                &mut prev,
                &mut ordinal,
                locator,
            );
            cur_role = Some(role.0);
            if let Some(rest) = role.1 {
                if !rest.is_empty() {
                    buf.push(rest);
                }
            }
            continue;
        }
        if cur_role.is_some() {
            buf.push(line.to_string());
        }
    }
    flush(
        cur_role,
        &mut buf,
        &mut events,
        &mut relations,
        &mut prev,
        &mut ordinal,
        locator,
    );
    if events.is_empty() {
        return Err(ImportError::new(
            ErrorCode::FormatUnsupported,
            "Could not parse transcript. Use JSON messages array or markdown ## User / ## Assistant sections.",
        ));
    }
    Ok(finish(
        events,
        relations,
        Vec::new(),
        locator,
        bytes,
        "markdown",
        Path::new(locator)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned()),
    ))
}

fn fence_open(trimmed: &str) -> Option<&str> {
    if trimmed.starts_with("```") {
        Some("```")
    } else if trimmed.starts_with("~~~") {
        Some("~~~")
    } else {
        None
    }
}

fn heading_role(trimmed: &str) -> Option<(MessageRole, Option<String>)> {
    let heading = trimmed
        .strip_prefix("### ")
        .or_else(|| trimmed.strip_prefix("## "))
        .or_else(|| trimmed.strip_prefix("# "))
        .or_else(|| {
            trimmed
                .strip_prefix("**")
                .and_then(|s| s.strip_suffix("**"))
        })
        .map(str::trim)?;
    let hl = heading.to_ascii_lowercase();
    if hl == "user" || hl == "human" || hl == "me" || hl.starts_with("user") {
        Some((MessageRole::User, None))
    } else if hl == "assistant" || hl == "ai" || hl.starts_with("assistant") {
        Some((MessageRole::Assistant, None))
    } else {
        None
    }
}

fn prefix_role(trimmed: &str) -> Option<(MessageRole, Option<String>)> {
    for (p, role) in [
        ("User:", MessageRole::User),
        ("user:", MessageRole::User),
        ("Human:", MessageRole::User),
        ("Assistant:", MessageRole::Assistant),
        ("assistant:", MessageRole::Assistant),
        ("Grok:", MessageRole::Assistant),
        ("AI:", MessageRole::Assistant),
    ] {
        if let Some(rest) = trimmed.strip_prefix(p) {
            let rest = rest.trim();
            return Some((
                role,
                if rest.is_empty() {
                    None
                } else {
                    Some(rest.to_string())
                },
            ));
        }
    }
    None
}

fn finish(
    events: Vec<ImportedEvent>,
    relations: Vec<EventRelation>,
    issues: Vec<ImportIssue>,
    locator: &str,
    bytes: &[u8],
    fingerprint: &str,
    title: Option<String>,
) -> AdapterOutput {
    let native = if locator.trim().is_empty() {
        "generic".into()
    } else {
        locator.to_string()
    };
    let mut out = AdapterOutput {
        evidence: SourceEvidence {
            provider: Provider::GenericFile,
            source_version: None,
            schema_fingerprint: fingerprint.into(),
            adapter_version: NORMALIZER_VERSION.into(),
            platform: current_platform(),
            artifacts: vec![ArtifactEvidence {
                artifact_id: locator.into(),
                kind: "file".into(),
                relative_locator: locator.into(),
                byte_length: bytes.len() as u64,
                sha256: sha256_bytes(bytes),
                complete_prefix_sha256: None,
            }],
        },
        graph: NormalizedGraph {
            events,
            relations,
            active_leaf: None,
            native_session_id: native,
            title,
            cwd: None,
        },
        inventory: Inventory::default(),
        issues,
        assets: Vec::new(),
        outcome: ImportOutcome::Complete,
    };
    out.graph.active_leaf = out.graph.events.last().map(|e| e.id().to_string());
    out.recount();
    if !out.issues.is_empty() {
        out.outcome = ImportOutcome::Partial;
    }
    out
}

/// Legacy pair list used by the old transcript command.
pub fn parse_legacy_pairs(raw: &str) -> Result<Vec<(String, String)>, String> {
    let out = normalize_bytes(raw.as_bytes(), "inline").map_err(|e| e.to_string())?;
    let mut pairs = Vec::new();
    for ev in out.graph.events {
        if let ImportedEvent::Message { role, blocks, .. } = ev {
            let role = match role {
                MessageRole::User => "user",
                MessageRole::Assistant => "assistant",
                MessageRole::System => continue,
            };
            let text = blocks
                .iter()
                .filter_map(ContentBlock::plain_text)
                .collect::<Vec<_>>()
                .join("\n");
            if !text.trim().is_empty() {
                pairs.push((role.into(), text));
            }
        }
    }
    if pairs.is_empty() {
        return Err("no user/assistant messages".into());
    }
    Ok(pairs)
}

#[cfg(test)]
mod tests {
    use super::*;
    include!("generic_tests_v10.rs");

    #[test]
    fn fenced_user_heading_is_not_a_role() {
        let md = r#"## User
hello
```
User: not a turn
## Assistant
inside fence
```
## Assistant
ok
"#;
        let out = normalize_bytes(md.as_bytes(), "fence.md").unwrap();
        let msgs: Vec<_> = out
            .graph
            .events
            .iter()
            .filter_map(|e| match e {
                ImportedEvent::Message { role, blocks, .. } => {
                    Some((*role, blocks[0].plain_text().unwrap_or("").to_string()))
                }
                _ => None,
            })
            .collect();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].0, MessageRole::User);
        assert!(msgs[0].1.contains("User: not a turn"));
        assert_eq!(msgs[1].0, MessageRole::Assistant);
        assert_eq!(msgs[1].1.trim(), "ok");
    }

    #[test]
    fn illegal_json_object_does_not_fall_back_to_markdown() {
        let raw = r#"{"foo":"User: hello\nAssistant: hi"}"#;
        let err = normalize_bytes(raw.as_bytes(), "bad.json").unwrap_err();
        assert_eq!(err.code, ErrorCode::FormatUnsupported);
    }

    #[test]
    fn json_array_keeps_source_timestamps() {
        let raw = r#"[{"role":"user","content":"a","timestamp":"2020-01-01T00:00:00Z"},
                      {"role":"assistant","content":"b","timestamp":"2020-01-01T00:00:01Z"}]"#;
        let out = normalize_bytes(raw.as_bytes(), "t.json").unwrap();
        assert_eq!(out.graph.events.len(), 2);
        assert_eq!(
            out.graph.events[0].base().timestamp.as_deref(),
            Some("2020-01-01T00:00:00Z")
        );
        assert_eq!(out.outcome, ImportOutcome::Complete);
    }

    #[test]
    fn bom_and_crlf_json_and_markdown_parse() {
        let json = "\u{feff}[\r\n{\"role\":\"user\",\"content\":\"hi\"}\r\n]\r\n";
        let out = normalize_bytes(json.as_bytes(), "bom.json").unwrap();
        assert_eq!(out.graph.events.len(), 1);
        assert_eq!(out.graph.events[0].searchable_text(), "hi");
        let md = "\u{feff}## User\r\nhello\r\n## Assistant\r\nok\r\n";
        let out = normalize_bytes(md.as_bytes(), "bom.md").unwrap();
        assert_eq!(out.graph.events.len(), 2);
        assert_eq!(out.graph.events[0].searchable_text().trim(), "hello");
        assert_eq!(out.graph.events[1].searchable_text().trim(), "ok");
    }

    #[test]
    fn streaming_file_parser_matches_inline_contracts() {
        let tmp = std::env::temp_dir().join(format!("generic-stream-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&tmp).unwrap();
        for (name, raw) in [
            (
                "array.json",
                "\u{feff} [\r\n{\"id\":\"u\",\"role\":\"user\",\"content\":\"你好🚀\",\"timestamp\":\"2020-01-01T00:00:00Z\"},\r\n{\"role\":\"assistant\",\"content\":[{\"text\":\"done\"}]}]\r\n",
            ),
            (
                "object.json",
                "{\"usage\":{\"totalTokens\":42},\"messages\":[{\"author\":\"human\",\"text\":\"q\"},{\"role\":\"bot\",\"message\":\"a\"}]}",
            ),
            (
                "chat.md",
                "\u{feff}\r\n## User\r\nhello\r\n```\r\nAssistant: not a turn\r\n```\r\n## Assistant\r\nok\r\n",
            ),
        ] {
            let path = tmp.join(name);
            std::fs::write(&path, raw.as_bytes()).unwrap();
            let from_file = normalize_file(&path).unwrap();
            let inline = normalize_bytes(raw.as_bytes(), name).unwrap();
            assert_eq!(from_file, inline, "streaming parser drifted for {name}");
        }
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn prepared_spool_matches_inline_events_metadata_and_digest() {
        let home = crate::session_migration::test_home::IsolatedAppHome::acquire("generic-spool");
        let source = home.path.join("array.json");
        let raw = "\u{feff} {\"tokenCount\":17,\"messages\":[{\"id\":\"u\",\"role\":\"human\",\"content\":\"你好🚀\",\"created_at\":\"2020-01-01T00:00:00Z\"},{\"role\":\"unknown\",\"content\":\"skip\"},{\"role\":\"bot\",\"content\":[{\"text\":\"done\"}]}]}";
        std::fs::write(&source, raw.as_bytes()).unwrap();
        let options = serde_json::json!({"rawRetention": "normalized-only"});
        let oracle = normalize_bytes(raw.as_bytes(), "array.json").unwrap();
        let expected_digest = crate::session_migration::hash::normalized_digest(&oracle, &options);

        let (mut actual, prepared) =
            prepare_file_in_root(&source, None, "array.json", &options, None).unwrap();
        assert!(actual.graph.events.is_empty());
        actual.graph.events =
            crate::session_migration::search::load_archive_events(&prepared.prepared.path, &|| {
                false
            })
            .unwrap();
        assert_eq!(actual, oracle);
        assert_eq!(prepared.prepared.digest, expected_digest);
        assert_eq!(prepared.event_headers.len(), actual.graph.events.len());
        assert!(prepared.prepared.path.join("event-index.sqlite").is_file());
    }

    #[test]
    fn prepared_envelope_priority_is_order_independent_and_fail_closed() {
        let home =
            crate::session_migration::test_home::IsolatedAppHome::acquire("generic-envelope");
        let options = serde_json::json!({"rawRetention": "normalized-only"});
        for (index, raw) in [
            r#"{"items":[{"role":"user","content":"items"}],"messages":[{"role":"user","content":"messages"}],"conversation":[{"role":"user","content":"conversation"}]}"#,
            r#"{"messages":[{"role":"user","content":"messages"}],"conversation":[{"role":"user","content":"conversation"}],"items":[{"role":"user","content":"items"}]}"#,
        ]
        .into_iter()
        .enumerate()
        {
            let source = home.path.join(format!("priority-{index}.json"));
            std::fs::write(&source, raw).unwrap();
            let (actual, prepared) = prepare_file_in_root(
                &source,
                None,
                source.file_name().unwrap().to_str().unwrap(),
                &options,
                None,
            )
            .unwrap();
            assert_eq!(actual.inventory.imported.messages, 1);
            let events = crate::session_migration::search::load_archive_events(
                &prepared.prepared.path,
                &|| false,
            )
            .unwrap();
            assert_eq!(events[0].searchable_text(), "messages");
        }

        for (index, raw) in [
            r#"{"messages":{},"conversation":[{"role":"user","content":"must not fall back"}]}"#,
            r#"{"items":[{"role":"user","content":"must not fall back"}],"messages":[]}"#,
            r#"{"messages":[{"role":"unknown","content":"skip"}],"conversation":[{"role":"user","content":"must not fall back"}]}"#,
        ]
        .into_iter()
        .enumerate()
        {
            let source = home.path.join(format!("reject-{index}.json"));
            std::fs::write(&source, raw).unwrap();
            assert_eq!(
                prepare_file_in_root(
                    &source,
                    None,
                    source.file_name().unwrap().to_str().unwrap(),
                    &options,
                    None,
                )
                .unwrap_err()
                .code,
                ErrorCode::FormatUnsupported
            );
        }
        let previews = crate::session_migration::catalog::staging_dir().join("previews");
        assert!(
            !previews.exists() || std::fs::read_dir(previews).unwrap().next().is_none(),
            "failed parses must remove builder and candidate spool directories"
        );
    }

    #[test]
    fn prepared_parser_rejects_oversized_single_event_and_cleans_staging() {
        use std::io::Write;

        let home =
            crate::session_migration::test_home::IsolatedAppHome::acquire("generic-single-limit");
        let source = home.path.join("oversized.json");
        let mut file = std::fs::File::create(&source).unwrap();
        file.write_all(br#"[{"role":"user","content":""#).unwrap();
        let chunk = [b'x'; 1024 * 1024];
        for _ in 0..33 {
            file.write_all(&chunk).unwrap();
        }
        file.write_all(br#""}]"#).unwrap();
        file.sync_all().unwrap();
        let err = prepare_file_in_root(
            &source,
            None,
            "oversized.json",
            &serde_json::json!({"rawRetention":"normalized-only"}),
            None,
        )
        .unwrap_err();
        assert_eq!(err.code, ErrorCode::ResourceLimit);
        let previews = crate::session_migration::catalog::staging_dir().join("previews");
        assert!(
            !previews.exists() || std::fs::read_dir(previews).unwrap().next().is_none(),
            "oversized event must not leave a prepared preview"
        );
    }

    #[test]
    fn prepared_parser_rejects_invalid_utf8_depth_and_duplicate_fields() {
        let home =
            crate::session_migration::test_home::IsolatedAppHome::acquire("generic-red-contracts");
        let options = serde_json::json!({"rawRetention":"normalized-only"});
        let invalid = home.path.join("invalid.json");
        std::fs::write(
            &invalid,
            [b'[', b'{', b'"', b'x', b'"', b':', 0xffu8, b'}', b']'],
        )
        .unwrap();
        assert_eq!(
            prepare_file_in_root(&invalid, None, "invalid.json", &options, None)
                .unwrap_err()
                .code,
            ErrorCode::FormatUnsupported
        );

        let deep = home.path.join("deep.json");
        std::fs::write(&deep, format!("{}0{}", "[".repeat(65), "]".repeat(65))).unwrap();
        assert_eq!(
            prepare_file_in_root(&deep, None, "deep.json", &options, None)
                .unwrap_err()
                .code,
            ErrorCode::ResourceLimit
        );

        for (index, raw) in [
            r#"{"messages":[{"role":"user","content":"a"}],"messages":[{"role":"user","content":"b"}]}"#,
            r#"{"usage":{"total_tokens":1,"total_tokens":2},"messages":[{"role":"user","content":"a"}]}"#,
            r#"{"token_count":1,"token_count":2,"messages":[{"role":"user","content":"a"}]}"#,
            r#"{"metadata":[,],"messages":[{"role":"user","content":"a"}]}"#,
        ]
        .into_iter()
        .enumerate()
        {
            let path = home.path.join(format!("duplicate-{index}.json"));
            std::fs::write(&path, raw).unwrap();
            assert_eq!(
                prepare_file_in_root(
                    &path,
                    None,
                    path.file_name().unwrap().to_str().unwrap(),
                    &options,
                    None,
                )
                    .unwrap_err()
                    .code,
                ErrorCode::FormatUnsupported
            );
        }
        let previews = crate::session_migration::catalog::staging_dir().join("previews");
        assert!(
            !previews.exists() || std::fs::read_dir(previews).unwrap().next().is_none(),
            "red-contract failures must remove all staging directories"
        );
    }

    #[test]
    fn prepared_parser_matches_token_precedence_across_field_orders() {
        let home =
            crate::session_migration::test_home::IsolatedAppHome::acquire("generic-token-order");
        let options = serde_json::json!({"rawRetention":"normalized-only"});
        let variants = [
            r#"{"usage":{"total_tokens":10},"token_count":20,"tokenCount":30,"total_tokens":40,"totalTokens":50,"messages":[{"role":"user","content":"a"}]}"#,
            r#"{"totalTokens":50,"messages":[{"role":"user","content":"a"}],"total_tokens":40,"tokenCount":30,"token_count":20,"usage":{"total_tokens":10}}"#,
            r#"{"usage":{"totalTokens":11},"token_count":20,"tokenCount":30,"total_tokens":40,"totalTokens":50,"messages":[{"role":"user","content":"a"}]}"#,
            r#"{"usage":{"other":99},"token_count":20,"tokenCount":30,"total_tokens":40,"totalTokens":50,"messages":[{"role":"user","content":"a"}]}"#,
        ];
        for (index, raw) in variants.into_iter().enumerate() {
            let path = home.path.join(format!("tokens-{index}.json"));
            std::fs::write(&path, raw).unwrap();
            let oracle = normalize_bytes(raw.as_bytes(), &format!("tokens-{index}.json")).unwrap();
            let (actual, prepared) =
                prepare_file_in_root(&path, None, &format!("tokens-{index}.json"), &options, None)
                    .unwrap();
            assert_eq!(
                actual.inventory.source_tokens, oracle.inventory.source_tokens,
                "{index}"
            );
            let events = crate::session_migration::search::load_archive_events(
                &prepared.prepared.path,
                &|| false,
            )
            .unwrap();
            assert_eq!(events, oracle.graph.events, "{index}");
        }
    }

    #[test]
    fn streaming_file_parser_keeps_limits_and_utf8_errors() {
        let tmp = std::env::temp_dir().join(format!("generic-stream-bad-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&tmp).unwrap();
        let deep = tmp.join("deep.json");
        std::fs::write(&deep, format!("{}0{}", "[".repeat(65), "]".repeat(65))).unwrap();
        assert_eq!(
            normalize_file(&deep).unwrap_err().code,
            ErrorCode::ResourceLimit
        );
        let invalid = tmp.join("invalid.md");
        std::fs::write(&invalid, [b'#', b'#', b' ', 0xff]).unwrap();
        assert_eq!(
            normalize_file(&invalid).unwrap_err().code,
            ErrorCode::FormatUnsupported
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn json_object_keeps_source_token_stats_on_inventory() {
        let raw = r#"{"usage":{"total_tokens":4242},"messages":[{"role":"user","content":"hi"}]}"#;
        let mut out = normalize_bytes(raw.as_bytes(), "usage.json").unwrap();
        assert_eq!(out.inventory.source_tokens, 4242);
        assert!(out.inventory.imported.messages >= 1);
        out.recount();
        assert_eq!(
            out.inventory.source_tokens, 4242,
            "recount must keep original source token stats"
        );
    }

    #[test]
    fn legal_object_schemas_are_accepted() {
        for key in ["messages", "conversation", "items"] {
            let raw = format!(
                r#"{{"{key}":[{{"role":"user","content":"q"}},{{"role":"assistant","content":"a"}}]}}"#
            );
            let out = normalize_bytes(raw.as_bytes(), "schema.json").unwrap();
            assert_eq!(out.graph.events.len(), 2, "{key}");
            assert_eq!(out.evidence.schema_fingerprint, "json-object");
        }
    }

    #[test]
    fn nested_json_over_budget_is_resource_limit() {
        let nested = format!("{}0{}", "[".repeat(65), "]".repeat(65));
        let err = normalize_bytes(nested.as_bytes(), "deep.json").unwrap_err();
        assert_eq!(err.code, ErrorCode::ResourceLimit);
        assert!(ensure_body_budget(SESSION_BODY_MAX_BYTES).is_ok());
        let err = ensure_body_budget(SESSION_BODY_MAX_BYTES + 1).unwrap_err();
        assert_eq!(err.code, ErrorCode::ResourceLimit);
    }
}
