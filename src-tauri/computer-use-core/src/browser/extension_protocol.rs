//! Versioned, bounded ExistingTab wire messages. No arbitrary source or generic params.
use serde::{Deserialize, Serialize};

pub use super::extension_image::ExtensionScreenshot;
use crate::pairing::PairingConnection;
use crate::protocol::{OBSERVATION_ARIA_CHARS, OBSERVATION_NAME_CHARS, OBSERVATION_NODE_CAP};

pub const EXTENSION_PROTOCOL: u32 = 1;
// 400k image + worst-case escaped text/node strings; other IPC routes stay at 64 KiB.
pub const EXTENSION_RESULT_BYTES: usize = 768 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ExtensionRequest {
    pub protocol: u32,
    pub request_id: String,
    pub sequence: u64,
    pub deadline_ms: u64,
    pub connection: PairingConnection,
    pub session: String,
    pub run_id: String,
    pub tab_id: String,
    pub document_generation: u64,
    pub grant_generation: u64,
    pub command: ExtensionCommand,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", deny_unknown_fields, rename_all = "camelCase")]
pub enum ExtensionCommand {
    Observe {
        #[serde(rename = "snapshotId")]
        snapshot_id: String,
        screenshot: bool,
        preview: bool,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ExtensionNode {
    pub element_ref: String,
    pub role: String,
    pub name: String,
    pub disabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ExtensionObservation {
    pub snapshot_id: String,
    pub document_id: String,
    pub title: String,
    pub url: String,
    pub text: String,
    pub nodes: Vec<ExtensionNode>,
    pub viewport_width: u32,
    pub viewport_height: u32,
    pub truncated: bool,
    pub screenshot: Option<ExtensionScreenshot>,
}

impl ExtensionObservation {
    pub fn validate(&self, expected_snapshot: &str, screenshot: bool) -> bool {
        let url = reqwest::Url::parse(&self.url).ok();
        let mut refs = std::collections::HashSet::new();
        self.snapshot_id == expected_snapshot
            && !self.document_id.is_empty()
            && self.document_id.len() <= 128
            && self
                .document_id
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'-')
            && self.title.chars().count() <= 512
            && self.url.len() <= 8192
            && url.is_some_and(|u| {
                matches!(u.scheme(), "http" | "https")
                    && u.host_str().is_some()
                    && u.username().is_empty()
                    && u.password().is_none()
            })
            && self.text.chars().count() <= OBSERVATION_ARIA_CHARS
            && self.nodes.len() <= OBSERVATION_NODE_CAP
            && (1..=16384).contains(&self.viewport_width)
            && (1..=16384).contains(&self.viewport_height)
            && self.screenshot.is_some() == screenshot
            && self
                .screenshot
                .as_ref()
                .is_none_or(|image| image.validate(self.viewport_width, self.viewport_height))
            && self.nodes.iter().all(|node| {
                !node.element_ref.is_empty()
                    && node.element_ref.len() <= 128
                    && node
                        .element_ref
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || c == b'-')
                    && refs.insert(&node.element_ref)
                    && !node.role.is_empty()
                    && node.role.chars().count() <= 64
                    && node.name.chars().count() <= OBSERVATION_NAME_CHARS
            })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields, rename_all = "camelCase")]
pub enum ExtensionOutcome {
    Observation { observation: ExtensionObservation },
    Rejected { reason: ExtensionRejection },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ExtensionRejection {
    Cancelled,
    DocumentChanged,
    NotVisible,
    Unsupported,
    Deadline,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ExtensionResult {
    pub request: ExtensionRequest,
    pub outcome: ExtensionOutcome,
}
