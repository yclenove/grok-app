//! Claude Code JSONL adapter. Directory names are not reversible paths.

use std::path::Path;

use serde_json::Value;

use crate::session_migration::detect::current_platform;
use crate::session_migration::errors::{ErrorCode, ImportError};
use crate::session_migration::jsonl::open_jsonl;
use crate::session_migration::types::{
    AdapterOutput, ArtifactEvidence, AssetRecord, AssetState, ContentBlock, EventBase,
    EventRelation, ImportIssue, ImportOutcome, ImportedEvent, Inventory, MessageRole,
    NormalizedGraph, Provider, RelationConfidence, RelationKind, SourceEvidence, SourceSpan,
    TimeOrigin, ToolInput, ToolOutcome, NORMALIZER_VERSION,
};

#[path = "claude_compaction.rs"]
mod compaction;
#[cfg(test)]
#[path = "claude_compaction_tests.rs"]
mod compaction_tests;

pub fn normalize_file(path: &Path) -> Result<AdapterOutput, ImportError> {
    let mut stream = open_jsonl(path)?;
    let locator = path.to_string_lossy().into_owned();
    let mut native_id = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut cwd = None;
    let mut titles = super::titles::ClaudeTitles::default();
    let mut events = Vec::new();
    let mut relations = Vec::new();
    let mut issues = Vec::new();
    let mut assets = Vec::new();
    let mut ordinal = 0u32;
    let mut last_main: Option<String> = None;
    let mut record_tails = std::collections::HashMap::new();
    let mut record_scopes = std::collections::HashMap::new();
    let mut sidechain_starts = Vec::new();

    for rec in stream.by_ref() {
        let rec = rec?;
        titles.observe(&rec.value);
        let ty = rec.value.get("type").and_then(|x| x.as_str()).unwrap_or("");
        if let Some(s) = rec.value.get("sessionId").and_then(|x| x.as_str()) {
            native_id = s.to_string();
        }
        if cwd.is_none() {
            cwd = rec
                .value
                .get("cwd")
                .and_then(|x| x.as_str())
                .map(|s| s.to_string());
        }
        let uuid = rec
            .value
            .get("uuid")
            .and_then(|x| x.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("claude:row:{}", rec.index));
        let parent = rec
            .value
            .get("parentUuid")
            .and_then(|x| x.as_str())
            .map(|s| s.to_string());
        let ts = rec
            .value
            .get("timestamp")
            .and_then(|x| x.as_str())
            .map(|s| s.to_string());
        let sidechain = rec.value.get("isSidechain").and_then(|x| x.as_bool()) == Some(true);
        let is_meta = rec.value.get("isMeta").and_then(|x| x.as_bool()) == Some(true);
        let record_start = events.len();
        match ty {
            "summary" | "custom-title" | "ai-title" | "last-prompt" => {
                events.push(ImportedEvent::Metadata {
                    base: ev_base(&format!("claude:{uuid}"), ordinal, ts, &locator, rec.index),
                    source_type: ty.into(),
                    payload: None,
                });
                ordinal += 1;
            }
            "user"
                if !is_meta
                    && rec.value.get("isCompactSummary").and_then(Value::as_bool) == Some(true) =>
            {
                let id = format!("claude:{uuid}");
                events.push(compaction::normalize_summary(
                    rec.value
                        .pointer("/message/content")
                        .unwrap_or(&Value::Null),
                    &uuid,
                    ev_base(&id, ordinal, ts, &locator, rec.index),
                    rec.index,
                    &mut assets,
                    &mut issues,
                ));
                if !sidechain {
                    last_main = Some(id);
                }
                ordinal += 1;
            }
            "user" | "assistant" => {
                let msg = rec.value.get("message").unwrap_or(&Value::Null);
                let role = if is_meta {
                    MessageRole::System
                } else if ty == "user" {
                    MessageRole::User
                } else {
                    MessageRole::Assistant
                };
                let parsed = parse_content_blocks(
                    msg.get("content").unwrap_or(&Value::Null),
                    &uuid,
                    &mut assets,
                    &locator,
                    rec.index,
                );
                let mut previous = parent.as_ref().map(|p| format!("claude:{p}"));
                let mut message_id = None;
                if parsed.is_empty() {
                    issues.push(
                        ImportIssue::new("missing-content", "message has no readable content")
                            .with_span(format!("record:{}", rec.index)),
                    );
                }
                for (block_index, part) in parsed.into_iter().enumerate() {
                    let id = if block_index == 0 {
                        format!("claude:{uuid}")
                    } else {
                        format!("claude:{uuid}:block:{block_index}")
                    };
                    let mut base = ev_base(&id, ordinal, ts.clone(), &locator, rec.index);
                    base.spans[0].block = Some(block_index as u32);
                    let event = match part {
                        ParsedPart::Block(block) => {
                            message_id = Some(id.clone());
                            ImportedEvent::Message {
                                base,
                                role,
                                blocks: vec![block],
                                complete: true,
                            }
                        }
                        ParsedPart::ToolUse {
                            call_id,
                            name,
                            input,
                        } => ImportedEvent::ToolCall {
                            base,
                            call_id,
                            name,
                            input: ToolInput::Json { value: input },
                            parent_message_id: message_id.clone(),
                        },
                        ParsedPart::ToolResult {
                            call_id,
                            blocks,
                            outcome,
                        } => ImportedEvent::ToolResult {
                            base,
                            call_id,
                            blocks,
                            outcome,
                        },
                        ParsedPart::Unknown(source_type) => {
                            issues.push(
                                ImportIssue::new("unknown-block", "unsupported content block")
                                    .with_span(format!("record:{}:block:{block_index}", rec.index)),
                            );
                            ImportedEvent::Unsupported {
                                base,
                                source_type,
                                payload: None,
                            }
                        }
                    };
                    events.push(event);
                    if let Some(p) = &previous {
                        relations.push(EventRelation {
                            kind: RelationKind::Parent,
                            from: id.clone(),
                            to: p.clone(),
                            confidence: RelationConfidence::Explicit,
                        });
                    }
                    previous = Some(id);
                    ordinal += 1;
                }
                if let Some(last) = events.get(record_start..).and_then(|slice| slice.last()) {
                    if !sidechain && !is_meta {
                        last_main = Some(last.id().to_string());
                    }
                }
            }
            "progress" | "system" => {
                let compact_boundary = ty == "system"
                    && rec.value.get("subtype").and_then(Value::as_str) == Some("compact_boundary");
                events.push(ImportedEvent::Metadata {
                    base: ev_base(&format!("claude:{uuid}"), ordinal, ts, &locator, rec.index),
                    source_type: if compact_boundary {
                        "compact_boundary"
                    } else {
                        ty
                    }
                    .into(),
                    payload: None,
                });
                if compact_boundary {
                    if let Some(logical_parent) = rec
                        .value
                        .get("logicalParentUuid")
                        .and_then(Value::as_str)
                        .filter(|id| !id.is_empty())
                    {
                        // Retain the source's historical link without reconnecting
                        // the live Parent chain or inventing summary coverage.
                        relations.push(EventRelation {
                            kind: RelationKind::Compacts,
                            from: format!("claude:{uuid}"),
                            to: format!("claude:{logical_parent}"),
                            confidence: RelationConfidence::Explicit,
                        });
                    }
                }
                ordinal += 1;
            }
            other if !other.is_empty() => {
                events.push(ImportedEvent::Unsupported {
                    base: ev_base(&format!("claude:{uuid}"), ordinal, ts, &locator, rec.index),
                    source_type: other.into(),
                    payload: None,
                });
                issues.push(
                    ImportIssue::new("unknown-record", format!("claude type {other}"))
                        .with_span(format!("record:{}", rec.index)),
                );
                ordinal += 1;
            }
            _ => {}
        }
        if let Some(first) = events.get(record_start) {
            let first_id = first.id().to_string();
            if !relations
                .iter()
                .rev()
                .take(events.len() - record_start)
                .any(|r| r.kind == RelationKind::Parent && r.from == first_id)
            {
                if let Some(p) = &parent {
                    relations.push(EventRelation {
                        kind: RelationKind::Parent,
                        from: first_id.clone(),
                        to: format!("claude:{p}"),
                        confidence: RelationConfidence::Explicit,
                    });
                }
            }
            record_tails.insert(
                format!("claude:{uuid}"),
                events.last().unwrap().id().to_string(),
            );
            record_scopes.insert(uuid.clone(), sidechain);
            if sidechain {
                sidechain_starts.push((first_id, parent));
            }
        }
    }

    // A source UUID names the entire record. Its next child follows the last
    // content block, including tool-only records and mixed result/text records.
    for relation in &mut relations {
        if let Some(tail) = record_tails.get(&relation.to) {
            // Internal block links already point to a precise preceding block.
            let same_record = relation
                .from
                .starts_with(&format!("{}:block:", relation.to));
            if !same_record {
                relation.to = tail.clone();
            }
        }
    }
    for (first, parent) in sidechain_starts {
        if let Some(parent) = parent {
            if record_scopes.get(&parent) != Some(&true) {
                let raw = format!("claude:{parent}");
                relations.push(EventRelation {
                    kind: RelationKind::Subagent,
                    from: first,
                    to: record_tails.get(&raw).cloned().unwrap_or(raw),
                    confidence: RelationConfidence::Explicit,
                });
            }
        }
    }
    if events.is_empty() {
        return Err(ImportError::new(
            ErrorCode::FormatUnsupported,
            "Claude JSONL had no readable records",
        ));
    }
    let prefix = stream.finish()?;
    issues.extend(prefix.issues);
    let title = titles.finish().unwrap_or_else(|| native_id.clone());
    let mut out = AdapterOutput {
        evidence: SourceEvidence {
            provider: Provider::ClaudeCode,
            source_version: None,
            schema_fingerprint: "claude-code-jsonl-v1".into(),
            adapter_version: NORMALIZER_VERSION.into(),
            platform: current_platform(),
            artifacts: vec![ArtifactEvidence {
                artifact_id: locator.clone(),
                kind: "jsonl".into(),
                relative_locator: locator,
                byte_length: prefix.byte_length,
                sha256: prefix.file_sha256.clone(),
                complete_prefix_sha256: Some(prefix.complete_prefix_sha256.clone()),
            }],
        },
        graph: NormalizedGraph {
            events,
            relations,
            active_leaf: last_main,
            native_session_id: native_id,
            title: Some(title),
            cwd,
        },
        inventory: Inventory::default(),
        issues,
        assets,
        outcome: if prefix.trailing_incomplete {
            ImportOutcome::Partial
        } else {
            ImportOutcome::Complete
        },
    };
    out.validate_graph();
    Ok(out)
}

