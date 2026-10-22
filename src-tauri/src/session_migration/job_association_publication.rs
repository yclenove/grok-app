//! An accepted publication witness, not a serializable caller permission flag.
//! The existing publisher owns locks, prepared files and all publication phases.
use super::{failure, DurableInput};
use crate::session_migration::{
    catalog::CatalogEntry,
    errors::{ErrorCode, ImportError},
    job_input_association::AssociationInput,
    jobs::{CommitItem, Receipt},
    registration_provenance::{AssociationProof, InputIdentity, ProofRecord},
    registration_provenance_store as store,
};
use rusqlite::Connection;

pub(in crate::session_migration) struct AcceptedPublication {
    proof: Box<AssociationProof>,
    association: AssociationInput,
    identity: InputIdentity,
    proof_digest: String,
}

fn unavailable() -> ImportError {
    ImportError::new(
        ErrorCode::HistoryAccessUnavailable,
        "publication requires the exact accepted association owner",
    )
}

pub(in crate::session_migration) fn latest_receipt(
    conn: &Connection,
    job: &str,
    candidate: &str,
    accepted: Option<&AcceptedPublication>,
) -> Result<Option<Receipt>, ImportError> {
    if let Some(accepted) = accepted {
        if job != accepted.association.job_id || candidate != accepted.identity.candidate_id {
            return Err(unavailable());
        }
        crate::session_migration::registration_provenance::receipt_store::latest_for_job(
            conn, job, candidate,
        )
    } else {
        crate::session_migration::jobs::latest_receipt(conn, job, candidate)
    }
}

impl AcceptedPublication {
    /// Called after the existing publication locks, before ID allocation or
    /// project effects. Rebuild canonical metadata from the verified preview.
    pub(in crate::session_migration) fn for_item(
        conn: &Connection,
        job_id: &str,
        item: &CommitItem,
    ) -> Result<Option<Self>, ImportError> {
        if item.identity_association.is_none() {
            return Ok(None);
        }
        let (input, _) = DurableInput::from_item(item)?;
        Self::for_input(conn, job_id, &input)
    }

    /// Metadata-only recovery must not reopen a disposed prepared preview.
    pub(super) fn for_input(
        conn: &Connection,
        job_id: &str,
        input: &DurableInput,
    ) -> Result<Option<Self>, ImportError> {
        let Some(association) = &input.identity_association else {
            return Ok(None);
        };
        if conn.is_autocommit() {
            let tx = conn.unchecked_transaction().map_err(failure)?;
            return Self::for_input(&tx, job_id, input);
        }
        input.validate_shape()?;
        let job = association.stored_job(conn, job_id)?;
        input.validate_context(conn, &job)?;
        let identity = input.input_identity()?;
        let Some(ProofRecord::Association(proof)) =
            store::get(conn, &association.confirmation.import_id)?
        else {
            return Err(unavailable());
        };
        proof.verify_retained_input(conn, association, &identity)?;
        let proof_digest = ProofRecord::Association(proof.clone()).digest()?;
        Ok(Some(Self {
            proof,
            association: association.clone(),
            identity,
            proof_digest,
        }))
    }

    /// Reauthenticate the captured facts and live target inside the publisher's
    /// final short writer. This method never reads source or preview files.
    pub(in crate::session_migration) fn target(
        &self,
        conn: &Connection,
        receipt: Option<&Receipt>,
    ) -> Result<CatalogEntry, ImportError> {
        if conn.is_autocommit() {
            let tx = conn.unchecked_transaction().map_err(failure)?;
            return self.target(&tx, receipt);
        }
        let record =
            store::get(conn, &self.association.confirmation.import_id)?.ok_or_else(unavailable)?;
        if record.digest()? != self.proof_digest {
            return Err(unavailable());
        }
        self.proof
            .verify_retained_input(conn, &self.association, &self.identity)?;
        self.proof.publication_target(conn, receipt)
    }
}
