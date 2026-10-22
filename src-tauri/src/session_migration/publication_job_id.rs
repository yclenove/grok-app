//! System-owned job locator dispatch. A UUID version selects required checks;
//! it is never accepted membership, producer identity or an Undo grant.
use uuid::{Builder, Uuid, Variant};

use super::PublicationProtocol;
use crate::session_migration::errors::{ErrorCode, ImportError};

impl PublicationProtocol {
    /// Called only after the original plan selects its required protocol.
    /// V4 retains the legacy producer's format; custom V8 uses the same random
    /// entropy with different version bits, not a clock or caller-supplied ID.
    pub(in crate::session_migration) fn new_job_id(self) -> String {
        let random = Uuid::new_v4();
        match self {
            Self::Legacy => random,
            Self::SignedOutcomeV1 => Builder::from_custom_bytes(*random.as_bytes()).into_uuid(),
        }
        .to_string()
    }

    /// Inspect the queried primary key before any mutable body/metadata can
    /// select a legacy route. Historical low-level producers used opaque IDs;
    /// keep that compatibility, but never reinterpret a recognized new UUID
    /// (including a noncanonical spelling) as an opaque legacy locator.
    pub(in crate::session_migration) fn for_job_id(job_id: &str) -> Result<Self, ImportError> {
        let Ok(id) = Uuid::parse_str(job_id) else {
            return Ok(Self::Legacy);
        };
        if id.get_variant() != Variant::RFC4122 {
            return Err(ImportError::new(
                ErrorCode::SchemaUnsupported,
                "unsupported import job UUID variant",
            )
            .job_local());
        }
        match id.get_version_num() {
            4 => Ok(Self::Legacy),
            8 if id.to_string() == job_id => Ok(Self::SignedOutcomeV1),
            8 => Err(ImportError::new(
                ErrorCode::PlanStale,
                "signed publication job ID is not canonical",
            )
            .job_local()),
            version => Err(ImportError::new(
                if version > 8 {
                    ErrorCode::ImportSchemaTooNew
                } else {
                    ErrorCode::SchemaUnsupported
                },
                "unsupported import job UUID version",
            )
            .job_local()),
        }
    }

    pub(in crate::session_migration) fn require_job_id(
        self,
        job_id: &str,
    ) -> Result<(), ImportError> {
        if Self::for_job_id(job_id)? != self {
            return Err(ImportError::new(
                ErrorCode::PlanStale,
                "import job ID differs from the original publication protocol",
            )
            .job_local());
        }
        Ok(())
    }
}
