//! Per-item progress updates retain the durable JSON format without rewriting
//! every job_items row or decoding every result at each cancellation checkpoint.

use std::collections::HashSet;

use rusqlite::{params, OptionalExtension, TransactionBehavior};

use super::errors::{sqlite_storage_error, ErrorCode, ImportError};
use super::jobs::{JobItem, JobRecord, JobState};

fn failure(error: impl std::fmt::Display) -> ImportError {
    ImportError::new(ErrorCode::SnapshotUnavailable, error.to_string())
}

/// Cancellation changes only the job header. Keep the existing authenticated
/// read and atomic state transition, but leave every durable input/phase to the
/// worker instead of issuing an unchanged UPDATE for every item in the batch.
#[cfg(test)]
pub(super) fn request_cancel(
    conn: &rusqlite::Connection,
    id: &str,
) -> Result<JobRecord, ImportError> {
    let crypto = super::publication_crypto::JobCrypto::prepare(conn, id)?;
    request_cancel_inner(conn, id, &crypto, None)
}

pub(super) fn request_cancel_action(
    conn: &rusqlite::Connection,
    id: &str,
    action: &super::job_actions::PreparedAction,
) -> Result<JobRecord, ImportError> {
    request_cancel_inner(conn, id, action.crypto(), Some(action))
}

fn request_cancel_inner(
    conn: &rusqlite::Connection,
    id: &str,
    crypto: &super::publication_crypto::JobCrypto,
    action: Option<&super::job_actions::PreparedAction>,
) -> Result<JobRecord, ImportError> {
    let tx = rusqlite::Transaction::new_unchecked(conn, TransactionBehavior::Immediate)
        .map_err(sqlite_storage_error)?;
    let (job, entry) = super::jobs::get_job_for_writer(&tx, id, crypto)?;
    let mut job =
        job.ok_or_else(|| ImportError::new(ErrorCode::SourceNotFound, "job not found"))?;
    if let Some(action) = action {
        if action.replay(&tx, &job)? {
            return Ok(job);
        }
    }
    if !super::job_control::terminal(job.state) {
        job.state = JobState::Cancelling;
    }
    job.state_version = job
        .state_version
        .checked_add(1)
        .ok_or_else(|| failure("job state version overflow"))?;
    entry.require_writer(&tx, id)?;
    let changes = tx.total_changes();
    tx.execute(
        "UPDATE jobs SET state=?2,state_version=?3,json=?4 WHERE job_id=?1",
        params![
            id,
            job.state.as_str(),
            job.state_version,
            serde_json::to_string(&job).map_err(failure)?
        ],
    )
    .map_err(sqlite_storage_error)?;
    if let Some(action) = action {
        action.record(&tx, &job, changes, 1)?;
    }
    tx.commit().map_err(sqlite_storage_error)?;
    super::job_control::changed_version(id, job.state_version);
    Ok(job)
}

pub(super) fn cancelling(conn: &rusqlite::Connection, id: &str) -> Result<bool, ImportError> {
    let state: Option<String> = conn
        .query_row("SELECT state FROM jobs WHERE job_id=?1", [id], |row| {
            row.get(0)
        })
        .optional()
        .map_err(sqlite_storage_error)?;
    let Some(state) = state else { return Ok(true) };
    let state: JobState =
        serde_json::from_value(serde_json::Value::String(state)).map_err(failure)?;
    Ok(state == JobState::Cancelling)
}

/// Cancel every queued item that has never crossed a receipt boundary in one
/// durable state transition. Receipt presence is deliberately checked as raw
/// rows: an unreadable receipt is still evidence that publication started and
/// must be handled by the normal recovery path.
#[cfg(test)]
pub(super) fn cancel_unstarted_queued(
    conn: &rusqlite::Connection,
    id: &str,
) -> Result<HashSet<String>, ImportError> {
    let crypto = super::publication_crypto::JobCrypto::prepare(conn, id)?;
    cancel_unstarted_with_crypto(conn, id, &crypto)
}

