//! Independent durable outcome reads. No CommitItem or retained preview, no
//! repair/re-signing, and no transferable SQL or Undo authorization witness.
use rusqlite::{Connection, Transaction};

use super::{
    record::Stage,
    transition::{dedup, require_ack},
    *,
};
use crate::session_migration::{
    catalog,
    errors::sqlite_storage_error,
    job_control::{publication_acceptance, RecoveryInput},
    jobs::{self, JobItem, Receipt},
    publication_crypto::JobCrypto,
    publication_protocol::PublicationProtocol,
};

/// A SQL-authenticated terminal point, not a capability to mutate its targets.
pub(super) struct Terminal {
    pub(super) receipt: Receipt,
    pub(super) item: JobItem,
    pub(super) record: String,
}

/// Validate the complete stable receipt prefix and terminal result together.
/// Callers authenticate the original acceptance/owner separately in THIS tx.
pub(super) fn terminal(
    tx: &Transaction<'_>,
    auth: &InstallationAuthenticator,
    allocated: &VerifiedRecord,
    original: &RecoveryInput<'_>,
) -> Result<Option<Terminal>, ImportError> {
    let binding = &allocated.body.binding;
    let allocation = allocated.allocation()?;
    if binding.input_digest != original.digest()?
        || binding.candidate_digest != sha256_bytes(original.candidate().as_bytes())
    {
        return Err(invalid());
    }
    let receipts = receipt::prefix(tx, allocation, binding, original.candidate())?;
    if journal::present(tx, binding, StageKind::Deduplicated)? {
        if !receipts.is_empty() {
            return Err(invalid());
        }
        let record = journal::load_deduplicated(tx, auth, binding)?;
        record.follows(allocated)?;
        let Stage::Deduplicated(data) = &record.body.stage else {
            return Err(invalid());
        };
        let rec = receipt::read(tx, &data.receipt_id)?.ok_or_else(invalid)?;
        original.validate_reused_receipt(&rec)?;
        if rec.import_id.as_deref() != Some(data.import_id.as_str())
            || rec.revision_id.as_deref() != Some(data.revision_id.as_str())
            || receipt::digest(&rec)? != data.receipt_digest
            || rec.selection_hash != data.selection_hash
            || dedup::revision_digest(tx, &data.import_id, &data.revision_id)?
                != data.revision_digest
        {
            return Err(invalid());
        }
        let entry = catalog::get_entry(tx, &data.import_id)?.ok_or_else(invalid)?;
        if dedup::identity_digest(&entry)? != data.entry_identity_digest {
            return Err(invalid());
        }
        let item = original.dedup_result(&data.import_id, &data.revision_id);
        if sha256_bytes(canonical(&item)?.as_bytes()) != data.item_digest {
            return Err(invalid());
        }
        return Ok(Some(Terminal {
            receipt: rec,
            item,
            record: record.encoded,
        }));
    }
    for rec in &receipts {
        original.validate_receipt(&binding.job_id, rec)?;
    }
    if let Some(linked) = receipts.get(3) {
        let stored = journal::load_required(tx, auth, binding, StageKind::Stored)?;
        stored.follows(allocated)?;
        let Stage::Stored(data) = &stored.body.stage else {
            return Err(invalid());
        };
        if data.linked_receipt_digest != receipt::digest(linked)? {
            return Err(invalid());
        }
        native_link::require_result(allocation, linked, &data.project)?;
    } else if journal::present(tx, binding, StageKind::Stored)? {
        return Err(invalid());
    }
    let Some(rec) = receipts.get(4) else {
        if journal::present(tx, binding, StageKind::Committed)? {
            return Err(invalid());
        }
        return Ok(None);
    };
    let record = journal::load_committed(tx, auth, binding)?;
    let Stage::Committed(data) = &record.body.stage else {
        return Err(invalid());
    };
    let item = original.result(
        &allocation.import_id,
        &allocation.revision_id,
        rec.mapping_project_id.clone(),
    );
    if data.receipt_digest != receipt::digest(rec)?
        || data.item_digest != sha256_bytes(canonical(&item)?.as_bytes())
    {
        return Err(invalid());
    }
    Ok(Some(Terminal {
        receipt: rec.clone(),
        item,
        record: record.encoded,
    }))
}

struct Snapshot {
    terminal: Terminal,
    rows_digest: String,
    version: u32,
}

/// Primitive result identity check, NOT accepted-source authentication. The
/// prior-owner reader separately authenticates original input/owner/allocation
/// and catalog rows in this same snapshot before calling it.
pub(super) fn require_original_terminal(
    tx: &Transaction<'_>,
    auth: &InstallationAuthenticator,
    allocated: &VerifiedRecord,
    original: &RecoveryInput<'_>,
    expected: &Receipt,
) -> Result<(), ImportError> {
    let result = terminal(tx, auth, allocated, original)?.ok_or_else(invalid)?;
    if result.receipt.job_id != allocated.body.binding.job_id
        || receipt::digest(&result.receipt)? != receipt::digest(expected)?
        || result.item.outcome.as_deref() == Some("deduplicated")
    {
        return Err(invalid());
    }
    // Reselect the unique slot from the currently authenticated membership,
    // rather than trust a detached index or a mutable result's project IDs.
    let index = crate::session_migration::jobs::item_store::index(
        tx,
        &expected.job_id,
        &expected.candidate_id,
    )?;
    require_ack(tx, &expected.job_id, &result.item, index, None)?;
    Ok(())
}

