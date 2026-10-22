//! The concrete Undo writer owns this snapshot until its final recheck. No
//! archive/native I/O, result repair, legacy upgrade or transferable grant.
use rusqlite::Transaction;
use std::collections::BTreeMap;

use super::{recovery::Terminal, transition::require_ack, *};
use crate::session_migration::{
    catalog::{self, CatalogEntry},
    job_control::{self, publication_acceptance, RecoveryInput},
    jobs::{JobItem, JobRecord, Receipt},
    publication_crypto::JobCrypto,
    publication_protocol::PublicationProtocol,
    registration_provenance,
};

#[path = "publication_undo_cleanup.rs"]
pub(in crate::session_migration) mod cleanup;

pub(in crate::session_migration) struct UndoSnapshot<'snapshot, 'connection> {
    tx: &'snapshot Transaction<'connection>,
    crypto: &'snapshot JobCrypto,
    job: String,
    points: Vec<Option<Point>>,
}

pub(super) struct Point {
    terminal: Terminal,
    input_digest: String,
    revision_digest: String,
    previous_digest: Option<String>,
    entry: CatalogEntry,
    tombstoned: bool,
}

fn unavailable() -> ImportError {
    ImportError::new(
        ErrorCode::HistoryAccessUnavailable,
        "Undo requires its original authenticated publication result",
    )
}

fn undo_error(error: ImportError) -> ImportError {
    if error.code == ErrorCode::PlanStale {
        unavailable()
    } else {
        error
    }
}

/// Result/SQL primitive only. The production caller authenticates original
/// source/acceptance/member/input/fence in this SAME writer before invoking it.
pub(super) fn point(
    tx: &Transaction<'_>,
    auth: &InstallationAuthenticator,
    allocated: &VerifiedRecord,
    original: &RecoveryInput<'_>,
) -> Result<Point, ImportError> {
    let terminal = recovery::terminal(tx, auth, allocated, original)?.ok_or_else(unavailable)?;
    let rec = &terminal.receipt;
    let import = rec.import_id.as_deref().ok_or_else(unavailable)?;
    let revision = rec.revision_id.as_deref().ok_or_else(unavailable)?;
    let dedup = terminal.item.outcome.as_deref() == Some("deduplicated");
    if !dedup {
        if rec.job_id != allocated.body.binding.job_id {
            return Err(unavailable());
        }
        original.require_published_revision(tx, import, revision)?;
    }
    let entry = catalog::get_entry(tx, import)?.ok_or_else(unavailable)?;
    registration_provenance::authorize(tx, &entry, rec)?;
    let revision_digest = transition::dedup::revision_digest(tx, import, revision)?;
    let previous_digest = if dedup {
        // A reused receipt belongs to its real publisher; it grants this job
        // neither a previous-revision restoration nor native created ownership.
        None
    } else {
        rec.previous_revision
            .as_deref()
            .map(|previous| {
                if rec.created_entry || previous == revision {
                    return Err(unavailable());
                }
                transition::dedup::revision_digest(tx, import, previous)
            })
            .transpose()?
    };
    let tombstoned = catalog::is_tombstoned(
        tx,
        &entry.installation_id,
        &entry.provider,
        &entry.native_session_id,
    )?;
    Ok(Point {
        terminal,
        input_digest: original.digest()?,
        revision_digest,
        previous_digest,
        entry,
        tombstoned,
    })
}

fn read_point(
    tx: &Transaction<'_>,
    crypto: &JobCrypto,
    job: &str,
    candidate: &str,
) -> Result<Point, ImportError> {
    let auth = crypto
        .require(job, PublicationProtocol::SignedOutcomeV1)?
        .ok_or_else(unavailable)?;
    job_control::with_stored_recovery_input(tx, job, candidate, crypto, |original| {
        original.owner(tx, job, crypto)?;
        let accepted = publication_acceptance::journal::load_candidate(tx, auth, job)?;
        let allocated = accepted.allocated(tx, candidate)?;
        point(tx, auth, &allocated, &original)
    })
    .map_err(undo_error)
}

impl<'snapshot, 'connection> UndoSnapshot<'snapshot, 'connection> {
    pub(in crate::session_migration) fn capture(
        tx: &'snapshot Transaction<'connection>,
        job: &JobRecord,
        crypto: &'snapshot JobCrypto,
    ) -> Result<Option<Self>, ImportError> {
        if tx.is_autocommit() {
            return Err(unavailable());
        }
        let protocol = PublicationProtocol::for_job_id(&job.job_id)?;
        if crypto.require(&job.job_id, protocol)?.is_none() {
            return Ok(None);
        }
        let mut points = Vec::with_capacity(job.items.len());
        for (index, item) in job.items.iter().enumerate() {
            let captured = if item.phase == "committed" {
                // Include dedup: a forged mutable outcome cannot skip validation.
                let captured = read_point(tx, crypto, &job.job_id, &item.candidate_id)?;
                require_ack(
                    tx,
                    &job.job_id,
                    &captured.terminal.item,
                    index,
                    Some(job.state_version),
                )
                .map_err(undo_error)?;
                Some(captured)
            } else {
                None
            };
            points.push(captured);
        }
        Ok(Some(Self {
            tx,
            crypto,
            job: job.job_id.clone(),
            points,
        }))
    }