enum ParsedPart {
    Block(ContentBlock),
    ToolUse {
        call_id: String,
        name: String,
        input: Value,
    },
    ToolResult {
        call_id: String,
        blocks: Vec<ContentBlock>,
        outcome: ToolOutcome,
    },
    Unknown(String),
}

fn parse_content_blocks(
    content: &Value,
    uuid: &str,
    assets: &mut Vec<AssetRecord>,
    locator: &str,
    index: u32,
) -> Vec<ParsedPart> {
    if let Some(s) = content.as_str() {
        return vec![ParsedPart::Block(ContentBlock::text(s))];
    }
    let Some(arr) = content.as_array() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (bi, b) in arr.iter().enumerate() {
        let ty = b.get("type").and_then(|x| x.as_str()).unwrap_or("");
        match ty {
            "text" => {
                if let Some(t) = b.get("text").and_then(|x| x.as_str()) {
                    out.push(ParsedPart::Block(ContentBlock::text(t)));
                }
            }
            "thinking" => {
                if let Some(t) = b
                    .get("thinking")
                    .or_else(|| b.get("text"))
                    .and_then(|x| x.as_str())
                {
                    out.push(ParsedPart::Block(ContentBlock::VisibleReasoning {
                        text: t.to_string(),
                    }));
                } else {
                    out.push(ParsedPart::Unknown("encrypted-thinking".into()));
                }
            }
            "redacted_thinking" => out.push(ParsedPart::Unknown("redacted-thinking".into())),
            "tool_use" => {
                let call_id = b
                    .get("id")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string();
                let name = b
                    .get("name")
                    .and_then(|x| x.as_str())
                    .unwrap_or("tool")
                    .to_string();
                let input = b.get("input").cloned().unwrap_or(Value::Null);
                out.push(ParsedPart::ToolUse {
                    call_id,
                    name,
                    input,
                });
            }
            "tool_result" => {
                let call_id = b
                    .get("tool_use_id")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string();
                let is_err = b.get("is_error").and_then(|x| x.as_bool()).unwrap_or(false);
                let blocks = match &b.get("content") {
                    Some(Value::String(s)) => vec![ContentBlock::text(s)],
                    Some(Value::Array(arr)) => arr
                        .iter()
                        .filter_map(|x| {
                            x.get("text")
                                .and_then(|t| t.as_str())
                                .map(ContentBlock::text)
                        })
                        .collect(),
                    _ => vec![ContentBlock::text("")],
                };
                out.push(ParsedPart::ToolResult {
                    call_id,
                    blocks,
                    outcome: if is_err {
                        ToolOutcome::Error
                    } else {
                        ToolOutcome::Success
                    },
                });
            }
            "image" => {
                let asset_id = format!("{uuid}:img:{bi}");
                let mime = b
                    .pointer("/source/media_type")
                    .and_then(|x| x.as_str())
                    .map(|s| s.to_string());
                let source_ref =
                    if let Some(data) = b.pointer("/source/data").and_then(|x| x.as_str()) {
                        let mime = mime
                            .clone()
                            .unwrap_or_else(|| "application/octet-stream".into());
                        Some(format!("data:{mime};base64,{data}"))
                    } else if let Some(url) = b.pointer("/source/url").and_then(|x| x.as_str()) {
                        Some(format!("url:{url}"))
                    } else {
                        b.pointer("/source/path")
                            .and_then(|x| x.as_str())
                            .map(|p| format!("path:{p}"))
                    };
                let state = if source_ref.is_none() {
                    AssetState::Missing
                } else {
                    AssetState::Unresolved
                };
                assets.push(AssetRecord {
                    asset_id: asset_id.clone(),
                    digest: None,
                    mime,
                    size: 0,
                    state,
                    source_span: SourceSpan {
                        artifact_id: locator.into(),
                        record: format!("record:{index}"),
                        block: Some(bi as u32),
                    },
                    source_ref,
                });
                out.push(ParsedPart::Block(ContentBlock::Asset {
                    asset_id,
                    alt: Some("image".into()),
                }));
            }
            other => out.push(ParsedPart::Unknown(other.into())),
        }
    }
    out
}

