//! Bounded, branch-aware event lookup for the imported-history reader.
//!
//! The reader must be able to jump to an indexed event without walking every
//! page from the current cursor.  This module keeps that path deliberately
//! small: relations determine membership and order, while the immutable event
//! index supplies only the requested context window.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use serde::{Deserialize, Serialize};

use super::cursor::{decode_bound_cursor, encode_bound_cursor, CursorScope};
use super::errors::{ErrorCode, ImportError};
use super::graph::lineage_ids;
use super::reader_health::{self, RevisionHealth};
use super::search::{indexed_event_by_id, indexed_event_metadata_by_ids, indexed_events_by_ids};
use super::types::{EventRelation, ImportedEvent};

pub const CONTEXT_CURSOR_FILTER: &str = "reader-context";
pub const CONTEXT_MAX_SIDE_EVENTS: usize = 32;
pub const READER_RESPONSE_EVENTS_BYTES: usize = super::budget::READ_PAGE_TEXT_BYTES;
const BRANCH_CACHE_ENTRIES: usize = 8;
const BRANCH_CACHE_TOTAL_IDS: usize = 200_000;
const MANIFEST_CACHE_ENTRIES: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
struct ManifestStamp {
    path: PathBuf,
    len: u64,
    modified_nanos: u128,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct BranchStamp {
    manifest: ManifestStamp,
    leaf: String,
}

#[derive(Debug, Clone)]
struct CachedBranch {
    stamp: BranchStamp,
    ids: Arc<Vec<String>>,
}

#[derive(Debug, Clone)]
struct CachedManifest {
    stamp: ManifestStamp,
    relations: Arc<Vec<EventRelation>>,
    active_leaf: Option<String>,
    health: RevisionHealth,
}

type CachedManifestSnapshot = (
    ManifestStamp,
    Arc<Vec<EventRelation>>,
    Option<String>,
    RevisionHealth,
);

static BRANCH_CACHE: OnceLock<Mutex<VecDeque<CachedBranch>>> = OnceLock::new();
static MANIFEST_CACHE: OnceLock<Mutex<VecDeque<CachedManifest>>> = OnceLock::new();

#[cfg(test)]
thread_local! {
    static LINEAGE_BUILDS: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    static MANIFEST_PARSES: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EventContextRequest {
    pub event_id: String,
    #[serde(default)]
    pub before: Option<usize>,
    #[serde(default)]
    pub after: Option<usize>,
    #[serde(default)]
    pub cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EventContextResponse {
    pub events: Vec<serde_json::Value>,
    pub health: RevisionHealth,
    pub target_index: u32,
    pub ordinal: u32,
    pub position: u32,
    pub total: u32,
    pub revision: String,
    pub leaf: String,
    pub anchor_cursor: String,
    pub previous_cursor: Option<String>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EventPageResponse {
    pub events: Vec<serde_json::Value>,
    pub health: RevisionHealth,
    pub previous_cursor: Option<String>,
    pub next_cursor: Option<String>,
    pub total: u32,
    pub start: u32,
    pub end: u32,
    pub revision: String,
    pub leaf: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageDirection {
    Forward,
    Backward,
}

impl PageDirection {
    pub fn parse(value: Option<&str>) -> Result<Self, ImportError> {
        match value.filter(|value| !value.trim().is_empty()) {
            None | Some("forward") => Ok(Self::Forward),
            Some("backward") => Ok(Self::Backward),
            Some(_) => Err(ImportError::new(
                ErrorCode::PlanStale,
                "invalid reader page direction",
            )),
        }
    }

    fn filter(self) -> &'static str {
        match self {
            Self::Forward => "lineage-forward",
            Self::Backward => "lineage-backward",
        }
    }
}

fn manifest_stamp(revision_dir: &Path) -> Result<ManifestStamp, ImportError> {
    let manifest = revision_dir.join("manifest.json");
    let metadata = std::fs::symlink_metadata(&manifest).map_err(|error| {
        ImportError::new(
            ErrorCode::SnapshotUnavailable,
            format!("stat archive manifest: {error}"),
        )
    })?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(ImportError::new(
            ErrorCode::HistoryAccessUnavailable,
            "archive manifest is not a regular file",
        ));
    }
    let modified_nanos = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    Ok(ManifestStamp {
        path: revision_dir.to_path_buf(),
        len: metadata.len(),
        modified_nanos,
    })
}

fn parse_manifest(
    revision_dir: &Path,
) -> Result<(Vec<EventRelation>, Option<String>, RevisionHealth), ImportError> {
    #[cfg(test)]
    MANIFEST_PARSES.with(|count| count.set(count.get().saturating_add(1)));
    let raw = std::fs::read_to_string(revision_dir.join("manifest.json")).map_err(|error| {
        ImportError::new(
            ErrorCode::SnapshotUnavailable,
            format!("read archive manifest: {error}"),
        )
    })?;
    let value: serde_json::Value = serde_json::from_str(&raw).map_err(|error| {
        ImportError::new(
            ErrorCode::SnapshotUnavailable,
            format!("parse archive manifest: {error}"),
        )
    })?;
    let relations =
        serde_json::from_value(value.pointer("/graph/relations").cloned().ok_or_else(|| {
            ImportError::new(ErrorCode::SnapshotUnavailable, "archive relations missing")
        })?)
        .map_err(|_| {
            ImportError::new(ErrorCode::SnapshotUnavailable, "invalid archive relations")
        })?;
    let active_leaf = value
        .pointer("/graph/activeLeaf")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned);
    Ok((relations, active_leaf, reader_health::from_manifest(&value)))
}

fn cached_manifest(revision_dir: &Path) -> Result<CachedManifestSnapshot, ImportError> {
    let before = manifest_stamp(revision_dir)?;
    let cache = MANIFEST_CACHE.get_or_init(|| Mutex::new(VecDeque::new()));
    {
        let mut entries = cache.lock().map_err(|_| {
            ImportError::new(
                ErrorCode::SnapshotUnavailable,
                "reader manifest cache is unavailable",
            )
        })?;
        if let Some(position) = entries.iter().position(|entry| entry.stamp == before) {
            let entry = entries
                .remove(position)
                .expect("located manifest cache entry");
            let result = (
                entry.stamp.clone(),
                Arc::clone(&entry.relations),
                entry.active_leaf.clone(),
                entry.health.clone(),
            );
            entries.push_back(entry);
            return Ok(result);
        }
    }

    let (relations, active_leaf, health) = parse_manifest(revision_dir)?;
    let after = manifest_stamp(revision_dir)?;
    if before != after {
        return Err(ImportError::new(
            ErrorCode::HistoryAccessUnavailable,
            "archive manifest changed while it was read",
        ));
    }
    let relations = Arc::new(relations);
    let mut entries = cache.lock().map_err(|_| {
        ImportError::new(
            ErrorCode::SnapshotUnavailable,
            "reader manifest cache is unavailable",
        )
    })?;
    entries.retain(|entry| entry.stamp.path != after.path);
    while entries.len() >= MANIFEST_CACHE_ENTRIES {
        entries.pop_front();
    }
    entries.push_back(CachedManifest {
        stamp: after.clone(),
        relations: Arc::clone(&relations),
        active_leaf: active_leaf.clone(),
        health: health.clone(),
    });
    Ok((after, relations, active_leaf, health))
}

fn reader_events_by_ids(
    revision_dir: &Path,
    ids: &[String],
) -> Result<Vec<serde_json::Value>, ImportError> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let metadata = indexed_event_metadata_by_ids(revision_dir, ids)?;
    if metadata.len() != ids.len() {
        return Err(ImportError::new(
            ErrorCode::HistoryAccessUnavailable,
            "reader branch index is incomplete",
        ));
    }
    let by_id: HashMap<_, _> = metadata
        .into_iter()
        .map(|event| (event.event_id.clone(), event))
        .collect();
    let mut values = Vec::with_capacity(ids.len());
    let mut stub_sizes = Vec::with_capacity(ids.len());
    for id in ids {
        let event = by_id.get(id).ok_or_else(|| {
            ImportError::new(
                ErrorCode::HistoryAccessUnavailable,
                "reader branch index is incomplete",
            )
        })?;
        let value = serde_json::json!({
            "id": event.event_id,
            "ordinal": event.ordinal,
            "kind": event.kind,
            "truncated": true,
        });
        let size = serde_json::to_vec(&value)
            .map_err(|error| ImportError::new(ErrorCode::SnapshotUnavailable, error.to_string()))?
            .len();
        values.push(value);
        stub_sizes.push(size);
    }
    let base_size = serde_json::to_vec(&values)
        .map_err(|error| ImportError::new(ErrorCode::SnapshotUnavailable, error.to_string()))?
        .len();
    if base_size > READER_RESPONSE_EVENTS_BYTES {
        return Err(ImportError::new(
            ErrorCode::ResourceLimit,
            "reader event identities exceed the response budget",
        ));
    }

    let mut remaining = READER_RESPONSE_EVENTS_BYTES - base_size;
    let mut full_ids = Vec::new();
    for (index, id) in ids.iter().enumerate() {
        let event = &by_id[id];
        let extra = event.json_len.saturating_sub(stub_sizes[index]);
        if extra <= remaining {
            remaining -= extra;
            full_ids.push(id.clone());
        }
    }
    let full = indexed_events_by_ids(revision_dir, &full_ids)?;
    if full.len() != full_ids.len() {
        return Err(ImportError::new(
            ErrorCode::HistoryAccessUnavailable,
            "reader branch index is incomplete",
        ));
    }
    let mut full_by_id: HashMap<String, ImportedEvent> = full
        .into_iter()
        .map(|event| (event.id().to_owned(), event))
        .collect();
    for (index, id) in ids.iter().enumerate() {
        if let Some(event) = full_by_id.remove(id) {
            values[index] = serde_json::to_value(event).map_err(|error| {
                ImportError::new(ErrorCode::SnapshotUnavailable, error.to_string())
            })?;
        }
    }

    while serde_json::to_vec(&values)
        .map_err(|error| ImportError::new(ErrorCode::SnapshotUnavailable, error.to_string()))?
        .len()
        > READER_RESPONSE_EVENTS_BYTES
    {
        let Some(index) = values.iter().rposition(|value| {
            value.get("truncated").and_then(serde_json::Value::as_bool) != Some(true)
        }) else {
            return Err(ImportError::new(
                ErrorCode::ResourceLimit,
                "reader event window exceeds the response budget",
            ));
        };
        let event = &by_id[&ids[index]];
        values[index] = serde_json::json!({
            "id": event.event_id,
            "ordinal": event.ordinal,
            "kind": event.kind,
            "truncated": true,
        });
    }
    Ok(values)
}

fn cached_lineage_with_stamp(
    stamp: ManifestStamp,
    leaf: &str,
    relations: &[EventRelation],
) -> Result<Arc<Vec<String>>, ImportError> {
    let stamp = BranchStamp {
        manifest: stamp,
        leaf: leaf.to_owned(),
    };
    let cache = BRANCH_CACHE.get_or_init(|| Mutex::new(VecDeque::new()));
    {
        let mut entries = cache.lock().map_err(|_| {
            ImportError::new(
                ErrorCode::SnapshotUnavailable,
                "reader branch cache is unavailable",
            )
        })?;
        if let Some(position) = entries.iter().position(|entry| entry.stamp == stamp) {
            let entry = entries
                .remove(position)
                .expect("located branch cache entry");
            let ids = Arc::clone(&entry.ids);
            entries.push_back(entry);
            return Ok(ids);
        }
    }
    #[cfg(test)]
    LINEAGE_BUILDS.with(|count| count.set(count.get().saturating_add(1)));
    let ids = Arc::new(lineage_ids(relations, leaf));
    if ids.len() <= BRANCH_CACHE_TOTAL_IDS {
        let mut entries = cache.lock().map_err(|_| {
            ImportError::new(
                ErrorCode::SnapshotUnavailable,
                "reader branch cache is unavailable",
            )
        })?;
        while entries.len() >= BRANCH_CACHE_ENTRIES
            || entries
                .iter()
                .map(|entry| entry.ids.len())
                .sum::<usize>()
                .saturating_add(ids.len())
                > BRANCH_CACHE_TOTAL_IDS
        {
            if entries.pop_front().is_none() {
                break;
            }
        }
        entries.push_back(CachedBranch {
            stamp,
            ids: Arc::clone(&ids),
        });
    }
    Ok(ids)
}

fn resolve_branch_with_health(
    revision_dir: &Path,
    requested: Option<&str>,
) -> Result<(String, Arc<Vec<String>>, RevisionHealth), ImportError> {
    let (stamp, relations, active_leaf, health) = cached_manifest(revision_dir)?;
    let leaf = requested
        .filter(|value| !value.trim().is_empty())
        .map(str::to_owned)
        .or(active_leaf)
        .ok_or_else(|| {
            ImportError::new(
                ErrorCode::HistoryAccessUnavailable,
                "reader leaf is required",
            )
        })?;
    if indexed_event_by_id(revision_dir, &leaf)?.is_none() {
        return Err(ImportError::new(
            ErrorCode::HistoryAccessUnavailable,
            "reader leaf is not present in the archive",
        ));
    }
    let chain = cached_lineage_with_stamp(stamp, &leaf, relations.as_slice())?;
    if chain.is_empty() {
        return Err(ImportError::new(
            ErrorCode::HistoryAccessUnavailable,
            "reader leaf is not present in the archive",
        ));
    }
    Ok((leaf, chain, health))
}

pub(crate) fn resolve_branch(
    revision_dir: &Path,
    requested: Option<&str>,
) -> Result<(String, Arc<Vec<String>>), ImportError> {
    resolve_branch_with_health(revision_dir, requested).map(|(leaf, chain, _)| (leaf, chain))
}

fn cursor_scope<'a>(owner: &'a str, revision: &'a str, leaf: &'a str) -> CursorScope<'a> {
    CursorScope {
        owner,
        snapshot: revision,
        leaf,
        filter: CONTEXT_CURSOR_FILTER,
    }
}

