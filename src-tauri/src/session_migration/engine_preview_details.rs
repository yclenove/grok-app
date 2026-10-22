//! Shared metadata and optional first-page display projection of a fixed preview.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use super::{asset_summary, preview_event, relation_summary, PreviewDetails, PreviewEvent};
use crate::session_migration::continuation::AllowedSelection;
use crate::session_migration::errors::{ErrorCode, ImportError};
use crate::session_migration::prepared_builder::PreparedEventHeader;
use crate::session_migration::types::{AdapterOutput, RelationKind};
use crate::session_migration::{budget, graph, search, selection};

pub(super) fn preview_details_from_prepared(
    adapter: &AdapterOutput,
    prepared: &Path,
    headers: &[PreparedEventHeader],
    materialize_first_page: bool,
) -> Result<PreviewDetails, ImportError> {
    let leaf = adapter
        .graph
        .active_leaf
        .clone()
        .or_else(|| headers.last().map(|event| event.event_id.clone()))
        .unwrap_or_default();
    let lineage = if leaf.is_empty() {
        Vec::new()
    } else {
        graph::lineage_ids(&adapter.graph.relations, &leaf)
    };
    let visible: HashMap<&str, bool> = headers
        .iter()
        .map(|event| (event.event_id.as_str(), event.model_visible))
        .collect();
    // Headers come from the writer that just finalized and indexed every record.
    // Do not silently drop broken parent references when bodies are deferred.
    if lineage.iter().any(|id| !visible.contains_key(id.as_str())) {
        return Err(missing_event());
    }
    let subagents = selection::subagent_member_ids(&adapter.graph.relations);
    let default = AllowedSelection {
        event_ids: lineage
            .iter()
            .filter(|event_id| {
                visible.get(event_id.as_str()).copied().unwrap_or(false)
                    && !subagents.contains(event_id.as_str())
            })
            .cloned()
            .collect(),
        ..AllowedSelection::default()
    };
    let events = if materialize_first_page {
        first_page(prepared, &lineage, &default, &subagents)?
    } else {
        Vec::new()
    };
    let parent_ids: HashSet<&str> = adapter
        .graph
        .relations
        .iter()
        .filter(|relation| relation.kind == RelationKind::Parent)
        .map(|relation| relation.to.as_str())
        .collect();
    Ok(PreviewDetails {
        title: adapter.graph.title.clone(),
        cwd: adapter.graph.cwd.clone(),
        active_leaf: adapter.graph.active_leaf.clone().or(Some(leaf)),
        leaves: headers
            .iter()
            .filter(|event| !parent_ids.contains(event.event_id.as_str()))
            .map(|event| event.event_id.clone())
            .collect(),
        relation_summary: relation_summary(&adapter.graph.relations),
        default_selection: default,
        assets: asset_summary(&adapter.assets),
        event_count: headers.len(),
        events,
        next_cursor: None,
        inventory: adapter.inventory.clone(),
        issues: adapter.issues.clone(),
        outcome: adapter.outcome,
    })
}

fn first_page(
    prepared: &Path,
    lineage: &[String],
    default: &AllowedSelection,
    subagents: &HashSet<&str>,
) -> Result<Vec<PreviewEvent>, ImportError> {
    let default_ids: HashSet<String> = default.event_ids.iter().cloned().collect();
    let mut bytes = 0;
    let mut events = Vec::new();
    for event_id in lineage.iter().take(budget::READ_PAGE_EVENTS) {
        let event = search::indexed_event_by_id(prepared, event_id)?.ok_or_else(missing_event)?;
        let item = preview_event(&event, &default_ids, subagents);
        if bytes + item.text.len() > budget::READ_PAGE_TEXT_BYTES {
            break;
        }
        bytes += item.text.len();
        events.push(item);
    }
    Ok(events)
}

fn missing_event() -> ImportError {
    ImportError::new(
        ErrorCode::SnapshotUnavailable,
        "preview branch references a missing event",
    )
}
