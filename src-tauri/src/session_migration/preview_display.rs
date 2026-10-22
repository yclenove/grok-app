//! Bounded ordered display projection; never copies an opaque/raw payload.
use super::types::{ContentBlock, ImportedEvent};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewFieldAvailability {
    pub message_text: bool,
    pub visible_reasoning: bool,
    pub tool_call_name: bool,
    pub tool_call_input: bool,
    pub tool_result_content: bool,
    pub compaction_summary: bool,
    pub assets: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewEvent {
    pub event_id: String,
    pub kind: String,
    pub role: Option<super::types::MessageRole>,
    pub timestamp: Option<String>,
    pub text: String,
    pub truncated: bool,
    pub selectable: bool,
    pub default_selected: bool,
    pub subagent: bool,
    pub field_availability: PreviewFieldAvailability,
    pub asset_ids: Vec<String>,
    pub display_parts: Vec<ContentBlock>,
    pub display_limited: bool,
    pub citation: String,
}

/// Leave room for the twenty-row fixed transfer envelope within its existing
/// 256KiB limit. Trim only the new display prefix, never selection IDs or archive
/// content. Already-oversized legacy metadata still fails the existing boundary.
pub(super) fn bound_ordered(row: &mut PreviewEvent) {
    const ROW_BYTES: usize = 12 * 1024;
    let original = row.display_parts.len();
    let mut bytes = serde_json::to_vec(row).map_or(usize::MAX, |value| value.len());
    while bytes > ROW_BYTES {
        let Some(part) = row.display_parts.pop() else {
            break;
        };
        let removed = serde_json::to_vec(&part).map_or(0, |value| value.len())
            + usize::from(!row.display_parts.is_empty());
        bytes = bytes.saturating_sub(removed);
    }
    row.display_limited |= row.display_parts.len() < original;
}

pub(super) fn ordered(event: &ImportedEvent) -> (Vec<ContentBlock>, bool) {
    let blocks = match event {
        ImportedEvent::Message { blocks, .. } | ImportedEvent::ToolResult { blocks, .. } => blocks,
        ImportedEvent::Compaction { summary, .. } => summary,
        _ => return (Vec::new(), false),
    };
    let mut remaining = 1024;
    let mut limited = blocks.len() > 64;
    let mut cap = |value: &str| {
        let text: String = value.chars().take(remaining).collect();
        let count = text.chars().count();
        limited |= count < value.chars().count();
        remaining -= count;
        text
    };
    let parts = blocks
        .iter()
        .take(64)
        .map(|block| match block {
            ContentBlock::Text { text, .. } | ContentBlock::Code { text, .. } => {
                ContentBlock::text(cap(text))
            }
            ContentBlock::VisibleReasoning { text } => {
                ContentBlock::VisibleReasoning { text: cap(text) }
            }
            ContentBlock::Asset { asset_id, alt } => ContentBlock::Asset {
                asset_id: asset_id.clone(),
                alt: alt
                    .as_deref()
                    .map(|value| value.chars().take(120).collect()),
            },
            ContentBlock::Opaque { source_type, .. } => ContentBlock::Opaque {
                source_type: source_type.chars().take(80).collect(),
                payload: None,
            },
        })
        .collect();
    (parts, limited)
}