fn encode_cursor(
    owner: &str,
    revision: &str,
    leaf: &str,
    filter: &str,
    offset: usize,
    event_id: &str,
) -> Result<String, ImportError> {
    let offset = u32::try_from(offset).map_err(|_| {
        ImportError::new(
            ErrorCode::ResourceLimit,
            "reader branch exceeds cursor capacity",
        )
    })?;
    encode_bound_cursor(
        CursorScope {
            owner,
            snapshot: revision,
            leaf,
            filter,
        },
        offset,
        Some(event_id.to_owned()),
    )
}

fn lineage_cursor(
    owner: &str,
    revision: &str,
    leaf: &str,
    direction: PageDirection,
    offset: usize,
    chain: &[String],
) -> Result<String, ImportError> {
    let mark = match direction {
        PageDirection::Forward => chain.get(offset),
        PageDirection::Backward => offset.checked_sub(1).and_then(|index| chain.get(index)),
    }
    .ok_or_else(|| ImportError::new(ErrorCode::PlanStale, "reader page cursor is out of range"))?;
    encode_cursor(owner, revision, leaf, direction.filter(), offset, mark)
}

fn decode_lineage_cursor(
    raw: &str,
    owner: &str,
    revision: &str,
    leaf: &str,
    direction: PageDirection,
    chain: &[String],
) -> Result<usize, ImportError> {
    let cursor = decode_bound_cursor(
        raw,
        CursorScope {
            owner,
            snapshot: revision,
            leaf,
            filter: direction.filter(),
        },
    )?;
    let offset = cursor.offset as usize;
    let expected = match direction {
        PageDirection::Forward => chain.get(offset),
        PageDirection::Backward => offset.checked_sub(1).and_then(|index| chain.get(index)),
    };
    if cursor.mark.as_deref() != expected.map(String::as_str) {
        return Err(ImportError::new(
            ErrorCode::PlanStale,
            "reader page cursor no longer identifies its boundary",
        ));
    }
    Ok(offset)
}

