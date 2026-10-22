//! Compact summaries carry history content, not a new user turn or tool call.

use serde_json::Value;

use super::{parse_content_blocks, ParsedPart};
use crate::session_migration::types::{
    AssetRecord, ContentBlock, EventBase, ImportIssue, ImportedEvent,
};

pub(super) fn normalize_summary(
    content: &Value,
    uuid: &str,
    base: EventBase,
    index: u32,
    assets: &mut Vec<AssetRecord>,
    issues: &mut Vec<ImportIssue>,
) -> ImportedEvent {
    let locator = &base.spans[0].artifact_id;
    let parts = parse_content_blocks(content, uuid, assets, locator, index);
    let mut summary = Vec::with_capacity(parts.len());
    if parts.is_empty() {
        issues.push(
            ImportIssue::new(
                "missing-compaction-content",
                "compact summary has no readable content",
            )
            .with_span(format!("record:{index}")),
        );
    }
    for part in parts {
        let source_type = match part {
            ParsedPart::Block(block) => {
                summary.push(block);
                continue;
            }
            ParsedPart::ToolUse { .. } => "tool_use".into(),
            ParsedPart::ToolResult { .. } => "tool_result".into(),
            ParsedPart::Unknown(source_type) => source_type,
        };
        issues.push(
            ImportIssue::new(
                "unknown-compaction-block",
                "unsupported compact summary content block",
            )
            .with_span(format!("record:{index}")),
        );
        summary.push(ContentBlock::Opaque {
            source_type,
            payload: None,
        });
    }
    ImportedEvent::Compaction {
        base,
        summary,
        covered_event_ids: None,
        coverage_known: false,
    }
}
