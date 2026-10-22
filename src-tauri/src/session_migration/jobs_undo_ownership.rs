//! Shared Undo result boundary. Database metadata only: never reopen a disposed
//! preview, source, archive, or native project store while holding this writer.
use super::{CatalogEntry, Connection, ErrorCode, ImportError, JobItem, Receipt};
use crate::session_migration::{catalog, registration_provenance};

fn refusal() -> ImportError {
    ImportError::new(
        ErrorCode::HistoryAccessUnavailable,
        "Undo result does not belong to its committed target",
    )
}

pub(super) fn require_target(
    conn: &Connection,
    job_id: &str,
    item: &JobItem,
    entry: &CatalogEntry,
    receipt: &Receipt,
) -> Result<(), ImportError> {
    if conn.is_autocommit()
        || receipt.phase != "committed"
        || receipt.job_id != job_id
        || receipt.candidate_id != item.candidate_id
        || receipt.import_id != item.import_id
        || receipt.revision_id != item.revision_id
        || receipt.mapping_project_id != item.mapping_project_id
        || receipt.import_id.as_deref() != Some(entry.import_id.as_str())
    {
        return Err(refusal());
    }
    // Adoption legitimately changes the catalog's source tuple, not historical
    // receipt bytes. Require the existing exact accepted-provenance bridge;
    // neither a matching import UUID nor a parsed proof alone authorizes it.
    registration_provenance::authorize(conn, entry, receipt)?;
    let revision_id = receipt.revision_id.as_deref().ok_or_else(refusal)?;
    let revision =
        catalog::get_revision(conn, &entry.import_id, revision_id)?.ok_or_else(refusal)?;
    if revision.source_snapshot_id != receipt.snapshot_hash {
        return Err(refusal());
    }
    if let Some(previous) = receipt.previous_revision.as_deref() {
        // UUID existence alone is insufficient: the previous revision must be
        // owned by this same import, not another genuinely published entry.
        if receipt.created_entry
            || previous == revision_id
            || catalog::get_revision(conn, &entry.import_id, previous)?.is_none()
        {
            return Err(refusal());
        }
    }
    Ok(())
}
