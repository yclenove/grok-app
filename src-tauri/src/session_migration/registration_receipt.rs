//! Bounded exact receipt points for association CAS and historical authorization.
//! This is scoped to receipt points, not a global SQLite allocation guarantee.
use super::{refusal, Receipt};
use crate::session_migration::errors::{sqlite_storage_error, ErrorCode, ImportError};
use rusqlite::{Connection, OptionalExtension};
use std::io::Read;

pub(in crate::session_migration) fn point(
    conn: &Connection,
    receipt: &str,
) -> Result<(i64, Receipt), ImportError> {
    if conn.is_autocommit() {
        let tx = conn.unchecked_transaction().map_err(sqlite_storage_error)?;
        return point(&tx, receipt);
    }
    let row: i64 = conn
        .query_row(
            "SELECT rowid FROM receipts WHERE receipt_id=?1",
            [receipt],
            |row| row.get(0),
        )
        .optional()
        .map_err(sqlite_storage_error)?
        .ok_or_else(refusal)?;
    read_row(conn, row)
}

pub(in crate::session_migration) fn latest(
    conn: &Connection,
    import: &str,
    revision: &str,
) -> Result<Option<Receipt>, ImportError> {
    if conn.is_autocommit() {
        let tx = conn.unchecked_transaction().map_err(sqlite_storage_error)?;
        return latest(&tx, import, revision);
    }
    let row: Option<i64> = conn.query_row(
        "SELECT rowid FROM receipts WHERE import_id=?1 AND revision_id=?2 ORDER BY rowid DESC LIMIT 1",
        rusqlite::params![import, revision], |row| row.get(0)).optional().map_err(sqlite_storage_error)?;
    row.map(|row| read_row(conn, row).map(|(_, receipt)| receipt))
        .transpose()
}

/// Select metadata only, then use the same bounded receipt codec. A damaged
/// association receipt must not allocate its complete SQL TEXT as a shortcut.
pub(in crate::session_migration) fn latest_for_job(
    conn: &Connection,
    job: &str,
    candidate: &str,
) -> Result<Option<Receipt>, ImportError> {
    if conn.is_autocommit() {
        let tx = conn.unchecked_transaction().map_err(sqlite_storage_error)?;
        return latest_for_job(&tx, job, candidate);
    }
    let row: Option<i64> = conn.query_row(
        "SELECT rowid FROM receipts WHERE job_id=?1 AND candidate_id=?2 ORDER BY rowid DESC LIMIT 1",
        rusqlite::params![job, candidate], |row| row.get(0)).optional().map_err(sqlite_storage_error)?;
    row.map(|row| read_row(conn, row).map(|(_, receipt)| receipt))
        .transpose()
}

pub(in crate::session_migration) fn read_row(
    conn: &Connection,
    row: i64,
) -> Result<(i64, Receipt), ImportError> {
    if conn.is_autocommit() {
        let tx = conn.unchecked_transaction().map_err(sqlite_storage_error)?;
        return read_row(&tx, row);
    }
    let text =
        |column: &str, bound: usize, nullable: bool| -> Result<Option<String>, ImportError> {
            // Column names are static application-owned identifiers, never input.
            let kind: String = conn
                .query_row(
                    &format!("SELECT typeof({column}) FROM receipts WHERE rowid=?1"),
                    [row],
                    |row| row.get(0),
                )
                .map_err(sqlite_storage_error)?;
            if nullable && kind == "null" {
                return Ok(None);
            }
            if kind != "text" {
                return Err(refusal());
            }
            let mut blob = conn
                .blob_open(rusqlite::DatabaseName::Main, "receipts", column, row, true)
                .map_err(sqlite_storage_error)?;
            if blob.len() > bound {
                return Err(ImportError::new(
                    ErrorCode::ResourceLimit,
                    "association receipt exceeds byte limit",
                ));
            }
            let mut text = String::with_capacity(blob.len());
            blob.read_to_string(&mut text).map_err(|_| refusal())?;
            Ok(Some(text))
        };
    let json = text("json", 16 * 1024 * 1024, false)?.ok_or_else(refusal)?;
    let receipt: Receipt = serde_json::from_str(&json).map_err(|error| {
        // Preserve the existing receipt codec contract; provenance mismatches
        // are different from malformed or unsupported receipt records.
        ImportError::new(ErrorCode::SchemaUnsupported, error.to_string())
    })?;
    for (column, bound, nullable, expected) in [
        ("receipt_id", 4096, false, Some(receipt.receipt_id.as_str())),
        ("job_id", 4096, false, Some(receipt.job_id.as_str())),
        (
            "candidate_id",
            16384,
            false,
            Some(receipt.candidate_id.as_str()),
        ),
        ("phase", 64, false, Some(receipt.phase.as_str())),
        ("import_id", 4096, true, receipt.import_id.as_deref()),
        ("revision_id", 4096, true, receipt.revision_id.as_deref()),
    ] {
        if text(column, bound, nullable)?.as_deref() != expected {
            return Err(refusal());
        }
    }
    Ok((row, receipt))
}
