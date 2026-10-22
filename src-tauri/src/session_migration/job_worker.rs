//! Invocation-local scheduling isolation, not permission to read/publish a job.
use super::*;
use rusqlite::OptionalExtension;

pub(super) enum JobRun {
    Completed,
    Refused(ImportError),
}

const MAX_REFUSALS: usize = 1024;

/// Authenticate all original input bytes in the very writer that starts work.
/// Header/member seals alone prove membership, not unchanged current bodies.
/// Do this once per invocation, not once per item/progress write (quadratic).
pub(super) fn start_job(
    conn: &Connection,
    id: &str,
    crypto: &crate::session_migration::publication_crypto::JobCrypto,
) -> Result<JobRecord, ImportError> {
    let tx = state_tx(conn)?;
    let protocol =
        crate::session_migration::publication_protocol::PublicationProtocol::for_job_id(id)?;
    if let Some(auth) = crypto.require(id, protocol)? {
        publication_acceptance::journal::validate_snapshot(&tx, auth, id)?;
    }
    change_job_in_tx(tx, id, crypto, None, |job| {
        if job.state != JobState::Cancelling {
            job.state = JobState::Running;
        }
        Ok(())
    })
}

pub(super) fn run_pending() -> Result<u32, ImportError> {
    #[cfg(test)]
    let _span = crate::session_migration::codec_profile::Span::start("worker-run-pending", 0);
    let _worker = ImportLock::acquire("worker.lock")?;
    let conn = open_catalog()?;
    let _fence = crate::session_migration::publication_write_fence::install(&conn);
    crate::session_migration::catalog_indexes::ensure_receipt_indexes(&conn)?;
    let mut completed = 0;
    let mut refused = Vec::new();
    // Previously authorized cleanup also survives a full App restart, without
    // retaining the frontend request ID or replaying content Undo. A refused
    // marker does not prevent unrelated queued work from draining below.
    let mut first_refusal = crate::session_migration::job_actions::undo::recover(&conn).err();
    loop {
        // Bounded JSON parameter avoids SQLite's variable-count limit. Select
        // afresh each iteration so newly enqueued work is not lost to a snapshot
        // of the queue. Refused IDs are not persisted or changed to failed.
        let excluded = serde_json::to_string(&refused).map_err(failure)?;
        let id: Option<String> = conn.query_row(
            "SELECT job_id FROM jobs WHERE state IN ('queued','running','cancelling')
             AND job_id NOT IN (SELECT value FROM json_each(?1)) ORDER BY created_at,job_id LIMIT 1",
            [excluded], |row| row.get(0),
        ).optional().map_err(crate::session_migration::errors::sqlite_storage_error)?;
        let Some(id) = id else {
            break;
        };
        #[cfg(all(debug_assertions, feature = "native-import-audit"))]
        {
            let state: String = conn
                .query_row("SELECT state FROM jobs WHERE job_id=?1", [&id], |row| {
                    row.get(0)
                })
                .map_err(crate::session_migration::errors::sqlite_storage_error)?;
            if state == "queued" {
                crate::session_migration::native_fault::pause(
                    crate::session_migration::native_fault::Point::Queued,
                    &id,
                );
            }
        }
        match run_job(&conn, &id)? {
            JobRun::Completed => completed += 1,
            JobRun::Refused(error) => {
                first_refusal.get_or_insert(error);
                refused.push(id);
                if refused.len() == MAX_REFUSALS {
                    break;
                }
            }
        }
    }
    // Healthy work can drain, but the caller/wake log still sees corruption.
    // A fresh invocation retries every refused owner with fresh authentication.
    first_refusal.map_or(Ok(completed), Err)
}