pub fn event_page(
    revision_dir: &Path,
    owner: &str,
    revision: &str,
    requested_leaf: Option<&str>,
    cursor: Option<&str>,
    limit: Option<usize>,
    direction: PageDirection,
) -> Result<EventPageResponse, ImportError> {
    let (leaf, chain, health) = resolve_branch_with_health(revision_dir, requested_leaf)?;
    let cap = crate::session_migration::budget::page_limit(limit)
        .min(crate::session_migration::budget::READ_PAGE_EVENTS);
    let boundary = match (direction, cursor.filter(|value| !value.trim().is_empty())) {
        (PageDirection::Forward, None) => 0,
        (PageDirection::Backward, None) => chain.len(),
        (_, Some(raw)) => {
            decode_lineage_cursor(raw, owner, revision, &leaf, direction, chain.as_slice())?
        }
    };
    let (start, end) = match direction {
        PageDirection::Forward => {
            if boundary >= chain.len() {
                return Err(ImportError::new(
                    ErrorCode::PlanStale,
                    "event cursor past end of branch",
                ));
            }
            (boundary, boundary.saturating_add(cap).min(chain.len()))
        }
        PageDirection::Backward => {
            if boundary == 0 || boundary > chain.len() {
                return Err(ImportError::new(
                    ErrorCode::PlanStale,
                    "event cursor before start of branch",
                ));
            }
            (boundary.saturating_sub(cap), boundary)
        }
    };
    let ids = &chain[start..end];
    let events = reader_events_by_ids(revision_dir, ids)?;
    if events.len() != ids.len() {
        return Err(ImportError::new(
            ErrorCode::HistoryAccessUnavailable,
            "reader branch index is incomplete",
        ));
    }
    let previous_cursor = (start > 0)
        .then(|| {
            lineage_cursor(
                owner,
                revision,
                &leaf,
                PageDirection::Backward,
                start,
                chain.as_slice(),
            )
        })
        .transpose()?;
    let next_cursor = (end < chain.len())
        .then(|| {
            lineage_cursor(
                owner,
                revision,
                &leaf,
                PageDirection::Forward,
                end,
                chain.as_slice(),
            )
        })
        .transpose()?;
    Ok(EventPageResponse {
        events,
        health,
        previous_cursor,
        next_cursor,
        total: u32::try_from(chain.len()).map_err(|_| {
            ImportError::new(ErrorCode::ResourceLimit, "reader branch exceeds capacity")
        })?,
        start: u32::try_from(start).map_err(|_| {
            ImportError::new(ErrorCode::ResourceLimit, "reader position exceeds capacity")
        })?,
        end: u32::try_from(end).map_err(|_| {
            ImportError::new(ErrorCode::ResourceLimit, "reader position exceeds capacity")
        })?,
        revision: revision.to_owned(),
        leaf,
    })
}

