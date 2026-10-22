//! Independent, installation-signed publication records. These are evidence,
//! NOT SQL authorization witnesses. Normal V3 production is wired; complete
//! compatibility and desktop acceptance remain required before delivery.
use serde::{Deserialize, Serialize};

use super::{
    cursor::InstallationAuthenticator,
    errors::{ErrorCode, ImportError},
    hash::{canonical_json, sha256_bytes},
};

pub(super) const VERSION: u32 = 1;
// This limit belongs only to fixed-size result metadata, never input/receipt.
const RECORD_BYTES: usize = 8192;

// Kept separately from historical input/receipt/proof codecs and their capacity.
#[path = "publication_outcome_journal.rs"]
pub(super) mod journal;
#[path = "publication_outcome_record.rs"]
mod record;
pub(super) use record::Binding;
pub(super) use record::{Allocation, ProjectAllocation};
use record::{Body, Stage, StageKind};

#[path = "publication_allocation.rs"]
mod allocation;
pub(super) use allocation::PreparedAllocation;

#[path = "publication_receipt.rs"]
mod receipt;
#[path = "publication_transition.rs"]
mod transition;
pub(super) use transition::SignedPublication;
#[path = "publication_native_link.rs"]
mod native_link;
#[path = "publication_prior.rs"]
mod prior;
#[path = "publication_recovery.rs"]
pub(super) mod recovery;
#[path = "publication_undo.rs"]
pub(super) mod undo;

// Normal signed creation uses these primitives, never a legacy ownership upgrade.
#[path = "publication_native_project.rs"]
mod native_project;
pub(crate) use native_project::ProjectCreation;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    body: Body,
    mac: String,
}

// No Debug/Serialize/Clone: prevent accidental MAC disclosure or treating a
// decoded proof as a transferable writer grant.
pub(super) struct SealedRecord {
    encoded: String,
    body: Body,
}

pub(super) struct VerifiedRecord {
    body: Body,
    digest: String,
    encoded: String,
}

fn invalid() -> ImportError {
    ImportError::new(
        ErrorCode::PlanStale,
        "publication outcome record is invalid",
    )
}

fn canonical<T: Serialize>(value: &T) -> Result<String, ImportError> {
    let value = serde_json::to_value(value).map_err(|_| invalid())?;
    Ok(canonical_json(&value))
}

// Intentionally private. Future producer code must live under this module and
// derive the body from the actual accepted/allocation/store transition. Readers
// and Undo cannot call this to bless their mutable receipts or job items.
fn seal(
    authenticator: &InstallationAuthenticator,
    binding: Binding,
    stage: Stage,
) -> Result<SealedRecord, ImportError> {
    let body = Body {
        version: VERSION,
        binding,
        stage,
    };
    body.validate()?;
    let payload = canonical(&body)?;
    let envelope = Envelope {
        mac: hex::encode(authenticator.sign(body.stage.kind().domain(), payload.as_bytes())),
        body,
    };
    let encoded = canonical(&envelope)?;
    if encoded.len() > RECORD_BYTES {
        return Err(invalid());
    }
    Ok(SealedRecord {
        encoded,
        body: envelope.body,
    })
}

fn verify(
    authenticator: &InstallationAuthenticator,
    bytes: &[u8],
    expected: &Binding,
    kind: StageKind,
) -> Result<VerifiedRecord, ImportError> {
    expected.validate()?;
    if bytes.len() > RECORD_BYTES || bytes.starts_with(&[0xef, 0xbb, 0xbf]) {
        return Err(invalid());
    }
    // Decode the original bytes with typed structs, not a collapsed JSON map:
    // duplicate, unknown and missing fields remain errors at every depth.
    let raw = std::str::from_utf8(bytes).map_err(|_| invalid())?;
    let envelope: Envelope = serde_json::from_str(raw).map_err(|_| invalid())?;
    envelope.body.validate()?;
    if &envelope.body.binding != expected || envelope.body.stage.kind() != kind {
        return Err(invalid());
    }
    record::digest(&envelope.mac)?;
    let mac = hex::decode(&envelope.mac).map_err(|_| invalid())?;
    let payload = canonical(&envelope.body)?;
    if !authenticator.verify(kind.domain(), payload.as_bytes(), &mac) {
        return Err(invalid());
    }
    // Chain canonical envelopes (including the seal), not mutable receipt IDs.
    let encoded = canonical(&envelope)?;
    let digest = sha256_bytes(encoded.as_bytes());
    Ok(VerifiedRecord {
        body: envelope.body,
        digest,
        encoded,
    })
}

impl VerifiedRecord {
    /// Pure shape/link validation only. Callers still authenticate acceptance,
    /// source/proof/member/input/fence facts in their own current transaction.
    pub(super) fn follows(&self, previous: &Self) -> Result<(), ImportError> {
        if self.body.binding != previous.body.binding {
            return Err(invalid());
        }
        match (&previous.body.stage, &self.body.stage) {
            (Stage::Allocated(allocation), Stage::Stored(result))
                if result.allocation_digest == previous.digest =>
            {
                allocation.project.permits(&result.project)
            }
            (Stage::Stored(_), Stage::Committed(commit))
                if commit.store_digest == previous.digest =>
            {
                Ok(())
            }
            (Stage::Allocated(allocation), Stage::Deduplicated(result))
                if result.allocation_digest == previous.digest
                    && !allocation.created_entry
                    && result.import_id == allocation.import_id
                    && result.revision_id != allocation.revision_id =>
            {
                Ok(())
            }
            _ => Err(invalid()),
        }
    }
}

#[cfg(test)]
#[path = "publication_outcome_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "publication_end_to_end_tests.rs"]
mod end_to_end_tests;
