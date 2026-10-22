//! Pair adjacent opposite legacy projections only with equal text and image references.

use serde_json::Value;

use super::content::Content;
use crate::session_migration::types::{
    AssetRecord, ImportIssue, ImportedEvent, MessageRole, SourceSpan,
};

pub(super) struct Projection {
    pub index: usize,
    pub from_event: bool,
    pub phase: Option<String>,
    pub asset_start: usize,
    text: String,
    images: Vec<String>,
    comparable: bool,
}

impl Projection {
    pub fn new(
        index: usize,
        from_event: bool,
        payload: &Value,
        asset_start: usize,
        content: &Content,
    ) -> Self {
        Self {
            index,
            from_event,
            phase: phase(payload),
            asset_start,
            text: content.text(),
            images: content.image_keys(),
            comparable: content.comparable(),
        }
    }
}

fn phase(payload: &Value) -> Option<String> {
    payload
        .get("phase")
        .and_then(Value::as_str)
        .map(str::to_owned)
}

// All mutable collections share the same sequential normalization cursor.
#[allow(clippy::too_many_arguments)]
pub(super) fn merge(
    events: &mut [ImportedEvent],
    pending: &mut Option<Projection>,
    from_event: bool,
    role: MessageRole,
    content: &mut Content,
    payload: &Value,
    locator: &str,
    record: u32,
    assets: &mut Vec<AssetRecord>,
    issues: &mut Vec<ImportIssue>,
) -> bool {
    let Some(previous) = pending.take() else {
        return false;
    };
    if from_event == previous.from_event {
        return false;
    }
    let next_phase = phase(payload);
    if previous.phase.is_some() && next_phase.is_some() && previous.phase != next_phase {
        return false;
    }
    let Some(ImportedEvent::Message {
        role: old_role,
        blocks,
        base,
        complete,
    }) = events.get_mut(previous.index)
    else {
        return false;
    };
    if *old_role != role || previous.text != content.text() {
        return false;
    }
    if !previous.comparable || !content.comparable() || previous.images != content.image_keys() {
        issues.push(
            ImportIssue::new(
                "codex-projection-unmatched",
                "Adjacent projections could not be proven equivalent; both were retained",
            )
            .with_span(format!("record:{record}")),
        );
        return false;
    }
    base.spans.push(SourceSpan {
        artifact_id: locator.into(),
        record: format!("record:{record}"),
        block: None,
    });
    if previous.from_event {
        // Response content has the precise wire order. Replace only this pending
        // message's assets; older occurrences (even identical bytes) are distinct.
        assets.truncate(previous.asset_start);
        *complete = content.complete();
        *blocks = std::mem::take(&mut content.blocks);
        assets.append(&mut content.assets);
        issues.append(&mut content.issues);
    }
    true
}
