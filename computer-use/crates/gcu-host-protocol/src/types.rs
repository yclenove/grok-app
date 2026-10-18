//! Host approval and read-only policy data, without host-api/core dependencies.
use crate::messages::Image;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Platform that verified an App identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Platform {
    /// macOS.
    Macos,
    /// Windows.
    Windows,
    /// Linux.
    Linux,
}

/// OS signature verification status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Signature {
    /// Valid signature status.
    Valid,
    /// Invalid signature status.
    Invalid,
    /// Unsigned signature status.
    Unsigned,
    /// Unknown signature status.
    Unknown,
}

/// Code hash, certificate, product, build, and Linux file identity.
///
/// Absent or empty on a legacy record. Persistent matching does not treat
/// that omission as a wildcard.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(deny_unknown_fields, default)]
pub struct GrantProof {
    /// macOS code-directory hash.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cdhash: Option<String>,
    /// SHA-256 of the signing certificate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub certificate_sha256: Option<String>,
    /// OS product name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub product: Option<String>,
    /// macOS bundle version. Windows build identity is the content hash.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build: Option<String>,
    /// Linux device id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_dev: Option<u64>,
    /// Linux inode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_inode: Option<u64>,
}

impl GrantProof {
    /// True when every fact is absent.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.cdhash.is_none()
            && self.certificate_sha256.is_none()
            && self.product.is_none()
            && self.build.is_none()
            && self.file_dev.is_none()
            && self.file_inode.is_none()
    }
}

/// OS-verified authorization identity; model names never establish identity.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Identity {
    /// Verifying platform.
    pub platform: Platform,
    /// Canonical authorization key.
    pub key: String,
    /// macOS signing team.
    pub team_id: Option<String>,
    /// Verified Windows publisher.
    pub publisher: Option<String>,
    /// Verified executable digest.
    pub sha256: Option<[u8; 32]>,
    /// OS verification result.
    pub signature: Signature,
    /// Canonical executable path.
    pub image_path: String,
    /// Extra §7.2 facts. Missing on records written before this key.
    #[serde(default, skip_serializing_if = "GrantProof::is_empty")]
    pub proof: GrantProof,
}

/// Application approval context.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct App {
    /// OS display name; only a label.
    pub name: String,
    /// Running process.
    pub pid: Option<u32>,
    /// OS-verified identity.
    pub identity: Identity,
    /// Small icon for display only.
    pub icon: Option<Image>,
}

/// Engine-computed action risk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Risk {
    /// Ordinary approval.
    Low,
    /// Sensitive or unknown semantics.
    High,
}

/// Actual user approval; timeout is an RPC error, not a grant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    /// Turn authorization decision.
    Turn,
    /// Session authorization decision.
    Session,
    /// Always authorization decision.
    Always,
    /// Deny authorization decision.
    Deny,
}

/// Request a real App approval; Host binds the RPC ID and authenticated engine.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RequestApp {
    /// Owning App session.
    pub session_id: String,
    /// Verified target.
    pub app: App,
    /// Model-provided plain text; label and sanitize in UI.
    pub reason: Option<String>,
    /// Engine-computed risk.
    pub risk: Risk,
}

/// One response to one approval; consumed at most once.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AppDecision {
    /// User decision; Host alone persists Always.
    pub decision: Decision,
}

/// Sensitive action binding.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ActionSummary {
    /// Engine description.
    pub text: String,
    /// State shown to the user.
    pub snapshot_id: String,
    /// Digest of exact action parameters.
    pub action_digest: [u8; 32],
}

/// Confirm one exact sensitive action.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ConfirmAction {
    /// Owning App session.
    pub session_id: String,
    /// Verified target.
    pub app: App,
    /// Exact state/action binding.
    pub summary: ActionSummary,
    /// Redacted target crop.
    pub crop: Option<Image>,
}

/// Sensitive action decision.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Confirmed {
    /// True only for the exact request shown to the user.
    pub confirmed: bool,
}

/// Tell the user to take over; never interpreted as approval.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Handoff {
    /// Owning App session.
    pub session_id: String,
    /// Target.
    pub app: App,
    /// Engine-written reason, not model text.
    pub reason: String,
}

/// Sensitive confirmation policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ConfirmationLevel {
    /// Off confirmation.
    Off,
    /// Standard confirmation.
    Standard,
    /// Strict confirmation.
    Strict,
}

/// Foreground delivery policy; absence of physical activity never grants it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ForegroundPolicy {
    /// Never foreground policy.
    Never,
    /// Ask foreground policy.
    Ask,
    /// Always foreground policy.
    Always,
}

/// Read-only policy snapshot owned by App.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    /// Monotonic policy revision.
    pub version: u64,
    /// Whether persistent grants may be used.
    pub always_allow_enabled: bool,
    /// Confirmation threshold.
    pub sensitive_action_confirmation: ConfirmationLevel,
    /// Approval timeout (default 120 seconds).
    pub timeout_secs: u64,
    /// Default ask or never.
    pub allow_foreground: ForegroundPolicy,
    /// Allow private delivery APIs.
    pub macos_private_spi: bool,
    /// Physical idle threshold.
    pub user_idle_ms: u64,
    /// Hotkey selection.
    pub stop_hotkey: String,
    /// Tree budget.
    pub max_tree_nodes: u32,
    /// Image budget.
    pub max_image_edge: u32,
    /// Image encoding.
    pub image_format: String,
    /// Additional denylist.
    pub extra_denied_apps: Vec<String>,
    /// Host-persisted identities.
    pub always: Vec<Identity>,
}
