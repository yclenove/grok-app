//! Mandatory observation lifecycle, independent of action execution receipts.
use serde::{Deserialize, Serialize};

use super::extension_completion::CompletionProof;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ObservationPurpose {
    Observation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ObservationProof {
    pub purpose: ObservationPurpose,
    pub proof: CompletionProof,
}
