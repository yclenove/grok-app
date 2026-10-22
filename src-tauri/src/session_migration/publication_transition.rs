//! Worker-owned transitions. Detached records never substitute for current
//! acceptance/source/proof/fence checks in each concrete writer transaction.
use rusqlite::{Connection, Transaction, TransactionBehavior};

use crate::session_migration::{
    errors::sqlite_storage_error,
    job_control::{provenance_scope::publication::PublicationOwner, publication_acceptance},
    jobs::{self, CommitItem, JobItem, Receipt},
    publication_crypto::JobCrypto,
    publication_protocol::PublicationProtocol,
    selection::PreparedSelectionValidation,
};

use super::{
    record::{Committed, StoreResult},
    *,
};

#[path = "publication_dedup.rs"]
pub(super) mod dedup;

// No Clone, Debug or serialization, and no retained SQL writer grant.
pub(in crate::session_migration) struct SignedPublication<'a> {
    auth: &'a InstallationAuthenticator,
    allocated: VerifiedRecord,
    item: &'a CommitItem,
    verified: &'a PreparedSelectionValidation,
}

pub(in crate::session_migration) struct PreparedFinal {
    receipt: Receipt,
    sealed: SealedRecord,
}

impl<'a> SignedPublication<'a> {
    pub(in crate::session_migration) fn capture(
        conn: &Connection,
        job: &str,
        item: &'a CommitItem,
        verified: &'a PreparedSelectionValidation,
        crypto: &'a JobCrypto,
        owner: &PublicationOwner<'_>,
    ) -> Result<Option<Self>, ImportError> {
        let protocol = PublicationProtocol::for_job_id(job)?;
        if protocol == PublicationProtocol::Legacy {
            return Ok(None);
        }
        if !conn.is_autocommit() {
            return Err(invalid());
        }
        let auth = crypto.require(job, protocol)?.ok_or_else(invalid)?;
        let tx = conn.unchecked_transaction().map_err(sqlite_storage_error)?;
        owner.verify(&tx)?;
        let accepted = publication_acceptance::journal::load_candidate(&tx, auth, job)?;
        let allocated = accepted.allocated(&tx, &item.candidate_id)?;
        let publication = Self {
            auth,
            allocated,
            item,
            verified,
        };
        if journal::present(
            &tx,
            &publication.allocated.body.binding,
            StageKind::Deduplicated,
        )? {
            publication.deduplicated_record(&tx)?;
        } else {
            publication.current(&tx)?;
        }
        Ok(Some(publication))
    }

    pub(in crate::session_migration) fn allocation(&self) -> Result<&Allocation, ImportError> {
        self.allocated.allocation()
    }

    fn job(&self) -> &str {
        &self.allocated.body.binding.job_id
    }

    fn require_allocation(&self, tx: &Transaction<'_>) -> Result<(), ImportError> {
        let accepted = publication_acceptance::journal::load_candidate(tx, self.auth, self.job())?;
        let current = accepted.allocated(tx, &self.item.candidate_id)?;
        if current.digest != self.allocated.digest {
            return Err(invalid());
        }
        Ok(())
    }

    fn require_writer_allocation(
        &self,
        tx: &Transaction<'_>,
        entry: &crate::session_migration::jobs::entry::AuthenticatedWriterEntry<'_, '_>,
    ) -> Result<(), ImportError> {
        let current = entry.allocated(tx, self.job(), &self.item.candidate_id)?;
        if current.digest != self.allocated.digest {
            return Err(invalid());
        }
        Ok(())
    }

    fn verify(
        &self,
        tx: &Transaction<'_>,
        owner: &PublicationOwner<'_>,
    ) -> Result<(), ImportError> {
        owner.verify(tx)?;
        self.require_allocation(tx)
    }

    fn authorize(
        &self,
        tx: &Transaction<'_>,
        owner: &PublicationOwner<'_>,
    ) -> Result<(), ImportError> {
        let entry = owner.verify_for_progress(tx)?.ok_or_else(invalid)?;
        self.require_writer_allocation(tx, &entry)?;
        entry.require_writer(tx, self.job())
    }

