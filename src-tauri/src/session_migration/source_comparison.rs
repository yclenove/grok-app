//! Explicit comparison of two fixed previews, never a source-selection mutation.
use super::errors::{ErrorCode, ImportError};
use super::{budget, cursor, engine, graph, hash, jobs, search, selection, source_member, types};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::io::Read;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ComparisonSide {
    pub preview_id: String,
    pub candidate_id: String,
    pub source_revision: String,
    pub leaf_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ComparisonRequest {
    pub version: u32,
    pub left: ComparisonSide,
    pub right: ComparisonSide,
    pub cursor: Option<String>,
    pub limit: Option<usize>,
}

pub(super) fn stale(message: &str) -> ImportError {
    ImportError::new(ErrorCode::PlanStale, message)
}

pub(super) fn resource_limit() -> ImportError {
    ImportError::new(
        ErrorCode::ResourceLimit,
        "comparison exceeds page byte budget",
    )
}

pub(super) struct Snapshot {
    path: PathBuf,
    lease: jobs::PreviewLease,
    manifest: Value,
    ids: Vec<String>,
}

impl Snapshot {
    fn load(side: &ComparisonSide) -> Result<Self, ImportError> {
        if uuid::Uuid::parse_str(&side.preview_id).is_err()
            || [&side.candidate_id, &side.source_revision, &side.leaf_id]
                .iter()
                .any(|value| value.is_empty() || value.len() > budget::READ_MAX_BYTES)
        {
            return Err(stale("invalid comparison side identity"));
        }
        let path = engine::comparison_snapshot_path(&side.preview_id)?;
        // RAM presence does not waive disk integrity or the shared semantic guard.
        let (lease, _) = jobs::verify_disk_preview_integrity(&path, &side.preview_id)?;
        if lease.source_revision != side.source_revision {
            return Err(stale("comparison source revision changed"));
        }
        selection::validate_prepared_selection(
            &selection::PreviewSelection {
                preview_id: side.preview_id.clone(),
                leaf_id: side.leaf_id.clone(),
                selection: selection::AllowedSelection::default(),
            },
            &path,
            selection::PreparedSelectionIdentity {
                preview_id: Some(&side.preview_id),
                candidate_id: Some(&side.candidate_id),
                snapshot_digest: Some(&lease.snapshot_digest),
                ..Default::default()
            },
        )?;
        let mut raw = Vec::new();
        std::fs::File::open(path.join("manifest.json"))
            .map_err(|_| stale("comparison manifest unavailable"))?
            .take(budget::JSON_RECORD_MAX_BYTES as u64 + 1)
            .read_to_end(&mut raw)
            .map_err(|_| stale("comparison manifest unreadable"))?;
        if raw.len() > budget::JSON_RECORD_MAX_BYTES {
            return Err(resource_limit());
        }
        let manifest: Value =
            serde_json::from_slice(&raw).map_err(|_| stale("invalid comparison manifest"))?;
        let relations: Vec<types::EventRelation> = serde_json::from_value(
            manifest
                .pointer("/graph/relations")
                .cloned()
                .ok_or_else(|| stale("comparison relations missing"))?,
        )
        .map_err(|_| stale("invalid comparison relations"))?;
        if !matches!(
            manifest["outcome"].as_str(),
            Some("complete" | "partial" | "unsupported" | "failed")
        ) {
            return Err(stale("invalid comparison outcome"));
        }
        let ids = graph::lineage_ids(&relations, &side.leaf_id);
        u32::try_from(ids.len()).map_err(|_| resource_limit())?;
        Ok(Self {
            path,
            lease,
            manifest,
            ids,
        })
    }

    fn pin(&self) -> Result<&source_member::SourceMemberPin, ImportError> {
        self.lease
            .source_member
            .as_ref()
            .ok_or_else(|| stale("comparison requires fixed source-member evidence"))
    }

    pub(super) fn event(
        &self,
        position: usize,
    ) -> Result<Option<types::ImportedEvent>, ImportError> {
        let Some(id) = self.ids.get(position) else {
            return Ok(None);
        };
        let mut events = search::indexed_events_by_ids(&self.path, std::slice::from_ref(id))?;
        if events.len() != 1 || events[0].id() != id {
            return Err(stale("comparison event missing from fixed snapshot"));
        }
        Ok(events.pop())
    }

    fn summary(&self, side: &ComparisonSide) -> Result<Value, ImportError> {
        Ok(json!({
            "previewId":side.preview_id, "candidateId":side.candidate_id,
            "leafId":side.leaf_id, "sourceRevision":side.source_revision,
            "snapshotDigest":self.lease.snapshot_digest,
            "groupId":self.pin()?.group_id, "eventCount":self.ids.len(),
            "title":self.manifest.pointer("/graph/title"),
            "cwd":self.manifest.pointer("/graph/cwd"),
            "outcome":self.manifest["outcome"],
        }))
    }

    fn verify_unchanged(&self) -> Result<(), ImportError> {
        let (current, _) = jobs::verify_disk_preview_integrity(&self.path, &self.lease.preview_id)?;
        if serde_json::to_value(current).map_err(|_| stale("invalid comparison lease"))?
            != serde_json::to_value(&self.lease).map_err(|_| stale("invalid comparison lease"))?
        {
            return Err(stale("comparison lease changed during read"));
        }
        Ok(())
    }
}

pub(super) fn fields(event: &types::ImportedEvent) -> Result<Value, ImportError> {
    let mut value = serde_json::to_value(event).map_err(|_| stale("invalid comparison event"))?;
    let object = value
        .as_object_mut()
        .ok_or_else(|| stale("invalid event object"))?;
    // Source envelopes are not body equivalence. Do not discard any typed body
    // field (including tool IDs, opaque payloads and compaction coverage).
    for field in ["id", "ordinal", "spans", "timestamp", "timeOrigin"] {
        object.remove(field);
    }
    Ok(value)
}

fn side_event(event: Option<&types::ImportedEvent>) -> Result<Value, ImportError> {
    let Some(event) = event else {
        return Ok(Value::Null);
    };
    let full = event.searchable_text();
    let excerpt: String = full.chars().take(512).collect();
    Ok(json!({
        "eventId":event.id(), "kind":event.kind_name(),
        "timestamp":event.base().timestamp, "timeOrigin":event.base().time_origin,
        "spans":event.base().spans, "excerpt":excerpt,
        "truncated":full.chars().count()>512, "fieldsDigest":hash::options_hash(&fields(event)?),
    }))
}

fn uncertain(event: Option<&types::ImportedEvent>) -> bool {
    use types::{ImportedEvent, ToolInput, ToolOutcome};
    match event {
        None | Some(ImportedEvent::Metadata { .. } | ImportedEvent::Unsupported { .. }) => true,
        Some(ImportedEvent::ToolCall {
            input: ToolInput::Payload { .. },
            ..
        }) => true,
        Some(ImportedEvent::ToolCall { .. }) => false,
        Some(ImportedEvent::Message {
            blocks, complete, ..
        }) => !complete || uncertain_blocks(blocks),
        Some(ImportedEvent::ToolResult {
            blocks, outcome, ..
        }) => *outcome == ToolOutcome::Unknown || uncertain_blocks(blocks),
        Some(ImportedEvent::Compaction {
            summary,
            coverage_known,
            ..
        }) => !coverage_known || uncertain_blocks(summary),
    }
}

fn uncertain_blocks(blocks: &[types::ContentBlock]) -> bool {
    blocks.iter().any(|block| {
        matches!(
            block,
            types::ContentBlock::Opaque { .. } | types::ContentBlock::Asset { .. }
        )
    })
}

fn row(
    position: usize,
    left: Option<&types::ImportedEvent>,
    right: Option<&types::ImportedEvent>,
) -> Result<Value, ImportError> {
    let mut differences = Vec::new();
    let mut metadata = Vec::new();
    match (left, right) {
        (Some(a), Some(b)) => {
            let a_fields = fields(a)?;
            let b_fields = fields(b)?;
            let keys: BTreeSet<_> = a_fields
                .as_object()
                .ok_or_else(|| stale("invalid fields"))?
                .keys()
                .chain(
                    b_fields
                        .as_object()
                        .ok_or_else(|| stale("invalid fields"))?
                        .keys(),
                )
                .collect();
            for key in keys {
                if a_fields.get(key) != b_fields.get(key) {
                    differences.push(key.as_str());
                }
            }
            if a.id() != b.id() {
                metadata.push("event-id");
            }
            if a.base().timestamp != b.base().timestamp
                || a.base().time_origin != b.base().time_origin
            {
                metadata.push("source-time");
            }
            return Ok(
                json!({"position":position, "left":side_event(left)?, "right":side_event(right)?,
                "differences":differences, "metadataDifferences":metadata,
                "uncertain":uncertain(left)||uncertain(right)}),
            );
        }
        _ => differences.push("missing-side"),
    }
    Ok(
        json!({"position":position, "left":side_event(left)?, "right":side_event(right)?,
        "differences":differences, "metadataDifferences":metadata, "uncertain":true}),
    )
}

/// Row and full-field readers must use the same fixed pair authorization.
pub(super) struct FixedPair {
    pub left: Snapshot,
    pub right: Snapshot,
    owner: String,
    snapshot: String,
    leaves: String,
}

impl FixedPair {
    pub fn load(a_side: &ComparisonSide, b_side: &ComparisonSide) -> Result<Self, ImportError> {
        let left = Snapshot::load(a_side)?;
        let right = Snapshot::load(b_side)?;
        let a = left.pin()?;
        let b = right.pin()?;
        if a.scan_id != b.scan_id
            || a.epoch != b.epoch
            || a.group_id != b.group_id
            || a.reference.source_id != b.reference.source_id
            || a.reference.provider != b.reference.provider
            || a.reference.host != b.reference.host
            || a.reference.native_session_id != b.reference.native_session_id
            || a.reference.candidate_id == b.reference.candidate_id
        {
            return Err(stale(
                "comparison previews are not distinct members of the same fixed group",
            ));
        }
        let owner = hash::options_hash(&json!([a_side, b_side]));
        let snapshot = hash::options_hash(&json!([
            left.lease.snapshot_digest,
            right.lease.snapshot_digest
        ]));
        let leaves = hash::options_hash(&json!([a_side.leaf_id, b_side.leaf_id]));
        Ok(Self {
            left,
            right,
            owner,
            snapshot,
            leaves,
        })
    }

    pub fn scope<'a>(&'a self, filter: &'a str) -> cursor::CursorScope<'a> {
        cursor::CursorScope {
            owner: &self.owner,
            snapshot: &self.snapshot,
            leaf: &self.leaves,
            filter,
        }
    }

    pub fn verify_unchanged(&self) -> Result<(), ImportError> {
        self.left.verify_unchanged()?;
        self.right.verify_unchanged()
    }
}

/// Every row is positional, never a claim of cross-format event alignment or
/// equal complete histories. Only the two explicitly opened snapshots are read.
pub fn page(request: ComparisonRequest) -> Result<Value, ImportError> {
    if request.version != 1 {
        return Err(stale("unsupported comparison version"));
    }
    let _operation = super::preview_lifecycle::operation()?;
    let pair = FixedPair::load(&request.left, &request.right)?;
    let (left, right) = (&pair.left, &pair.right);
    let scope = pair.scope("source-comparison:v1");
    let offset = match request.cursor.as_deref() {
        Some(raw) => cursor::decode_bound_cursor(raw, scope)?.offset as usize,
        None => 0,
    };
    let total = left.ids.len().max(right.ids.len());
    if offset > total {
        return Err(stale("comparison cursor past end"));
    }
    let mut rows = Vec::new();
    let mut used = 0;
    let limit = budget::page_limit(request.limit).min(budget::READ_PAGE_EVENTS);
    let mut position = offset;
    while position < total && rows.len() < limit {
        let a_event = left.event(position)?;
        let b_event = right.event(position)?;
        let row = row(position, a_event.as_ref(), b_event.as_ref())?;
        let size = serde_json::to_vec(&row)
            .map_err(|_| stale("invalid comparison row"))?
            .len()
            + 1;
        if used + size > budget::READ_PAGE_TEXT_BYTES - 4096 {
            if rows.is_empty() {
                return Err(resource_limit());
            }
            break;
        }
        used += size;
        rows.push(row);
        position += 1;
    }
    let next = if position < total {
        Some(cursor::encode_bound_cursor(scope, position as u32, None)?)
    } else {
        None
    };
    let mut metadata = Vec::new();
    for field in ["title", "cwd", "relations"] {
        if left.manifest["graph"].get(field) != right.manifest["graph"].get(field) {
            metadata.push(field);
        }
    }
    let uncertain = left.manifest["outcome"] != "complete"
        || right.manifest["outcome"] != "complete"
        || rows.iter().any(|row| row["uncertain"] == true);
    let result = json!({"version":1, "alignment":"position-only", "equivalence":"unverified",
        "left":left.summary(&request.left)?, "right":right.summary(&request.right)?,
        "metadataDifferences":metadata, "totalPositions":total,
        "rows":rows, "nextCursor":next, "complete":position==total,
        "uncertain":uncertain});
    if serde_json::to_vec(&result)
        .map_err(|_| stale("invalid comparison response"))?
        .len()
        > budget::READ_PAGE_TEXT_BYTES
    {
        return Err(resource_limit());
    }
    pair.verify_unchanged()?;
    Ok(result)
}

#[cfg(test)]
#[path = "source_comparison_tests.rs"]
pub(crate) mod tests;
