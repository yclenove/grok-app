//! Frozen v1 event schema. Adapters return evidence + graph + inventory + issues.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::errors::ErrorCode;

pub const SCHEMA_VERSION: u32 = 1;
pub const NORMALIZER_VERSION: &str = "1.0.6";
pub const CATALOG_SCHEMA_VERSION: u32 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Provider {
    Codex,
    ClaudeCode,
    Cursor,
    GenericFile,
}

impl Provider {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::ClaudeCode => "claude-code",
            Self::Cursor => "cursor",
            Self::GenericFile => "generic-file",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "codex" => Some(Self::Codex),
            "claude" | "claude-code" | "claude_code" => Some(Self::ClaudeCode),
            "cursor" => Some(Self::Cursor),
            "generic" | "generic-file" | "file" => Some(Self::GenericFile),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ImportOutcome {
    Complete,
    Partial,
    Unsupported,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TimeOrigin {
    Source,
    FileHint,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MessageRole {
    User,
    Assistant,
    System,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SourceSpan {
    pub artifact_id: String,
    pub record: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub block: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PayloadRef {
    pub blob_id: String,
    pub media_type: String,
    pub byte_length: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum ContentBlock {
    Text {
        text: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        language: Option<String>,
    },
    Code {
        text: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        language: Option<String>,
    },
    Asset {
        asset_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        alt: Option<String>,
    },
    VisibleReasoning {
        text: String,
    },
    Opaque {
        source_type: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        payload: Option<PayloadRef>,
    },
}

impl ContentBlock {
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text {
            text: text.into(),
            language: None,
        }
    }

    pub fn plain_text(&self) -> Option<&str> {
        match self {
            Self::Text { text, .. } | Self::Code { text, .. } | Self::VisibleReasoning { text } => {
                Some(text.as_str())
            }
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "encoding", rename_all = "kebab-case")]
pub enum ToolInput {
    Json { value: Value },
    Payload { payload: PayloadRef },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ToolOutcome {
    Success,
    Error,
    Cancelled,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EventBase {
    pub id: String,
    pub ordinal: u32,
    pub spans: Vec<SourceSpan>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<String>,
    pub time_origin: TimeOrigin,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum ImportedEvent {
    Message {
        #[serde(flatten)]
        base: EventBase,
        role: MessageRole,
        blocks: Vec<ContentBlock>,
        complete: bool,
    },
    ToolCall {
        #[serde(flatten)]
        base: EventBase,
        call_id: String,
        name: String,
        input: ToolInput,
        #[serde(skip_serializing_if = "Option::is_none")]
        parent_message_id: Option<String>,
    },
    ToolResult {
        #[serde(flatten)]
        base: EventBase,
        call_id: String,
        blocks: Vec<ContentBlock>,
        outcome: ToolOutcome,
    },
    Compaction {
        #[serde(flatten)]
        base: EventBase,
        summary: Vec<ContentBlock>,
        #[serde(skip_serializing_if = "Option::is_none")]
        covered_event_ids: Option<Vec<String>>,
        coverage_known: bool,
    },
    Metadata {
        #[serde(flatten)]
        base: EventBase,
        source_type: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        payload: Option<PayloadRef>,
    },
    Unsupported {
        #[serde(flatten)]
        base: EventBase,
        source_type: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        payload: Option<PayloadRef>,
    },
}

impl ImportedEvent {
    pub fn base(&self) -> &EventBase {
        match self {
            Self::Message { base, .. }
            | Self::ToolCall { base, .. }
            | Self::ToolResult { base, .. }
            | Self::Compaction { base, .. }
            | Self::Metadata { base, .. }
            | Self::Unsupported { base, .. } => base,
        }
    }

    pub fn id(&self) -> &str {
        &self.base().id
    }

    pub fn kind_name(&self) -> &'static str {
        match self {
            Self::Message { .. } => "message",
            Self::ToolCall { .. } => "tool-call",
            Self::ToolResult { .. } => "tool-result",
            Self::Compaction { .. } => "compaction",
            Self::Metadata { .. } => "metadata",
            Self::Unsupported { .. } => "unsupported",
        }
    }

    pub fn searchable_text(&self) -> String {
        match self {
            Self::Message { blocks, .. }
            | Self::ToolResult { blocks, .. }
            | Self::Compaction {
                summary: blocks, ..
            } => blocks
                .iter()
                .filter_map(ContentBlock::plain_text)
                .collect::<Vec<_>>()
                .join("\n"),
            Self::ToolCall { name, input, .. } => match input {
                ToolInput::Json { value } => format!("{name} {value}"),
                ToolInput::Payload { payload } => {
                    format!("{name} blob:{}", payload.blob_id)
                }
            },
            Self::Metadata { source_type, .. } | Self::Unsupported { source_type, .. } => {
                source_type.clone()
            }
        }
    }

    /// Model projection is distinct from local reading/search. Source system
    /// records and opaque/image payloads never become instructions or grants.
    pub fn model_visible_text(&self, include_reasoning: bool) -> String {
        match self {
            Self::Message {
                role: MessageRole::System,
                ..
            }
            | Self::Metadata { .. }
            | Self::Unsupported { .. } => String::new(),
            Self::Message { blocks, .. }
            | Self::ToolResult { blocks, .. }
            | Self::Compaction {
                summary: blocks, ..
            } => blocks
                .iter()
                .filter_map(|b| match b {
                    ContentBlock::VisibleReasoning { .. } if !include_reasoning => None,
                    _ => b.plain_text(),
                })
                .collect::<Vec<_>>()
                .join("\n"),
            Self::ToolCall { .. } => self.searchable_text(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RelationKind {
    Parent,
    Fork,
    Subagent,
    Compacts,
    RollsBack,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RelationConfidence {
    Explicit,
    Inferred,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EventRelation {
    pub kind: RelationKind,
    pub from: String,
    pub to: String,
    pub confidence: RelationConfidence,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CountBucket {
    pub messages: u32,
    pub tool_calls: u32,
    pub tool_results: u32,
    pub attachments: u32,
    pub subagents: u32,
}

impl CountBucket {
    pub fn add_event(&mut self, ev: &ImportedEvent) {
        match ev {
            ImportedEvent::Message { .. } | ImportedEvent::Unsupported { .. } => self.messages += 1,
            ImportedEvent::ToolCall { .. } => self.tool_calls += 1,
            ImportedEvent::ToolResult { .. } => self.tool_results += 1,
            _ => {}
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Inventory {
    pub discovered: CountBucket,
    pub selected: CountBucket,
    pub imported: CountBucket,
    pub deduplicated: CountBucket,
    pub unsupported: CountBucket,
    pub excluded: CountBucket,
    pub missing: CountBucket,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected_count: Option<u32>,
    /// Token totals reported by the original source. Never folded into Grok usage.
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    pub source_tokens: u64,
}

fn is_zero_u64(v: &u64) -> bool {
    *v == 0
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ImportIssue {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub span: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_code: Option<ErrorCode>,
}

impl ImportIssue {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            span: None,
            error_code: None,
        }
    }

    pub fn with_span(mut self, span: impl Into<String>) -> Self {
        self.span = Some(span.into());
        self
    }

    pub fn with_error(mut self, code: ErrorCode) -> Self {
        self.error_code = Some(code);
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AssetState {
    Stored,
    Missing,
    Excluded,
    Oversize,
    Unresolved,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AssetRecord {
    pub asset_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub digest: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime: Option<String>,
    pub size: u64,
    pub state: AssetState,
    pub source_span: SourceSpan,
    /// Original locator used to copy bytes. Never used as a display path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_ref: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SourceEvidence {
    pub provider: Provider,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_version: Option<String>,
    pub schema_fingerprint: String,
    pub adapter_version: String,
    pub platform: String,
    pub artifacts: Vec<ArtifactEvidence>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactEvidence {
    pub artifact_id: String,
    pub kind: String,
    pub relative_locator: String,
    pub byte_length: u64,
    pub sha256: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub complete_prefix_sha256: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NormalizedGraph {
    pub events: Vec<ImportedEvent>,
    pub relations: Vec<EventRelation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_leaf: Option<String>,
    pub native_session_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AdapterOutput {
    pub evidence: SourceEvidence,
    pub graph: NormalizedGraph,
    pub inventory: Inventory,
    pub issues: Vec<ImportIssue>,
    pub assets: Vec<AssetRecord>,
    pub outcome: ImportOutcome,
}

impl AdapterOutput {
    pub fn validate_graph(&mut self) {
        for issue in super::graph::validate(&self.graph.events, &self.graph.relations).issues {
            if !self.issues.contains(&issue) {
                self.issues.push(issue);
            }
        }
        if let Some(leaf) = &self.graph.active_leaf {
            if !self.graph.events.iter().any(|event| event.id() == leaf) {
                self.issues.push(ImportIssue::new(
                    "missing-active-leaf",
                    "active branch leaf is absent",
                ));
                self.graph.active_leaf = None;
            }
        }
        // Graph validation precedes attachment materialization. An unresolved
        // reference is still pending here; the copier's final recount decides
        // whether it became stored, missing, oversize or unresolved.
        self.recount_with_pending_assets(true);
    }

    pub fn recount(&mut self) {
        self.recount_with_pending_assets(false);
    }

    fn recount_with_pending_assets(&mut self, pending_assets: bool) {
        let mut imported = CountBucket::default();
        let mut unsupported = CountBucket::default();
        for ev in &self.graph.events {
            match ev {
                ImportedEvent::Unsupported { .. } => unsupported.add_event(ev),
                _ => imported.add_event(ev),
            }
        }
        imported.attachments = self
            .assets
            .iter()
            .filter(|a| a.state == AssetState::Stored)
            .count() as u32;
        self.inventory.missing.attachments = self
            .assets
            .iter()
            .filter(|a| {
                matches!(
                    a.state,
                    AssetState::Missing | AssetState::Oversize | AssetState::Unresolved
                )
            })
            .count() as u32;
        self.inventory.imported = imported;
        self.inventory.unsupported = unsupported;
        if self.outcome == ImportOutcome::Complete
            && (!self.issues.is_empty()
                || self.assets.iter().any(|asset| {
                    matches!(asset.state, AssetState::Missing | AssetState::Oversize)
                        || (!pending_assets && asset.state == AssetState::Unresolved)
                })
                || self.inventory.unsupported.messages
                    + self.inventory.unsupported.tool_calls
                    + self.inventory.unsupported.tool_results
                    > 0)
        {
            self.outcome = ImportOutcome::Partial;
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SourceCandidate {
    pub candidate_id: String,
    pub provider: Provider,
    pub native_session_id: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    pub locator: String,
    pub source_revision: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub archived: Option<bool>,
    pub completeness: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frozen_event_schema_and_catalog_schema_versions_are_explicit() {
        assert_eq!(SCHEMA_VERSION, 1);
        assert_eq!(CATALOG_SCHEMA_VERSION, 2);
        assert_eq!(NORMALIZER_VERSION, "1.0.6");
    }
}
