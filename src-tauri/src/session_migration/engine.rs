//! Scan / preview / plan / commit orchestration.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::adapters;
use super::budget::PREVIEW_TTL_SECS;
#[cfg(test)]
use super::catalog::list_entries;
use super::catalog::{
    get_entry, list_entries_page_filtered, list_entry_providers, load_draft_json, load_plan_json,
    open_catalog, persist_draft_json, persist_plan_json, CatalogEntry, CatalogSort,
    EntryPageFilter,
};
use super::continuation::{
    prepare_draft, AllowedSelection, ContinuationSidecar, DispatchState, HandoffDraft,
};
use super::cursor::{
    decode_bound_cursor, decode_optional_offset, encode_bound_cursor, CursorScope,
};
use super::detect::current_platform;
use super::errors::{ErrorCode, ImportError};
use super::ids::is_import_id;
#[cfg(test)]
use super::jobs::undo_job;
use super::jobs::{
    find_job_by_request, load_published_auth, CommitItem, JobRecord, JobState, PreparedRevision,
};
use super::search::indexed_events_by_ids;
#[cfg(test)]
use super::search::{indexed_event_metadata_by_ids, search_branch_ids, SearchHit};
#[cfg(test)]
use super::selection::PreviewSelection;
use super::selection::{validate_prepared_selection, PreparedSelectionIdentity, SelectionSet};
#[cfg(test)]
use super::types::SourceCandidate;
use super::types::{AdapterOutput, Provider};
use super::workspace::WorkspaceMapping;

pub use super::detect::default_roots as list_default_roots;
pub use super::preview_display::{PreviewEvent, PreviewFieldAvailability};
pub use super::scan::{
    page as scan_page, select_current as scan_select_current, state as scan_pump,
};
#[cfg(test)]
use super::scan::{scan_append_discoveries, scan_refresh, scan_start};
pub use super::scan::{ScanSourcePath, ScanState};

