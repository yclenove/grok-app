//! Version 2 is explicitly negotiated. Version 1 remains observation-only.
use serde::{Deserialize, Serialize};

use super::extension_completion::{CompletionBinding, CompletionProof, ACTION_COMPLETION_PROTOCOL};
use crate::pairing::PairingConnection;

mod command;
pub use command::{
    ActKind, ActionName, ClickParameters, ExtensionActionCommand, LeftButton, ScrollParameters,
    TextParameters, WaitParameters,
};

pub const ACTION_PACKET_BYTES: usize = 64 * 1024;

/// Internal Host admission; never deserialized from extension or model HTTP.
pub struct ExistingActionInput<'a> {
    pub session: &'a str,
    pub run: &'a str,
    pub tab: &'a str,
    pub grant_generation: u64,
    pub document_generation: u64,
    pub command: ExtensionActionCommand,
    pub cancellation: crate::execution::ActionCancellation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ActionNegotiation {
    pub protocol: u32,
    pub completion_protocol: u32,
    pub connection: PairingConnection,
    pub actions: Vec<ActionName>,
}

impl ActionNegotiation {
    pub(crate) fn valid(&self) -> bool {
        self.protocol == ACTION_COMPLETION_PROTOCOL
            && self.completion_protocol == ACTION_COMPLETION_PROTOCOL
            && !self.actions.is_empty()
            && self.actions.len() <= 5
            && self
                .actions
                .iter()
                .enumerate()
                .all(|(i, action)| !self.actions[..i].contains(action))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ExtensionActionRequest {
    pub protocol: u32,
    pub request_id: String,
    pub sequence: u64,
    pub deadline_ms: u64,
    pub connection: PairingConnection,
    pub session: String,
    pub run_id: String,
    pub tab_id: String,
    pub document_id: String,
    pub document_generation: u64,
    pub grant_generation: u64,
    pub command: ExtensionActionCommand,
}

impl ExtensionActionRequest {
    pub(crate) fn binding(&self) -> CompletionBinding {
        CompletionBinding {
            protocol: self.protocol,
            request_id: self.request_id.clone(),
            connection: self.connection.clone(),
            session: self.session.clone(),
            run_id: self.run_id.clone(),
            tab_id: self.tab_id.clone(),
            document_id: self.document_id.clone(),
            document_generation: self.document_generation,
            grant_generation: self.grant_generation,
            snapshot_id: self.command.snapshot_id().into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ActionDispatch {
    pub request: ExtensionActionRequest,
    pub proof: CompletionProof,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ActionStatus {
    Rejected,
    Unknown,
    Applied,
    Verified,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActionOutcome {
    pub status: ActionStatus,
    pub detail: String,
}

impl ActionOutcome {
    pub(crate) fn valid(&self) -> bool {
        !self.detail.is_empty()
            && self.detail.len() <= 64
            && self
                .detail
                .bytes()
                .all(|c| c.is_ascii_lowercase() || c == b'_')
    }
}

impl std::fmt::Debug for ActionOutcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ActionOutcome")
            .field("status", &self.status)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActionResult {
    pub request: ExtensionActionRequest,
    pub proof: CompletionProof,
    pub outcome: ActionOutcome,
}

#[cfg(test)]
mod tests;