    pub(in crate::session_migration) fn receipt(&self, index: usize) -> Option<Receipt> {
        self.points
            .get(index)
            .and_then(Option::as_ref)
            .and_then(|point| {
                (point.terminal.item.outcome.as_deref() != Some("deduplicated"))
                    .then(|| point.terminal.receipt.clone())
            })
    }

    /// After save_job, authenticate the original chain again but check the NEW
    /// expected phase/error/version ACK, not its old committed representation.
    pub(in crate::session_migration) fn require_after(
        self,
        tx: &Transaction<'_>,
        job: &JobRecord,
    ) -> Result<Vec<(String, VerifiedRecord)>, ImportError> {
        if !std::ptr::eq(self.tx, tx)
            || tx.is_autocommit()
            || self.job != job.job_id
            || self.points.len() != job.items.len()
        {
            return Err(unavailable());
        }
        let mut entries: BTreeMap<String, (CatalogEntry, bool)> = BTreeMap::new();
        for before in self.points.iter().flatten() {
            let state = entries
                .entry(before.entry.import_id.clone())
                .or_insert_with(|| (before.entry.clone(), before.tombstoned));
            before.require_entry(&state.0, state.1)?;
        }
        let mut creations = Vec::new();
        for (index, (before, expected)) in self.points.iter().zip(&job.items).enumerate() {
            let Some(before) = before else { continue };
            let after = read_point(tx, self.crypto, &self.job, &expected.candidate_id)?;
            before.require_after(&after, expected)?;
            let state = entries
                .get_mut(&before.entry.import_id)
                .ok_or_else(unavailable)?;
            before.project_entry(&mut state.0, &mut state.1, expected)?;
            require_ack(tx, &self.job, expected, index, Some(job.state_version))
                .map_err(undo_error)?;
            if expected.phase == "undone" && before.terminal.receipt.created_project {
                let auth = self
                    .crypto
                    .require(&self.job, PublicationProtocol::SignedOutcomeV1)?
                    .ok_or_else(unavailable)?;
                let accepted =
                    publication_acceptance::journal::load_candidate(tx, auth, &self.job)?;
                let allocated = accepted.allocated(tx, &expected.candidate_id)?;
                allocated.project_creation(auth)?;
                // Only detached, verified allocation context survives COMMIT.
                // Native cleanup rechecks its origin/CAS and current references.
                creations.push((expected.candidate_id.clone(), allocated));
            }
        }
        for (import, (expected, tombstoned)) in entries {
            let entry = catalog::get_entry(tx, &import)?.ok_or_else(unavailable)?;
            if canonical(&entry)? != canonical(&expected)?
                || catalog::is_tombstoned(
                    tx,
                    &entry.installation_id,
                    &entry.provider,
                    &entry.native_session_id,
                )? != tombstoned
            {
                return Err(unavailable());
            }
        }
        Ok(creations)
    }
}

impl Point {
    pub(super) fn require_after(
        &self,
        after: &Self,
        expected: &JobItem,
    ) -> Result<(), ImportError> {
        let mut permitted = self.terminal.item.clone();
        permitted.phase.clone_from(&expected.phase);
        permitted.error.clone_from(&expected.error);
        let dedup = self.terminal.item.outcome.as_deref() == Some("deduplicated");
        if (dedup && expected.phase != "committed")
            || (!dedup && !matches!(expected.phase.as_str(), "undone" | "conflict" | "committed"))
            || canonical(&permitted)? != canonical(expected)?
            || self.input_digest != after.input_digest
            || self.terminal.record != after.terminal.record
            || canonical(&self.terminal.item)? != canonical(&after.terminal.item)?
            || receipt::digest(&self.terminal.receipt)? != receipt::digest(&after.terminal.receipt)?
            || self.revision_digest != after.revision_digest
            || self.previous_digest != after.previous_digest
        {
            return Err(unavailable());
        }
        Ok(())
    }

    pub(super) fn project_entry(
        &self,
        entry: &mut CatalogEntry,
        tombstoned: &mut bool,
        expected: &JobItem,
    ) -> Result<(), ImportError> {
        if self.terminal.item.outcome.as_deref() != Some("deduplicated")
            && expected.phase == "undone"
            && !*tombstoned
        {
            if entry.current_revision != self.terminal.item.revision_id {
                return Err(unavailable());
            }
            if let Some(previous) = &self.terminal.receipt.previous_revision {
                entry.current_revision = Some(previous.clone());
            } else if self.terminal.receipt.created_entry {
                entry.hidden = true;
                *tombstoned = true;
            } else {
                return Err(unavailable());
            }
        }
        Ok(())
    }

    pub(super) fn require_entry(
        &self,
        entry: &CatalogEntry,
        tombstoned: bool,
    ) -> Result<(), ImportError> {
        if canonical(entry)? != canonical(&self.entry)? || tombstoned != self.tombstoned {
            return Err(unavailable());
        }
        Ok(())
    }
}