    fn validate(&self, rec: &Receipt) -> Result<(), ImportError> {
        receipt::require_allocation(self.allocation()?, &self.allocated.body.binding, rec)?;
        let selection = self.item.selection_set.canonicalized();
        jobs::validate_receipt_for_item(
            rec,
            self.item,
            &selection,
            &selection.hash(),
            &crate::session_migration::hash::options_hash(&self.item.options),
            self.verified,
        )
    }

    fn current(&self, tx: &Transaction<'_>) -> Result<Vec<Receipt>, ImportError> {
        if journal::present(tx, &self.allocated.body.binding, StageKind::Deduplicated)? {
            return Err(invalid());
        }
        let receipts = receipt::prefix(
            tx,
            self.allocation()?,
            &self.allocated.body.binding,
            &self.item.candidate_id,
        )?;
        for rec in &receipts {
            self.validate(rec)?;
        }
        if let Some(linked) = receipts.get(3) {
            self.stored(tx, linked)?;
        } else if journal::present(tx, &self.allocated.body.binding, StageKind::Stored)? {
            return Err(invalid());
        }
        if let Some(rec) = receipts.get(4) {
            let record = journal::load_committed(tx, self.auth, &self.allocated.body.binding)?;
            let Stage::Committed(result) = record.body.stage else {
                return Err(invalid());
            };
            if result.receipt_digest != receipt::digest(rec)? {
                return Err(invalid());
            }
        } else if journal::present(tx, &self.allocated.body.binding, StageKind::Committed)? {
            return Err(invalid());
        }
        Ok(receipts)
    }

    pub(in crate::session_migration) fn prior(
        &self,
        conn: &Connection,
        owner: &PublicationOwner<'_>,
    ) -> Result<Option<Receipt>, ImportError> {
        let tx = conn.unchecked_transaction().map_err(sqlite_storage_error)?;
        self.verify(&tx, owner)?;
        Ok(self.current(&tx)?.pop())
    }

    /// Only a freshly built immutable-input receipt enters here; all identities
    /// were already chosen by acceptance, never generated by the worker.
    pub(in crate::session_migration) fn prepare(
        &self,
        conn: &Connection,
        owner: &PublicationOwner<'_>,
        rec: &mut Receipt,
    ) -> Result<(), ImportError> {
        native_link::planned(self.allocation()?, rec);
        self.validate(rec)?;
        let tx = Transaction::new_unchecked(conn, TransactionBehavior::Immediate)
            .map_err(sqlite_storage_error)?;
        if !self.current(&tx)?.is_empty() || rec.phase != "prepared" {
            return Err(invalid());
        }
        self.authorize(&tx, owner)?;
        receipt::insert_once(&tx, rec)?;
        self.verify(&tx, owner)?;
        self.require_latest(&tx, rec)?;
        tx.commit().map_err(sqlite_storage_error)
    }

    fn require_latest(&self, tx: &Transaction<'_>, rec: &Receipt) -> Result<(), ImportError> {
        let current = self.current(tx)?.pop().ok_or_else(invalid)?;
        if receipt::digest(&current)? != receipt::digest(rec)? {
            return Err(invalid());
        }
        Ok(())
    }

    pub(in crate::session_migration) fn advance(
        &self,
        conn: &Connection,
        owner: &PublicationOwner<'_>,
        rec: &mut Receipt,
        phase: &str,
    ) -> Result<(), ImportError> {
        if !matches!(phase, "staged" | "published") {
            return Err(invalid());
        }
        let mut next = rec.clone();
        next.phase = phase.into();
        next.receipt_id = receipt::phase_id(self.allocation()?, phase)?;
        self.validate(&next)?;
        let tx = Transaction::new_unchecked(conn, TransactionBehavior::Immediate)
            .map_err(sqlite_storage_error)?;
        let previous = self.current(&tx)?.pop().ok_or_else(invalid)?;
        // The caller has just measured staging and may have set file_hashes;
        // compare that actual successor, not a mutated copy of the old point.
        receipt::require_successor(&previous, &next)?;
        self.authorize(&tx, owner)?;
        receipt::insert_once(&tx, &next)?;
        self.verify(&tx, owner)?;
        self.require_latest(&tx, &next)?;
        tx.commit().map_err(sqlite_storage_error)?;
        *rec = next;
        Ok(())
    }

