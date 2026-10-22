//! Stable V3 phase addresses; historical receipt2 JSON stays unchanged.
//! SQL point checks stream the original body with no new ordinary receipt cap.
use super::*;
use crate::session_migration::{
    errors::sqlite_storage_error,
    jobs::{self, Receipt},
};
use rusqlite::{params, Connection, OptionalExtension, Transaction};

pub(super) fn phase_id(allocation: &Allocation, phase: &str) -> Result<String, ImportError> {
    if !matches!(
        phase,
        "prepared" | "staged" | "published" | "linked" | "committed"
    ) {
        return Err(invalid());
    }
    if phase == "prepared" {
        return Ok(allocation.receipt_id.clone());
    }
    let digest = sha256_bytes(
        canonical(&serde_json::json!([
            "session-import-publication-receipt-phase-v1",
            allocation.receipt_id,
            phase
        ]))?
        .as_bytes(),
    );
    let bytes = hex::decode(digest).map_err(|_| invalid())?;
    let mut id = [0u8; 16];
    id.copy_from_slice(&bytes[..16]);
    // Application-defined deterministic UUIDv8, not a new authority token.
    id[6] = (id[6] & 0x0f) | 0x80;
    id[8] = (id[8] & 0x3f) | 0x80;
    Ok(uuid::Uuid::from_bytes(id).to_string())
}

pub(super) fn digest(receipt: &Receipt) -> Result<String, ImportError> {
    Ok(sha256_bytes(canonical(receipt)?.as_bytes()))
}

pub(super) fn read(conn: &Connection, id: &str) -> Result<Option<Receipt>, ImportError> {
    if conn.is_autocommit() {
        return Err(invalid());
    }
    let point: Option<(i64, String)> = conn
        .query_row(
            "SELECT rowid,typeof(json) FROM receipts WHERE receipt_id=?1",
            [id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(sqlite_storage_error)?;
    let Some((rowid, kind)) = point else {
        return Ok(None);
    };
    if kind != "text" {
        return Err(invalid());
    }
    let blob = conn
        .blob_open(
            rusqlite::DatabaseName::Main,
            "receipts",
            "json",
            rowid,
            true,
        )
        .map_err(sqlite_storage_error)?;
    let receipt: Receipt =
        serde_json::from_reader(std::io::BufReader::with_capacity(64 * 1024, blob))
            .map_err(|_| invalid())?;
    jobs::validate_receipt_shape(&receipt)?;
    let consistent: bool = conn.query_row(
        "SELECT receipt_id=?2 AND job_id=?3 AND candidate_id=?4 AND phase=?5 AND import_id IS ?6 AND revision_id IS ?7 FROM receipts WHERE rowid=?1",
        params![rowid, receipt.receipt_id, receipt.job_id, receipt.candidate_id, receipt.phase, receipt.import_id, receipt.revision_id],
        |row| row.get(0),
    ).map_err(sqlite_storage_error)?;
    if !consistent || receipt.receipt_id != id {
        return Err(invalid());
    }
    Ok(Some(receipt))
}

pub(super) fn require_point(conn: &Connection, expected: &Receipt) -> Result<(), ImportError> {
    let current = read(conn, &expected.receipt_id)?.ok_or_else(invalid)?;
    if digest(&current)? != digest(expected)? {
        return Err(invalid());
    }
    Ok(())
}

pub(super) fn insert_once(tx: &Transaction<'_>, expected: &Receipt) -> Result<(), ImportError> {
    match read(tx, &expected.receipt_id)? {
        Some(current) if digest(&current)? == digest(expected)? => Ok(()),
        Some(_) => Err(invalid()),
        None => {
            jobs::write_receipt(tx, expected)?;
            // An INSERT trigger cannot quietly replace the result we sign.
            require_point(tx, expected)
        }
    }
}

pub(super) fn require_allocation(
    allocation: &Allocation,
    binding: &Binding,
    rec: &Receipt,
) -> Result<(), ImportError> {
    if rec.job_id != binding.job_id
        || sha256_bytes(rec.candidate_id.as_bytes()) != binding.candidate_digest
        || rec.receipt_id != phase_id(allocation, &rec.phase)?
        || rec.import_id.as_deref() != Some(allocation.import_id.as_str())
        || rec.revision_id.as_deref() != Some(allocation.revision_id.as_str())
        || rec.previous_revision != allocation.previous_revision
        || rec.created_entry != allocation.created_entry
    {
        return Err(invalid());
    }
    Ok(())
}

pub(super) const PHASES: [&str; 5] = ["prepared", "staged", "published", "linked", "committed"];

/// The five point addresses must form a prefix. A missing middle row is not
/// permission to replay an earlier phase, nor may a foreign append hide it.
pub(super) fn prefix(
    tx: &Transaction<'_>,
    allocation: &Allocation,
    binding: &Binding,
    candidate: &str,
) -> Result<Vec<Receipt>, ImportError> {
    let mut receipts = Vec::new();
    let mut missing = false;
    for phase in PHASES {
        match read(tx, &phase_id(allocation, phase)?)? {
            Some(rec) if !missing => {
                require_allocation(allocation, binding, &rec)?;
                if rec.phase != phase || rec.candidate_id != candidate {
                    return Err(invalid());
                }
                if matches!(phase, "prepared" | "staged" | "published") {
                    native_link::require_planned(allocation, &rec)?;
                }
                if phase == "prepared" && !rec.file_hashes.is_empty() {
                    return Err(invalid());
                }
                if let Some(previous) = receipts.last() {
                    require_successor(previous, &rec)?;
                }
                receipts.push(rec);
            }
            Some(_) => return Err(invalid()),
            None => missing = true,
        }
    }
    let (count, last): (u64, Option<String>) = tx.query_row(
        "SELECT COUNT(*),(SELECT receipt_id FROM receipts WHERE job_id=?1 AND candidate_id=?2 ORDER BY rowid DESC LIMIT 1) FROM receipts WHERE job_id=?1 AND candidate_id=?2",
        params![binding.job_id, candidate], |row| Ok((row.get(0)?, row.get(1)?)),
    ).map_err(sqlite_storage_error)?;
    if count != receipts.len() as u64
        || last.as_deref() != receipts.last().map(|rec| rec.receipt_id.as_str())
    {
        return Err(invalid());
    }
    Ok(receipts)
}

pub(super) fn require_successor(previous: &Receipt, next: &Receipt) -> Result<(), ImportError> {
    let position = PHASES
        .iter()
        .position(|phase| *phase == previous.phase)
        .ok_or_else(invalid)?;
    if PHASES.get(position + 1).copied() != Some(next.phase.as_str()) {
        return Err(invalid());
    }
    let mut expected = previous.clone();
    expected.phase.clone_from(&next.phase);
    expected.receipt_id.clone_from(&next.receipt_id);
    if next.phase == "staged" {
        expected.file_hashes.clone_from(&next.file_hashes);
    }
    if next.phase == "linked" {
        expected
            .mapping_project_id
            .clone_from(&next.mapping_project_id);
        expected.created_project = next.created_project;
        expected.project_decision.clone_from(&next.project_decision);
        expected
            .project_name_at_link
            .clone_from(&next.project_name_at_link);
        expected
            .project_path_at_link
            .clone_from(&next.project_path_at_link);
        expected.project_pinned_at_link = next.project_pinned_at_link;
        expected.project_trusted_at_link = next.project_trusted_at_link;
    }
    if digest(&expected)? != digest(next)? {
        return Err(invalid());
    }
    Ok(())
}
