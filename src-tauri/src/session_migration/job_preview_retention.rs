//! Serialize acceptance and reconcile its durable retention epoch after crash.
//! Order: lifecycle -> acceptance -> publication -> SQLite -> lease. No lease
//! callback enters SQLite, and immutable file hashes never run under its writer.

use std::fs::{File, OpenOptions};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use super::super::errors::{io_storage_error, ErrorCode, ImportError};
use super::super::jobs::{JobRecord, PreparedRevision, PreviewLease};
use super::super::{catalog, hash, preview_lease_io, preview_lifecycle};
use super::DurableInput;
use rusqlite::Connection;

#[path = "job_retention_catalog_file.rs"]
mod catalog_file;
#[path = "job_retention_outcome.rs"]
mod outcome;
#[path = "job_retention_receipt.rs"]
mod receipt;

fn deferred(error: impl std::fmt::Display) -> ImportError {
    ImportError::new(
        ErrorCode::SnapshotUnavailable,
        format!("preview retention reconciliation deferred: {error}"),
    )
}

static ACCEPTANCE: Mutex<()> = Mutex::new(());

pub(super) struct PendingInputs {
    recovery: Option<receipt::Receipt>,
    catalog_file: Option<catalog_file::BoundFile>,
    prepared: Vec<Arc<PreparedRevision>>,
    file: File,
    _process: MutexGuard<'static, ()>,
}

