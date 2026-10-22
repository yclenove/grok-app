//! Catalog identity for crash reconciliation, never a reader grant.
use std::io::Read;

use rusqlite::{Connection, DatabaseName, OptionalExtension, Transaction, TransactionBehavior};

use super::super::errors::{sqlite_storage_error, ErrorCode, ImportError};

const KEY: &str = "preview_retention_catalog_epoch";

pub(in crate::session_migration) fn read(conn: &Connection) -> Result<Option<String>, ImportError> {
    let value = read_text(conn, KEY, 36)?;
    if value
        .as_ref()
        .is_some_and(|value| !uuid::Uuid::parse_str(value).is_ok_and(|id| id.to_string() == *value))
    {
        return Err(invalid("invalid retention catalog epoch"));
    }
    Ok(value)
}

fn invalid(message: &'static str) -> ImportError {
    ImportError::new(ErrorCode::SnapshotUnavailable, message)
}

/// Point-key metadata only. Inspect type and incremental byte length before
/// allocating a value; SQL CAST/length can materialize a hostile cell first.
/// Missing is absence; wrong type, length or UTF-8 is never absence.
pub(in crate::session_migration) fn read_text(
    conn: &Connection,
    key: &str,
    expected_bytes: usize,
) -> Result<Option<String>, ImportError> {
    if conn.is_autocommit() {
        let tx = conn.unchecked_transaction().map_err(sqlite_storage_error)?;
        return read_text(&tx, key, expected_bytes);
    }
    let identity: Option<(i64, String)> = conn
        .query_row(
            "SELECT rowid,typeof(value) FROM meta WHERE key=?1",
            [key],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(sqlite_storage_error)?;
    let Some((rowid, kind)) = identity else {
        return Ok(None);
    };
    if kind != "text" {
        return Err(invalid("retention metadata must be text"));
    }
    let mut blob = conn
        .blob_open(DatabaseName::Main, "meta", "value", rowid, true)
        .map_err(sqlite_storage_error)?;
    if blob.len() != expected_bytes {
        return Err(invalid("retention metadata byte length is invalid"));
    }
    let mut bytes = vec![0; expected_bytes];
    blob.read_exact(&mut bytes)
        .map_err(|_| invalid("retention metadata could not be read"))?;
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|_| invalid("retention metadata UTF-8 is invalid"))
}

pub(super) fn initialize_new(conn: &Connection) -> Result<(), ImportError> {
    conn.execute(
        "INSERT INTO meta(key,value) VALUES(?1,?2)",
        [KEY, &uuid::Uuid::new_v4().to_string()],
    )
    .map_err(sqlite_storage_error)?;
    Ok(())
}

pub(super) fn ensure(conn: &Connection) -> Result<(), ImportError> {
    if read(conn)?.is_some() {
        return Ok(());
    }
    // An older supported catalog can initialize on a normal writable open,
    // but never retroactively adopt unresolved recovery from another epoch.
    if super::super::job_control::preview_retention::pending() {
        return Err(ImportError::new(
            ErrorCode::SnapshotUnavailable,
            "retention catalog epoch missing with pending recovery",
        ));
    }
    let tx = Transaction::new_unchecked(conn, TransactionBehavior::Immediate)
        .map_err(sqlite_storage_error)?;
    if read(&tx)?.is_none() {
        initialize_new(&tx)?;
    }
    tx.commit().map_err(sqlite_storage_error)
}
