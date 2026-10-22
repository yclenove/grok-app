//! Ordered legacy Codex content. References identify source occurrences, not blobs.

use serde_json::Value;

use crate::session_migration::hash::{canonical_json, sha256_bytes};
use crate::session_migration::types::{
    AssetRecord, AssetState, ContentBlock, ImportIssue, SourceSpan, ToolOutcome,
};

#[derive(Default)]
pub(super) struct Content {
    pub blocks: Vec<ContentBlock>,
    pub assets: Vec<AssetRecord>,
    pub issues: Vec<ImportIssue>,
}

impl Content {
    pub fn text(&self) -> String {
        self.blocks
            .iter()
            .filter_map(ContentBlock::plain_text)
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn complete(&self) -> bool {
        !self
            .blocks
            .iter()
            .any(|block| matches!(block, ContentBlock::Opaque { .. }))
            && self.issues.is_empty()
    }

    pub fn comparable(&self) -> bool {
        !self
            .blocks
            .iter()
            .any(|block| matches!(block, ContentBlock::Opaque { .. }))
            && self.assets.iter().all(|asset| {
                asset
                    .source_ref
                    .as_deref()
                    .is_some_and(|s| !s.starts_with("opaque:codex-invalid"))
            })
            && !self
                .issues
                .iter()
                .any(|issue| issue.code == "codex-image-order")
    }

    pub fn image_keys(&self) -> Vec<String> {
        self.assets
            .iter()
            .map(|asset| sha256_bytes(asset.source_ref.as_deref().unwrap_or("").as_bytes()))
            .collect()
    }

    fn issue(&mut self, code: &str, record: &str) {
        self.issues.push(
            ImportIssue::new(
                code,
                "Codex content is retained with unsupported or unresolved references",
            )
            .with_span(record),
        );
    }

    fn opaque(&mut self, kind: &str, record: &str) {
        self.blocks.push(ContentBlock::Opaque {
            source_type: kind.chars().filter(|c| !c.is_control()).take(128).collect(),
            payload: None,
        });
        self.issue("codex-content-unsupported", record);
    }

    fn image(&mut self, reference: Option<String>, locator: &str, record: &str, block: u32) {
        let source_ref = reference.unwrap_or_else(|| "opaque:codex-invalid-image".into());
        if source_ref.starts_with("opaque:") || source_ref.starts_with("url:") {
            self.issue("codex-image-unresolved", record);
        }
        let asset_id = format!(
            "codex-image:{}",
            sha256_bytes(canonical_json(&serde_json::json!([locator, record, block])).as_bytes())
        );
        self.blocks.push(ContentBlock::Asset {
            asset_id: asset_id.clone(),
            alt: None,
        });
        self.assets.push(AssetRecord {
            asset_id,
            digest: None,
            mime: Some("image/unknown".into()),
            size: 0,
            state: AssetState::Unresolved,
            source_span: SourceSpan {
                artifact_id: locator.into(),
                record: record.into(),
                block: Some(block),
            },
            source_ref: Some(source_ref),
        });
    }
}

fn inline_reference(value: &Value) -> Option<String> {
    let reference = value.as_str().filter(|s| !s.is_empty())?;
    if reference.starts_with("data:image/") {
        let (metadata, data) = reference.split_once(',')?;
        if data.is_empty() || !metadata.split(';').any(|part| part == "base64") {
            return None;
        }
        Some(reference.into())
    } else if reference.starts_with("https://") || reference.starts_with("http://") {
        Some(format!("url:{reference}"))
    } else {
        None
    }
}

fn file_reference(value: &Value) -> Option<String> {
    value
        .as_str()
        .filter(|s| !s.is_empty())
        .map(|s| format!("opaque:codex-file:{s}"))
}

pub(super) fn response(payload: &Value, locator: &str, record: &str) -> Content {
    let mut out = Content::default();
    if let Some(text) = payload.get("content").and_then(Value::as_str) {
        if !text.is_empty() {
            out.blocks.push(ContentBlock::text(text));
        }
        return out;
    }
    let Some(parts) = payload.get("content").and_then(Value::as_array) else {
        out.opaque("malformed-message-content", record);
        return out;
    };
    // Preserve the historical single-text shape/digest for ordinary text-only messages.
    if parts.iter().all(|part| {
        matches!(
            part.get("type").and_then(Value::as_str),
            Some("input_text" | "output_text" | "text")
        ) && text_part(part).is_some()
    }) {
        let text = parts
            .iter()
            .filter_map(text_part)
            .collect::<Vec<_>>()
            .join("\n");
        if !text.is_empty() {
            out.blocks.push(ContentBlock::text(text));
        }
        return out;
    }
    for (index, part) in parts.iter().enumerate() {
        let kind = part
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("unknown-content");
        match kind {
            "input_text" | "output_text" | "text" => {
                if let Some(text) = text_part(part) {
                    out.blocks.push(ContentBlock::text(text));
                } else {
                    out.opaque("malformed-text", record);
                }
            }
            "input_image" => {
                let reference = match (part.get("image_url"), part.get("file_id")) {
                    (Some(url), None) => inline_reference(url),
                    (None, Some(file)) => file_reference(file),
                    _ => None,
                };
                out.image(reference, locator, record, index as u32);
            }
            _ => out.opaque(kind, record),
        }
    }
    out
}

/// `function_call_output` / `custom_tool_call_output`. A string keeps the
/// historical single text block. codex-cli 0.160 writes an ordered array of
/// content items; the object form carries an explicit `success` flag. Without
/// a flag the call returned output, which earlier imports already record as
/// success; exit codes stay verbatim in the text. Anything else is disclosed.
pub(super) fn tool_output(payload: &Value, locator: &str, record: &str) -> (Content, ToolOutcome) {
    let mut out = Content::default();
    match payload.get("output") {
        Some(Value::String(text)) => {
            out.blocks.push(ContentBlock::text(text.as_str()));
            (out, ToolOutcome::Success)
        }
        Some(Value::Array(items)) => {
            items_into(items, locator, record, &mut out);
            (out, ToolOutcome::Success)
        }
        Some(Value::Object(object)) if object.contains_key("content") => {
            let outcome = match object.get("success") {
                None | Some(Value::Null) => ToolOutcome::Success,
                Some(Value::Bool(true)) => ToolOutcome::Success,
                Some(Value::Bool(false)) => ToolOutcome::Error,
                Some(_) => ToolOutcome::Unknown,
            };
            match &object["content"] {
                Value::String(text) => out.blocks.push(ContentBlock::text(text.as_str())),
                Value::Array(items) => items_into(items, locator, record, &mut out),
                _ => out.opaque("malformed-tool-output", record),
            }
            (out, outcome)
        }
        Some(other) => {
            let kind = match other {
                Value::Null => "null-tool-output",
                Value::Object(_) => "object-tool-output",
                _ => "scalar-tool-output",
            };
            out.opaque(kind, record);
            (out, ToolOutcome::Unknown)
        }
        None => {
            out.opaque("missing-tool-output", record);
            (out, ToolOutcome::Unknown)
        }
    }
}

fn items_into(items: &[Value], locator: &str, record: &str, out: &mut Content) {
    for (index, item) in items.iter().enumerate() {
        match item.get("type").and_then(Value::as_str) {
            Some("input_text" | "output_text" | "text") => match text_part(item) {
                Some(text) => out.blocks.push(ContentBlock::text(text)),
                None => out.opaque("malformed-text", record),
            },
            Some("input_image") => {
                let reference = match (item.get("image_url"), item.get("file_id")) {
                    (Some(url), None) => inline_reference(url),
                    (None, Some(file)) => file_reference(file),
                    _ => None,
                };
                out.image(reference, locator, record, index as u32);
            }
            other => out.opaque(other.unwrap_or("unknown-content"), record),
        }
    }
}

/// codex-cli writes workspace instructions and environment context as
/// user-role messages. They are context, not conversation turns: kept as
/// system events, never titles, and not user turns for rollback counting.
pub(super) fn injected_context(payload: &Value) -> bool {
    const PREFIXES: [&str; 3] = [
        "<environment_context>",
        "<user_instructions>",
        "# AGENTS.md instructions",
    ];
    let injected = |text: &str| {
        let text = text.trim_start();
        PREFIXES.iter().any(|prefix| text.starts_with(prefix))
    };
    match payload.get("content") {
        Some(Value::String(text)) => injected(text),
        Some(Value::Array(parts)) => {
            !parts.is_empty()
                && parts.iter().all(|part| {
                    matches!(
                        part.get("type").and_then(Value::as_str),
                        Some("input_text" | "text")
                    ) && text_part(part).is_some_and(injected)
                })
        }
        _ => false,
    }
}

fn text_part(part: &Value) -> Option<&str> {
    part.get("text")
        .or_else(|| part.get("input_text"))
        .and_then(Value::as_str)
}

/// Text for a title only: codex-cli wraps an attached image as
/// `<image name=… path=…>` / `</image>` parts and may add its own
/// "could not read the local image" note; neither is what the user asked.
pub(super) fn title_text(payload: &Value) -> Option<String> {
    let parts = match payload.get("content") {
        Some(Value::String(text)) => return Some(text.clone()),
        Some(Value::Array(parts)) => parts,
        _ => return None,
    };
    let wrapper = |text: &str| {
        let text = text.trim();
        (text.starts_with("<image ") && text.ends_with('>'))
            || text == "</image>"
            || text.starts_with("Codex could not read the local image")
    };
    let parts: Vec<_> = parts
        .iter()
        .filter(|part| {
            matches!(
                part.get("type").and_then(Value::as_str),
                Some("input_text" | "output_text" | "text")
            )
        })
        .filter_map(text_part)
        .filter(|text| !wrapper(text))
        .collect();
    (!parts.is_empty()).then(|| parts.join("\n"))
}

pub(super) fn text(payload: &Value) -> Option<String> {
    if let Some(text) = payload.get("content").and_then(Value::as_str) {
        return Some(text.into());
    }
    let parts = payload.get("content")?.as_array()?;
    let parts: Vec<_> = parts
        .iter()
        .filter(|part| {
            matches!(
                part.get("type").and_then(Value::as_str),
                Some("input_text" | "output_text" | "text")
            )
        })
        .filter_map(text_part)
        .collect();
    (!parts.is_empty()).then(|| parts.join("\n"))
}

pub(super) fn event(payload: &Value, locator: &str, record: &str) -> Content {
    let mut out = Content::default();
    if let Some(text) = payload
        .get("message")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
    {
        out.blocks.push(ContentBlock::text(text));
    }
    let inline = list(payload, "images", &mut out, record);
    let files = list(payload, "file_ids", &mut out, record);
    let order = payload.get("image_order").filter(|v| !v.is_null());
    let kinds = order.and_then(Value::as_array);
    let valid_order = kinds.is_some_and(|kinds| {
        kinds
            .iter()
            .filter(|v| v.as_str() == Some("inline"))
            .count()
            == inline.len()
            && kinds.iter().filter(|v| v.as_str() == Some("file")).count() == files.len()
            && kinds.len() == inline.len() + files.len()
    });
    let explicit_order =
        kinds.is_some_and(|kinds| !kinds.is_empty()) || order.is_some_and(|v| !v.is_array());
    if explicit_order && !valid_order {
        // Retain all references using the documented legacy fallback, but disclose lost order.
        out.issue("codex-image-order", record);
    }
    let mut inline_index = 0;
    let mut file_index = 0;
    if valid_order && explicit_order {
        for kind in kinds.unwrap() {
            let reference = if kind.as_str() == Some("inline") {
                let value = inline_reference(&inline[inline_index]);
                inline_index += 1;
                value
            } else {
                let value = file_reference(&files[file_index]);
                file_index += 1;
                value
            };
            let block = out.blocks.len() as u32;
            out.image(reference, locator, record, block);
        }
    } else {
        for reference in inline
            .iter()
            .map(inline_reference)
            .chain(files.iter().map(file_reference))
        {
            let block = out.blocks.len() as u32;
            out.image(reference, locator, record, block);
        }
    }
    for path in list(payload, "local_images", &mut out, record) {
        let reference = path
            .as_str()
            .filter(|s| !s.is_empty())
            .map(|s| format!("path:{s}"));
        let block = out.blocks.len() as u32;
        out.image(reference, locator, record, block);
    }
    for field in ["audio", "local_audio"] {
        if payload.get(field).is_some_and(|value| {
            !value.is_null() && value.as_array().is_none_or(|items| !items.is_empty())
        }) {
            out.opaque(field, record);
        }
    }
    out
}

fn list<'a>(payload: &'a Value, field: &str, out: &mut Content, record: &str) -> &'a [Value] {
    match payload.get(field) {
        None | Some(Value::Null) => &[],
        Some(Value::Array(items)) => items,
        Some(_) => {
            out.opaque(field, record);
            &[]
        }
    }
}
