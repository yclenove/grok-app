//! Codex legacy JSONL rollout adapter.

use std::path::Path;
use std::rc::Rc;

use serde_json::Value;

#[path = "codex_content.rs"]
mod content;
#[path = "codex_history_base.rs"]
pub(crate) mod history_base;
#[path = "codex_projection.rs"]
mod projection_content;
use content::text as content_text;
use projection_content::{merge as merge_projection, Projection};

use crate::session_migration::detect::current_platform;
use crate::session_migration::errors::{ErrorCode, ImportError};
#[cfg(test)]
use crate::session_migration::hash::sha256_bytes;
use crate::session_migration::jsonl::{open_jsonl, JsonlPrefix, JsonlRecord};
#[cfg(test)]
use crate::session_migration::types::SourceCandidate;
use crate::session_migration::types::{
    AdapterOutput, ArtifactEvidence, ContentBlock, EventBase, EventRelation, ImportIssue,
    ImportOutcome, ImportedEvent, Inventory, MessageRole, NormalizedGraph, Provider,
    RelationConfidence, RelationKind, SourceEvidence, SourceSpan, TimeOrigin, ToolInput,
};

#[cfg(test)]
pub fn discover(root: &Path) -> Result<Vec<SourceCandidate>, ImportError> {
    let mut out = Vec::new();
    for (dir, archived) in [
        (root.join("sessions"), false),
        (root.join("archived_sessions"), true),
    ] {
        collect_jsonl(&dir, archived, &mut out);
    }
    if out.is_empty() {
        collect_jsonl(root, false, &mut out);
    }
    Ok(out)
}

#[cfg(test)]
fn collect_jsonl(dir: &Path, archived: bool, out: &mut Vec<SourceCandidate>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for ent in rd.flatten() {
        let p = ent.path();
        if p.is_dir() {
            collect_jsonl(&p, archived, out);
            continue;
        }
        if p.extension().and_then(|s| s.to_str()) != Some("jsonl") {
            continue;
        }
        if let Ok(c) = candidate_from_file(&p, archived) {
            out.push(c);
        }
    }
}

#[cfg(test)]
fn candidate_from_file(path: &Path, archived: bool) -> Result<SourceCandidate, ImportError> {
    let mut stream = open_jsonl(path)?;
    let mut id = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "codex".into());
    let mut cwd = None;
    let mut started = None;
    let mut title = None;
    for rec in stream.by_ref() {
        let rec = rec?;
        if rec.value.get("type").and_then(|x| x.as_str()) == Some("session_meta") {
            if let Some(p) = rec.value.get("payload") {
                if let Some(s) = p.get("id").and_then(|x| x.as_str()) {
                    id = s.to_string();
                }
                cwd = p.get("cwd").and_then(|x| x.as_str()).map(|s| s.to_string());
                started = p
                    .get("timestamp")
                    .and_then(|x| x.as_str())
                    .map(|s| s.to_string())
                    .or_else(|| {
                        rec.value
                            .get("timestamp")
                            .and_then(|x| x.as_str())
                            .map(|s| s.to_string())
                    });
            }
        }
        if title.is_none() {
            title = first_user_title(&rec.value);
        }
    }
    let prefix = stream.finish()?;
    let title = title.unwrap_or_else(|| id.clone());
    Ok(SourceCandidate {
        candidate_id: format!("codex:{}", path.to_string_lossy()),
        provider: Provider::Codex,
        native_session_id: id,
        title,
        cwd,
        started_at: started.clone(),
        updated_at: started,
        locator: path.to_string_lossy().into_owned(),
        source_revision: prefix.complete_prefix_sha256,
        archived: Some(archived),
        completeness: if prefix.trailing_incomplete {
            "prefix".into()
        } else {
            "body".into()
        },
    })
}

