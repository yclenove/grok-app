//! One SQL snapshot for input/context reads. Preloaded crypto carries no grant.
use rusqlite::Connection;

use super::{association_publication, input_reader, DurableInput};
use crate::session_migration::{
    errors::{sqlite_storage_error, ErrorCode, ImportError},
    hash,
    jobs::{self, JobItem, Receipt},
    publication_crypto::JobCrypto,
    publication_protocol::PublicationProtocol,
};

fn stale(message: &'static str) -> ImportError {
    ImportError::new(ErrorCode::PlanStale, message)
}

fn crypto_in_snapshot(
    conn: &Connection,
    job_id: &str,
    candidate: &str,
    crypto: &JobCrypto,
) -> Result<Option<String>, ImportError> {
    if conn.is_autocommit() {
        return Err(stale("input verification requires one snapshot"));
    }
    let protocol = PublicationProtocol::for_job_id(job_id)?;
    crypto
        .require(job_id, protocol)?
        .map(|auth| {
            use super::publication_acceptance::journal;
            // On the fenced worker connection one candidate is authenticated
            // per read; the complete set is proved at start and before the
            // job turns terminal. Elsewhere the full membership is checked.
            if crate::session_migration::publication_write_fence::check(conn, job_id, candidate)? {
                journal::validate_candidate(conn, auth, job_id, candidate)
            } else {
                journal::validate_input_snapshot(conn, auth, job_id, candidate)
            }
        })
        .transpose()
}

pub(super) fn load(
    conn: &Connection,
    job_id: &str,
    candidate: &str,
    crypto: &JobCrypto,
) -> Result<DurableInput, ImportError> {
    if conn.is_autocommit() {
        let tx = conn.unchecked_transaction().map_err(sqlite_storage_error)?;
        return load(&tx, job_id, candidate, crypto);
    }
    #[cfg(test)]
    let _span = crate::session_migration::codec_profile::Span::start("durable-input-load", 0);
    crypto_in_snapshot(conn, job_id, candidate, crypto)?;
    let json = input_reader::load(conn, job_id, candidate)?;
    let input = DurableInput::parse(&json)?;
    if input.candidate_id != candidate {
        return Err(stale("input candidate differs from its job item"));
    }
    if let Some(association) = &input.identity_association {
        let job = association.stored_job(conn, job_id)?;
        input.validate_context(conn, &job)?;
    }
    Ok(input)
}

fn committed_receipt(
    conn: &Connection,
    job_id: &str,
    previous: &JobItem,
    input: &DurableInput,
    crypto: &JobCrypto,
) -> Result<Option<Receipt>, ImportError> {
    let digest = crypto_in_snapshot(conn, job_id, &previous.candidate_id, crypto)?;
    if input.candidate_id != previous.candidate_id
        || digest.is_some_and(|expected| {
            serde_json::to_vec(input)
                .map(|bytes| hash::sha256_bytes(&bytes) != expected)
                .unwrap_or(true)
        })
    {
        return Err(stale("recovery input differs from its accepted original"));
    }
    let accepted = association_publication::AcceptedPublication::for_input(conn, job_id, input)?;
    let receipt = association_publication::latest_receipt(
        conn,
        job_id,
        &previous.candidate_id,
        accepted.as_ref(),
    )?;
    if let Some(accepted) = &accepted {
        accepted.target(conn, receipt.as_ref())?;
    }
    let Some(receipt) = receipt else {
        return Ok(None);
    };
    jobs::validate_receipt_shape(&receipt)?;
    if receipt.phase != "committed" {
        return Ok(None);
    }
    input.validate_committed_receipt(job_id, &receipt)?;
    if previous
        .import_id
        .as_ref()
        .is_some_and(|id| Some(id.as_str()) != receipt.import_id.as_deref())
        || previous
            .revision_id
            .as_ref()
            .is_some_and(|id| Some(id.as_str()) != receipt.revision_id.as_deref())
    {
        return Err(stale(
            "committed receipt does not match the recorded job result",
        ));
    }
    Ok(Some(receipt))
}

fn recover_legacy(
    conn: &Connection,
    job_id: &str,
    previous: &JobItem,
    input: &DurableInput,
    crypto: &JobCrypto,
) -> Result<Option<JobItem>, ImportError> {
    let receipt = if conn.is_autocommit() {
        let tx = conn.unchecked_transaction().map_err(sqlite_storage_error)?;
        committed_receipt(&tx, job_id, previous, input, crypto)?
    } else {
        // Existing fault tests retain their exact metadata refusal. A valid
        // receipt cannot continue to filesystem verification under this writer.
        committed_receipt(conn, job_id, previous, input, crypto)?
    };
    let Some(receipt) = receipt else {
        return Ok(None);
    };
    if !conn.is_autocommit() {
        return Err(ImportError::new(
            ErrorCode::HistoryAccessUnavailable,
            "archive recovery must run outside a database transaction",
        )
        .job_local());
    }
    // Native/archive verification and projection repair stay outside the SQL
    // snapshot. Their existing current catalog/proof checks remain mandatory.
    jobs::recover_committed_receipt(conn, &receipt).map(Some)
}

/// Keep the committed ACK identity throughout worker dispatch. Legacy retains
/// its historical recovery/setter semantics; signed recovery must not use them.
pub(super) fn recover_progress(
    conn: &Connection,
    job_id: &str,
    index: usize,
    previous: &JobItem,
    input: &DurableInput,
    crypto: &JobCrypto,
) -> Result<Option<jobs::ItemProgress>, ImportError> {
    #[cfg(test)]
    let _span =
        crate::session_migration::codec_profile::Span::start("durable-progress-recovery", 0);
    match PublicationProtocol::for_job_id(job_id)? {
        PublicationProtocol::Legacy => recover_legacy(conn, job_id, previous, input, crypto)
            .map(|result| result.map(jobs::ItemProgress::pending)),
        PublicationProtocol::SignedOutcomeV1 => {
            if input.candidate_id != previous.candidate_id {
                return Err(stale("recovery input differs from its job item"));
            }
            crate::session_migration::publication_outcome::recovery::recover(
                conn,
                job_id,
                index,
                &input.recovery_input(),
                crypto,
            )
            .map(|result| result.map(|(item, version)| jobs::ItemProgress::recorded(item, version)))
        }
    }
}

#[cfg(test)]
pub(super) fn recover(
    conn: &Connection,
    job_id: &str,
    previous: &JobItem,
    input: &DurableInput,
    crypto: &JobCrypto,
) -> Result<Option<JobItem>, ImportError> {
    recover_legacy(conn, job_id, previous, input, crypto)
}

#[cfg(test)]
#[path = "job_durable_reader_tests.rs"]
mod tests;
