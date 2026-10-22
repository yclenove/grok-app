//! Derive a continuation's prefix from the receipt-bound published selection.
//! No caller-supplied event/asset/field list can enlarge this scope. Recompute at
//! preparation, native creation, selected export and model-history read boundaries.
use std::collections::HashSet;

use super::{
    continuation,
    errors::{ErrorCode, ImportError},
    jobs,
    selection::{self, AllowedSelection},
    types::{ContentBlock, EventRelation, ImportedEvent, MessageRole},
};

pub(super) struct ContinuationScope {
    pub revision_id: String,
    pub leaf_id: String,
    pub selection: AllowedSelection,
}

fn denied() -> ImportError {
    ImportError::new(
        ErrorCode::HistoryAccessUnavailable,
        "continuation endpoint is outside the published message branch",
    )
}

pub(super) fn resolve(
    import_id: &str,
    revision: Option<&str>,
    leaf: Option<&str>,
) -> Result<ContinuationScope, ImportError> {
    let auth = jobs::load_published_auth(import_id, revision, None)?;
    let endpoint = leaf.filter(|id| !id.is_empty()).unwrap_or(&auth.leaf_id);
    let selection = if endpoint == auth.leaf_id {
        auth.selection
    } else {
        let dir = super::catalog::revision_dir(import_id, &auth.revision_id)?;
        let relations = continuation::load_relations(import_id, &auth.revision_id)?;
        let events = super::search::indexed_events_by_ids(&dir, &auth.selection.event_ids)?;
        prefix(
            &auth.selection,
            &auth.leaf_id,
            endpoint,
            &events,
            &relations,
        )?
    };
    Ok(ContinuationScope {
        revision_id: auth.revision_id,
        leaf_id: endpoint.to_string(),
        selection,
    })
}

fn prefix(
    published: &AllowedSelection,
    published_leaf: &str,
    endpoint: &str,
    events: &[ImportedEvent],
    relations: &[EventRelation],
) -> Result<AllowedSelection, ImportError> {
    published.validate_for_leaf(relations, events, published_leaf)?;
    // The validated published scope also includes explicitly selected subagent
    // branches. A message on one of those can be a continuation endpoint too.
    if !published.event_ids.iter().any(|id| id == endpoint) {
        return Err(denied());
    }
    let last = events
        .iter()
        .find(|event| event.id() == endpoint)
        .ok_or_else(denied)?;
    if !matches!(
        last,
        ImportedEvent::Message {
            role: MessageRole::User | MessageRole::Assistant,
            ..
        }
    ) {
        return Err(denied());
    }
    let branch =
        selection::branch_scope_ids(events, relations, endpoint, published.include_subagents)?;
    let main: HashSet<String> = super::graph::lineage_ids(relations, endpoint)
        .into_iter()
        .collect();
    // Main ancestors are graph-defined even if source timestamps are irregular.
    // A subagent attached to an ancestor must not expose work emitted later.
    let keep: HashSet<&str> = events
        .iter()
        .filter(|event| {
            branch.contains(event.id())
                && (main.contains(event.id()) || event.base().ordinal <= last.base().ordinal)
        })
        .map(ImportedEvent::id)
        .collect();
    let assets: HashSet<&str> = events
        .iter()
        .filter(|event| keep.contains(event.id()))
        .flat_map(|event| match event {
            ImportedEvent::Message { blocks, .. }
            | ImportedEvent::ToolResult { blocks, .. }
            | ImportedEvent::Compaction {
                summary: blocks, ..
            } => blocks.as_slice(),
            _ => &[],
        })
        .filter_map(|block| match block {
            ContentBlock::Asset { asset_id, .. } => Some(asset_id.as_str()),
            _ => None,
        })
        .collect();
    let mut narrowed = published.clone();
    narrowed.event_ids.retain(|id| keep.contains(id.as_str()));
    narrowed
        .pinned_event_ids
        .retain(|id| keep.contains(id.as_str()));
    narrowed.asset_ids.retain(|id| assets.contains(id.as_str()));
    narrowed.validate_for_leaf(relations, events, endpoint)?;
    Ok(narrowed)
}

#[cfg(test)]
#[path = "continuation_scope_tests.rs"]
mod tests;

/// A second review may remove permissions, never manufacture new authority.
pub(super) fn narrow(
    ceiling: &AllowedSelection,
    requested: &AllowedSelection,
) -> Result<AllowedSelection, ImportError> {
    ceiling.validate_shape()?;
    requested.validate_shape()?;
    let subset = |values: &[String], bound: &[String]| {
        let bound: HashSet<&str> = bound.iter().map(String::as_str).collect();
        values.iter().all(|id| bound.contains(id.as_str()))
    };
    let a = &ceiling.fields;
    let b = &requested.fields;
    if !subset(&requested.event_ids, &ceiling.event_ids)
        || !subset(&requested.pinned_event_ids, &ceiling.pinned_event_ids)
        || !subset(&requested.asset_ids, &ceiling.asset_ids)
        || (requested.include_subagents && !ceiling.include_subagents)
        || (b.message_text && !a.message_text)
        || (b.tool_call_name && !a.tool_call_name)
        || (b.tool_call_input && !a.tool_call_input)
        || (b.tool_result_content && !a.tool_result_content)
        || (b.compaction_summary && !a.compaction_summary)
        || (b.visible_reasoning && !a.visible_reasoning)
    {
        return Err(ImportError::new(
            ErrorCode::HistoryAccessUnavailable,
            "continuation review exceeds its existing permission boundary",
        ));
    }
    Ok(requested.canonicalized())
}

/// Old sidecars resolve the full published scope. New sidecars bind an explicit
/// subset by its canonical hash; dropping or editing it cannot restore access.
pub(super) fn bound_selection(
    import_id: &str,
    revision: &str,
    leaf: &str,
    selection: Option<&AllowedSelection>,
    expected_hash: &str,
) -> Result<AllowedSelection, ImportError> {
    let auth = resolve(import_id, Some(revision), Some(leaf))?;
    let resolved = match selection {
        Some(selection) => narrow(&auth.selection, selection)?,
        None => auth.selection,
    };
    if resolved.hash() != expected_hash {
        return Err(ImportError::new(
            ErrorCode::HistoryAccessUnavailable,
            "continuation selection hash mismatch",
        ));
    }
    Ok(resolved)
}