fn ev_base(id: &str, ordinal: u32, ts: Option<String>, locator: &str, index: u32) -> EventBase {
    EventBase {
        id: id.to_string(),
        ordinal,
        spans: vec![SourceSpan {
            artifact_id: locator.into(),
            record: format!("record:{index}"),
            block: None,
        }],
        timestamp: ts.clone(),
        time_origin: if ts.is_some() {
            TimeOrigin::Source
        } else {
            TimeOrigin::Unknown
        },
    }
}

/// Directory names like `-Users-me-work` are irreversible. Never treat `-` as `/`.
#[cfg(test)]
pub fn directory_name_is_not_a_path(dir_name: &str) -> bool {
    dir_name.starts_with('-')
        || dir_name.contains('-') && !dir_name.contains('/') && !dir_name.contains('\\')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encoded_dir_is_not_decoded() {
        assert!(directory_name_is_not_a_path("-Users-me-Documents-project"));
        let fake = Path::new("-Users-me-Documents-project");
        assert_eq!(
            fake.to_string_lossy().replace('-', "/"),
            "/Users/me/Documents/project"
        );
        // Adapter must not perform that replacement as a cwd.
    }

    #[test]
    fn tool_result_is_not_a_user_bubble() {
        let raw = r#"{"type":"user","uuid":"u1","parentUuid":null,"sessionId":"s","timestamp":"2026-01-01T00:00:00Z","cwd":"/synth/repo","message":{"role":"user","content":"hello"}}
{"type":"assistant","uuid":"a1","parentUuid":"u1","sessionId":"s","timestamp":"2026-01-01T00:00:01Z","message":{"role":"assistant","content":[{"type":"text","text":"working"},{"type":"tool_use","id":"t1","name":"Read","input":{"path":"a.txt"}}]}}
{"type":"user","uuid":"r1","parentUuid":"a1","sessionId":"s","timestamp":"2026-01-01T00:00:02Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"file ok"}]}}
{"type":"assistant","uuid":"a2","parentUuid":"r1","sessionId":"s","timestamp":"2026-01-01T00:00:03Z","message":{"role":"assistant","content":[{"type":"text","text":"done"}]}}
"#;
        let tmp = std::env::temp_dir().join("claude-g1.jsonl");
        std::fs::write(&tmp, raw).unwrap();
        let out = normalize_file(&tmp).unwrap();
        let user_msgs = out
            .graph
            .events
            .iter()
            .filter(|e| {
                matches!(
                    e,
                    ImportedEvent::Message {
                        role: MessageRole::User,
                        ..
                    }
                )
            })
            .count();
        let assistant_msgs = out
            .graph
            .events
            .iter()
            .filter(|e| {
                matches!(
                    e,
                    ImportedEvent::Message {
                        role: MessageRole::Assistant,
                        ..
                    }
                )
            })
            .count();
        let calls = out
            .graph
            .events
            .iter()
            .filter(|e| matches!(e, ImportedEvent::ToolCall { .. }))
            .count();
        let results = out
            .graph
            .events
            .iter()
            .filter(|e| matches!(e, ImportedEvent::ToolResult { .. }))
            .count();
        assert_eq!(user_msgs, 1);
        assert_eq!(assistant_msgs, 2);
        assert_eq!(calls, 1);
        assert_eq!(results, 1);
        let result_is_user = out.graph.events.iter().any(|e| {
            matches!(e, ImportedEvent::Message { role: MessageRole::User, blocks, .. } if blocks.iter().any(|b| b.plain_text() == Some("file ok")))
        });
        assert!(!result_is_user);
        assert_eq!(out.outcome, ImportOutcome::Complete, "{:?}", out.issues);
        let chain = crate::session_migration::graph::lineage(
            &out.graph.events,
            &out.graph.relations,
            out.graph.active_leaf.as_deref().unwrap(),
        );
        assert_eq!(chain.len(), 5);
        assert_eq!(chain[2].kind_name(), "tool-call");
        assert_eq!(chain[3].kind_name(), "tool-result");
    }

    #[test]
    fn tool_only_and_mixed_tool_result_records_keep_order_and_parent_identity() {
        let tmp = std::env::temp_dir().join(format!("claude-mixed-{}.jsonl", uuid::Uuid::new_v4()));
        let raw = concat!(
            "{\"type\":\"assistant\",\"uuid\":\"a\",\"message\":{\"content\":[{\"type\":\"tool_use\",\"id\":\"t\",\"name\":\"Read\",\"input\":{}}]}}\n",
            "{\"type\":\"user\",\"uuid\":\"r\",\"parentUuid\":\"a\",\"message\":{\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"t\",\"content\":\"result\"},{\"type\":\"text\",\"text\":\"another instruction\"}]}}\n",
            "{\"type\":\"assistant\",\"uuid\":\"end\",\"parentUuid\":\"r\",\"message\":{\"content\":\"done\"}}\n"
        );
        std::fs::write(&tmp, raw).unwrap();
        let out = normalize_file(&tmp).unwrap();
        std::fs::remove_file(tmp).unwrap();
        assert_eq!(out.outcome, ImportOutcome::Complete, "{:?}", out.issues);
        let chain = crate::session_migration::graph::lineage(
            &out.graph.events,
            &out.graph.relations,
            "claude:end",
        );
        assert_eq!(
            chain.iter().map(|e| e.kind_name()).collect::<Vec<_>>(),
            vec!["tool-call", "tool-result", "message", "message"]
        );
        assert_eq!(chain[1].searchable_text(), "result");
        assert_eq!(chain[2].searchable_text(), "another instruction");
    }

    #[test]
    fn image_block_is_not_stored_until_bytes_are_copied() {
        let tmp = std::env::temp_dir().join(format!("claude-img-{}.jsonl", uuid::Uuid::new_v4()));
        let raw = r#"{"type":"user","uuid":"u1","message":{"content":[{"type":"image","source":{"path":"C:\\gone\\pic.png","media_type":"image/png"}}]}}"#;
        std::fs::write(&tmp, raw).unwrap();
        let out = normalize_file(&tmp).unwrap();
        let _ = std::fs::remove_file(&tmp);
        assert_eq!(out.assets.len(), 1);
        assert_ne!(out.assets[0].state, AssetState::Stored);
        assert!(out.assets[0].digest.is_none());
        assert_eq!(
            out.assets[0].source_ref.as_deref(),
            Some("path:C:\\gone\\pic.png")
        );
    }

    #[test]
    fn summary_title_sidechain_stays_off_main_lineage() {
        let tmp = std::env::temp_dir().join(format!("claude-side-{}.jsonl", uuid::Uuid::new_v4()));
        let raw = concat!(
            "{\"type\":\"summary\",\"summary\":\"named chat\",\"uuid\":\"sum\"}\n",
            "{\"type\":\"user\",\"uuid\":\"u1\",\"message\":{\"content\":\"main\"}}\n",
            "{\"type\":\"assistant\",\"uuid\":\"side\",\"parentUuid\":\"u1\",\"isSidechain\":true,\"message\":{\"content\":\"secret-side\"}}\n",
            "{\"type\":\"assistant\",\"uuid\":\"a1\",\"parentUuid\":\"u1\",\"message\":{\"content\":\"main-answer\"}}\n"
        );
        std::fs::write(&tmp, raw).unwrap();
        let out = normalize_file(&tmp).unwrap();
        let _ = std::fs::remove_file(&tmp);
        assert_eq!(out.graph.title.as_deref(), Some("named chat"));
        assert!(out
            .graph
            .events
            .iter()
            .any(|e| e.searchable_text() == "secret-side"));
        assert!(out
            .graph
            .relations
            .iter()
            .any(|r| r.kind == RelationKind::Subagent));
        let main = crate::session_migration::graph::lineage(
            &out.graph.events,
            &out.graph.relations,
            out.graph.active_leaf.as_deref().unwrap(),
        );
        assert!(
            !main.iter().any(|e| e.searchable_text() == "secret-side"),
            "default lineage must not authorize sidechain text"
        );
        assert!(main.iter().any(|e| e.searchable_text() == "main-answer"));
    }

    #[test]
    fn missing_parent_and_encrypted_thinking_are_partial_without_secrets() {
        let tmp = std::env::temp_dir().join(format!("claude-f11-{}.jsonl", uuid::Uuid::new_v4()));
        let raw = concat!(
            "{\"type\":\"user\",\"uuid\":\"u1\",\"parentUuid\":\"ghost\",\"message\":{\"content\":\"hi\"}}\n",
            "{\"type\":\"assistant\",\"uuid\":\"a1\",\"parentUuid\":\"u1\",\"message\":{\"content\":[{\"type\":\"redacted_thinking\"},{\"type\":\"thinking\",\"encrypted\":\"SECRET-BLOB\"},{\"type\":\"text\",\"text\":\"ok\"}]}}\n"
        );
        std::fs::write(&tmp, raw).unwrap();
        let out = normalize_file(&tmp).unwrap();
        let _ = std::fs::remove_file(&tmp);
        assert_eq!(out.outcome, ImportOutcome::Partial);
        assert!(out.issues.iter().any(|i| i.code == "missing-parent"));
        assert!(out.graph.events.iter().any(|e| matches!(
            e,
            ImportedEvent::Unsupported { source_type, .. } if source_type.contains("thinking")
        )));
        let dump = serde_json::to_string(&out.graph).unwrap();
        assert!(
            !dump.contains("SECRET-BLOB"),
            "encrypted thinking must not be copied: {dump}"
        );
    }

    #[test]
    fn sibling_same_call_id_does_not_pair_across_branches() {
        let tmp = std::env::temp_dir().join(format!("claude-sib-{}.jsonl", uuid::Uuid::new_v4()));
        let raw = concat!(
            "{\"type\":\"user\",\"uuid\":\"u1\",\"message\":{\"content\":\"go\"}}\n",
            "{\"type\":\"assistant\",\"uuid\":\"a1\",\"parentUuid\":\"u1\",\"message\":{\"content\":[{\"type\":\"tool_use\",\"id\":\"t1\",\"name\":\"Read\",\"input\":{}}]}}\n",
            "{\"type\":\"assistant\",\"uuid\":\"a2\",\"parentUuid\":\"u1\",\"message\":{\"content\":[{\"type\":\"tool_use\",\"id\":\"t1\",\"name\":\"Read\",\"input\":{}}]}}\n",
            "{\"type\":\"user\",\"uuid\":\"r1\",\"parentUuid\":\"a1\",\"message\":{\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"t1\",\"content\":\"from-a1\"}]}}\n"
        );
        std::fs::write(&tmp, raw).unwrap();
        let out = normalize_file(&tmp).unwrap();
        let _ = std::fs::remove_file(&tmp);
        let report =
            crate::session_migration::graph::validate(&out.graph.events, &out.graph.relations);
        assert!(
            report.unpaired_calls >= 1,
            "sibling call must stay unpaired: {:?}",
            report.issues
        );
        let a1_line = crate::session_migration::graph::lineage(
            &out.graph.events,
            &out.graph.relations,
            "claude:r1",
        );
        assert!(a1_line.iter().any(|e| e.searchable_text() == "from-a1"));
        assert!(
            !a1_line.iter().any(|e| e.id() == "claude:a2"),
            "result branch must not include the sibling call"
        );
    }

    #[test]
    fn encoded_session_directory_is_not_used_as_cwd() {
        let root = std::env::temp_dir().join(format!("claude-enc-{}", uuid::Uuid::new_v4()));
        let dir = root.join("-Users-me-Documents-project");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("s.jsonl");
        std::fs::write(
            &path,
            r#"{"type":"user","uuid":"u1","sessionId":"s","message":{"content":"hi"}}"#,
        )
        .unwrap();
        let out = normalize_file(&path).unwrap();
        assert!(
            out.graph.cwd.is_none(),
            "must not invent cwd from {}: {:?}",
            dir.display(),
            out.graph.cwd
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
