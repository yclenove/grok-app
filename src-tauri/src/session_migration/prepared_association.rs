//! Explicit historical review over the exact immutable members of a prepare batch.
use serde::{Deserialize, Serialize};

use super::errors::{ErrorCode, ImportError};
use super::identity_association::{
    self, AssociationConfirmation, AssociationRequest, AssociationReview,
};
use super::{engine, prepare, workspace::WorkspaceMapping};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PreparedAssociationRequest {
    pub version: u32,
    pub batch_id: String,
    pub expected_state_version: u64,
    pub candidate_id: String,
    pub preview_id: String,
    pub import_id: String,
    pub expected_revision: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreparedAssociationReview {
    pub batch_id: String,
    pub state_version: u64,
    pub candidate_id: String,
    pub leaf_id: String,
    pub review: AssociationReview,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PreparedAssociationPlanRequest {
    pub version: u32,
    pub batch_id: String,
    pub expected_state_version: u64,
    pub mappings: Vec<WorkspaceMapping>,
    pub identity_associations: Vec<AssociationConfirmation>,
}

fn version(value: u32) -> Result<(), ImportError> {
    if value == 1 {
        return Ok(());
    }
    Err(ImportError::new(
        if value > 1 {
            ErrorCode::ImportSchemaTooNew
        } else {
            ErrorCode::SchemaUnsupported
        },
        "unsupported prepared association version",
    ))
}

impl PreparedAssociationRequest {
    pub fn inspect(&self) -> Result<PreparedAssociationReview, ImportError> {
        version(self.version)?;
        let member = prepare::validated_member(
            &self.batch_id,
            self.expected_state_version,
            &self.candidate_id,
            &self.preview_id,
        )?;
        let review = identity_association::inspect(&AssociationRequest {
            version: self.version,
            import_id: self.import_id.clone(),
            expected_revision: self.expected_revision.clone(),
            preview: member.selection.clone(),
        })?;
        Ok(PreparedAssociationReview {
            batch_id: self.batch_id.clone(),
            state_version: self.expected_state_version,
            candidate_id: self.candidate_id.clone(),
            leaf_id: member.selection.leaf_id.clone(),
            review,
        })
    }
}

impl PreparedAssociationPlanRequest {
    pub fn build(self) -> Result<engine::PlanState, ImportError> {
        version(self.version)?;
        if self.identity_associations.is_empty() {
            return Err(ImportError::new(
                ErrorCode::PlanStale,
                "prepared association plan requires explicit confirmations",
            ));
        }
        let prepared = prepare::validated_plan_inputs(&self.batch_id)?;
        if prepared.origin.state_version != self.expected_state_version {
            return Err(ImportError::new(
                ErrorCode::PlanStale,
                "prepare batch changed",
            ));
        }
        engine::plan_from_validated_prepare_with_associations(
            prepared,
            self.mappings,
            self.identity_associations,
        )
    }
}