pub(super) fn cancel_unstarted_with_crypto(
    conn: &rusqlite::Connection,
    id: &str,
    crypto: &super::publication_crypto::JobCrypto,
) -> Result<HashSet<String>, ImportError> {
    let tx = rusqlite::Transaction::new_unchecked(conn, TransactionBehavior::Immediate)
        .map_err(sqlite_storage_error)?;
    // One authentication for the whole batch, before owning its body or making
    // any phase projection. The witness cannot outlive this writer snapshot.
    let entry = super::jobs::entry::authenticate_writer(&tx, id, crypto)?;
    let (column_state, column_version, raw): (String, u32, String) = tx
        .query_row(
            "SELECT state,state_version,json FROM jobs WHERE job_id=?1",
            [id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(sqlite_storage_error)?
        .ok_or_else(|| failure("job not found"))?;
    let column_state: JobState =
        serde_json::from_value(serde_json::Value::String(column_state)).map_err(failure)?;
    let mut job: super::jobs::JobRecord = serde_json::from_str(&raw).map_err(failure)?;
    if job.state != column_state || job.state_version != column_version {
        return Err(failure("job header columns do not match its JSON"));
    }
    if job.state != JobState::Cancelling {
        tx.commit().map_err(sqlite_storage_error)?;
        return Ok(HashSet::new());
    }

    let started = {
        let mut stmt = tx
            .prepare("SELECT DISTINCT candidate_id FROM receipts WHERE job_id=?1")
            .map_err(sqlite_storage_error)?;
        let rows = stmt
            .query_map([id], |row| row.get::<_, String>(0))
            .map_err(sqlite_storage_error)?;
        rows.collect::<Result<HashSet<_>, _>>()
            .map_err(sqlite_storage_error)?
    };
    let mut cancelled = HashSet::new();
    for item in &mut job.items {
        if item.phase == "queued" && !started.contains(&item.candidate_id) {
            item.phase = "cancelled".into();
            item.outcome = Some("cancelled".into());
            item.error = None;
            cancelled.insert(item.candidate_id.clone());
        }
    }
    if cancelled.is_empty() {
        tx.commit().map_err(sqlite_storage_error)?;
        return Ok(cancelled);
    }

    let cancelled_json = serde_json::to_string(&cancelled).map_err(failure)?;
    entry.require_writer(&tx, id)?;
    // The header is authoritative for eligibility. Repair existing phase
    // projections, but never synthesize a missing durable input row.
    tx.execute(
        "UPDATE job_items AS item SET phase='cancelled'
             WHERE item.job_id=?1
               AND item.candidate_id IN (SELECT value FROM json_each(?2))
               AND NOT EXISTS (
                 SELECT 1 FROM receipts AS receipt
                 WHERE receipt.job_id=item.job_id
                   AND receipt.candidate_id=item.candidate_id
               )",
        params![id, cancelled_json],
    )
    .map_err(sqlite_storage_error)?;
    job.state_version = job
        .state_version
        .checked_add(1)
        .ok_or_else(|| failure("job state version overflow"))?;
    let updated = tx
        .execute(
            "UPDATE jobs SET state_version=?2,json=?3
             WHERE job_id=?1 AND state='cancelling' AND state_version=?4",
            params![
                id,
                job.state_version,
                serde_json::to_string(&job).map_err(failure)?,
                column_version
            ],
        )
        .map_err(sqlite_storage_error)?;
    if updated != 1 {
        return Err(failure("job state changed during cancellation"));
    }
    tx.commit().map_err(sqlite_storage_error)?;
    super::job_control::changed_version(id, job.state_version);
    Ok(cancelled)
}

/// The worker owns the job's item ordering. Verify its slot on every write, and
/// keep current job state (including a concurrent cancellation) untouched.
#[cfg(test)]
pub(super) fn set_item(
    conn: &rusqlite::Connection,
    id: &str,
    index: usize,
    item: &JobItem,
) -> Result<(), ImportError> {
    let crypto = super::publication_crypto::JobCrypto::prepare(conn, id)?;
    set_item_with_crypto(conn, id, index, item, &crypto)
}

pub(super) fn set_item_with_crypto(
    conn: &rusqlite::Connection,
    id: &str,
    index: usize,
    item: &JobItem,
    crypto: &super::publication_crypto::JobCrypto,
) -> Result<(), ImportError> {
    let tx = rusqlite::Transaction::new_unchecked(conn, TransactionBehavior::Immediate)
        .map_err(sqlite_storage_error)?;
    let entry = super::jobs::entry::authenticate_writer(&tx, id, crypto)?;
    let version = set_item_authenticated(&tx, id, index, item, entry)?;
    tx.commit().map_err(sqlite_storage_error)?;
    super::job_control::changed_version(id, version);
    Ok(())
}

/// The publisher consumes its existing authentication in the same final writer.
/// No event may be emitted until the caller commits both publication and ACK.
pub(super) fn set_item_authenticated(
    tx: &rusqlite::Transaction<'_>,
    id: &str,
    index: usize,
    item: &JobItem,
    entry: super::jobs::entry::AuthenticatedWriterEntry<'_, '_>,
) -> Result<u32, ImportError> {
    entry.require_writer(tx, id)?;
    let path = format!("$.items[{index}]");
    let existing: Option<String> = tx
        .query_row(
            "SELECT json_extract(json, ?2) FROM jobs WHERE job_id=?1",
            params![id, path],
            |row| row.get(0),
        )
        .optional()
        .map_err(sqlite_storage_error)?
        .flatten();
    let previous: JobItem = serde_json::from_str(
        existing
            .as_deref()
            .ok_or_else(|| failure("job item disappeared"))?,
    )
    .map_err(failure)?;
    if previous.candidate_id != item.candidate_id {
        return Err(failure("job item order changed"));
    }
    let changed = tx
        .execute(
            "UPDATE job_items SET phase=?3 WHERE job_id=?1 AND candidate_id=?2",
            params![id, item.candidate_id, item.phase],
        )
        .map_err(sqlite_storage_error)?;
    // A damaged/legacy job may have lost its durable input. Keep that visible
    // as failed (or cancelled) in the header so it cannot block later jobs.
    // Never fabricate an input or report successful publication in this case.
    if changed != 1 && !matches!(item.phase.as_str(), "failed" | "cancelled") {
        return Err(failure("durable job item disappeared"));
    }
    let version: u32 = tx
        .query_row(
            "UPDATE jobs SET json=json_set(json, ?2, json(?3), '$.stateVersion', state_version+1),
             state_version=state_version+1 WHERE job_id=?1 RETURNING state_version",
            params![id, path, serde_json::to_string(item).map_err(failure)?],
            |row| row.get(0),
        )
        .map_err(sqlite_storage_error)?;
    Ok(version)
}
