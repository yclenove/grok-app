//! Fixed-size acceptance metadata; original plan/input bodies keep their formats.
use serde::{Deserialize, Serialize};

use super::{invalid, ImportError, VERSION};
use crate::session_migration::{
    errors::ErrorCode, jobs::JobRecord, publication_protocol::PublicationProtocol,
};

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Header {
    pub version: u32,
    pub outcome_version: u32,
    pub job_id: String,
    pub context_digest: String,
    pub plan_id: String,
    pub plan_hash: String,
    pub plan_digest: String,
    pub scope_digest: String,
    pub member_count: u32,
    pub members_digest: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Member {
    pub version: u32,
    pub job_id: String,
    pub header_digest: String,
    pub candidate_digest: String,
    pub input_digest: String,
    pub allocation_digest: String,
}

pub(super) fn version(actual: u32, supported: u32) -> Result<(), ImportError> {
    if actual == supported {
        return Ok(());
    }
    Err(ImportError::new(
        if actual > supported {
            ErrorCode::ImportSchemaTooNew
        } else {
            ErrorCode::SchemaUnsupported
        },
        "unsupported signed publication acceptance version",
    )
    .job_local())
}

pub(super) fn digest(value: &str) -> Result<(), ImportError> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(invalid());
    }
    Ok(())
}

pub(super) fn context_digest(job: &JobRecord) -> String {
    crate::session_migration::hash::options_hash(&serde_json::json!([
        job.job_id,
        job.request_id,
        job.plan_id,
        job.plan_hash,
        job.created_at
    ]))
}

pub(super) fn members_digest(members: &std::collections::BTreeMap<String, String>) -> String {
    // Map ordering binds the complete set without making display order authority.
    crate::session_migration::hash::options_hash(&serde_json::json!([
        "session-import-publication-accepted-members-v1",
        members
    ]))
}

pub(super) fn member_commitment(input: &str, allocation: &str) -> String {
    #[cfg(test)]
    let _span =
        crate::session_migration::codec_profile::Span::start("acceptance-member-commitment", 0);
    crate::session_migration::hash::options_hash(&serde_json::json!([input, allocation]))
}

impl Header {
    pub(super) fn validate(&self) -> Result<(), ImportError> {
        version(self.version, VERSION)?;
        version(
            self.outcome_version,
            crate::session_migration::publication_outcome::VERSION,
        )?;
        PublicationProtocol::SignedOutcomeV1.require_job_id(&self.job_id)?;
        if self.member_count == 0
            || !uuid::Uuid::parse_str(&self.plan_id).is_ok_and(|id| id.to_string() == self.plan_id)
        {
            return Err(invalid());
        }
        for value in [
            &self.context_digest,
            &self.plan_hash,
            &self.plan_digest,
            &self.scope_digest,
            &self.members_digest,
        ] {
            digest(value)?;
        }
        Ok(())
    }
}

impl Member {
    pub(super) fn validate(&self) -> Result<(), ImportError> {
        version(self.version, VERSION)?;
        PublicationProtocol::SignedOutcomeV1.require_job_id(&self.job_id)?;
        for value in [
            &self.header_digest,
            &self.candidate_digest,
            &self.input_digest,
            &self.allocation_digest,
        ] {
            digest(value)?;
        }
        Ok(())
    }
}