fn first_user_text(v: &Value) -> Option<String> {
    let payload = v.get("payload").unwrap_or(v);
    if payload.get("role").and_then(|x| x.as_str()) == Some("user") {
        if content::injected_context(payload) {
            return None;
        }
        return content::title_text(payload);
    }
    if payload.get("type").and_then(|x| x.as_str()) == Some("user_message") {
        return payload
            .get("message")
            .and_then(|x| x.as_str())
            .map(|s| s.to_string());
    }
    None
}

pub(crate) fn first_user_title(v: &Value) -> Option<String> {
    let text = first_user_text(v)?;
    let title: String = text
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .filter(|c| !c.is_control())
        .take(80)
        .collect();
    (!title.is_empty()).then_some(title)
}

pub fn normalize_file(path: &Path) -> Result<AdapterOutput, ImportError> {
    let mut stream = open_jsonl(path)?;
    let mut out = normalize_records(stream.by_ref(), path, &JsonlPrefix::default())?;
    let prefix = stream.finish()?;
    let artifact = &mut out.evidence.artifacts[0];
    artifact.byte_length = prefix.byte_length;
    artifact.sha256 = prefix.file_sha256;
    artifact.complete_prefix_sha256 = Some(prefix.complete_prefix_sha256);
    out.issues.extend(prefix.issues);
    out.validate_graph();
    Ok(out)
}

pub use history_base::normalize_file_in_root;

/// Codex evidence version. 1.0.7 (2026-10-21): array/object tool outputs,
/// injected context as system events, resume branches with history_base.
/// The global NORMALIZER_VERSION stays 1.0.6 because the Cursor identity
/// association protocol pins it; re-imports still get a new revision because
/// the normalized content digest changes.
pub const CODEX_ADAPTER_VERSION: &str = "1.0.7";

pub fn normalize_records(
    records: impl IntoIterator<Item = Result<JsonlRecord, ImportError>>,
    path: &Path,
    prefix: &crate::session_migration::jsonl::JsonlPrefix,
) -> Result<AdapterOutput, ImportError> {
    let locator: Rc<str> = path.to_string_lossy().into();
    normalize_sourced(
        records
            .into_iter()
            .map(move |record| record.map(|record| (locator.clone(), 0, record))),
        path,
        prefix,
    )
}