    fn stored(
        &self,
        tx: &Transaction<'_>,
        linked: &Receipt,
    ) -> Result<VerifiedRecord, ImportError> {
        let stored = journal::load_required(
            tx,
            self.auth,
            &self.allocated.body.binding,
            StageKind::Stored,
        )?;
        stored.follows(&self.allocated)?;
        let Stage::Stored(result) = &stored.body.stage else {
            return Err(invalid());
        };
        if result.linked_receipt_digest != receipt::digest(linked)? {
            return Err(invalid());
        }
        native_link::require_result(self.allocation()?, linked, &result.project)?;
        Ok(stored)
    }

    pub(in crate::session_migration) fn link(
        &self,
        conn: &Connection,
        owner: &PublicationOwner<'_>,
        rec: &mut Receipt,
    ) -> Result<(), ImportError> {
        {
            let tx = conn.unchecked_transaction().map_err(sqlite_storage_error)?;
            self.verify(&tx, owner)?;
            self.require_latest(&tx, rec)?;
            if rec.phase != "published" {
                return Err(invalid());
            }
        }
        // No catalog snapshot or writer spans filesystem/native store I/O.
        let mut linked = rec.clone();
        let path = self
            .item
            .explicit_target
            .as_deref()
            .filter(|path| !path.is_empty())
            .unwrap_or(&self.item.root_path);
        let project = native_link::apply(&self.allocated, self.auth, path, &mut linked)?;
        #[cfg(test)]
        crate::session_migration::preview_lifecycle_tests::after_publish_point("after_native_link");
        linked.phase = "linked".into();
        linked.receipt_id = receipt::phase_id(self.allocation()?, "linked")?;
        let sealed = seal(
            self.auth,
            self.allocated.body.binding.clone(),
            Stage::Stored(StoreResult {
                allocation_digest: self.allocated.digest.clone(),
                linked_receipt_digest: receipt::digest(&linked)?,
                project,
            }),
        )?;
        let tx = Transaction::new_unchecked(conn, TransactionBehavior::Immediate)
            .map_err(sqlite_storage_error)?;
        self.require_latest(&tx, rec)?;
        self.authorize(&tx, owner)?;
        receipt::insert_once(&tx, &linked)?;
        journal::insert(&tx, self.auth, &sealed)?;
        self.verify(&tx, owner)?;
        self.require_latest(&tx, &linked)?;
        self.require_sealed(&tx, &sealed)?;
        tx.commit().map_err(sqlite_storage_error)?;
        *rec = linked;
        Ok(())
    }

    /// Invoked before ACK (the first final SQL write). The caller consumes its
    /// current writer witness; no fresh key or native/archive I/O is allowed.
    pub(in crate::session_migration) fn before_final(
        &self,
        tx: &Transaction<'_>,
        entry: &crate::session_migration::jobs::entry::AuthenticatedWriterEntry<'_, '_>,
        rec: &Receipt,
        item: &JobItem,
    ) -> Result<PreparedFinal, ImportError> {
        self.require_writer_allocation(tx, entry)?;
        self.require_latest(tx, rec)?;
        if rec.phase != "linked" {
            return Err(invalid());
        }
        let stored = self.stored(tx, rec)?;
        let mut committed = rec.clone();
        committed.phase = "committed".into();
        committed.receipt_id = receipt::phase_id(self.allocation()?, "committed")?;
        receipt::require_successor(rec, &committed)?;
        let sealed = seal(
            self.auth,
            self.allocated.body.binding.clone(),
            Stage::Committed(Committed {
                store_digest: stored.digest,
                receipt_digest: receipt::digest(&committed)?,
                item_digest: sha256_bytes(canonical(item)?.as_bytes()),
            }),
        )?;
        Ok(PreparedFinal {
            receipt: committed,
            sealed,
        })
    }