impl PendingInputs {
    pub(super) fn acquire() -> Result<Self, ImportError> {
        let deadline = Instant::now() + Duration::from_secs(3);
        let process = preview_lifecycle::wait_mutex(
            &ACCEPTANCE,
            deadline,
            "preview acceptance is busy; retry the plan",
        )?;
        let path = catalog::imports_dir().join("preview-acceptance.lock");
        catalog::ensure_inside_imports(&path)?;
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(path)
            .map_err(io_storage_error)?;
        loop {
            match fs2::FileExt::try_lock_exclusive(&file) {
                Ok(()) => break,
                Err(error) if preview_lifecycle::busy(&error) && Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(20));
                }
                Err(error) if preview_lifecycle::busy(&error) => {
                    return Err(ImportError::new(
                        ErrorCode::SourceBusy,
                        "preview acceptance is busy; retry the plan",
                    ));
                }
                Err(error) => return Err(io_storage_error(error)),
            }
        }
        Ok(Self {
            recovery: None,
            catalog_file: None,
            prepared: Vec::new(),
            file,
            _process: process,
        })
    }

    pub(super) fn existing(
        &self,
        conn: &Connection,
        job: &JobRecord,
    ) -> Result<Option<JobRecord>, ImportError> {
        outcome::existing(conn, job)
    }

    pub(super) fn reconcile(
        &self,
        conn: &Connection,
        protection: &preview_lifecycle::Protection<'_>,
    ) -> Result<(), ImportError> {
        for recovery in receipt::load_all()? {
            reconcile_one(conn, protection, &recovery)?;
        }
        Ok(())
    }

    pub(super) fn begin(
        &mut self,
        conn: &Connection,
        job: &JobRecord,
        prepared: &[(DurableInput, Arc<PreparedRevision>)],
    ) -> Result<(), ImportError> {
        let catalog_file = catalog_file::BoundFile::open(conn)?;
        let mut entries = vec![];
        for (input, revision) in prepared {
            let (lease, hashes) = super::super::jobs::verify_disk_preview_integrity(
                &revision.path,
                &input.selection_set.single_entry()?.preview_id,
            )?;
            if hashes != revision.file_hashes
                || lease.snapshot_digest != revision.digest
                || lease.candidate_id != input.candidate_id
            {
                return Err(deferred("preview changed before retention intent"));
            }
            if lease.retained_by_job && outcome::owner(conn, &revision.path, &lease)?.is_none() {
                return Err(deferred(
                    "retained preview has no authenticated accepted owner",
                ));
            }
            entries.push(receipt::Entry {
                preview_id: lease.preview_id.clone(),
                candidate_digest: hash::sha256_bytes(input.candidate_id.as_bytes()),
                input_digest: hash::sha256_bytes(
                    serde_json::to_string(input).map_err(deferred)?.as_bytes(),
                ),
                lease_digest: receipt::lease_digest(&lease)?,
                added_retention: !lease.retained_by_job,
            });
        }
        let recovery = receipt::Receipt {
            version: 3,
            epoch_id: uuid::Uuid::new_v4().to_string(),
            catalog_epoch: catalog::retention_catalog_epoch(conn)?
                .ok_or_else(|| deferred("retention catalog epoch missing"))?,
            catalog_file: catalog_file.identity.clone(),
            job_id: job.job_id.clone(),
            context_digest: receipt::context_digest(job),
            entries,
        };
        // Complete identity intent is flushed before any false -> true lease.
        catalog_file.verify(conn, &recovery.catalog_file)?;
        recovery.persist()?;
        self.catalog_file = Some(catalog_file);
        self.recovery = Some(recovery);
        self.prepared = prepared
            .iter()
            .map(|(_, revision)| Arc::clone(revision))
            .collect();
        Ok(())
    }

    pub(super) fn verify_catalog(&self, conn: &Connection) -> Result<(), ImportError> {
        let recovery = self
            .recovery
            .as_ref()
            .ok_or_else(|| deferred("acceptance recovery intent missing"))?;
        self.catalog_file
            .as_ref()
            .ok_or_else(|| deferred("acceptance catalog handle missing"))?
            .verify(conn, &recovery.catalog_file)
    }

    pub(super) fn record_acceptance(&self, conn: &Connection) -> Result<(), ImportError> {
        // Filesystem identity was inspected before acquiring the writer. Keep
        // the live bound handle through COMMIT, but only touch SQL here.
        self.recovery
            .as_ref()
            .ok_or_else(|| deferred("acceptance recovery intent missing"))?
            .record_acceptance(conn)
    }

    pub(super) fn retain(
        &self,
        revision: &Arc<PreparedRevision>,
        protection: &preview_lifecycle::Protection<'_>,
    ) -> Result<(), ImportError> {
        let recovery = self
            .recovery
            .as_ref()
            .ok_or_else(|| deferred("retention intent missing"))?;
        let entry = recovery
            .entries
            .iter()
            .find(|entry| receipt::preview_path(entry) == revision.path)
            .ok_or_else(|| deferred("retention input is outside its epoch"))?;
        preview_lease_io::update_held(&revision.path, protection, |lease| {
            if receipt::lease_digest(lease)? != entry.lease_digest {
                return Err(deferred("preview changed before retention"));
            }
            lease.retained_by_job = true;
            Ok(())
        })
    }

    pub(super) fn accept(
        &mut self,
        conn: &Connection,
        protection: &preview_lifecycle::Protection<'_>,
    ) {
        for prepared in &self.prepared {
            prepared.retain_for_job();
        }
        // COMMIT already succeeded. Metadata retirement failure must not turn
        // a durable accepted job into a rejected UI answer; retry reconciles it.
        if let Err(error) = self.rollback(conn, protection) {
            tracing::warn!("accepted preview retention reconciliation deferred: {error}");
        }
    }

    pub(super) fn rollback(
        &mut self,
        conn: &Connection,
        protection: &preview_lifecycle::Protection<'_>,
    ) -> Result<(), ImportError> {
        let Some(expected) = &self.recovery else {
            return Ok(());
        };
        // Authenticate actual current file bytes, not just an in-memory intent.
        let recovery = receipt::load_all()?
            .into_iter()
            .find(|receipt| receipt.epoch_id == expected.epoch_id)
            .ok_or_else(|| deferred("recovery metadata disappeared"))?;
        if serde_json::to_vec(&recovery).map_err(deferred)?
            != serde_json::to_vec(expected).map_err(deferred)?
        {
            return Err(deferred("recovery epoch was replaced"));
        }
        reconcile_one(conn, protection, &recovery)?;
        self.recovery = None;
        self.catalog_file = None;
        Ok(())
    }
}

