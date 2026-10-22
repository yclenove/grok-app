//! Strict v1 body schema. No source text, project names/paths or receipt bodies;
//! canonical digests bind those arbitrarily long records without new limits.
use serde::{Deserialize, Deserializer, Serialize};
use uuid::Uuid;

use super::{invalid, ErrorCode, ImportError, VERSION};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(in crate::session_migration) struct Binding {
    pub job_id: String,
    pub context_digest: String,
    pub plan_id: String,
    pub plan_hash: String,
    pub plan_digest: String,
    pub acceptance_digest: String,
    pub candidate_digest: String,
    pub input_digest: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Body {
    pub version: u32,
    pub binding: Binding,
    pub stage: Stage,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(in crate::session_migration) enum Stage {
    Allocated(Allocation),
    Stored(StoreResult),
    Committed(Committed),
    Deduplicated(Deduplicated),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(in crate::session_migration) struct Allocation {
    pub receipt_id: String,
    pub import_id: String,
    pub revision_id: String,
    #[serde(deserialize_with = "required_option")]
    pub previous_revision: Option<String>,
    pub created_entry: bool,
    pub project: ProjectAllocation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(in crate::session_migration) enum ProjectAllocation {
    Unassigned {},
    Match {
        #[serde(rename = "projectId")]
        project_id: String,
        #[serde(rename = "pathDigest")]
        path_digest: String,
        #[serde(rename = "allowUnassigned")]
        allow_unassigned: bool,
    },
    Create {
        #[serde(rename = "projectId")]
        project_id: String,
        #[serde(rename = "pathDigest")]
        path_digest: String,
        #[serde(rename = "allowUnassigned")]
        allow_unassigned: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(in crate::session_migration) struct StoreResult {
    pub allocation_digest: String,
    pub linked_receipt_digest: String,
    pub project: ProjectResult,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(in crate::session_migration) enum ProjectResult {
    Unassigned {},
    UnassignedReported {},
    Matched {
        #[serde(rename = "projectId")]
        project_id: String,
        #[serde(rename = "pathDigest")]
        path_digest: String,
        #[serde(rename = "snapshotDigest")]
        snapshot_digest: String,
    },
    // Only the actual native resolver can attest to equivalent local path
    // spellings. Retain both raw digests; never rewrite the reused project row.
    MatchedAlias {
        #[serde(rename = "projectId")]
        project_id: String,
        #[serde(rename = "pathDigest")]
        path_digest: String,
        #[serde(rename = "targetPathDigest")]
        target_path_digest: String,
        #[serde(rename = "snapshotDigest")]
        snapshot_digest: String,
    },
    Created {
        #[serde(rename = "projectId")]
        project_id: String,
        #[serde(rename = "pathDigest")]
        path_digest: String,
        #[serde(rename = "snapshotDigest")]
        snapshot_digest: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(in crate::session_migration) struct Committed {
    pub store_digest: String,
    pub receipt_digest: String,
    pub item_digest: String,
}

/// A terminal reuse result, not a store/create result. The reserved new revision
/// stays unused; the actual verified old revision is recorded explicitly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(in crate::session_migration) struct Deduplicated {
    pub allocation_digest: String,
    pub import_id: String,
    pub revision_id: String,
    pub receipt_id: String,
    pub receipt_digest: String,
    pub revision_digest: String,
    pub selection_hash: String,
    pub entry_identity_digest: String,
    pub item_digest: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::session_migration) enum StageKind {
    Allocated,
    Stored,
    Committed,
    Deduplicated,
}

impl StageKind {
    pub(super) fn key(self) -> &'static str {
        match self {
            Self::Allocated => "allocated",
            Self::Stored => "stored",
            Self::Committed => "committed",
            Self::Deduplicated => "deduplicated",
        }
    }

    pub(super) fn domain(self) -> &'static str {
        match self {
            Self::Allocated => "session-import-publication-allocation-v1",
            Self::Stored => "session-import-publication-store-result-v1",
            Self::Committed => "session-import-publication-committed-v1",
            Self::Deduplicated => "session-import-publication-deduplicated-v1",
        }
    }
}

// Without this explicit deserializer, serde treats a missing Option field as
// None. The wire requires previousRevision even when its value is null.
fn required_option<'de, D: Deserializer<'de>>(de: D) -> Result<Option<String>, D::Error> {
    Option::deserialize(de)
}

pub(super) fn digest(raw: &str) -> Result<(), ImportError> {
    if raw.len() == 64
        && raw
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
    {
        Ok(())
    } else {
        Err(invalid())
    }
}

fn uuid(raw: &str) -> Result<(), ImportError> {
    if Uuid::parse_str(raw).is_ok_and(|id| !id.is_nil() && id.to_string() == raw) {
        Ok(())
    } else {
        Err(invalid())
    }
}

// Existing native project IDs need not be UUIDs. Only newly allocated project
// IDs are canonical UUIDs; matching a legacy row must preserve its exact ID.
fn project_id(raw: &str) -> Result<(), ImportError> {
    if !raw.is_empty()
        && raw.len() <= 256
        && raw != crate::store::GENERAL_PROJECT_ID
        && !raw.chars().any(char::is_control)
    {
        Ok(())
    } else {
        Err(invalid())
    }
}

impl Binding {
    pub(in crate::session_migration) fn validate(&self) -> Result<(), ImportError> {
        uuid(&self.job_id)?;
        uuid(&self.plan_id)?;
        for raw in [
            &self.context_digest,
            &self.plan_hash,
            &self.plan_digest,
            &self.acceptance_digest,
            &self.candidate_digest,
            &self.input_digest,
        ] {
            digest(raw)?;
        }
        Ok(())
    }
}

impl Body {
    pub(super) fn validate(&self) -> Result<(), ImportError> {
        if self.version != VERSION {
            return Err(ImportError::new(
                if self.version > VERSION {
                    ErrorCode::ImportSchemaTooNew
                } else {
                    ErrorCode::SchemaUnsupported
                },
                "unsupported publication outcome schema",
            ));
        }
        self.binding.validate()?;
        match &self.stage {
            Stage::Allocated(record) => {
                uuid(&record.receipt_id)?;
                uuid(record.import_id.strip_prefix("imp:").ok_or_else(invalid)?)?;
                uuid(&record.revision_id)?;
                if let Some(previous) = &record.previous_revision {
                    uuid(previous)?;
                    if record.created_entry || previous == &record.revision_id {
                        return Err(invalid());
                    }
                }
                match &record.project {
                    ProjectAllocation::Unassigned {} => Ok(()),
                    ProjectAllocation::Match {
                        project_id: id,
                        path_digest,
                        ..
                    } => {
                        project_id(id)?;
                        digest(path_digest)
                    }
                    ProjectAllocation::Create {
                        project_id,
                        path_digest,
                        ..
                    } => {
                        uuid(project_id)?;
                        digest(path_digest)
                    }
                }
            }
            Stage::Stored(record) => {
                digest(&record.allocation_digest)?;
                digest(&record.linked_receipt_digest)?;
                match &record.project {
                    ProjectResult::Unassigned {} | ProjectResult::UnassignedReported {} => Ok(()),
                    ProjectResult::Matched {
                        project_id: id,
                        path_digest,
                        snapshot_digest,
                    } => {
                        project_id(id)?;
                        digest(path_digest)?;
                        digest(snapshot_digest)
                    }
                    ProjectResult::Created {
                        project_id,
                        path_digest,
                        snapshot_digest,
                    } => {
                        uuid(project_id)?;
                        digest(path_digest)?;
                        digest(snapshot_digest)
                    }
                    ProjectResult::MatchedAlias {
                        project_id: id,
                        path_digest,
                        target_path_digest,
                        snapshot_digest,
                    } => {
                        project_id(id)?;
                        digest(path_digest)?;
                        digest(target_path_digest)?;
                        if path_digest == target_path_digest {
                            return Err(invalid());
                        }
                        digest(snapshot_digest)
                    }
                }
            }
            Stage::Committed(record) => {
                digest(&record.store_digest)?;
                digest(&record.receipt_digest)?;
                digest(&record.item_digest)
            }
            Stage::Deduplicated(record) => {
                uuid(record.import_id.strip_prefix("imp:").ok_or_else(invalid)?)?;
                uuid(&record.revision_id)?;
                uuid(&record.receipt_id)?;
                for raw in [
                    &record.allocation_digest,
                    &record.receipt_digest,
                    &record.revision_digest,
                    &record.selection_hash,
                    &record.entry_identity_digest,
                    &record.item_digest,
                ] {
                    digest(raw)?;
                }
                Ok(())
            }
        }
    }
}

impl Stage {
    pub(super) fn kind(&self) -> StageKind {
        match self {
            Self::Allocated(_) => StageKind::Allocated,
            Self::Stored(_) => StageKind::Stored,
            Self::Committed(_) => StageKind::Committed,
            Self::Deduplicated(_) => StageKind::Deduplicated,
        }
    }
}

impl ProjectAllocation {
    pub(super) fn permits(&self, result: &ProjectResult) -> Result<(), ImportError> {
        let allowed = match (self, result) {
            (Self::Unassigned {}, ProjectResult::Unassigned {}) => true,
            (
                Self::Create {
                    allow_unassigned: true,
                    ..
                }
                | Self::Match {
                    allow_unassigned: true,
                    ..
                },
                ProjectResult::UnassignedReported {},
            ) => true,
            (
                Self::Match {
                    project_id,
                    path_digest,
                    ..
                },
                ProjectResult::Matched {
                    project_id: id,
                    path_digest: path,
                    ..
                },
            ) => project_id == id && path_digest == path,
            (
                Self::Create {
                    project_id,
                    path_digest,
                    ..
                },
                ProjectResult::Created {
                    project_id: id,
                    path_digest: path,
                    ..
                },
            ) => project_id == id && path_digest == path,
            (
                Self::Create { path_digest, .. },
                ProjectResult::Matched {
                    path_digest: path, ..
                },
            ) => path_digest == path,
            (
                Self::Create { path_digest, .. },
                ProjectResult::MatchedAlias {
                    target_path_digest, ..
                },
            ) => path_digest == target_path_digest,
            _ => false,
        };
        if allowed {
            Ok(())
        } else {
            Err(invalid())
        }
    }
}