fn snapshot(
    tx: &Transaction<'_>,
    job: &str,
    index: usize,
    original: &RecoveryInput<'_>,
    crypto: &JobCrypto,
) -> Result<Option<Snapshot>, ImportError> {
    let auth = crypto
        .require(job, PublicationProtocol::SignedOutcomeV1)?
        .ok_or_else(invalid)?;
    original.owner(tx, job, crypto)?;
    let accepted = publication_acceptance::journal::load_candidate(tx, auth, job)?;
    let allocated = accepted.allocated(tx, original.candidate())?;
    let Some(terminal) = terminal(tx, auth, &allocated, original)? else {
        dedup::require_unacknowledged(tx, job, original.candidate(), index)?;
        return Ok(None);
    };
    let rec = &terminal.receipt;
    let import = rec.import_id.as_deref().ok_or_else(invalid)?;
    let revision = rec.revision_id.as_deref().ok_or_else(invalid)?;
    let entry = catalog::get_entry(tx, import)?.ok_or_else(invalid)?;
    let row = catalog::get_revision(tx, import, revision)?.ok_or_else(invalid)?;
    let latest: String = tx.query_row(
        "SELECT receipt_id FROM receipts WHERE import_id=?1 AND revision_id=?2 ORDER BY rowid DESC LIMIT 1",
        rusqlite::params![import, revision], |row| row.get(0),
    ).map_err(sqlite_storage_error)?;
    if entry.hidden || row.source_snapshot_id != rec.snapshot_hash {
        return Err(invalid());
    }
    if latest != rec.receipt_id {
        return Err(invalid());
    }
    if rec.job_id == job {
        original.require_published_revision(tx, import, revision)?;
    }
    // This is only a detached SQL capture. recover authenticates any reused
    // receipt's actual publisher before archive I/O and in the final snapshot.
    crate::session_migration::registration_provenance::authorize(tx, &entry, rec)?;
    let version = require_ack(tx, job, &terminal.item, index, None)?;
    // Re-opened snapshots compare exact SQL facts as well as signed envelopes;
    // a mutable display title never becomes the reconstructed immutable ACK.
    let rows_digest = sha256_bytes(
        canonical(&(
            &entry,
            &row,
            &allocated.encoded,
            &terminal.record,
            receipt::digest(rec)?,
            dedup::revision_digest(tx, import, revision)?,
        ))?
        .as_bytes(),
    );
    Ok(Some(Snapshot {
        terminal,
        rows_digest,
        version,
    }))
}

pub(in crate::session_migration) fn recover(
    conn: &Connection,
    job: &str,
    index: usize,
    original: &RecoveryInput<'_>,
    crypto: &JobCrypto,
) -> Result<Option<(JobItem, u32)>, ImportError> {
    if !conn.is_autocommit() {
        return Err(ImportError::new(
            ErrorCode::HistoryAccessUnavailable,
            "signed archive recovery requires an independent database snapshot",
        )
        .job_local());
    }
    let before = {
        let tx = conn.unchecked_transaction().map_err(sqlite_storage_error)?;
        snapshot(&tx, job, index, original, crypto)?
    };
    let Some(before) = before else {
        return Ok(None);
    };
    let rec = &before.terminal.receipt;
    let prior = if rec.job_id != job {
        let prior = super::prior::ReceiptOwner::prepare(conn, rec)?;
        let tx = conn.unchecked_transaction().map_err(sqlite_storage_error)?;
        let current = snapshot(&tx, job, index, original, crypto)?.ok_or_else(invalid)?;
        if current.rows_digest != before.rows_digest {
            return Err(invalid());
        }
        prior.require(&tx, &current.terminal.receipt)?;
        Some(prior)
    } else {
        None
    };
    let dir = catalog::revision_dir(
        rec.import_id.as_deref().ok_or_else(invalid)?,
        rec.revision_id.as_deref().ok_or_else(invalid)?,
    )?;
    // All filesystem/key/projection work is outside a catalog snapshot. This
    // reader does not create or reclaim any native project or source artifact.
    let selection = jobs::verify_published_for_read(&dir, rec)?;
    if selection.canonicalized() != rec.selection_set.canonicalized() {
        return Err(invalid());
    }
    crate::session_migration::search::ensure_body_search_projection(conn, &dir)?;
    let tx = conn.unchecked_transaction().map_err(sqlite_storage_error)?;
    let after = snapshot(&tx, job, index, original, crypto)?.ok_or_else(invalid)?;
    if after.rows_digest != before.rows_digest {
        return Err(invalid());
    }
    if let Some(prior) = prior {
        prior.require(&tx, &after.terminal.receipt)?;
    }
    Ok(Some((after.terminal.item, after.version)))
}

#[cfg(test)]
#[path = "publication_recovery_tests.rs"]
mod tests;