#[derive(Debug, Clone)]
struct PreviewState {
    pub source_member: Option<super::source_member::SourceMemberPin>,
    pub candidate_id: String,
    pub source_id: String,
    pub root_path: String,
    pub legacy_id: String,
    pub adapter: AdapterOutput,
    pub prepared: Arc<super::jobs::PreparedRevision>,
    pub expires: Instant,
    pub active_leaf: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewRelationSummary {
    pub parent_count: u32,
    pub fork_count: u32,
    pub subagent_count: u32,
    pub compaction_count: u32,
    pub rollback_count: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewAssetSummary {
    pub stored: u32,
    pub missing: u32,
    pub ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewDetails {
    pub title: Option<String>,
    pub cwd: Option<String>,
    pub active_leaf: Option<String>,
    pub leaves: Vec<String>,
    pub relation_summary: PreviewRelationSummary,
    pub default_selection: AllowedSelection,
    pub assets: PreviewAssetSummary,
    pub event_count: usize,
    pub events: Vec<PreviewEvent>,
    pub next_cursor: Option<String>,
    pub inventory: super::types::Inventory,
    pub issues: Vec<super::types::ImportIssue>,
    pub outcome: super::types::ImportOutcome,
}

fn preview_event(
    event: &super::types::ImportedEvent,
    default_ids: &HashSet<String>,
    subagents: &HashSet<&str>,
) -> PreviewEvent {
    let full = event.searchable_text();
    let text: String = full.chars().take(1024).collect();
    let asset_ids = event_asset_ids(event);
    let (display_parts, display_limited) = super::preview_display::ordered(event);
    let mut row = PreviewEvent {
        event_id: event.id().to_string(),
        kind: event.kind_name().into(),
        role: match event {
            super::types::ImportedEvent::Message { role, .. } => Some(*role),
            _ => None,
        },
        timestamp: event.base().timestamp.clone(),
        truncated: text.chars().count() < full.chars().count(),
        selectable: !super::selection::is_never_model_visible(event),
        default_selected: default_ids.contains(event.id()),
        subagent: subagents.contains(event.id()),
        field_availability: field_availability(event),
        citation: event.id().to_string(),
        asset_ids,
        display_parts,
        display_limited,
        text,
    };
    super::preview_display::bound_ordered(&mut row);
    row
}

fn event_asset_ids(event: &super::types::ImportedEvent) -> Vec<String> {
    let blocks = match event {
        super::types::ImportedEvent::Message { blocks, .. }
        | super::types::ImportedEvent::ToolResult { blocks, .. }
        | super::types::ImportedEvent::Compaction {
            summary: blocks, ..
        } => blocks.as_slice(),
        _ => return Vec::new(),
    };
    blocks
        .iter()
        .filter_map(|block| match block {
            super::types::ContentBlock::Asset { asset_id, .. } => Some(asset_id.clone()),
            _ => None,
        })
        .collect()
}

fn field_availability(event: &super::types::ImportedEvent) -> PreviewFieldAvailability {
    let mut avail = PreviewFieldAvailability {
        message_text: false,
        visible_reasoning: false,
        tool_call_name: false,
        tool_call_input: false,
        tool_result_content: false,
        compaction_summary: false,
        assets: false,
    };
    match event {
        super::types::ImportedEvent::Message { blocks, .. } => {
            avail.message_text = true;
            for block in blocks {
                match block {
                    super::types::ContentBlock::VisibleReasoning { .. } => {
                        avail.visible_reasoning = true;
                    }
                    super::types::ContentBlock::Asset { .. } => avail.assets = true,
                    _ => {}
                }
            }
        }
        super::types::ImportedEvent::ToolCall { .. } => {
            avail.tool_call_name = true;
            avail.tool_call_input = true;
        }
        super::types::ImportedEvent::ToolResult { blocks, .. } => {
            avail.tool_result_content = true;
            avail.assets = blocks
                .iter()
                .any(|b| matches!(b, super::types::ContentBlock::Asset { .. }));
        }
        super::types::ImportedEvent::Compaction { summary, .. } => {
            avail.compaction_summary = true;
            avail.visible_reasoning = summary
                .iter()
                .any(|block| matches!(block, super::types::ContentBlock::VisibleReasoning { .. }));
            avail.assets = summary
                .iter()
                .any(|block| matches!(block, super::types::ContentBlock::Asset { .. }));
        }
        _ => {}
    }
    avail
}

fn relation_summary(relations: &[super::types::EventRelation]) -> PreviewRelationSummary {
    let mut summary = PreviewRelationSummary {
        parent_count: 0,
        fork_count: 0,
        subagent_count: 0,
        compaction_count: 0,
        rollback_count: 0,
    };
    for relation in relations {
        match relation.kind {
            super::types::RelationKind::Parent => summary.parent_count += 1,
            super::types::RelationKind::Fork => summary.fork_count += 1,
            super::types::RelationKind::Subagent => summary.subagent_count += 1,
            super::types::RelationKind::Compacts => summary.compaction_count += 1,
            super::types::RelationKind::RollsBack => summary.rollback_count += 1,
        }
    }
    summary
}

fn asset_summary(assets: &[super::types::AssetRecord]) -> PreviewAssetSummary {
    PreviewAssetSummary {
        stored: assets
            .iter()
            .filter(|a| a.state == super::types::AssetState::Stored)
            .count() as u32,
        missing: assets
            .iter()
            .filter(|a| a.state != super::types::AssetState::Stored)
            .count() as u32,
        ids: assets.iter().map(|a| a.asset_id.clone()).take(50).collect(),
    }
}

fn preview_cursor_scope<'a>(preview_id: &'a str, leaf: &'a str) -> CursorScope<'a> {
    CursorScope {
        owner: preview_id,
        snapshot: preview_id,
        leaf,
        filter: "preview",
    }
}

fn encode_preview_cursor(preview_id: &str, leaf: &str, offset: u32) -> Result<String, ImportError> {
    encode_bound_cursor(preview_cursor_scope(preview_id, leaf), offset, None)
}

fn decode_preview_cursor(
    preview_id: &str,
    leaf: &str,
    cursor: Option<&str>,
) -> Result<u32, ImportError> {
    decode_optional_offset(cursor, preview_cursor_scope(preview_id, leaf))
}

#[path = "engine_preview_details.rs"]
mod engine_preview_details;
use engine_preview_details::preview_details_from_prepared;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanState {
    // Internal authority only; the established command response stays unchanged.
    #[serde(skip)]
    pub(in crate::session_migration) publication_protocol:
        super::publication_protocol::PublicationProtocol,
    #[serde(skip)]
    pub(in crate::session_migration) snapshots: HashMap<String, super::plan_snapshot::PlanSnapshot>,
    #[serde(skip)]
    pub(in crate::session_migration) identity_associations:
        Vec<super::identity_association::AssociationConfirmation>,
    pub plan_id: String,
    pub plan_hash: String,
    pub preview_ids: Vec<String>,
    pub mappings: Vec<WorkspaceMapping>,
    pub created: String,
    pub selection_set: SelectionSet,
    pub selection_hash: String,
}

struct Runtime {
    previews: HashMap<String, PreviewState>,
    drafts: HashMap<String, HandoffDraft>,
}

impl Runtime {
    fn new() -> Self {
        Self {
            previews: HashMap::new(),
            drafts: HashMap::new(),
        }
    }
}

static RUNTIME: Mutex<Option<Runtime>> = Mutex::new(None);

fn rt<T>(f: impl FnOnce(&mut Runtime) -> T) -> T {
    let mut g = RUNTIME.lock().unwrap_or_else(|p| p.into_inner());
    if g.is_none() {
        *g = Some(Runtime::new());
    }
    f(g.as_mut().unwrap())
}

fn drop_expired_previews() {
    rt(|r| {
        r.previews
            .retain(|_, preview| preview.expires > Instant::now());
    });
}

/// Drop expired RAM previews and delete unreferenced staging snapshots.
pub fn cleanup_preview_leases() -> Result<u32, ImportError> {
    drop_expired_previews();
    super::jobs::cleanup_unreferenced_preview_dirs()
}

#[cfg(test)]
pub fn expire_preview(preview_id: &str) {
    rt(|r| {
        if let Some(preview) = r.previews.get_mut(preview_id) {
            preview.expires = Instant::now() - Duration::from_secs(1);
            let _ = super::jobs::expire_preview_lease_file(&preview.prepared.path);
        }
    });
}

#[cfg(test)]
pub fn reset_runtime() {
    let mut guard = RUNTIME.lock().unwrap_or_else(|p| p.into_inner());
    *guard = None;
}

#[cfg(test)]
pub fn forget_preview(preview_id: &str) {
    rt(|r| {
        r.previews.remove(preview_id);
    });
}

/// Retry uses the persisted input, including after previews expire or restart.
#[cfg(test)]
pub fn retry_failed(job_id: &str) -> Result<JobRecord, ImportError> {
    super::job_control::retry(job_id)?;
    job_get(job_id)
}

pub fn preview(
    candidate_id: &str,
    expected_source_revision: Option<&str>,
) -> Result<(String, PreviewDetails), ImportError> {
    let reference = super::scan::candidate(candidate_id)?;
    preview_reference(reference, expected_source_revision, None)
}

pub(crate) fn preview_reference(
    reference: super::scan::CandidateRef,
    expected_source_revision: Option<&str>,
    prepare_batch_id: Option<&str>,
) -> Result<(String, PreviewDetails), ImportError> {
    let origin = if prepare_batch_id.is_some() {
        super::preview_gate::PreviewOrigin::Batch
    } else {
        super::preview_gate::PreviewOrigin::Manual
    };
    let _permit = super::preview_gate::acquire(origin);
    let _operation = super::preview_lifecycle::operation()?;
    super::source_locations::validate_reference(&reference)?;
    super::source_member::validate_reference(&reference)?;
    let preview_id = Uuid::new_v4().to_string();
    let candidate_id = reference.candidate.candidate_id.clone();
    let cand = reference.candidate;
    if let Some(exp) = expected_source_revision {
        if exp != cand.source_revision {
            return Err(ImportError::new(
                ErrorCode::SourceChanged,
                "source revision changed since listing",
            ));
        }
    }
    let (path, composer_id) = if cand.provider == Provider::Cursor {
        adapters::cursor::split_composer_locator(&cand.locator)
    } else {
        (PathBuf::from(&cand.locator), None)
    };
    let resolved = super::sources::canonical_root(&path)?;
    if !resolved.starts_with(&reference.root_path) {
        return Err(ImportError::new(
            ErrorCode::SourceChanged,
            "artifact moved outside the registered source",
        ));
    }
    let metadata_hint = cand.source_revision.starts_with("metadata-v1:");
    if metadata_hint && super::discovery::metadata_revision(&path)? != cand.source_revision {
        return Err(ImportError::new(
            ErrorCode::SourceChanged,
            "source changed since metadata discovery",
        ));
    }
    let options = serde_json::json!({"rawRetention": "normalized-only"});
    let (mut adapter, streamed) = if cand.provider == Provider::GenericFile {
        let (adapter, prepared) = adapters::generic::prepare_file_in_root(
            &path,
            Some(Path::new(&reference.root_path)),
            &cand.native_session_id,
            &options,
            Some(&preview_id),
        )?;
        (adapter, Some(prepared))
    } else if let Some(id) = composer_id {
        (adapters::cursor::normalize_composer(&path, Some(id))?, None)
    } else {
        (
            adapters::normalize_in_location(
                cand.provider,
                &path,
                Some(Path::new(&reference.root_path)),
                reference.location.as_ref().and_then(|l| l.identity_scope()),
            )?,
            None,
        )
    };
    if cand.provider == Provider::Cursor
        && adapter.graph.native_session_id != cand.native_session_id
    {
        return Err(ImportError::new(
            ErrorCode::SourceChanged,
            "Cursor identity differs from its fixed discovery reference",
        ));
    }
    let current = adapter
        .evidence
        .artifacts
        .first()
        .map(|a| {
            a.complete_prefix_sha256
                .as_ref()
                .unwrap_or(&a.sha256)
                .as_str()
        })
        .ok_or_else(|| {
            ImportError::new(
                ErrorCode::SnapshotUnavailable,
                "adapter has no snapshot evidence",
            )
        })?;
    if metadata_hint {
        // This hint is not a full-content digest. Normalization above fixes and
        // verifies the full source snapshot; the hint only detects stale listing.
        if super::discovery::metadata_revision(&path)? != cand.source_revision {
            return Err(ImportError::new(
                ErrorCode::SourceChanged,
                "source changed during metadata preview",
            ));
        }
    } else if let Some(exp) = expected_source_revision {
        if exp != current {
            return Err(ImportError::new(
                ErrorCode::SourceChanged,
                "source changed during preview",
            ));
        }
    }
    let (mut prepared, headers) = if let Some(streamed) = streamed {
        (streamed.prepared, streamed.event_headers)
    } else {
        adapter.validate_graph();
        let headers = adapter
            .graph
            .events
            .iter()
            .map(|event| super::prepared_builder::PreparedEventHeader {
                event_id: event.id().to_string(),
                ordinal: event.base().ordinal,
                model_visible: !super::selection::is_never_model_visible(event),
            })
            .collect();
        let prepared = super::jobs::prepare_revision(
            &mut adapter,
            &options,
            Some(Path::new(&reference.root_path)),
            Some(&preview_id),
        )?;
        (prepared, headers)
    };
    super::source_member::seal_preview(&mut prepared, reference.source_member.as_ref())?;
    // Preparation above materializes assets and validates/indexes the entire
    // fixed snapshot. Batch workers need metadata, not display-only event bodies.
    let mut details = preview_details_from_prepared(
        &adapter,
        &prepared.path,
        &headers,
        prepare_batch_id.is_none(),
    )?;
    // The fixed snapshot lives on disk. Batch previews retain metadata and an
    // Arc lease, not a second copy of every conversation's body in memory.
    adapter.graph.events.clear();
    adapter.graph.events.shrink_to_fit();
    let expires = Instant::now() + Duration::from_secs(PREVIEW_TTL_SECS);
    #[cfg(test)]
    super::preview_lifecycle_tests::after_publish_point("preview_before_lease");
    super::jobs::write_preview_lease(
        &prepared.path,
        &super::jobs::PreviewLease {
            source_member: reference.source_member.clone(),
            preview_id: preview_id.clone(),
            candidate_id: candidate_id.clone(),
            source_revision: cand.source_revision.clone(),
            prepare_batch_id: prepare_batch_id.map(str::to_string),
            source_id: reference.source_id.clone(),
            root_path: reference.root_path.clone(),
            legacy_id: reference.legacy_id.clone(),
            snapshot_digest: prepared.digest.clone(),
            expires_unix: chrono::Utc::now().timestamp() + PREVIEW_TTL_SECS as i64,
            retained_by_job: false,
            file_hashes: prepared.file_hashes.clone(),
        },
    )?;
    #[cfg(test)]
    super::preview_lifecycle_tests::after_publish_point("preview_after_lease");
    let state = PreviewState {
        source_member: reference.source_member,
        candidate_id,
        source_id: reference.source_id,
        root_path: reference.root_path,
        legacy_id: reference.legacy_id,
        adapter,
        prepared,
        expires,
        active_leaf: details.active_leaf.clone().unwrap_or_default(),
    };
    rt(|r| {
        r.previews.insert(preview_id.clone(), state);
    });
    if details.events.len() < details.event_count {
        let leaf = details.active_leaf.clone().unwrap_or_default();
        details.next_cursor = Some(encode_preview_cursor(
            &preview_id,
            &leaf,
            details.events.len() as u32,
        )?);
    }
    Ok((preview_id, details))
}

/// Later preview pages come from the prepared index, not a second full graph in IPC.
pub fn preview_events_page(
    preview_id: &str,
    cursor: Option<&str>,
    limit: Option<usize>,
    leaf: Option<&str>,
) -> Result<(Vec<PreviewEvent>, Option<String>), ImportError> {
    let _operation = super::preview_lifecycle::operation()?;
    let path = preview_snapshot_path(preview_id)?;
    let memory_leaf = rt(|r| {
        r.previews
            .get(preview_id)
            .map(|p| p.active_leaf.clone())
            .unwrap_or_default()
    });
    let stored_leaf = if memory_leaf.is_empty() {
        preview_leaf(preview_id, &path)?
    } else {
        memory_leaf
    };
    let relations = relations_from_dir(&path)?;
    let requested_leaf = leaf.filter(|s| !s.is_empty()).map(str::to_string);
    let leaf = requested_leaf
        .clone()
        .or_else(|| {
            cursor.and_then(|raw| {
                serde_json::from_str::<(String, String, u32)>(raw)
                    .ok()
                    .map(|(_, l, _)| l)
            })
        })
        .filter(|s| !s.is_empty())
        .unwrap_or(stored_leaf);
    if leaf.is_empty() {
        return Err(ImportError::new(
            ErrorCode::HistoryAccessUnavailable,
            "preview leaf is required",
        ));
    }
    let offset = decode_preview_cursor(preview_id, &leaf, cursor)?;
    if requested_leaf.is_some() {
        rt(|r| {
            if let Some(preview) = r.previews.get_mut(preview_id) {
                preview.active_leaf = leaf.clone();
            }
        });
    }
    let chain = super::graph::lineage_ids(&relations, &leaf);
    let cap = super::budget::page_limit(limit).min(super::budget::READ_PAGE_EVENTS);
    if offset as usize > chain.len() {
        return Err(ImportError::new(
            ErrorCode::PlanStale,
            "cursor past end of branch",
        ));
    }
    let page_ids: Vec<String> = chain
        .iter()
        .skip(offset as usize)
        .take(cap)
        .cloned()
        .collect();
    let events = indexed_events_by_ids(&path, &page_ids)?;
    let subagents = super::selection::subagent_member_ids(&relations);
    // Default flags are per event on this lineage page. Do not call
    // default_selection on the page slice — walking from leaf would stop at
    // the first missing parent and mark earlier pages unselected.
    let default_ids: HashSet<String> = events
        .iter()
        .filter(|event| {
            !super::selection::is_never_model_visible(event) && !subagents.contains(event.id())
        })
        .map(|event| event.id().to_string())
        .collect();
    let rows: Vec<PreviewEvent> = events
        .iter()
        .map(|event| preview_event(event, &default_ids, &subagents))
        .collect();
    let next = if offset as usize + rows.len() < chain.len() {
        Some(encode_preview_cursor(
            preview_id,
            &leaf,
            offset + rows.len() as u32,
        )?)
    } else {
        None
    };
    Ok((rows, next))
}

fn relations_from_dir(
    path: &std::path::Path,
) -> Result<Vec<super::types::EventRelation>, ImportError> {
    let text = std::fs::read_to_string(path.join("manifest.json"))
        .map_err(|e| ImportError::new(ErrorCode::SnapshotUnavailable, e.to_string()))?;
    let v: serde_json::Value = serde_json::from_str(&text)
        .map_err(|e| ImportError::new(ErrorCode::SnapshotUnavailable, e.to_string()))?;
    serde_json::from_value(v.pointer("/graph/relations").cloned().ok_or_else(|| {
        ImportError::new(ErrorCode::SnapshotUnavailable, "archive relations missing")
    })?)
    .map_err(|_| ImportError::new(ErrorCode::SnapshotUnavailable, "invalid archive relations"))
}

fn preview_snapshot_path(preview_id: &str) -> Result<PathBuf, ImportError> {
    if let Some(preview) = rt(|r| r.previews.get(preview_id).cloned()) {
        if preview.expires > Instant::now()
            || super::preview_store::batch_retains(&preview.prepared.path, preview_id)?
        {
            return Ok(preview.prepared.path.clone());
        }
        return Err(ImportError::new(ErrorCode::PlanStale, "preview expired"));
    }
    super::preview_store::disk_snapshot_path(preview_id)
}

pub(crate) fn comparison_snapshot_path(preview_id: &str) -> Result<PathBuf, ImportError> {
    let path = preview_snapshot_path(preview_id)?;
    if let Some(preview) = rt(|runtime| runtime.previews.get(preview_id).cloned()) {
        super::source_member::require_preview_pin(&path, preview.source_member.as_ref())?;
    }
    Ok(path)
}

#[cfg(test)]
pub fn plan(
    preview_ids: Vec<String>,
    mappings: Vec<WorkspaceMapping>,
) -> Result<PlanState, ImportError> {
    let selection_set = default_selection_set(&preview_ids)?;
    plan_with_selection_set(preview_ids, mappings, selection_set)
}

#[cfg(test)]
pub fn plan_with_selection(
    preview_ids: Vec<String>,
    mappings: Vec<WorkspaceMapping>,
    selection: Option<&AllowedSelection>,
) -> Result<PlanState, ImportError> {
    let selection_set = match selection {
        Some(sel) if preview_ids.len() == 1 => {
            let path = preview_snapshot_path(&preview_ids[0])?;
            let leaf = preview_leaf(&preview_ids[0], &path)?;
            SelectionSet {
                entries: vec![PreviewSelection {
                    preview_id: preview_ids[0].clone(),
                    leaf_id: leaf,
                    selection: sel.clone(),
                }],
                ..SelectionSet::default()
            }
        }
        Some(_) => {
            return Err(ImportError::new(
                ErrorCode::HistoryAccessUnavailable,
                "each preview needs its own selection",
            ));
        }
        None => default_selection_set(&preview_ids)?,
    };
    plan_with_selection_set(preview_ids, mappings, selection_set)
}

#[cfg(test)]
pub fn plan_with_selections(
    preview_ids: Vec<String>,
    mappings: Vec<WorkspaceMapping>,
    selections: Option<Vec<PreviewSelection>>,
) -> Result<PlanState, ImportError> {
    let selection_set = match selections {
        Some(entries) => SelectionSet {
            entries,
            ..SelectionSet::default()
        },
        None => default_selection_set(&preview_ids)?,
    };
    plan_with_selection_set(preview_ids, mappings, selection_set)
}

fn preview_leaf(preview_id: &str, path: &Path) -> Result<String, ImportError> {
    let memory_leaf = rt(|runtime| {
        runtime
            .previews
            .get(preview_id)
            .map(|preview| preview.active_leaf.clone())
    });
    if let Some(leaf) = memory_leaf.filter(|leaf| !leaf.is_empty()) {
        return Ok(leaf);
    }
    let manifest = std::fs::read(path.join("manifest.json"))
        .map_err(|error| ImportError::new(ErrorCode::SnapshotUnavailable, error.to_string()))?;
    let value: serde_json::Value = serde_json::from_slice(&manifest)
        .map_err(|error| ImportError::new(ErrorCode::SnapshotUnavailable, error.to_string()))?;
    if let Some(leaf) = value
        .pointer("/graph/activeLeaf")
        .and_then(serde_json::Value::as_str)
        .filter(|leaf| !leaf.is_empty())
    {
        return Ok(leaf.to_string());
    }
    super::search::last_indexed_event_id(path)?
        .ok_or_else(|| ImportError::new(ErrorCode::PartialHistory, "preview has no branch leaf"))
}

#[cfg(test)]
fn default_selection_set(preview_ids: &[String]) -> Result<SelectionSet, ImportError> {
    let mut entries = Vec::with_capacity(preview_ids.len());
    for preview_id in preview_ids {
        let path = preview_snapshot_path(preview_id)?;
        let leaf_id = preview_leaf(preview_id, &path)?;
        let relations = relations_from_dir(&path)?;
        let lineage = super::graph::lineage_ids(&relations, &leaf_id);
        let metadata = indexed_event_metadata_by_ids(&path, &lineage)?;
        if metadata.len() != lineage.len() {
            return Err(ImportError::new(
                ErrorCode::SnapshotUnavailable,
                "preview branch references a missing event",
            ));
        }
        let model_visible: HashSet<&str> = metadata
            .iter()
            .filter(|event| event.model_visible)
            .map(|event| event.event_id.as_str())
            .collect();
        let subagents = super::selection::subagent_member_ids(&relations);
        entries.push(PreviewSelection {
            preview_id: preview_id.clone(),
            leaf_id: leaf_id.clone(),
            selection: AllowedSelection {
                event_ids: lineage
                    .into_iter()
                    .filter(|event_id| {
                        model_visible.contains(event_id.as_str())
                            && !subagents.contains(event_id.as_str())
                    })
                    .collect(),
                ..AllowedSelection::default()
            },
        });
    }
    Ok(SelectionSet {
        entries,
        ..SelectionSet::default()
    })
}

pub fn plan_with_selection_set(
    preview_ids: Vec<String>,
    mappings: Vec<WorkspaceMapping>,
    selection_set: SelectionSet,
) -> Result<PlanState, ImportError> {
    plan_with_identity_associations(preview_ids, mappings, selection_set, vec![])
}

pub(super) fn plan_with_identity_associations(
    preview_ids: Vec<String>,
    mappings: Vec<WorkspaceMapping>,
    selection_set: SelectionSet,
    identity_associations: Vec<super::identity_association::AssociationConfirmation>,
) -> Result<PlanState, ImportError> {
    let _operation = super::preview_lifecycle::operation()?;
    // A refused confirmation must not renew/retain/reclaim any preview lease.
    if identity_associations.is_empty() {
        drop_expired_previews();
    }
    if preview_ids.is_empty() {
        return Err(ImportError::new(
            ErrorCode::HistoryAccessUnavailable,
            "plan requires at least one preview",
        ));
    }
    // Reject duplicate or malformed raw IDs before canonicalization can erase
    // evidence of a confused/deceptive wire value.
    selection_set.validate_shape()?;
    let expected: HashSet<&str> = preview_ids.iter().map(String::as_str).collect();
    let actual: HashSet<&str> = selection_set
        .entries
        .iter()
        .map(|entry| entry.preview_id.as_str())
        .collect();
    if expected.len() != preview_ids.len()
        || actual.len() != selection_set.entries.len()
        || expected != actual
    {
        return Err(ImportError::new(
            ErrorCode::HistoryAccessUnavailable,
            "plan selections must cover each preview exactly once",
        ));
    }

    let mut snapshots = Vec::with_capacity(selection_set.entries.len());
    for entry in &selection_set.entries {
        let path = preview_snapshot_path(&entry.preview_id)?;
        let verified = validate_prepared_selection(
            entry,
            &path,
            PreparedSelectionIdentity {
                preview_id: Some(&entry.preview_id),
                ..PreparedSelectionIdentity::default()
            },
        )?;
        snapshots.push(engine_prepare_plan::VerifiedPlanSnapshot {
            mapping_subject: super::mapping_scope::MappingSubject::new(
                verified.source_id,
                verified.provider,
                verified.cwd,
            ),
            preview_id: entry.preview_id.clone(),
            snapshot_digest: verified.snapshot_digest,
            source_member: verified.source_member,
            path,
        });
    }
    if identity_associations.is_empty() {
        engine_prepare_plan::persist_verified_plan(preview_ids, mappings, selection_set, snapshots)
    } else {
        engine_prepare_plan::persist_verified_association_plan(
            preview_ids,
            mappings,
            selection_set,
            snapshots,
            serde_json::json!({}),
            identity_associations,
        )
    }
}

const PLAN_RECORD_VERSION: u32 = 1;

fn canonical_plan_payload(
    preview_ids: &[String],
    mappings: &[WorkspaceMapping],
    snapshots: &[serde_json::Value],
    selection_set: &SelectionSet,
    selection_hash: &str,
    options: &serde_json::Value,
) -> serde_json::Value {
    serde_json::json!({
        "version": PLAN_RECORD_VERSION,
        "previewIds": preview_ids,
        "mappings": mappings,
        "snapshots": snapshots,
        "selectionSet": selection_set,
        "selectionHash": selection_hash,
        "options": options,
    })
}

#[path = "engine_prepare_plan.rs"]
mod engine_prepare_plan;
pub(crate) use engine_prepare_plan::plan_from_validated_prepare;
pub(crate) use engine_prepare_plan::plan_from_validated_prepare_with_associations;
#[cfg(test)]
pub(in crate::session_migration) use engine_prepare_plan::take_durable_plan_loads;
pub(in crate::session_migration) use engine_prepare_plan::{
    association_input_plan, job_entry_plan, job_entry_plan_json,
};

#[path = "engine_commit.rs"]
mod engine_commit;
pub use engine_commit::*;

#[cfg(test)]
#[path = "engine_legacy_job_fixture.rs"]
mod legacy_job_fixture;
#[cfg(test)]
pub(super) use legacy_job_fixture::enqueue as enqueue_legacy_plan;
#[cfg(test)]
use legacy_job_fixture::historical_plan as historical_legacy_plan;

#[path = "engine_continue.rs"]
mod engine_continue;
pub use engine_continue::*;

#[path = "engine_recovery.rs"]
mod engine_recovery;
pub use engine_recovery::*;

#[cfg(test)]
#[path = "preview_lease_tests.rs"]
mod preview_lease_tests;

#[cfg(test)]
#[path = "engine_plan_storage_tests.rs"]
mod plan_storage_tests;

#[cfg(test)]
#[path = "engine_batch_preview_tests.rs"]
mod batch_preview_tests;

#[cfg(test)]
#[path = "engine_title_tests.rs"]
mod title_tests;

#[cfg(test)]
#[path = "engine_source_fidelity_tests.rs"]
mod source_fidelity_tests;

#[cfg(test)]
#[path = "engine_damage_tests.rs"]
mod damage_tests;

#[cfg(test)]
#[path = "engine_codex_native_tests.rs"]
mod codex_native_tests;

#[cfg(test)]
#[path = "engine_cursor_identity_tests.rs"]
mod cursor_identity_source_fidelity_tests;

#[cfg(test)]
#[path = "engine_legacy_identity_tests.rs"]
mod legacy_identity_tests;

#[cfg(test)]
pub(super) use legacy_identity_tests::association_crash_child;

#[cfg(test)]
#[path = "engine_source_member_tests.rs"]
mod member_pin_source_fidelity_tests;

#[cfg(test)]
#[path = "published_auth_tests.rs"]
mod published_auth_tests;

#[cfg(test)]
#[path = "bounded_io_tests.rs"]
mod bounded_io_tests;

#[cfg(test)]
#[path = "harness_tests.rs"]
mod harness_tests;

#[cfg(test)]
#[path = "recovery_tests.rs"]
mod recovery_tests;

#[cfg(test)]
mod tests {
    use super::*;
    include!("engine_tests_a.rs");
    include!("engine_tests_b.rs");
    include!("engine_tests_c.rs");
}
