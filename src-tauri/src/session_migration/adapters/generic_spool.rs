use std::io::BufRead;
use std::path::Path;

use serde_json::Value;

use super::generic_stream::{
    finish_streaming_reader, skip_bom_and_leading_whitespace, streaming_reader, StreamingReader,
};
use super::{fence_open, heading_role, normalize_role, prefix_role};
use crate::session_migration::errors::{ErrorCode, ImportError};
use crate::session_migration::prepared_builder::{PreparedRevisionBuilder, PreparedRevisionOutput};
use crate::session_migration::types::{
    AdapterOutput, ArtifactEvidence, ContentBlock, CountBucket, EventBase, EventRelation,
    ImportIssue, ImportOutcome, ImportedEvent, Inventory, MessageRole, NormalizedGraph, Provider,
    RelationConfidence, RelationKind, SourceEvidence, SourceSpan, TimeOrigin, NORMALIZER_VERSION,
};

#[derive(Default)]
struct ArraySummary {
    relations: Vec<EventRelation>,
    issues: Vec<ImportIssue>,
    event_count: u32,
    active_leaf: Option<String>,
}

pub(super) fn normalize_item(
    value: Value,
    source_index: usize,
    ordinal: u32,
    locator: &str,
    issues: &mut Vec<ImportIssue>,
) -> Option<ImportedEvent> {
    let mut item = match value {
        Value::Object(item) => item,
        _ => serde_json::Map::new(),
    };
    let role_value = take_first_present(&mut item, &["role", "author"]);
    let role_raw = role_value.as_ref().and_then(Value::as_str).unwrap_or("");
    let Some(role) = normalize_role(role_raw) else {
        issues.push(
            ImportIssue::new(
                "unknown-role",
                format!("row {source_index} has role '{role_raw}'"),
            )
            .with_span(format!("index:{source_index}")),
        );
        return None;
    };
    let text = extract_text(&mut item);
    if text.trim().is_empty() {
        return None;
    }
    let timestamp = string_value(take_first_present(
        &mut item,
        &["timestamp", "createdAt", "created_at"],
    ));
    let id = string_value(item.remove("id"))
        .map(|id| format!("generic:{id}"))
        .unwrap_or_else(|| format!("generic:{source_index}"));
    Some(ImportedEvent::Message {
        base: EventBase {
            id,
            ordinal,
            spans: vec![SourceSpan {
                artifact_id: locator.into(),
                record: format!("index:{source_index}"),
                block: None,
            }],
            timestamp: timestamp.clone(),
            time_origin: if timestamp.is_some() {
                TimeOrigin::Source
            } else {
                TimeOrigin::Unknown
            },
        },
        role,
        blocks: vec![ContentBlock::text(text)],
        complete: true,
    })
}

fn take_first_present(item: &mut serde_json::Map<String, Value>, keys: &[&str]) -> Option<Value> {
    for key in keys {
        if item.contains_key(*key) {
            return item.remove(*key);
        }
    }
    None
}

fn string_value(value: Option<Value>) -> Option<String> {
    match value {
        Some(Value::String(text)) => Some(text),
        _ => None,
    }
}

fn extract_text(item: &mut serde_json::Map<String, Value>) -> String {
    let content = match item.remove("content") {
        Some(Value::String(text)) => return text,
        other => other,
    };
    if let Some(text) = string_value(item.remove("text")) {
        return text;
    }
    if let Some(text) = string_value(item.remove("message")) {
        return text;
    }
    let Some(Value::Array(parts)) = content else {
        return String::new();
    };
    let mut texts = Vec::new();
    for part in parts {
        match part {
            Value::String(text) => texts.push(text),
            Value::Object(mut object) => {
                if let Some(text) = string_value(object.remove("text")) {
                    texts.push(text);
                }
            }
            _ => {}
        }
    }
    texts.join("\n")
}

pub(super) fn prepare_file(
    path: &Path,
    locator: &str,
    native_session_id: &str,
    byte_length: u64,
    options: &serde_json::Value,
    preview_directory: Option<&str>,
) -> Result<(AdapterOutput, PreparedRevisionOutput), ImportError> {
    let mut reader = streaming_reader(path, byte_length)?;
    let first = skip_bom_and_leading_whitespace(&mut reader)?
        .ok_or_else(|| ImportError::new(ErrorCode::FormatUnsupported, "empty transcript"))?;
    let mut builder = match preview_directory {
        Some(directory_id) => PreparedRevisionBuilder::new_in_preview_dir(directory_id)?,
        None => PreparedRevisionBuilder::new()?,
    };
    let builder_path = builder.path().to_path_buf();
    let (summary, source_tokens, fingerprint) = match first {
        b'[' | b'{' => super::generic_spool_raw::prepare_json(
            &mut reader,
            &builder_path,
            locator,
            &mut builder,
            first,
        )?,
        _ => {
            let markdown = parse_markdown(&mut reader, locator, &mut builder)?;
            (
                super::generic_spool_raw::JsonSummary {
                    relations: markdown.relations,
                    issues: markdown.issues,
                    event_count: markdown.event_count,
                    active_leaf: markdown.active_leaf,
                },
                0,
                "markdown",
            )
        }
    };
    if summary.event_count == 0 {
        return Err(ImportError::new(
            ErrorCode::FormatUnsupported,
            if matches!(first, b'{' | b'[') {
                "JSON array had no user/assistant messages"
            } else {
                "Could not parse transcript. Use JSON messages array or markdown ## User / ## Assistant sections."
            },
        ));
    }
    let sha256 = finish_streaming_reader(reader, path, byte_length)?;
    let inventory = Inventory {
        imported: CountBucket {
            messages: summary.event_count,
            ..CountBucket::default()
        },
        source_tokens,
        ..Inventory::default()
    };
    let outcome = if summary.issues.is_empty() {
        ImportOutcome::Complete
    } else {
        ImportOutcome::Partial
    };
    let adapter = AdapterOutput {
        evidence: SourceEvidence {
            provider: Provider::GenericFile,
            source_version: None,
            schema_fingerprint: fingerprint.into(),
            adapter_version: NORMALIZER_VERSION.into(),
            platform: crate::session_migration::detect::current_platform(),
            artifacts: vec![ArtifactEvidence {
                artifact_id: locator.into(),
                kind: "file".into(),
                relative_locator: locator.into(),
                byte_length,
                sha256,
                complete_prefix_sha256: None,
            }],
        },
        graph: NormalizedGraph {
            events: Vec::new(),
            relations: summary.relations,
            active_leaf: summary.active_leaf,
            native_session_id: native_session_id.to_string(),
            title: Path::new(locator)
                .file_stem()
                .map(|stem| stem.to_string_lossy().into_owned()),
            cwd: None,
        },
        inventory,
        issues: summary.issues,
        assets: Vec::new(),
        outcome,
    };
    let prepared = builder.finalize(&adapter, options)?;
    Ok((adapter, prepared))
}

