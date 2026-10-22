//! Canonical plan inputs must remain bindings, not discarded hash material.
use super::errors::{ErrorCode, ImportError};
use super::selection::PreparedSelectionValidation;
use super::source_member::SourceMemberPin;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct MemberBinding {
    version: u32,
    digest: String,
}

impl MemberBinding {
    fn capture(pin: &SourceMemberPin) -> Result<Self, ImportError> {
        pin.verify()?;
        Ok(Self {
            version: 1,
            digest: super::hash::options_hash(&serde_json::json!([
                "session-import-plan-member-v1",
                pin
            ])),
        })
    }

    fn validate(&self) -> Result<(), ImportError> {
        if self.version != 1 {
            return Err(ImportError::new(
                if self.version > 1 {
                    ErrorCode::ImportSchemaTooNew
                } else {
                    ErrorCode::SchemaUnsupported
                },
                "unsupported plan member binding version",
            ));
        }
        if self.digest.len() != 64 || !self.digest.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(stale("invalid plan member binding digest"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PlanSnapshot {
    pub preview_id: String,
    pub snapshot_digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    source_member: Option<MemberBinding>,
}

fn stale(message: &str) -> ImportError {
    ImportError::new(ErrorCode::PlanStale, message)
}

impl PlanSnapshot {
    pub fn capture(
        preview_id: String,
        snapshot_digest: String,
        pin: Option<&SourceMemberPin>,
    ) -> Result<Self, ImportError> {
        Ok(Self {
            preview_id,
            snapshot_digest,
            source_member: pin.map(MemberBinding::capture).transpose()?,
        })
    }

    pub fn require(&self, verified: &PreparedSelectionValidation) -> Result<(), ImportError> {
        self.require_metadata(&verified.snapshot_digest, verified.source_member.as_ref())
    }

    pub fn require_metadata(
        &self,
        digest: &str,
        source_member: Option<&SourceMemberPin>,
    ) -> Result<(), ImportError> {
        let binding = source_member.map(MemberBinding::capture).transpose()?;
        if self.snapshot_digest != digest || self.source_member != binding {
            return Err(stale(
                "prepared snapshot or source member differs from plan",
            ));
        }
        Ok(())
    }
}

pub(crate) fn parse(
    raw: &[serde_json::Value],
    previews: &[String],
) -> Result<HashMap<String, PlanSnapshot>, ImportError> {
    let expected: HashSet<&str> = previews.iter().map(String::as_str).collect();
    let mut snapshots = HashMap::new();
    for value in raw {
        let snapshot: PlanSnapshot = serde_json::from_value(value.clone())
            .map_err(|_| stale("invalid durable plan snapshot binding"))?;
        if let Some(binding) = &snapshot.source_member {
            binding.validate()?;
        }
        if snapshot.snapshot_digest.is_empty()
            || !expected.contains(snapshot.preview_id.as_str())
            || snapshots
                .insert(snapshot.preview_id.clone(), snapshot)
                .is_some()
        {
            return Err(stale(
                "durable plan snapshots must cover previews exactly once",
            ));
        }
    }
    if snapshots.len() != previews.len() || expected.len() != previews.len() {
        return Err(stale("durable plan snapshot binding is missing"));
    }
    Ok(snapshots)
}
