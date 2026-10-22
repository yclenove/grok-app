//! Bounded revision metadata, never authority to read an archive.

use rusqlite::{params, OptionalExtension};
use serde::Serialize;

use super::catalog;
use super::cursor::{decode_bound_cursor, encode_bound_cursor, CursorScope};
use super::errors::{ErrorCode, ImportError};

pub const REVISION_PAGE_MAX: usize = 50;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RevisionSummary {
    pub revision_id: String,
    pub created_at: String,
    pub outcome: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RevisionPage {
    pub import_id: String,
    pub current_revision: String,
    pub snapshot: String,
    pub items: Vec<RevisionSummary>,
    pub previous_cursor: Option<String>,
    pub next_cursor: Option<String>,
    pub total: u32,
}

fn storage(error: rusqlite::Error) -> ImportError {
    ImportError::new(ErrorCode::SnapshotUnavailable, error.to_string())
}

fn invalid(message: &str) -> ImportError {
    ImportError::new(ErrorCode::SnapshotUnavailable, message)
}

fn valid_revision(id: &str) -> bool {
    id.len() == 36 && uuid::Uuid::parse_str(id).is_ok()
}

pub fn page(
    import_id: &str,
    cursor: Option<&str>,
    limit: Option<usize>,
) -> Result<RevisionPage, ImportError> {
    super::ids::parse_import_uuid(import_id)?;
    if !catalog::catalog_exists()? {
        return Err(ImportError::new(
            ErrorCode::SourceNotFound,
            "import not found",
        ));
    }
    let mut conn = catalog::open_catalog_readonly()?;
    let tx = conn.transaction().map_err(storage)?;
    // SQL bounds text before materializing it, including corrupt local metadata.
    let (hidden, current): (i64, Option<String>) = tx
        .query_row(
            "SELECT hidden, CASE WHEN typeof(current_revision)='text'
               AND length(CAST(current_revision AS BLOB))=36 THEN current_revision END
             FROM entries WHERE import_id=?1",
            [import_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(storage)?
        .ok_or_else(|| ImportError::new(ErrorCode::SourceNotFound, "import not found"))?;
    if hidden != 0 {
        return Err(ImportError::new(
            ErrorCode::HistoryAccessUnavailable,
            "imported history is hidden",
        ));
    }
    let current = current
        .filter(|id| valid_revision(id))
        .ok_or_else(|| invalid("invalid current revision"))?;
    let (count, last_row, current_count): (i64, i64, i64) = tx
        .query_row(
            "SELECT COUNT(*), COALESCE(MAX(rowid),0),
               COALESCE(SUM(CASE WHEN revision_id=?2 THEN 1 ELSE 0 END),0)
             FROM revisions WHERE import_id=?1",
            params![import_id, current],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .map_err(storage)?;
    if current_count != 1 {
        return Err(invalid("current revision is missing"));
    }
    let total = u32::try_from(count).map_err(|_| invalid("invalid revision count"))?;
    let snapshot = super::hash::sha256_bytes(
        format!("revision-list-v1:{current}:{count}:{last_row}").as_bytes(),
    );
    let scope = CursorScope {
        owner: import_id,
        snapshot: &snapshot,
        leaf: "",
        filter: "revision-history-newest",
    };
    let start = match cursor {
        None => 0,
        Some(raw) => decode_bound_cursor(raw, scope)?.offset,
    };
    if start >= total {
        return Err(ImportError::new(
            ErrorCode::PlanStale,
            "revision cursor past end",
        ));
    }
    let cap = limit
        .unwrap_or(REVISION_PAGE_MAX)
        .clamp(1, REVISION_PAGE_MAX);
    let mut items = {
        let mut stmt = tx.prepare(
            "SELECT
               CASE WHEN typeof(revision_id)='text' AND length(CAST(revision_id AS BLOB))=36 THEN revision_id END,
               CASE WHEN typeof(created_at)='text' AND length(CAST(created_at AS BLOB))<=64 THEN created_at END,
               CASE WHEN typeof(outcome)='text' AND length(CAST(outcome AS BLOB))<=32 THEN outcome END
             FROM revisions WHERE import_id=?1
             ORDER BY created_at DESC, revision_id DESC LIMIT ?2 OFFSET ?3",
        ).map_err(storage)?;
        let rows = stmt
            .query_map(params![import_id, (cap + 1) as i64, start], |row| {
                Ok(RevisionSummary {
                    revision_id: row.get(0)?,
                    created_at: row.get(1)?,
                    outcome: row.get(2)?,
                })
            })
            .map_err(storage)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(storage)?
    };
    for item in &items {
        if !valid_revision(&item.revision_id)
            || chrono::DateTime::parse_from_rfc3339(&item.created_at).is_err()
            || !matches!(
                item.outcome.as_str(),
                "complete" | "partial" | "unsupported" | "failed"
            )
        {
            return Err(invalid("invalid revision metadata"));
        }
    }
    let next_cursor = if items.len() > cap {
        items.pop();
        Some(encode_bound_cursor(
            scope,
            start + items.len() as u32,
            None,
        )?)
    } else {
        None
    };
    let previous_cursor = if start > 0 {
        Some(encode_bound_cursor(
            scope,
            start.saturating_sub(cap as u32),
            None,
        )?)
    } else {
        None
    };
    tx.commit().map_err(storage)?;
    Ok(RevisionPage {
        import_id: import_id.to_owned(),
        current_revision: current,
        snapshot,
        items,
        previous_cursor,
        next_cursor,
        total,
    })
}

#[cfg(test)]
#[path = "revision_history_tests.rs"]
mod tests;
