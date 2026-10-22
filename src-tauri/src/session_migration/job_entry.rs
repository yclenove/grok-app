//! Authentication before full job reads or SQLite JSON projections.
//! Ordinary job/input bodies are never loaded or capped to classify absence.
use rusqlite::Connection;

use crate::session_migration::{
    errors::{ErrorCode, ImportError},
    job_control::association_members::AcceptedMembers,
    publication_crypto::JobCrypto,
    publication_protocol::PublicationProtocol,
    registration_provenance::old_owner,
};

/// Not serializable/caller constructible. Retains the same-snapshot member
/// witness until the body/projection reader finishes; no persisted format.
pub(in crate::session_migration) struct AuthenticatedEntry<'snapshot> {
    connection: &'snapshot Connection,
    job_id: String,
    changes: u64,
    members: Option<AcceptedMembers<'snapshot>>,
}

/// Only this constructor accepts a concrete Transaction. A read-only entry
/// cannot be upgraded into a writer grant by passing a Connection afterwards.
pub(in crate::session_migration) struct AuthenticatedWriterEntry<'snapshot, 'connection> {
    entry: AuthenticatedEntry<'snapshot>,
    tx: &'snapshot rusqlite::Transaction<'connection>,
    crypto: &'snapshot JobCrypto,
    acceptance: Option<
        crate::session_migration::job_control::publication_acceptance::journal::AcceptedJob<
            'snapshot,
            'connection,
        >,
    >,
}

pub(in crate::session_migration) enum UndoReceipt {
    Ordinary,
    Captured(Option<Box<crate::session_migration::jobs::Receipt>>),
}

pub(in crate::session_migration) struct UndoPreparation<'snapshot, 'connection> {
    pub(super) receipts: Vec<UndoReceipt>,
    pub(super) signed: Option<
        crate::session_migration::publication_outcome::undo::UndoSnapshot<'snapshot, 'connection>,
    >,
}

impl UndoReceipt {
    pub(in crate::session_migration) fn read(
        self,
        conn: &Connection,
        job_id: &str,
        candidate_id: &str,
    ) -> Result<Option<crate::session_migration::jobs::Receipt>, ImportError> {
        match self {
            Self::Ordinary => {
                crate::session_migration::jobs::latest_receipt(conn, job_id, candidate_id)
            }
            Self::Captured(receipt) => Ok(receipt.map(|receipt| *receipt)),
        }
    }
}

impl<'snapshot, 'connection> AuthenticatedWriterEntry<'snapshot, 'connection> {
    /// Validate every eligible association before Undo changes any target.
    /// Ordinary items retain their existing receipt-read order and conflicts.
    /// The witness is consumed in the writer; no authority survives a COMMIT.
    pub(in crate::session_migration) fn undo_receipts(
        self,
        tx: &'snapshot rusqlite::Transaction<'connection>,
        job: &crate::session_migration::jobs::JobRecord,
    ) -> Result<UndoPreparation<'snapshot, 'connection>, ImportError> {
        let signed = crate::session_migration::publication_outcome::undo::UndoSnapshot::capture(
            tx,
            job,
            self.crypto,
        )?;
        let mut receipts = Vec::with_capacity(job.items.len());
        for (index, item) in job.items.iter().enumerate() {
            let receipt = if let Some(signed) = &signed {
                UndoReceipt::Captured(signed.receipt(index).map(Box::new))
            } else if item.phase != "committed" || item.outcome.as_deref() == Some("deduplicated") {
                UndoReceipt::Ordinary
            } else if let Some(members) = &self.entry.members {
                members.undo_receipt(tx, &job.job_id, item, self.crypto)?
            } else {
                UndoReceipt::Ordinary
            };
            receipts.push(receipt);
        }
        self.require_writer(tx, &job.job_id)?;
        Ok(UndoPreparation { receipts, signed })
    }

    /// Consume once, before the first SQL write in the borrowed transaction.
    /// A different connection/job or a same-connection mutation is not a grant.
    pub(in crate::session_migration) fn require_writer(
        self,
        tx: &rusqlite::Transaction<'_>,
        job_id: &str,
    ) -> Result<(), ImportError> {
        self.require_current(tx, job_id)
    }

    /// Reuse only this writer's current signed header/member-set validation.
    /// The selected member, original input and allocation are still read now.
    /// The returned record is detached evidence, never a SQL writer grant.
    pub(in crate::session_migration) fn allocated(
        &self,
        tx: &rusqlite::Transaction<'_>,
        job_id: &str,
        candidate: &str,
    ) -> Result<crate::session_migration::publication_outcome::VerifiedRecord, ImportError> {
        self.require_current(tx, job_id)?;
        self.acceptance
            .as_ref()
            .ok_or_else(|| {
                ImportError::new(
                    ErrorCode::PlanStale,
                    "signed allocation requires acceptance",
                )
            })?
            .allocated(tx, candidate)
    }

    fn require_current(
        &self,
        tx: &rusqlite::Transaction<'_>,
        job_id: &str,
    ) -> Result<(), ImportError> {
        if !std::ptr::eq(self.tx, tx)
            || tx.is_autocommit()
            || !std::ptr::eq(self.entry.connection, &**tx)
            || self.entry.job_id != job_id
            || self.entry.changes != tx.total_changes()
        {
            return Err(ImportError::new(
                ErrorCode::PlanStale,
                "job progress authentication changed before its writer",
            ));
        }
        if let Some(acceptance) = &self.acceptance {
            acceptance.require_current(tx)?;
        }
        Ok(())
    }
}

