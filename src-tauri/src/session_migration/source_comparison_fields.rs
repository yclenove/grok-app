//! Bounded JSON segments of typed fields, authorized by the same fixed pair.
use super::source_comparison::{fields, resource_limit, stale, ComparisonSide, FixedPair};
use super::{budget, cursor, errors::ImportError, hash};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

const FIELD_MAX_BYTES: usize = 64 * 1024;
const FIELD_DEFAULT_BYTES: usize = 4096;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FieldSide {
    Left,
    Right,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FieldRequest {
    pub version: u32,
    pub left: ComparisonSide,
    pub right: ComparisonSide,
    pub side: FieldSide,
    pub position: u32,
    pub field: String,
    pub expected_fields_digest: String,
    pub cursor: Option<String>,
    pub limit_bytes: Option<usize>,
}

fn field_text(value: Option<&Value>) -> Option<String> {
    value.map(hash::canonical_json)
}

fn segment_end(text: &str, start: usize, limit: usize) -> Result<usize, ImportError> {
    if start > text.len() || !text.is_char_boundary(start) {
        return Err(stale("field cursor is not a valid UTF-8 byte offset"));
    }
    let mut end = start.saturating_add(limit).min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    // A smaller-than-codepoint request cannot produce a looping empty segment.
    if end == start && start < text.len() {
        return Err(resource_limit());
    }
    Ok(end)
}

pub fn page(request: FieldRequest) -> Result<Value, ImportError> {
    if request.version != 1
        || request.field.is_empty()
        || request.field.len() > 128
        || !request
            .field
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_')
        || request.expected_fields_digest.len() != 64
        || !request
            .expected_fields_digest
            .bytes()
            .all(|c| c.is_ascii_hexdigit())
    {
        return Err(stale("invalid field request"));
    }
    let _operation = super::preview_lifecycle::operation()?;
    let pair = FixedPair::load(&request.left, &request.right)?;
    let snapshot = match request.side {
        FieldSide::Left => &pair.left,
        FieldSide::Right => &pair.right,
    };
    let event = snapshot
        .event(request.position as usize)?
        .ok_or_else(|| stale("field event is missing on this side"))?;
    let body = fields(&event)?;
    let body_digest = hash::options_hash(&body);
    if body_digest != request.expected_fields_digest {
        return Err(stale("field event body differs from the compared row"));
    }
    let object = body
        .as_object()
        .ok_or_else(|| stale("invalid typed event body"))?;
    let names: Vec<_> = object.keys().collect();
    let text = field_text(object.get(&request.field));
    let field_digest = text
        .as_ref()
        .map(|text| hash::sha256_bytes(text.as_bytes()));
    let filter = format!(
        "source-comparison-field:v1:{}",
        hash::options_hash(&json!({
            "side":request.side, "position":request.position, "field":request.field,
            "fieldsDigest":body_digest, "fieldDigest":field_digest,
        }))
    );
    let scope = pair.scope(&filter);
    let start = match request.cursor.as_deref() {
        Some(raw) => {
            if raw.len() > budget::READ_MAX_BYTES {
                return Err(stale("field cursor exceeds byte budget"));
            }
            let decoded = cursor::decode_bound_cursor(raw, scope)?;
            if decoded.mark.is_some() || text.is_none() {
                return Err(stale("invalid field continuation"));
            }
            decoded.offset as usize
        }
        None => 0,
    };
    let limit = request
        .limit_bytes
        .unwrap_or(FIELD_DEFAULT_BYTES)
        .clamp(1, FIELD_MAX_BYTES);
    let value = text.as_deref().unwrap_or("");
    u32::try_from(value.len()).map_err(|_| resource_limit())?;
    let end = segment_end(value, start, limit)?;
    let complete = end == value.len();
    let next = if complete {
        None
    } else {
        Some(cursor::encode_bound_cursor(scope, end as u32, None)?)
    };
    let result = json!({"version":1, "left":request.left, "right":request.right,
        "side":request.side, "position":request.position, "field":request.field,
        "eventId":event.id(), "fieldsDigest":body_digest, "fieldDigest":field_digest,
        "fieldNames":names, "available":text.is_some(), "text":&value[start..end],
        "startByte":start, "endByte":end, "totalBytes":value.len(),
        "nextCursor":next, "complete":complete});
    if serde_json::to_vec(&result)
        .map_err(|_| stale("invalid field response"))?
        .len()
        > budget::READ_PAGE_TEXT_BYTES
    {
        return Err(resource_limit());
    }
    // This also covers an unavailable field. Neither side may expire mid-read.
    pair.verify_unchanged()?;
    Ok(result)
}

#[cfg(test)]
#[path = "source_comparison_field_tests.rs"]
mod tests;
