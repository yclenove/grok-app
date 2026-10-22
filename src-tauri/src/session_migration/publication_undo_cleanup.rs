//! Reconstruct original creation evidence for an already-undone item. The
//! cleanup caller must repeat this in its actual native-removal transaction.
use super::*;

pub(in crate::session_migration) struct Cleanup {
    pub receipt: Receipt,
    pub allocation: VerifiedRecord,
    pub identity_digest: String,
    pub target_digest: String,
}

pub(in crate::session_migration) fn capture(
    tx: &Transaction<'_>,
    job: &JobRecord,
    candidate: &str,
    crypto: &JobCrypto,
) -> Result<Cleanup, ImportError> {
    let index = job
        .items
        .iter()
        .position(|item| item.candidate_id == candidate)
        .ok_or_else(unavailable)?;
    let item = &job.items[index];
    if item.phase != "undone" {
        return Err(unavailable());
    }
    let point = read_point(tx, crypto, &job.job_id, candidate)?;
    let mut permitted = point.terminal.item.clone();
    permitted.phase = "undone".into();
    permitted.error.clone_from(&item.error);
    if !point.terminal.receipt.created_project
        || permitted.outcome.as_deref() == Some("deduplicated")
        || canonical(&permitted)? != canonical(item)?
    {
        return Err(unavailable());
    }
    require_ack(tx, &job.job_id, item, index, Some(job.state_version)).map_err(undo_error)?;
    let auth = crypto
        .require(&job.job_id, PublicationProtocol::SignedOutcomeV1)?
        .ok_or_else(unavailable)?;
    let accepted = publication_acceptance::journal::load_candidate(tx, auth, &job.job_id)?;
    let allocation = accepted.allocated(tx, candidate)?;
    allocation.project_creation(auth)?;
    let identity_digest = sha256_bytes(
        canonical(&(
            receipt::digest(&point.terminal.receipt)?,
            &allocation.digest,
        ))?
        .as_bytes(),
    );
    let target_digest = sha256_bytes(canonical(&(point.entry, point.tombstoned))?.as_bytes());
    Ok(Cleanup {
        receipt: point.terminal.receipt,
        allocation,
        identity_digest,
        target_digest,
    })
}