fn authenticate_common<'snapshot>(
    conn: &'snapshot Connection,
    job_id: &str,
) -> Result<(AuthenticatedEntry<'snapshot>, Option<PublicationProtocol>), ImportError> {
    #[cfg(test)]
    let _span = crate::session_migration::codec_profile::Span::start("entry-common", 0);
    if conn.is_autocommit() {
        return Err(ImportError::new(
            ErrorCode::PlanStale,
            "job entry requires one snapshot",
        ));
    }
    let entry = |members| AuthenticatedEntry {
        connection: conn,
        job_id: job_id.into(),
        changes: conn.total_changes(),
        members,
    };
    // An absent job has no body or projection to authenticate. Reporting its
    // absence must not depend on an unrelated damaged acceptance candidate.
    let present = crate::session_migration::publication_crypto::present(conn, job_id)?;
    if !present {
        return Ok((entry(None), None));
    }
    // This dispatch survives deletion of every new metadata row and the plan.
    // V8 must still authenticate signed acceptance below, even if all new
    // metadata rows and the original plan have been removed.
    let protocol = PublicationProtocol::for_job_id(job_id)?;
    let proofs = match crate::session_migration::job_control::provenance_scope::job_candidates(
        conn, job_id,
    )? {
        Some(proofs) => proofs,
        // Missing both new facts is not ordinary absence. Older producers keep
        // the strict closure until their own compatibility gate is resolved.
        None => old_owner::job_candidates(conn, job_id).map_err(ImportError::catalog_wide)?,
    };
    let Some(first) = proofs.first() else {
        return Ok((entry(None), Some(protocol)));
    };
    let members = first.job_members(conn)?;
    if members.count() != proofs.len() {
        return Err(ImportError::new(
            ErrorCode::PlanStale,
            "accepted job proof/member set is incomplete",
        ));
    }
    for proof in proofs {
        proof.verify_job_member(conn, &members)?;
    }
    Ok((entry(Some(members)), Some(protocol)))
}

pub(in crate::session_migration) fn authenticate_with_crypto<'snapshot>(
    conn: &'snapshot Connection,
    job_id: &str,
    crypto: &JobCrypto,
) -> Result<AuthenticatedEntry<'snapshot>, ImportError> {
    let (entry, protocol) = authenticate_common(conn, job_id)?;
    if let Some(protocol) = protocol {
        if let Some(auth) = crypto.require(job_id, protocol)? {
            crate::session_migration::job_control::publication_acceptance::journal::validate_snapshot(
                conn, auth, job_id,
            )?;
        }
    }
    Ok(entry)
}

/// One publication owner's check. On the fenced worker connection, when the
/// open transaction changed no other item's input and no acceptance row, only
/// this candidate is authenticated (header, member, exact input). Otherwise,
/// and on any unfenced connection, the complete snapshot is validated as
/// before. Every input is hashed when work starts and again in the writer that
/// makes the job terminal; repeating that for each step of each item made
/// publication quadratic in the batch size.
pub(in crate::session_migration) fn authenticate_candidate_with_crypto<'snapshot>(
    conn: &'snapshot Connection,
    job_id: &str,
    candidate: &str,
    crypto: &JobCrypto,
) -> Result<AuthenticatedEntry<'snapshot>, ImportError> {
    let (entry, protocol) = authenticate_common(conn, job_id)?;
    if let Some(protocol) = protocol {
        if let Some(auth) = crypto.require(job_id, protocol)? {
            use crate::session_migration::job_control::publication_acceptance::journal;
            if crate::session_migration::publication_write_fence::check(conn, job_id, candidate)? {
                journal::validate_candidate(conn, auth, job_id, candidate)?;
            } else {
                journal::validate_snapshot(conn, auth, job_id)?;
            }
        }
    }
    Ok(entry)
}

pub(in crate::session_migration) fn authenticate_writer<'snapshot, 'connection>(
    tx: &'snapshot rusqlite::Transaction<'connection>,
    job_id: &str,
    crypto: &'snapshot JobCrypto,
) -> Result<AuthenticatedWriterEntry<'snapshot, 'connection>, ImportError> {
    let (entry, protocol) = authenticate_common(tx, job_id)?;
    let acceptance = match protocol {
        Some(protocol) => crypto
            .require(job_id, protocol)?
            .map(|auth| {
                crate::session_migration::job_control::publication_acceptance::journal::load_candidate(
                    tx, auth, job_id,
                )
            })
            .transpose()?,
        None => None,
    };
    Ok(AuthenticatedWriterEntry {
        entry,
        tx,
        crypto,
        acceptance,
    })
}

pub(super) fn read_with_crypto<T>(
    conn: &Connection,
    job_id: &str,
    crypto: &JobCrypto,
    reader: impl FnOnce(&Connection) -> Result<T, ImportError>,
) -> Result<T, ImportError> {
    if conn.is_autocommit() {
        let tx = conn
            .unchecked_transaction()
            .map_err(crate::session_migration::errors::sqlite_storage_error)?;
        return read_with_crypto(&tx, job_id, crypto, reader);
    }
    let _entry = authenticate_with_crypto(conn, job_id, crypto)?;
    reader(conn)
}