fn parse_markdown(
    reader: &mut StreamingReader,
    locator: &str,
    builder: &mut PreparedRevisionBuilder,
) -> Result<ArraySummary, ImportError> {
    let mut summary = ArraySummary::default();
    let mut current_role: Option<MessageRole> = None;
    let mut body = String::new();
    let mut in_fence = false;
    let mut fence_mark: Option<String> = None;
    let mut line = String::new();
    loop {
        line.clear();
        let count = reader.read_line(&mut line).map_err(|error| {
            if error.kind() == std::io::ErrorKind::InvalidData {
                ImportError::new(ErrorCode::FormatUnsupported, "file is not UTF-8")
            } else {
                ImportError::new(ErrorCode::SourceNotFound, error.to_string())
            }
        })?;
        if count == 0 {
            break;
        }
        while line.ends_with('\n') || line.ends_with('\r') {
            line.pop();
        }
        let trimmed = line.trim();
        if let Some(mark) = fence_open(trimmed) {
            if in_fence && fence_mark.as_deref() == Some(mark) {
                in_fence = false;
                fence_mark = None;
            } else if !in_fence {
                in_fence = true;
                fence_mark = Some(mark.to_string());
            }
            if current_role.is_some() {
                append_markdown_line(&mut body, &line)?;
            }
            continue;
        }
        if in_fence {
            if current_role.is_some() {
                append_markdown_line(&mut body, &line)?;
            }
            continue;
        }
        if let Some((role, rest)) = heading_role(trimmed).or_else(|| prefix_role(trimmed)) {
            flush_markdown(current_role, &mut body, locator, builder, &mut summary)?;
            current_role = Some(role);
            if let Some(rest) = rest.filter(|rest| !rest.is_empty()) {
                append_markdown_line(&mut body, &rest)?;
            }
            continue;
        }
        if current_role.is_some() {
            append_markdown_line(&mut body, &line)?;
        }
    }
    flush_markdown(current_role, &mut body, locator, builder, &mut summary)?;
    Ok(summary)
}

fn append_markdown_line(body: &mut String, line: &str) -> Result<(), ImportError> {
    if body.len().saturating_add(line.len()).saturating_add(1)
        > crate::session_migration::budget::JSON_RECORD_MAX_BYTES
    {
        return Err(ImportError::new(
            ErrorCode::ResourceLimit,
            "markdown message exceeds 32 MiB archive record limit",
        ));
    }
    body.push_str(line);
    body.push('\n');
    Ok(())
}

fn flush_markdown(
    role: Option<MessageRole>,
    body: &mut String,
    locator: &str,
    builder: &mut PreparedRevisionBuilder,
    summary: &mut ArraySummary,
) -> Result<(), ImportError> {
    let Some(role) = role else { return Ok(()) };
    let text = take_trimmed(body);
    if text.is_empty() {
        return Ok(());
    }
    let id = format!("generic:md:{}", summary.event_count);
    let event = ImportedEvent::Message {
        base: EventBase {
            id: id.clone(),
            ordinal: summary.event_count,
            spans: vec![SourceSpan {
                artifact_id: locator.into(),
                record: format!("md:{}", summary.event_count),
                block: None,
            }],
            timestamp: None,
            time_origin: TimeOrigin::Unknown,
        },
        role,
        blocks: vec![ContentBlock::text(text)],
        complete: true,
    };
    builder.push_event(&event)?;
    if let Some(parent) = summary.active_leaf.take() {
        summary.relations.push(EventRelation {
            kind: RelationKind::Parent,
            from: id.clone(),
            to: parent,
            confidence: RelationConfidence::Inferred,
        });
    }
    summary.active_leaf = Some(id);
    summary.event_count += 1;
    Ok(())
}

fn take_trimmed(buffer: &mut String) -> String {
    while buffer.chars().next_back().is_some_and(char::is_whitespace) {
        buffer.pop();
    }
    let start = buffer
        .char_indices()
        .find_map(|(index, ch)| (!ch.is_whitespace()).then_some(index))
        .unwrap_or(buffer.len());
    if start > 0 {
        buffer.drain(..start);
    }
    std::mem::take(buffer)
}
