//! Status reads of a prepare batch while its worker may hold the database.
use rusqlite::Connection;

use super::{batch_path, ensure_running, status_raw, PrepareBatchStatus};
use crate::session_migration::budget::{DB_BUSY_MS, DB_BUSY_RETRIES};
use crate::session_migration::errors::{sqlite_storage_error, ErrorCode, ImportError};
use crate::session_migration::sqlite_open;

/// Open the batch for a status read. The connection is read-write so WAL can
/// map its shared memory, and `query_only` blocks SQL writes. Journal mode is
/// left alone: setting it takes a write lock while the UI polls a worker that
/// holds `BEGIN IMMEDIATE`.
pub(super) fn open_status(batch_id: &str) -> Result<Connection, ImportError> {
    let path = batch_path(batch_id)?;
    if !path.is_file() {
        return Err(ImportError::new(
            ErrorCode::PlanStale,
            "prepare batch no longer exists",
        ));
    }
    let conn = sqlite_open::open(path).map_err(sqlite_storage_error)?;
    conn.busy_timeout(std::time::Duration::from_millis(DB_BUSY_MS))
        .map_err(sqlite_storage_error)?;
    conn.execute_batch("PRAGMA foreign_keys=ON; PRAGMA query_only=ON;")
        .map_err(sqlite_storage_error)?;
    Ok(conn)
}

fn status_once(batch_id: &str) -> Result<PrepareBatchStatus, ImportError> {
    let current = status_raw(batch_id)?;
    if current.state.runnable() {
        ensure_running(batch_id)?;
    }
    status_raw(batch_id)
}

pub fn status(batch_id: &str) -> Result<PrepareBatchStatus, ImportError> {
    // A checkpoint or the Windows pending-byte lock can still return
    // SQLITE_BUSY immediately. Dropping the connection releases the shared
    // lock SQLite asks the caller to drop, then one bounded retry runs.
    // Anything still busy stays SOURCE_BUSY (retryable), not a snapshot failure.
    let mut attempts_left = DB_BUSY_RETRIES;
    loop {
        match status_once(batch_id) {
            Err(error) if error.code == ErrorCode::SourceBusy && attempts_left > 0 => {
                attempts_left -= 1;
            }
            result => return result,
        }
    }
}