    pub(in crate::session_migration) fn finish(
        &self,
        tx: &Transaction<'_>,
        owner: &PublicationOwner<'_>,
        prepared: PreparedFinal,
        item: &JobItem,
        ack: (usize, u32),
    ) -> Result<(), ImportError> {
        receipt::insert_once(tx, &prepared.receipt)?;
        journal::insert(tx, self.auth, &prepared.sealed)?;
        crate::session_migration::project_link::remember_entry_mapping(tx, &prepared.receipt)?;
        self.verify(tx, owner)?;
        self.require_latest(tx, &prepared.receipt)?;
        self.require_sealed(tx, &prepared.sealed)?;
        self.require_committed(tx, &prepared.receipt, item, ack.0, Some(ack.1))?;
        Ok(())
    }

    fn require_sealed(
        &self,
        tx: &Transaction<'_>,
        expected: &SealedRecord,
    ) -> Result<(), ImportError> {
        let actual = journal::load_required(
            tx,
            self.auth,
            &expected.body.binding,
            expected.body.stage.kind(),
        )?;
        if actual.encoded != expected.encoded {
            return Err(invalid());
        }
        Ok(())
    }

    pub(in crate::session_migration) fn validate_committed(
        &self,
        conn: &Connection,
        owner: &PublicationOwner<'_>,
        rec: &Receipt,
        item: &JobItem,
        index: usize,
    ) -> Result<u32, ImportError> {
        let tx = conn.unchecked_transaction().map_err(sqlite_storage_error)?;
        self.verify(&tx, owner)?;
        self.require_latest(&tx, rec)?;
        self.require_committed(&tx, rec, item, index, None)
    }

    pub(in crate::session_migration) fn require_committed(
        &self,
        tx: &Transaction<'_>,
        rec: &Receipt,
        item: &JobItem,
        index: usize,
        version: Option<u32>,
    ) -> Result<u32, ImportError> {
        let committed = journal::load_committed(tx, self.auth, &self.allocated.body.binding)?;
        let Stage::Committed(result) = committed.body.stage else {
            return Err(invalid());
        };
        if rec.phase != "committed"
            || result.receipt_digest != receipt::digest(rec)?
            || result.item_digest != sha256_bytes(canonical(item)?.as_bytes())
        {
            return Err(invalid());
        }
        require_ack(tx, self.job(), item, index, version)
    }
}

pub(super) fn require_ack(
    tx: &Transaction<'_>,
    job: &str,
    item: &JobItem,
    index: usize,
    expected_version: Option<u32>,
) -> Result<u32, ImportError> {
    let json = crate::session_migration::jobs::item_store::item_json(tx, job, index)?;
    let (version, json_version, phase): (u32, u32, String) = tx.query_row(
        "SELECT j.state_version,json_extract(j.json,'$.stateVersion'),i.phase FROM jobs j JOIN job_items i ON i.job_id=j.job_id AND i.candidate_id=?2 WHERE j.job_id=?1",
        rusqlite::params![job, item.candidate_id],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    ).map_err(sqlite_storage_error)?;
    let actual: JobItem = serde_json::from_str(&json).map_err(|_| invalid())?;
    let original: serde_json::Value = serde_json::from_str(&json).map_err(|_| invalid())?;
    if canonical(&actual)? != canonical(item)?
        || canonical(&original)? != canonical(item)?
        || phase != item.phase
        || json_version != version
        || expected_version.is_some_and(|expected| expected != version)
    {
        return Err(invalid());
    }
    // Return the version from the very snapshot that verified the exact ACK.
    // Replays carry it back to the worker instead of acknowledging it again.
    Ok(version)
}

#[cfg(test)]
#[path = "publication_transition_tests.rs"]
mod tests;