fn validate_context_cursor(
    raw: Option<&str>,
    owner: &str,
    revision: &str,
    leaf: &str,
    chain: &[String],
    target: usize,
) -> Result<(), ImportError> {
    let Some(raw) = raw.filter(|value| !value.trim().is_empty()) else {
        return Ok(());
    };
    let cursor = decode_bound_cursor(raw, cursor_scope(owner, revision, leaf))?;
    let offset = cursor.offset as usize;
    if offset != target || cursor.mark.as_deref() != chain.get(offset).map(String::as_str) {
        return Err(ImportError::new(
            ErrorCode::PlanStale,
            "reader cursor does not identify the requested event",
        ));
    }
    Ok(())
}

/// Fetch a bounded context window around one event from an immutable revision.
///
/// `owner` is the import id and `revision` is the catalog revision id.  Both
/// are included in the authenticated cursors, so a cursor cannot be replayed
/// against another import, revision, branch, or filter.
pub fn event_context(
    revision_dir: &Path,
    owner: &str,
    revision: &str,
    requested_leaf: Option<&str>,
    request: &EventContextRequest,
) -> Result<EventContextResponse, ImportError> {
    if request.event_id.trim().is_empty() {
        return Err(ImportError::new(
            ErrorCode::HistoryAccessUnavailable,
            "reader event id is required",
        ));
    }
    let (leaf, chain, health) = resolve_branch_with_health(revision_dir, requested_leaf)?;
    let position = chain
        .iter()
        .position(|id| id == &request.event_id)
        .ok_or_else(|| {
            ImportError::new(
                ErrorCode::HistoryAccessUnavailable,
                "event is outside the selected reader branch",
            )
        })?;
    let target = indexed_event_by_id(revision_dir, &request.event_id)?.ok_or_else(|| {
        ImportError::new(
            ErrorCode::HistoryAccessUnavailable,
            "reader event is unavailable",
        )
    })?;
    validate_context_cursor(
        request.cursor.as_deref(),
        owner,
        revision,
        &leaf,
        chain.as_slice(),
        position,
    )?;

    let before = request.before.unwrap_or(8).min(CONTEXT_MAX_SIDE_EVENTS);
    let after = request.after.unwrap_or(8).min(CONTEXT_MAX_SIDE_EVENTS);
    let start = position.saturating_sub(before);
    let end = position
        .saturating_add(after)
        .saturating_add(1)
        .min(chain.len());
    let ids = &chain[start..end];
    let events = reader_events_by_ids(revision_dir, ids)?;
    let target_index = u32::try_from(position - start).map_err(|_| {
        ImportError::new(
            ErrorCode::ResourceLimit,
            "reader target index exceeds capacity",
        )
    })?;
    let total = u32::try_from(chain.len()).map_err(|_| {
        ImportError::new(ErrorCode::ResourceLimit, "reader branch exceeds capacity")
    })?;
    let anchor_cursor = encode_cursor(
        owner,
        revision,
        &leaf,
        CONTEXT_CURSOR_FILTER,
        position,
        &chain[position],
    )?;
    let previous_cursor = (start > 0)
        .then(|| {
            lineage_cursor(
                owner,
                revision,
                &leaf,
                PageDirection::Backward,
                start,
                chain.as_slice(),
            )
        })
        .transpose()?;
    let next_cursor = (end < chain.len())
        .then(|| {
            lineage_cursor(
                owner,
                revision,
                &leaf,
                PageDirection::Forward,
                end,
                chain.as_slice(),
            )
        })
        .transpose()?;
    Ok(EventContextResponse {
        events,
        health,
        target_index,
        ordinal: target.base().ordinal,
        position: u32::try_from(position).map_err(|_| {
            ImportError::new(ErrorCode::ResourceLimit, "reader position exceeds capacity")
        })?,
        total,
        revision: revision.to_owned(),
        leaf,
        anchor_cursor,
        previous_cursor,
        next_cursor,
    })
}

#[cfg(test)]
#[path = "reader_tests.rs"]
mod tests;