fn reconcile_one(
    conn: &Connection,
    protection: &preview_lifecycle::Protection<'_>,
    recovery: &receipt::Receipt,
) -> Result<(), ImportError> {
    let catalog_file = catalog_file::BoundFile::open(conn)?;
    catalog_file.verify(conn, &recovery.catalog_file)?;
    // Stage all filesystem evidence before entering SQLite's read snapshot.
    let mut leases = vec![];
    for entry in &recovery.entries {
        let path = receipt::preview_path(entry);
        let (lease, _) =
            super::super::jobs::verify_disk_preview_integrity(&path, &entry.preview_id)?;
        if receipt::lease_digest(&lease)? != entry.lease_digest
            || hash::sha256_bytes(lease.candidate_id.as_bytes()) != entry.candidate_digest
        {
            return Err(deferred(
                "immutable preview differs from its recovery epoch",
            ));
        }
        leases.push((path, lease));
    }
    let candidates = leases
        .iter()
        .map(|(_, lease)| lease.candidate_id.clone())
        .collect::<Vec<_>>();
    let cryptos = super::super::publication_crypto::JobCryptos::prepare(
        conn,
        &candidates,
        std::slice::from_ref(&recovery.job_id),
    )?;
    let tx = conn.unchecked_transaction().map_err(deferred)?;
    let committed = outcome::committed(&tx, recovery, &leases, &cryptos)?;
    let retained = if committed {
        vec![true; leases.len()]
    } else {
        outcome::foreign_owners(&tx, &leases)?;
        let owners = outcome::owners(&tx, &leases, &cryptos)?;
        for (entry, owner) in recovery.entries.iter().zip(&owners) {
            if !entry.added_retention && owner.is_none() {
                return Err(deferred("preexisting retention owner is unresolved"));
            }
        }
        owners.into_iter().map(|owner| owner.is_some()).collect()
    };
    drop(tx);
    catalog_file.verify(conn, &recovery.catalog_file)?;
    for (((path, _), entry), keep) in leases.iter().zip(&recovery.entries).zip(retained) {
        preview_lease_io::update_held(path, protection, |lease| {
            if receipt::lease_digest(lease)? != entry.lease_digest {
                return Err(deferred("preview replaced during reconciliation"));
            }
            if keep {
                lease.retained_by_job = true;
            } else if entry.added_retention {
                lease.retained_by_job = false;
            }
            // Latest expiry and batch fields are deliberately untouched.
            Ok(())
        })?;
    }
    recovery.remove()
}

pub(in crate::session_migration) fn reconcile_cleanup(
    conn: &Connection,
    cleanup: &preview_lifecycle::CleanupGuard,
) -> Result<(), ImportError> {
    let owner = PendingInputs::acquire()?;
    owner.reconcile(conn, &preview_lifecycle::Protection::Cleanup(cleanup))
}

pub(in crate::session_migration) fn reconcile_operation(
    conn: &Connection,
    operation: &preview_lifecycle::OperationGuard,
) -> Result<(), ImportError> {
    let owner = PendingInputs::acquire()?;
    owner.reconcile(conn, &preview_lifecycle::Protection::Operation(operation))
}

pub(in crate::session_migration) fn completed_owners(
    conn: &Connection,
    leases: &[(std::path::PathBuf, PreviewLease)],
) -> Result<Vec<Result<bool, ImportError>>, ImportError> {
    Ok(outcome::owner_results(conn, leases)?
        .into_iter()
        .map(|owner| {
            owner?
                .map(|retryable| !retryable)
                .ok_or_else(|| deferred("retained preview has no authenticated completed owner"))
        })
        .collect())
}

pub(in crate::session_migration) fn pending() -> bool {
    receipt::pending()
}

#[cfg(test)]
pub(in crate::session_migration) fn fail_receipt_once(point: &'static str) {
    receipt::fail_once(point);
}

impl Drop for PendingInputs {
    fn drop(&mut self) {
        // A crash/panic or failed repair leaves the durable epoch intact. Drop
        // cannot reenter lifecycle from CleanupGuard or drain recovery evidence.
        let _ = fs2::FileExt::unlock(&self.file);
    }
}
