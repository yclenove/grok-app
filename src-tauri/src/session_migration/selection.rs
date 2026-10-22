//! Canonical, versioned scope shared by import plans and model-facing history.

use std::collections::HashSet;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::errors::{ErrorCode, ImportError};
use super::hash::options_hash;
use super::types::{
    ContentBlock, EventRelation, ImportedEvent, MessageRole, RelationKind, ToolInput,
};
#[cfg(test)]
use super::types::{EventBase, RelationConfidence, TimeOrigin, ToolOutcome};

pub const SELECTION_SCHEMA_VERSION: u32 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolField {
    Name,
    Input,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SelectionFields {
    pub message_text: bool,
    pub tool_call_name: bool,
    pub tool_call_input: bool,
    pub tool_result_content: bool,
    pub compaction_summary: bool,
    pub visible_reasoning: bool,
}

impl Default for SelectionFields {
    fn default() -> Self {
        Self {
            message_text: true,
            tool_call_name: true,
            tool_call_input: true,
            tool_result_content: true,
            compaction_summary: true,
            visible_reasoning: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AllowedSelection {
    pub version: u32,
    pub event_ids: Vec<String>,
    pub pinned_event_ids: Vec<String>,
    pub asset_ids: Vec<String>,
    pub fields: SelectionFields,
    pub include_subagents: bool,
}

impl Default for AllowedSelection {
    fn default() -> Self {
        Self {
            version: SELECTION_SCHEMA_VERSION,
            event_ids: Vec::new(),
            pinned_event_ids: Vec::new(),
            asset_ids: Vec::new(),
            fields: SelectionFields::default(),
            include_subagents: false,
        }
    }
}

impl AllowedSelection {
    #[cfg(test)]
    pub fn for_events(event_ids: Vec<String>) -> Self {
        Self {
            event_ids,
            ..Self::default()
        }
    }

    pub fn canonicalized(&self) -> Self {
        let mut value = self.clone();
        value.event_ids.sort();
        value.event_ids.dedup();
        value.pinned_event_ids.sort();
        value.pinned_event_ids.dedup();
        value.asset_ids.sort();
        value.asset_ids.dedup();
        value
    }

    pub fn hash(&self) -> String {
        options_hash(&serde_json::to_value(self.canonicalized()).expect("selection serializes"))
    }

    #[cfg(test)]
    pub fn hash_for_leaf(&self, leaf_id: &str) -> String {
        options_hash(&serde_json::json!({
            "leafId": leaf_id,
            "selection": self.canonicalized()
        }))
    }

    #[cfg(test)]
    pub fn canonical_json_string(&self) -> String {
        super::hash::canonical_json(
            &serde_json::to_value(self.canonicalized()).expect("selection serializes"),
        )
    }

    pub fn validate_shape(&self) -> Result<(), ImportError> {
        if self.version != SELECTION_SCHEMA_VERSION {
            return Err(ImportError::new(
                if self.version > SELECTION_SCHEMA_VERSION {
                    ErrorCode::ImportSchemaTooNew
                } else {
                    ErrorCode::SchemaUnsupported
                },
                format!("unsupported selection schema {}", self.version),
            ));
        }
        validate_unique_ids("event", &self.event_ids)?;
        validate_unique_ids("pinned event", &self.pinned_event_ids)?;
        validate_unique_ids("asset", &self.asset_ids)?;
        let selected: HashSet<&str> = self.event_ids.iter().map(String::as_str).collect();
        if self
            .pinned_event_ids
            .iter()
            .any(|id| !selected.contains(id.as_str()))
        {
            return Err(ImportError::new(
                ErrorCode::HistoryAccessUnavailable,
                "pinned event is outside the selected events",
            ));
        }
        Ok(())
    }

    pub fn permits_event(&self, event: &ImportedEvent, relations: &[EventRelation]) -> bool {
        if !self.event_ids.iter().any(|id| id == event.id()) || is_never_model_visible(event) {
            return false;
        }
        self.include_subagents || !subagent_member_ids(relations).contains(event.id())
    }

    pub fn permits_asset(&self, asset_id: &str) -> bool {
        self.asset_ids.iter().any(|id| id == asset_id)
    }

    pub fn permits_tool_field(&self, field: ToolField) -> bool {
        match field {
            ToolField::Name => self.fields.tool_call_name,
            ToolField::Input => self.fields.tool_call_input,
        }
    }

    pub fn permits_block(&self, event: &ImportedEvent, block: &ContentBlock) -> bool {
        match block {
            ContentBlock::VisibleReasoning { .. } => self.fields.visible_reasoning,
            ContentBlock::Opaque { .. } => false,
            ContentBlock::Asset { asset_id, .. } => self.permits_asset(asset_id),
            ContentBlock::Text { .. } | ContentBlock::Code { .. } => match event {
                ImportedEvent::Message { .. } => self.fields.message_text,
                ImportedEvent::ToolResult { .. } => self.fields.tool_result_content,
                ImportedEvent::Compaction { .. } => self.fields.compaction_summary,
                _ => false,
            },
        }
    }

    /// Unique model/search/export projection. Local reading keeps `searchable_text()`.
    pub fn project_event(
        &self,
        event: &ImportedEvent,
        relations: &[EventRelation],
    ) -> Option<String> {
        if !self.permits_event(event, relations) {
            return None;
        }
        let text = match event {
            ImportedEvent::Message { blocks, .. }
            | ImportedEvent::ToolResult { blocks, .. }
            | ImportedEvent::Compaction {
                summary: blocks, ..
            } => blocks
                .iter()
                .filter(|block| self.permits_block(event, block))
                .filter_map(|block| match block {
                    ContentBlock::Asset { alt, .. } => alt.as_deref(),
                    _ => block.plain_text(),
                })
                .collect::<Vec<_>>()
                .join("\n"),
            ImportedEvent::ToolCall { name, input, .. } => {
                let mut fields = Vec::new();
                if self.permits_tool_field(ToolField::Name) {
                    fields.push(name.clone());
                }
                if self.permits_tool_field(ToolField::Input) {
                    fields.push(match input {
                        ToolInput::Json { value } => value.to_string(),
                        ToolInput::Payload { payload } => format!("blob:{}", payload.blob_id),
                    });
                }
                fields.join(" ")
            }
            ImportedEvent::Metadata { .. } | ImportedEvent::Unsupported { .. } => String::new(),
        };
        (!text.is_empty()).then_some(text)
    }

    pub fn model_event_ids(
        &self,
        events: &[ImportedEvent],
        relations: &[EventRelation],
    ) -> Vec<String> {
        events
            .iter()
            .filter(|event| self.project_event(event, relations).is_some())
            .map(|event| event.id().to_string())
            .collect()
    }

    #[cfg(test)]
    pub fn validate_for_branch(
        &self,
        events: &[ImportedEvent],
        relations: &[EventRelation],
        leaf_id: &str,
    ) -> Result<(), ImportError> {
        self.validate_shape()?;
        let allowed = branch_scope_ids(events, relations, leaf_id, self.include_subagents)?;
        if self
            .event_ids
            .iter()
            .any(|id| !allowed.contains(id.as_str()))
        {
            return Err(ImportError::new(
                ErrorCode::HistoryAccessUnavailable,
                "selection is outside the chosen branch",
            ));
        }
        if events.iter().any(|event| {
            self.event_ids.iter().any(|id| id == event.id()) && is_never_model_visible(event)
        }) {
            return Err(ImportError::new(
                ErrorCode::HistoryAccessUnavailable,
                "selection contains a non-model event",
            ));
        }
        Ok(())
    }

    /// Same rules as `validate_for_branch` without loading the full event graph.
    pub fn validate_for_leaf(
        &self,
        relations: &[EventRelation],
        events: &[ImportedEvent],
        leaf_id: &str,
    ) -> Result<(), ImportError> {
        self.validate_shape()?;
        let mut allowed: HashSet<String> = super::graph::lineage_ids(relations, leaf_id)
            .into_iter()
            .collect();
        if self.include_subagents {
            let all_subagents = subagent_member_ids(relations);
            let mut roots: Vec<String> = relations
                .iter()
                .filter(|relation| {
                    relation.kind == RelationKind::Subagent && allowed.contains(&relation.to)
                })
                .map(|relation| relation.from.clone())
                .collect();
            while let Some(id) = roots.pop() {
                if all_subagents.contains(id.as_str()) && allowed.insert(id.clone()) {
                    roots.extend(
                        relations
                            .iter()
                            .filter(|relation| {
                                relation.kind == RelationKind::Parent && relation.to == id
                            })
                            .map(|relation| relation.from.clone()),
                    );
                }
            }
        }
        if self
            .event_ids
            .iter()
            .any(|id| !allowed.contains(id.as_str()))
        {
            return Err(ImportError::new(
                ErrorCode::HistoryAccessUnavailable,
                "selection is outside the chosen branch",
            ));
        }
        if events.iter().any(|event| {
            self.event_ids.iter().any(|id| id == event.id()) && is_never_model_visible(event)
        }) {
            return Err(ImportError::new(
                ErrorCode::HistoryAccessUnavailable,
                "selection contains a non-model event",
            ));
        }
        Ok(())
    }
}

/// v2 is live. v1 drafts may be read with deny-by-default assets and must be re-prepared.
#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredSelection {
    pub selection: AllowedSelection,
    pub requires_reprepare: bool,
}

#[cfg(test)]
pub fn parse_stored_selection(value: &serde_json::Value) -> Result<StoredSelection, ImportError> {
    let version = match value.get("version") {
        None => 0,
        Some(value) => value.as_u64().ok_or_else(|| {
            ImportError::new(
                ErrorCode::SchemaUnsupported,
                "invalid selection schema version",
            )
        })?,
    };
    if version > u64::from(SELECTION_SCHEMA_VERSION) {
        return Err(ImportError::new(
            ErrorCode::ImportSchemaTooNew,
            format!("unsupported selection schema {version}"),
        ));
    }
    let v1 = version == 1
        || (version == 0
            && (value.get("includeReasoning").is_some() || value.get("includeImages").is_some()));
    if v1 {
        return parse_v1_draft(value);
    }
    let selection: AllowedSelection = serde_json::from_value(value.clone()).map_err(|e| {
        ImportError::new(
            ErrorCode::SchemaUnsupported,
            format!("invalid selection: {e}"),
        )
    })?;
    selection.validate_shape()?;
    Ok(StoredSelection {
        selection: selection.canonicalized(),
        requires_reprepare: false,
    })
}

/// Parse the live v2 plan/publish wire format. Legacy drafts are intentionally
/// not accepted here; callers that own old draft storage must use
/// `parse_stored_selection` and re-prepare it before publication.
pub fn parse_live_selection_set(value: &serde_json::Value) -> Result<SelectionSet, ImportError> {
    let raw: SelectionSet = serde_json::from_value(value.clone()).map_err(|error| {
        ImportError::new(
            ErrorCode::SchemaUnsupported,
            format!("invalid live selection set: {error}"),
        )
    })?;
    raw.validate_shape()?;
    Ok(raw.canonicalized())
}

#[cfg(test)]
fn parse_v1_draft(value: &serde_json::Value) -> Result<StoredSelection, ImportError> {
    fn string_ids(value: &serde_json::Value, key: &str) -> Result<Vec<String>, ImportError> {
        let Some(raw) = value.get(key) else {
            return Ok(Vec::new());
        };
        let items = raw.as_array().ok_or_else(|| {
            ImportError::new(
                ErrorCode::SchemaUnsupported,
                format!("legacy selection {key} must be an array"),
            )
        })?;
        items
            .iter()
            .map(|item| {
                item.as_str().map(str::to_string).ok_or_else(|| {
                    ImportError::new(
                        ErrorCode::SchemaUnsupported,
                        format!("legacy selection {key} contains a non-string ID"),
                    )
                })
            })
            .collect()
    }
    fn optional_bool(value: &serde_json::Value, key: &str) -> Result<bool, ImportError> {
        match value.get(key) {
            None => Ok(false),
            Some(value) => value.as_bool().ok_or_else(|| {
                ImportError::new(
                    ErrorCode::SchemaUnsupported,
                    format!("legacy selection {key} must be boolean"),
                )
            }),
        }
    }
    let mut selection = AllowedSelection::for_events(string_ids(value, "eventIds")?);
    selection.pinned_event_ids = string_ids(value, "pinnedEventIds")?;
    selection.include_subagents = optional_bool(value, "includeSubagents")?;
    selection.fields.visible_reasoning = optional_bool(value, "includeReasoning")?;
    let _ = optional_bool(value, "includeImages")?;
    selection.asset_ids.clear();
    selection.validate_shape()?;
    Ok(StoredSelection {
        selection: selection.canonicalized(),
        requires_reprepare: true,
    })
}

pub(crate) fn is_never_model_visible(event: &ImportedEvent) -> bool {
    matches!(
        event,
        ImportedEvent::Message {
            role: MessageRole::System,
            ..
        } | ImportedEvent::Metadata { .. }
            | ImportedEvent::Unsupported { .. }
    )
}

pub(crate) fn subagent_member_ids(relations: &[EventRelation]) -> HashSet<&str> {
    let mut roots: HashSet<&str> = relations
        .iter()
        .filter(|relation| relation.kind == RelationKind::Subagent)
        .map(|relation| relation.from.as_str())
        .collect();
    let mut children: std::collections::HashMap<&str, Vec<&str>> = std::collections::HashMap::new();
    for relation in relations {
        if relation.kind == RelationKind::Parent {
            children
                .entry(relation.to.as_str())
                .or_default()
                .push(relation.from.as_str());
        }
    }
    let mut stack: Vec<&str> = roots.iter().copied().collect();
    while let Some(id) = stack.pop() {
        if let Some(descendants) = children.get(id) {
            for descendant in descendants {
                if roots.insert(*descendant) {
                    stack.push(*descendant);
                }
            }
        }
    }
    roots
}

pub(super) fn branch_scope_ids<'a>(
    events: &'a [ImportedEvent],
    relations: &'a [EventRelation],
    leaf_id: &str,
    include_subagents: bool,
) -> Result<HashSet<&'a str>, ImportError> {
    if !events.iter().any(|event| event.id() == leaf_id) {
        return Err(ImportError::new(
            ErrorCode::HistoryAccessUnavailable,
            "chosen branch leaf does not exist",
        ));
    }
    let main: HashSet<&str> = super::graph::lineage(events, relations, leaf_id)
        .into_iter()
        .map(ImportedEvent::id)
        .collect();
    if !include_subagents {
        return Ok(main);
    }

    let all_subagents = subagent_member_ids(relations);
    let mut allowed = main.clone();
    let mut roots: Vec<&str> = relations
        .iter()
        .filter(|relation| {
            relation.kind == RelationKind::Subagent && main.contains(relation.to.as_str())
        })
        .map(|relation| relation.from.as_str())
        .collect();
    while let Some(id) = roots.pop() {
        if all_subagents.contains(id) && allowed.insert(id) {
            roots.extend(
                relations
                    .iter()
                    .filter(|relation| relation.kind == RelationKind::Parent && relation.to == id)
                    .map(|relation| relation.from.as_str()),
            );
        }
    }
    Ok(allowed)
}

fn validate_unique_ids(label: &str, values: &[String]) -> Result<(), ImportError> {
    let mut seen = HashSet::with_capacity(values.len());
    for value in values {
        if value.trim().is_empty() || !seen.insert(value.as_str()) {
            return Err(ImportError::new(
                ErrorCode::HistoryAccessUnavailable,
                format!("invalid or duplicate {label} id"),
            ));
        }
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PreviewSelection {
    pub preview_id: String,
    pub leaf_id: String,
    pub selection: AllowedSelection,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SelectionSet {
    pub version: u32,
    pub entries: Vec<PreviewSelection>,
}

impl Default for SelectionSet {
    fn default() -> Self {
        Self {
            version: SELECTION_SCHEMA_VERSION,
            entries: Vec::new(),
        }
    }
}

impl SelectionSet {
    pub fn single(entry: PreviewSelection) -> Self {
        Self {
            version: SELECTION_SCHEMA_VERSION,
            entries: vec![entry],
        }
    }

    pub fn canonicalized(&self) -> Self {
        let mut value = self.clone();
        for entry in &mut value.entries {
            entry.selection = entry.selection.canonicalized();
        }
        value.entries.sort_by(|a, b| {
            a.preview_id
                .cmp(&b.preview_id)
                .then_with(|| a.leaf_id.cmp(&b.leaf_id))
        });
        value
    }

    pub fn hash(&self) -> String {
        options_hash(&serde_json::to_value(self.canonicalized()).expect("selection set serializes"))
    }

    pub fn validate_shape(&self) -> Result<(), ImportError> {
        if self.version != SELECTION_SCHEMA_VERSION {
            return Err(ImportError::new(
                if self.version > SELECTION_SCHEMA_VERSION {
                    ErrorCode::ImportSchemaTooNew
                } else {
                    ErrorCode::SchemaUnsupported
                },
                format!("unsupported selection set schema {}", self.version),
            ));
        }
        if self.entries.is_empty() {
            return Err(ImportError::new(
                ErrorCode::HistoryAccessUnavailable,
                "selection set requires at least one preview",
            ));
        }
        let mut previews = HashSet::with_capacity(self.entries.len());
        for entry in &self.entries {
            if entry.preview_id.trim().is_empty()
                || entry.leaf_id.trim().is_empty()
                || !previews.insert(entry.preview_id.as_str())
            {
                return Err(ImportError::new(
                    ErrorCode::HistoryAccessUnavailable,
                    "selection set has an invalid or duplicate preview",
                ));
            }
            entry.selection.validate_shape()?;
        }
        Ok(())
    }

    pub fn single_entry(&self) -> Result<&PreviewSelection, ImportError> {
        self.validate_shape()?;
        if self.entries.len() != 1 {
            return Err(ImportError::new(
                ErrorCode::HistoryAccessUnavailable,
                "job item requires exactly one preview selection",
            ));
        }
        Ok(&self.entries[0])
    }
}

/// Identity frozen by the preview lease. Callers may require only the fields
/// they have at their boundary; durable-job reload supplies every field.
#[derive(Debug, Clone, Copy, Default)]
pub struct PreparedSelectionIdentity<'a> {
    pub preview_id: Option<&'a str>,
    pub candidate_id: Option<&'a str>,
    pub source_id: Option<&'a str>,
    pub root_path: Option<&'a str>,
    pub legacy_id: Option<&'a str>,
    pub snapshot_digest: Option<&'a str>,
    pub provider: Option<&'a str>,
    pub native_session_id: Option<&'a str>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedSelectionValidation {
    pub source_member: Option<super::source_member::SourceMemberPin>,
    pub cwd: Option<String>,
    pub snapshot_digest: String,
    pub candidate_id: String,
    pub source_id: String,
    pub root_path: String,
    pub legacy_id: String,
    pub provider: String,
    pub native_session_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SelectionFileIdentity {
    source_member: Option<super::source_member::SourceMemberPin>,
    cwd: Option<String>,
    snapshot_digest: String,
    provider: String,
    native_session_id: String,
}

fn identity_mismatch() -> ImportError {
    ImportError::new(
        ErrorCode::PlanStale,
        "prepared selection identity does not match its preview lease",
    )
}

fn require_identity(actual: &str, expected: Option<&str>) -> Result<(), ImportError> {
    if expected.is_some_and(|expected| expected != actual) {
        return Err(identity_mismatch());
    }
    Ok(())
}

/// Validate one raw selection against the immutable files prepared by preview.
/// This is the single semantic evaluator used by plan, durable reload and
/// published-revision verification. The caller must pass the raw value: shape
/// and duplicate checks deliberately happen before canonicalization.
pub fn validate_prepared_selection(
    entry: &PreviewSelection,
    path: &Path,
    expected: PreparedSelectionIdentity<'_>,
) -> Result<PreparedSelectionValidation, ImportError> {
    entry.selection.validate_shape()?;
    if entry.preview_id.trim().is_empty() || entry.leaf_id.trim().is_empty() {
        return Err(ImportError::new(
            ErrorCode::HistoryAccessUnavailable,
            "prepared selection requires preview and leaf identities",
        ));
    }
    require_identity(&entry.preview_id, expected.preview_id)?;

    let lease = super::jobs::read_preview_lease(path)?
        .ok_or_else(|| ImportError::new(ErrorCode::PlanStale, "preview lease missing"))?;
    require_identity(&lease.preview_id, Some(&entry.preview_id))?;
    require_identity(&lease.candidate_id, expected.candidate_id)?;
    require_identity(&lease.source_id, expected.source_id)?;
    require_identity(&lease.root_path, expected.root_path)?;
    require_identity(&lease.legacy_id, expected.legacy_id)?;
    require_identity(&lease.snapshot_digest, expected.snapshot_digest)?;

    let files = validate_selection_files(
        entry,
        path,
        PreparedSelectionIdentity {
            preview_id: expected.preview_id,
            snapshot_digest: Some(&lease.snapshot_digest),
            provider: expected.provider,
            native_session_id: expected.native_session_id,
            ..PreparedSelectionIdentity::default()
        },
    )?;

    super::source_member::validate_lease(&lease, files.source_member.as_ref())?;
    Ok(PreparedSelectionValidation {
        source_member: files.source_member,
        cwd: files.cwd,
        snapshot_digest: lease.snapshot_digest,
        candidate_id: lease.candidate_id,
        source_id: lease.source_id,
        root_path: lease.root_path,
        legacy_id: lease.legacy_id,
        provider: files.provider,
        native_session_id: files.native_session_id,
    })
}

/// Validate the immutable files of a published revision without depending on
/// the preview lease (whose expiry/retention state is intentionally mutable).
pub fn validate_published_selection(
    entry: &PreviewSelection,
    path: &Path,
    expected: PreparedSelectionIdentity<'_>,
) -> Result<Option<super::source_member::SourceMemberPin>, ImportError> {
    validate_selection_files(entry, path, expected).map(|files| files.source_member)
}

fn validate_selection_files(
    entry: &PreviewSelection,
    path: &Path,
    expected: PreparedSelectionIdentity<'_>,
) -> Result<SelectionFileIdentity, ImportError> {
    entry.selection.validate_shape()?;
    if entry.preview_id.trim().is_empty() || entry.leaf_id.trim().is_empty() {
        return Err(ImportError::new(
            ErrorCode::HistoryAccessUnavailable,
            "selection artifact requires preview and leaf identities",
        ));
    }
    require_identity(&entry.preview_id, expected.preview_id)?;

    let manifest_path = path.join("manifest.json");
    let manifest_raw = std::fs::read(&manifest_path)
        .map_err(|error| ImportError::new(ErrorCode::SnapshotUnavailable, error.to_string()))?;
    let manifest: serde_json::Value = serde_json::from_slice(&manifest_raw)
        .map_err(|error| ImportError::new(ErrorCode::SnapshotUnavailable, error.to_string()))?;
    let manifest_digest = manifest
        .get("sourceSnapshotId")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            ImportError::new(
                ErrorCode::SnapshotUnavailable,
                "prepared manifest snapshot identity is missing",
            )
        })?;
    require_identity(manifest_digest, expected.snapshot_digest)?;
    let provider = manifest
        .pointer("/evidence/provider")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            ImportError::new(
                ErrorCode::SnapshotUnavailable,
                "prepared manifest provider identity is missing",
            )
        })?;
    let native_session_id = manifest
        .pointer("/graph/nativeSessionId")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            ImportError::new(
                ErrorCode::SnapshotUnavailable,
                "prepared manifest native session identity is missing",
            )
        })?;
    require_identity(provider, expected.provider)?;
    require_identity(native_session_id, expected.native_session_id)?;
    let source_member =
        super::source_member::from_manifest(&manifest, provider, native_session_id)?;
    let relations: Vec<EventRelation> = serde_json::from_value(
        manifest
            .pointer("/graph/relations")
            .cloned()
            .ok_or_else(|| {
                ImportError::new(
                    ErrorCode::SnapshotUnavailable,
                    "prepared manifest relations are missing",
                )
            })?,
    )
    .map_err(|error| ImportError::new(ErrorCode::SnapshotUnavailable, error.to_string()))?;

    let mut required_ids = entry.selection.event_ids.clone();
    required_ids.push(entry.leaf_id.clone());
    required_ids.sort();
    required_ids.dedup();
    let metadata = super::search::indexed_event_metadata_by_ids(path, &required_ids)?;
    let found: HashSet<&str> = metadata
        .iter()
        .map(|event| event.event_id.as_str())
        .collect();
    if required_ids
        .iter()
        .any(|event_id| !found.contains(event_id.as_str()))
    {
        return Err(ImportError::new(
            ErrorCode::HistoryAccessUnavailable,
            "selection references an unknown event or leaf",
        ));
    }
    let mut allowed: HashSet<String> = super::graph::lineage_ids(&relations, &entry.leaf_id)
        .into_iter()
        .collect();
    if entry.selection.include_subagents {
        let all_subagents = subagent_member_ids(&relations);
        let mut roots: Vec<String> = relations
            .iter()
            .filter(|relation| {
                relation.kind == RelationKind::Subagent && allowed.contains(&relation.to)
            })
            .map(|relation| relation.from.clone())
            .collect();
        while let Some(id) = roots.pop() {
            if all_subagents.contains(id.as_str()) && allowed.insert(id.clone()) {
                roots.extend(
                    relations
                        .iter()
                        .filter(|relation| {
                            relation.kind == RelationKind::Parent && relation.to == id
                        })
                        .map(|relation| relation.from.clone()),
                );
            }
        }
    }
    if entry
        .selection
        .event_ids
        .iter()
        .any(|event_id| !allowed.contains(event_id))
    {
        return Err(ImportError::new(
            ErrorCode::HistoryAccessUnavailable,
            "selection is outside the chosen branch",
        ));
    }
    let selected_ids: HashSet<&str> = entry
        .selection
        .event_ids
        .iter()
        .map(String::as_str)
        .collect();
    if metadata
        .iter()
        .any(|event| selected_ids.contains(event.event_id.as_str()) && !event.model_visible)
    {
        return Err(ImportError::new(
            ErrorCode::HistoryAccessUnavailable,
            "selection contains a non-model event",
        ));
    }

    let asset_path = path.join("assets.json");
    let raw_assets = std::fs::read(&asset_path)
        .map_err(|error| ImportError::new(ErrorCode::SnapshotUnavailable, error.to_string()))?;
    let assets: Vec<super::types::AssetRecord> = serde_json::from_slice(&raw_assets)
        .map_err(|error| ImportError::new(ErrorCode::SnapshotUnavailable, error.to_string()))?;
    let mut asset_record_ids = HashSet::with_capacity(assets.len());
    if assets.iter().any(|asset| {
        asset.asset_id.trim().is_empty() || !asset_record_ids.insert(asset.asset_id.as_str())
    }) {
        return Err(ImportError::new(
            ErrorCode::SnapshotUnavailable,
            "prepared assets contain an invalid or duplicate identity",
        ));
    }
    let referenced_assets: HashSet<&str> = metadata
        .iter()
        .filter(|event| selected_ids.contains(event.event_id.as_str()))
        .flat_map(|event| event.asset_ids.iter().map(String::as_str))
        .collect();
    for asset_id in &entry.selection.asset_ids {
        let asset = assets
            .iter()
            .find(|asset| asset.asset_id == *asset_id)
            .filter(|asset| asset.state == super::types::AssetState::Stored)
            .ok_or_else(|| {
                ImportError::new(
                    ErrorCode::HistoryAccessUnavailable,
                    "selection references an unavailable asset",
                )
            })?;
        if !referenced_assets.contains(asset_id.as_str()) {
            return Err(ImportError::new(
                ErrorCode::HistoryAccessUnavailable,
                "selection references an asset outside selected events",
            ));
        }
        let digest = asset
            .digest
            .as_deref()
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                ImportError::new(
                    ErrorCode::SnapshotUnavailable,
                    "stored selected asset has no content digest",
                )
            })?;
        if digest.len() != 64
            || !digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(ImportError::new(
                ErrorCode::SnapshotUnavailable,
                "stored selected asset has an invalid content digest",
            ));
        }
        let blob = super::assets::blob_file(&path.join("blobs"), digest);
        super::catalog::ensure_inside_imports(&blob)?;
        let metadata = std::fs::symlink_metadata(&blob)
            .map_err(|error| ImportError::new(ErrorCode::SnapshotUnavailable, error.to_string()))?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err(ImportError::new(
                ErrorCode::SnapshotUnavailable,
                "selected asset blob is missing or not a regular file",
            ));
        }
        let actual_digest = super::hash::sha256_file(&blob)
            .map_err(|error| ImportError::new(ErrorCode::SnapshotUnavailable, error))?;
        if actual_digest != digest {
            return Err(ImportError::new(
                ErrorCode::SnapshotUnavailable,
                "selected asset blob does not match its content digest",
            ));
        }
    }

    Ok(SelectionFileIdentity {
        source_member,
        cwd: serde_json::from_value(
            manifest
                .pointer("/graph/cwd")
                .cloned()
                .unwrap_or(serde_json::Value::Null),
        )
        .map_err(|error| {
            ImportError::new(
                ErrorCode::PlanStale,
                format!("invalid prepared cwd: {error}"),
            )
        })?,
        snapshot_digest: manifest_digest.to_string(),
        provider: provider.to_string(),
        native_session_id: native_session_id.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    include!("selection_tests.rs");
}
