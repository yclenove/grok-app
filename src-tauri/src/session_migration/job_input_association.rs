//! Explicit plan/job binding carried by input v3; this intent is not a grant.
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use super::{
    engine::PlanState,
    errors::{ErrorCode, ImportError},
    jobs::JobRecord,
    selection::SelectionSet,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct AssociationInput {
    pub confirmation: super::identity_association::AssociationConfirmation,
    pub job_id: String,
    pub request_id: String,
    pub plan_id: String,
    pub plan_hash: String,
}

fn stale(message: &str) -> ImportError {
    ImportError::new(ErrorCode::PlanStale, message)
}

const STORED_JOB_SQL: &str = "SELECT rowid,typeof(request_id),typeof(plan_hash),
    typeof(state),typeof(state_version),typeof(json),typeof(created_at)
    FROM jobs WHERE job_id=?1";

#[path = "job_association_header.rs"]
mod stored_header;

/// Metadata only, not a caller-constructible authentication witness.
pub(in crate::session_migration) fn bounded_job_metadata(
    conn: &Connection,
    job_id: &str,
) -> Result<JobRecord, ImportError> {
    stored_header::load(conn, job_id)
}

pub(in crate::session_migration) fn ordinary_job_metadata(
    conn: &Connection,
    job_id: &str,
) -> Result<JobRecord, ImportError> {
    stored_header::ordinary_metadata(conn, job_id)
}

impl AssociationInput {
    pub fn validate(&self, selections: &SelectionSet) -> Result<(), ImportError> {
        self.confirmation.validate()?;
        let uuid = |text: &str| uuid::Uuid::parse_str(text).is_ok_and(|id| id.to_string() == text);
        if !uuid(&self.job_id)
            || !uuid(&self.plan_id)
            || self.request_id.is_empty()
            || self.plan_hash.len() != 64
            || !self
                .plan_hash
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || selections.single_entry()?.preview_id != self.confirmation.preview_id
        {
            return Err(stale(
                "association input requires canonical exact plan/job/preview binding",
            ));
        }
        Ok(())
    }

    pub fn require_job(&self, job: &JobRecord) -> Result<(), ImportError> {
        if self.job_id != job.job_id
            || self.request_id != job.request_id
            || self.plan_id != job.plan_id
            || self.plan_hash != job.plan_hash
        {
            return Err(stale(
                "association input differs from the durable job context",
            ));
        }
        Ok(())
    }

    pub fn require_plan(
        &self,
        plan: &PlanState,
        selections: &SelectionSet,
    ) -> Result<(), ImportError> {
        self.validate(selections)?;
        let entry = selections.single_entry()?;
        if self.plan_id != plan.plan_id
            || self.plan_hash != plan.plan_hash
            || !plan.identity_associations.contains(&self.confirmation)
            || !plan.selection_set.entries.contains(entry)
        {
            return Err(stale(
                "association input differs from its confirmed raw plan selection",
            ));
        }
        Ok(())
    }

    /// One bounded header, not every item/body in a large job. No source reads.
    pub fn stored_job(&self, conn: &Connection, job_id: &str) -> Result<JobRecord, ImportError> {
        let job = stored_header::load(conn, job_id)?;
        self.require_job(&job)?;
        Ok(job)
    }
}

/// Ordinary publication cannot ignore or reinterpret an association intent.
pub(crate) fn refuse_publication(
    association: Option<&AssociationInput>,
) -> Result<(), ImportError> {
    if association.is_some() {
        return Err(ImportError::new(
            ErrorCode::FormatUnsupported,
            "identity associations require confirmed-plan acceptance",
        ));
    }
    Ok(())
}

#[cfg(test)]
#[path = "job_association_header_tests.rs"]
mod legacy_identity_tests;