/// Records carry their own artifact (spans) and an id offset: a resume branch
/// numbers its records after the parent prefix so event ids stay unique, while
/// spans keep each file's physical record index. Plain files use offset 0.
fn normalize_sourced(
    records: impl IntoIterator<Item = Result<(Rc<str>, u32, JsonlRecord), ImportError>>,
    path: &Path,
    prefix: &crate::session_migration::jsonl::JsonlPrefix,
) -> Result<AdapterOutput, ImportError> {
    let primary_locator = path.to_string_lossy().into_owned();
    let mut native_id = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "codex".into());
    let mut cwd = None;
    let mut source_version = None;
    let mut title = None;
    let mut events = Vec::new();
    let mut assets = Vec::new();
    let mut relations = Vec::new();
    let mut issues = prefix.issues.clone();
    let mut prev: Option<String> = None;
    let mut ordinal = 0u32;
    let mut projection: Option<Projection> = None;
    let mut active_leaf = None;
    let mut fork_from: Option<String> = None;
    let mut user_turn_starts: Vec<Option<String>> = Vec::new();

    for item in records {
        let (source, id_offset, rec) = item?;
        let locator: &str = &source;
        let id_index = rec.index + id_offset;
        // A resume branch is titled by its own turns, as discovery lists it.
        if title.is_none() && locator == primary_locator {
            title = first_user_title(&rec.value);
        }
        let ty = rec.value.get("type").and_then(|x| x.as_str()).unwrap_or("");
        let payload = rec.value.get("payload").cloned().unwrap_or(Value::Null);
        let event_type = payload.get("type").and_then(Value::as_str).unwrap_or("");
        let can_pair = (ty == "event_msg"
            && matches!(
                event_type,
                "user_message" | "agent_message" | "agent_message_delta"
            ))
            || (ty == "response_item" && event_type == "message");
        if !can_pair {
            projection = None;
        }
        let ts = rec
            .value
            .get("timestamp")
            .and_then(|x| x.as_str())
            .map(|s| s.to_string());
        let before = prev.clone();
        let first_new_event = events.len();
        let ty = if ty == "event_msg" && event_type == "thread_rolled_back" {
            "thread_rolled_back"
        } else {
            ty
        };
        match ty {
            "session_meta" => {
                if let Some(s) = payload.get("id").and_then(|x| x.as_str()) {
                    let stem = Path::new(locator)
                        .file_stem()
                        .map(|stem| stem.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    native_id =
                        history_base::branch_identity(&stem, &payload).unwrap_or_else(|| s.into());
                }
                cwd = payload
                    .get("cwd")
                    .and_then(|x| x.as_str())
                    .map(|s| s.to_string());
                source_version = payload
                    .get("cli_version")
                    .and_then(|x| x.as_str())
                    .map(|s| s.to_string());
            }
            "event_msg" => {
                let pty = payload.get("type").and_then(|x| x.as_str()).unwrap_or("");
                if pty == "agent_message_delta" {
                    continue;
                }
                if matches!(pty, "agent_message" | "user_message") {
                    let role = if pty == "user_message" {
                        MessageRole::User
                    } else {
                        MessageRole::Assistant
                    };
                    let mut content =
                        content::event(&payload, locator, &format!("record:{}", rec.index));
                    if content.blocks.is_empty() {
                        projection = None;
                        continue;
                    }
                    if merge_projection(
                        &mut events,
                        &mut projection,
                        true,
                        role,
                        &mut content,
                        &payload,
                        locator,
                        rec.index,
                        &mut assets,
                        &mut issues,
                    ) {
                        continue;
                    }
                    projection = Some(Projection::new(
                        events.len(),
                        true,
                        &payload,
                        assets.len(),
                        &content,
                    ));
                    push_message(
                        &mut events,
                        &mut relations,
                        &mut prev,
                        &mut ordinal,
                        &mut active_leaf,
                        role,
                        &mut content,
                        ts.clone(),
                        locator,
                        rec.index,
                        format!("event:{}", id_index),
                    );
                    assets.append(&mut content.assets);
                    issues.append(&mut content.issues);
                } else if pty == "task_complete" || pty == "turn_aborted" {
                    events.push(ImportedEvent::Metadata {
                        base: base_ev(
                            format!("codex:meta:{}", id_index),
                            ordinal,
                            ts.clone(),
                            locator,
                            rec.index,
                        ),
                        source_type: pty.into(),
                        payload: None,
                    });
                    ordinal += 1;
                }
            }
            "response_item" => {
                let pty = payload.get("type").and_then(|x| x.as_str()).unwrap_or("");
                match pty {
                    "message" => {
                        let role = payload.get("role").and_then(|x| x.as_str()).unwrap_or("");
                        let Some(mut mrole) = map_role(role) else {
                            continue;
                        };
                        if mrole == MessageRole::User && content::injected_context(&payload) {
                            mrole = MessageRole::System;
                        }
                        let mut content =
                            content::response(&payload, locator, &format!("record:{}", rec.index));
                        if content.blocks.is_empty() {
                            projection = None;
                            continue;
                        }
                        let item_id = payload
                            .get("id")
                            .and_then(|x| x.as_str())
                            .map(|s| s.to_string())
                            .unwrap_or_else(|| format!("resp:{}", id_index));
                        if merge_projection(
                            &mut events,
                            &mut projection,
                            false,
                            mrole,
                            &mut content,
                            &payload,
                            locator,
                            rec.index,
                            &mut assets,
                            &mut issues,
                        ) {
                            continue;
                        }
                        projection = Some(Projection::new(
                            events.len(),
                            false,
                            &payload,
                            assets.len(),
                            &content,
                        ));
                        push_message(
                            &mut events,
                            &mut relations,
                            &mut prev,
                            &mut ordinal,
                            &mut active_leaf,
                            mrole,
                            &mut content,
                            ts.clone(),
                            locator,
                            rec.index,
                            item_id.clone(),
                        );
                        assets.append(&mut content.assets);
                        issues.append(&mut content.issues);
                    }
                    "function_call" | "custom_tool_call" => {
                        let call_id = payload
                            .get("call_id")
                            .or_else(|| payload.get("id"))
                            .and_then(|x| x.as_str())
                            .unwrap_or("")
                            .to_string();
                        let name = payload
                            .get("name")
                            .and_then(|x| x.as_str())
                            .unwrap_or("tool")
                            .to_string();
                        // custom_tool_call (codex-cli 0.160) carries a string `input`.
                        let args = payload
                            .get("arguments")
                            .or_else(|| payload.get("input"))
                            .cloned()
                            .unwrap_or(Value::Null);
                        let args = if let Some(s) = args.as_str() {
                            serde_json::from_str(s).unwrap_or(Value::String(s.to_string()))
                        } else {
                            args
                        };
                        let id = format!("codex:call:{call_id}");
                        events.push(ImportedEvent::ToolCall {
                            base: base_ev(id.clone(), ordinal, ts.clone(), locator, rec.index),
                            call_id,
                            name,
                            input: ToolInput::Json { value: args },
                            parent_message_id: prev.clone(),
                        });
                        link_parent(&mut relations, &id, &prev);
                        prev = Some(id.clone());
                        active_leaf = Some(id);
                        ordinal += 1;
                    }
                    "function_call_output" | "custom_tool_call_output" => {
                        let call_id = payload
                            .get("call_id")
                            .and_then(|x| x.as_str())
                            .unwrap_or("")
                            .to_string();
                        let (mut output, outcome) = content::tool_output(
                            &payload,
                            locator,
                            &format!("record:{}", rec.index),
                        );
                        let id = format!("codex:result:{call_id}:{}", id_index);
                        events.push(ImportedEvent::ToolResult {
                            base: base_ev(id.clone(), ordinal, ts.clone(), locator, rec.index),
                            call_id,
                            blocks: std::mem::take(&mut output.blocks),
                            outcome,
                        });
                        assets.append(&mut output.assets);
                        issues.append(&mut output.issues);
                        link_parent(&mut relations, &id, &prev);
                        prev = Some(id.clone());
                        active_leaf = Some(id);
                        ordinal += 1;
                    }
                    other => {
                        events.push(ImportedEvent::Unsupported {
                            base: base_ev(
                                format!("codex:unsup:{}", id_index),
                                ordinal,
                                ts.clone(),
                                locator,
                                rec.index,
                            ),
                            source_type: other.into(),
                            payload: None,
                        });
                        issues.push(
                            ImportIssue::new(
                                "unsupported-item",
                                format!("response_item type {other}"),
                            )
                            .with_span(format!("record:{}", rec.index)),
                        );
                        ordinal += 1;
                    }
                }
            }
            "compacted" | "compaction" | "context_compaction" => {
                let replacement = payload.get("replacement_history").and_then(Value::as_array);
                let mut summary = payload
                    .get("message")
                    .or_else(|| payload.get("summary"))
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string();
                if summary.is_empty() {
                    summary = replacement
                        .and_then(|items| items.iter().rev().find_map(content_text))
                        .unwrap_or_else(|| "[compaction]".into());
                }
                let coverage_known = payload.get("replacement_history").is_some();
                let covered_event_ids = coverage_known
                    .then(|| events.iter().map(|event| event.id().to_string()).collect());
                let id = format!("codex:compact:{}", id_index);
                events.push(ImportedEvent::Compaction {
                    base: base_ev(id.clone(), ordinal, ts.clone(), locator, rec.index),
                    summary: vec![ContentBlock::text(summary.clone())],
                    covered_event_ids,
                    coverage_known,
                });
                if let Some(p) = &prev {
                    relations.push(EventRelation {
                        kind: RelationKind::Compacts,
                        from: id.clone(),
                        to: p.clone(),
                        confidence: RelationConfidence::Inferred,
                    });
                }
                if !coverage_known {
                    link_parent(&mut relations, &id, &prev);
                }
                prev = Some(id.clone());
                active_leaf = Some(id.clone());
                ordinal += 1;
                if let Some(items) = replacement {
                    apply_replacement_history(
                        items,
                        &summary,
                        rec.index,
                        ts.clone(),
                        locator,
                        &mut events,
                        &mut relations,
                        &mut prev,
                        &mut ordinal,
                        &mut active_leaf,
                        &mut assets,
                        &mut issues,
                    );
                }
            }
            "thread_rolled_back" => {
                let id = format!("codex:rollback:{}", id_index);
                let count = payload
                    .get("num_turns")
                    .and_then(Value::as_u64)
                    .and_then(|n| usize::try_from(n).ok());
                match count {
                    Some(count) if count <= user_turn_starts.len() => {
                        if count > 0 {
                            let keep = user_turn_starts.len() - count;
                            prev = user_turn_starts[keep].clone();
                            user_turn_starts.truncate(keep);
                        }
                    }
                    _ => {
                        issues.push(
                            ImportIssue::new(
                                "rollback-incomplete",
                                "rollback exceeds known user turns",
                            )
                            .with_span(format!("record:{}", rec.index)),
                        );
                        prev = None;
                        user_turn_starts.clear();
                    }
                }
                events.push(ImportedEvent::Metadata {
                    base: base_ev(id.clone(), ordinal, ts.clone(), locator, rec.index),
                    source_type: "thread_rolled_back".into(),
                    payload: None,
                });
                if let Some(removed) = before.clone() {
                    relations.push(EventRelation {
                        kind: RelationKind::RollsBack,
                        from: id.clone(),
                        to: removed,
                        confidence: RelationConfidence::Explicit,
                    });
                }
                link_parent(&mut relations, &id, &prev);
                prev = Some(id.clone());
                active_leaf = Some(id);
                ordinal += 1;
            }
            "rollback" | "turn_rollback" => {
                let id = format!("codex:rollback:{}", id_index);
                let target = payload
                    .get("to")
                    .or_else(|| payload.get("item_id"))
                    .and_then(|x| x.as_str())
                    .map(|s| s.to_string())
                    .map(|target| {
                        if target.starts_with("codex:") {
                            target
                        } else {
                            format!("codex:{target}")
                        }
                    });
                events.push(ImportedEvent::Metadata {
                    base: base_ev(id.clone(), ordinal, ts.clone(), locator, rec.index),
                    source_type: "rollback".into(),
                    payload: None,
                });
                if let Some(t) = target {
                    relations.push(EventRelation {
                        kind: RelationKind::RollsBack,
                        from: id.clone(),
                        to: t.clone(),
                        confidence: RelationConfidence::Explicit,
                    });
                    if events.iter().any(|event| event.id() == t) {
                        prev = Some(t);
                    } else {
                        prev = None;
                        issues.push(ImportIssue::new(
                            "rollback-incomplete",
                            "rollback target is missing",
                        ));
                    }
                } else {
                    prev = None;
                    issues.push(ImportIssue::new(
                        "rollback-incomplete",
                        "rollback target is unknown",
                    ));
                }
                active_leaf = prev.clone();
                ordinal += 1;
            }
            "fork" | "session_fork" => {
                let id = format!("codex:fork:{}", id_index);
                events.push(ImportedEvent::Metadata {
                    base: base_ev(id.clone(), ordinal, ts.clone(), locator, rec.index),
                    source_type: "fork".into(),
                    payload: None,
                });
                if let Some(p) = fork_from.clone().or_else(|| prev.clone()) {
                    relations.push(EventRelation {
                        kind: RelationKind::Fork,
                        from: id.clone(),
                        to: p,
                        confidence: RelationConfidence::Explicit,
                    });
                }
                fork_from = prev.clone();
                ordinal += 1;
            }
            "turn_context" => {}
            "" => {
                issues.push(
                    ImportIssue::new("unknown-record", "record missing type")
                        .with_span(format!("record:{}", rec.index)),
                );
            }
            _ => {}
        }
        if events.get(first_new_event..).is_some_and(|new| {
            new.iter().any(|event| {
                matches!(
                    event,
                    ImportedEvent::Message {
                        role: MessageRole::User,
                        ..
                    }
                )
            })
        }) {
            user_turn_starts.push(before);
        }
    }

    if events.is_empty() {
        return Err(ImportError::new(
            ErrorCode::FormatUnsupported,
            "Codex JSONL had no readable messages",
        ));
    }
    let mut out = AdapterOutput {
        evidence: SourceEvidence {
            provider: Provider::Codex,
            source_version,
            schema_fingerprint: "codex-rollout-jsonl-v1".into(),
            adapter_version: CODEX_ADAPTER_VERSION.into(),
            platform: current_platform(),
            artifacts: vec![ArtifactEvidence {
                artifact_id: primary_locator.clone(),
                kind: "jsonl".into(),
                relative_locator: primary_locator,
                byte_length: prefix.byte_length,
                sha256: prefix.file_sha256.clone(),
                complete_prefix_sha256: Some(prefix.complete_prefix_sha256.clone()),
            }],
        },
        graph: NormalizedGraph {
            events,
            relations,
            active_leaf,
            native_session_id: native_id.clone(),
            title: Some(title.unwrap_or(native_id)),
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

fn map_role(role: &str) -> Option<MessageRole> {
    match role {
        "user" => Some(MessageRole::User),
        "assistant" => Some(MessageRole::Assistant),
        // Developer messages are injected instructions; keep them as hidden system context.
        "system" | "developer" => Some(MessageRole::System),
        _ => None,
    }
}

fn base_ev(id: String, ordinal: u32, ts: Option<String>, locator: &str, index: u32) -> EventBase {
    EventBase {
        id,
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

fn link_parent(relations: &mut Vec<EventRelation>, from: &str, prev: &Option<String>) {
    if let Some(p) = prev {
        relations.push(EventRelation {
            kind: RelationKind::Parent,
            from: from.into(),
            to: p.clone(),
            confidence: RelationConfidence::Inferred,
        });
    }
}

#[allow(clippy::too_many_arguments)]
/// Replacement history is a new live chain hanging off the compaction node.
/// Existing events stay as the covered branch and are never spliced in.
fn apply_replacement_history(
    items: &[Value],
    summary: &str,
    compact_index: u32,
    ts: Option<String>,
    locator: &str,
    events: &mut Vec<ImportedEvent>,
    relations: &mut Vec<EventRelation>,
    prev: &mut Option<String>,
    ordinal: &mut u32,
    active_leaf: &mut Option<String>,
    assets: &mut Vec<crate::session_migration::types::AssetRecord>,
    issues: &mut Vec<ImportIssue>,
) {
    let mut existing: std::collections::HashSet<String> =
        events.iter().map(|event| event.id().to_string()).collect();
    for (offset, item) in items.iter().enumerate() {
        let item_type = item
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("message");
        if item_type != "message" {
            let mut base = base_ev(
                format!("codex:compact-unsupported:{compact_index}:{offset}"),
                *ordinal,
                ts.clone(),
                locator,
                compact_index,
            );
            base.spans[0].record = format!("record:{compact_index}:replacement:{offset}");
            events.push(ImportedEvent::Unsupported {
                base,
                source_type: item_type.chars().take(128).collect(),
                payload: None,
            });
            issues.push(
                ImportIssue::new(
                    "unsupported-item",
                    "Unsupported Codex replacement history item",
                )
                .with_span(format!("record:{compact_index}:replacement:{offset}")),
            );
            *ordinal += 1;
            continue;
        }
        let role = item.get("role").and_then(Value::as_str).unwrap_or("");
        let Some(mrole) = map_role(role) else {
            continue;
        };
        let item_id = item
            .get("id")
            .and_then(Value::as_str)
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("compact-hist:{compact_index}:{offset}"));
        let id = format!("codex:{item_id}");
        if !existing.insert(id) {
            continue;
        }
        let mut content = content::response(
            item,
            locator,
            &format!("record:{compact_index}:replacement:{offset}"),
        );
        if content.blocks.is_empty() {
            continue;
        }
        if mrole == MessageRole::Assistant
            && content.assets.is_empty()
            && content.complete()
            && content.text() == summary
        {
            continue;
        }
        push_message(
            events,
            relations,
            prev,
            ordinal,
            active_leaf,
            mrole,
            &mut content,
            ts.clone(),
            locator,
            compact_index,
            item_id,
        );
        assets.append(&mut content.assets);
        issues.append(&mut content.issues);
    }
}

// These references are one normalization cursor. Keeping them explicit avoids
// hiding partial state updates behind an independently mutable helper object.
#[allow(clippy::too_many_arguments)]
fn push_message(
    events: &mut Vec<ImportedEvent>,
    relations: &mut Vec<EventRelation>,
    prev: &mut Option<String>,
    ordinal: &mut u32,
    active_leaf: &mut Option<String>,
    role: MessageRole,
    content: &mut content::Content,
    ts: Option<String>,
    locator: &str,
    index: u32,
    item_id: String,
) {
    let id = format!("codex:{item_id}");
    events.push(ImportedEvent::Message {
        base: base_ev(id.clone(), *ordinal, ts, locator, index),
        role,
        complete: content.complete(),
        blocks: std::mem::take(&mut content.blocks),
    });
    link_parent(relations, &id, prev);
    *prev = Some(id.clone());
    *active_leaf = Some(id);
    *ordinal += 1;
}

#[cfg(test)]
fn metadata_only_db(path: &Path) -> AdapterOutput {
    AdapterOutput {
        evidence: SourceEvidence {
            provider: Provider::Codex,
            source_version: None,
            schema_fingerprint: "codex-thread-history-metadata".into(),
            adapter_version: CODEX_ADAPTER_VERSION.into(),
            platform: current_platform(),
            artifacts: vec![ArtifactEvidence {
                artifact_id: path.to_string_lossy().into_owned(),
                kind: "sqlite".into(),
                relative_locator: path.to_string_lossy().into_owned(),
                byte_length: path.metadata().map(|m| m.len()).unwrap_or(0),
                sha256: std::fs::read(path)
                    .map(|bytes| sha256_bytes(&bytes))
                    .unwrap_or_default(),
                complete_prefix_sha256: None,
            }],
        },
        graph: NormalizedGraph {
            events: vec![],
            relations: vec![],
            active_leaf: None,
            native_session_id: path
                .file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
                .unwrap_or_default(),
            title: Some("Codex metadata only".into()),
            cwd: None,
        },
        inventory: Inventory::default(),
        issues: vec![ImportIssue::new(
            "metadata-only",
            "Codex historyMode/paginated projection has no readable body",
        )
        .with_error(ErrorCode::SchemaUnsupported)],
        assets: vec![],
        outcome: ImportOutcome::Unsupported,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    include!("codex_tests.rs");
}

#[cfg(test)]
#[path = "codex_media_tests.rs"]
mod media_tests;

#[cfg(test)]
#[path = "codex_tool_output_tests.rs"]
mod tool_output_tests;
