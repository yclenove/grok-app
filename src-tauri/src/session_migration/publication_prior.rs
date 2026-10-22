//! Preloaded crypto for the receipt's actual publisher, not the dedup job.
//! Reauthenticate its original result in each concrete snapshot, never Legacy
//! fallback for signed IDs, and never export an Undo/native creation grant.
use rusqlite::Transaction;

use super::*;
use crate::session_migration::{
    catalog,
    errors::sqlite_storage_error,
    job_control::{self, publication_acceptance},
    jobs::Receipt,
    publication_crypto::JobCrypto,
    publication_protocol::PublicationProtocol,
    registration_provenance,
};

pub(super) struct ReceiptOwner {
    job: String,
    receipt_digest: String,
    crypto: JobCrypto,
}

impl ReceiptOwner {
    pub(super) fn prepare(conn: &rusqlite::Connection, rec: &Receipt) -> Result<Self, ImportError> {
        if !conn.is_autocommit() {
            return Err(invalid());
        }
        PublicationProtocol::for_job_id(&rec.job_id)?;
        Ok(Self {
            job: rec.job_id.clone(),
            receipt_digest: receipt::digest(rec)?,
            crypto: JobCrypto::prepare(conn, &rec.job_id)?,
        })
    }

    pub(super) fn require(&self, tx: &Transaction<'_>, rec: &Receipt) -> Result<(), ImportError> {
        if tx.is_autocommit()
            || self.job != rec.job_id
            || self.receipt_digest != receipt::digest(rec)?
        {
            return Err(invalid());
        }
        receipt::require_point(tx, rec)?;
        let import = rec.import_id.as_deref().ok_or_else(invalid)?;
        let revision = rec.revision_id.as_deref().ok_or_else(invalid)?;
        let entry = catalog::get_entry(tx, import)?.ok_or_else(invalid)?;
        let row = catalog::get_revision(tx, import, revision)?.ok_or_else(invalid)?;
        let latest: String = tx.query_row(
            "SELECT receipt_id FROM receipts WHERE import_id=?1 AND revision_id=?2 ORDER BY rowid DESC LIMIT 1",
            rusqlite::params![import, revision], |row| row.get(0),
        ).map_err(sqlite_storage_error)?;
        if rec.phase != "committed"
            || row.source_snapshot_id != rec.snapshot_hash
            || latest != rec.receipt_id
        {
            return Err(invalid());
        }
        registration_provenance::authorize(tx, &entry, rec)?;
        let protocol = PublicationProtocol::for_job_id(&self.job)?;
        let Some(auth) = self.crypto.require(&self.job, protocol)? else {
            return Ok(());
        };
        job_control::with_stored_recovery_input(
            tx,
            &self.job,
            &rec.candidate_id,
            &self.crypto,
            |original| {
                // The constructor authenticates the original owner in THIS
                // snapshot; crypto or a detached record alone grants nothing.
                original.owner(tx, &self.job, &self.crypto)?;
                let accepted =
                    publication_acceptance::journal::load_candidate(tx, auth, &self.job)?;
                let allocated = accepted.allocated(tx, original.candidate())?;
                recovery::require_original_terminal(tx, auth, &allocated, &original, rec)?;
                original.require_published_revision(tx, import, revision)
            },
        )
    }
}
