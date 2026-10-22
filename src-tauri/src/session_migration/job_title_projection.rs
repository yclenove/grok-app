//! Read-only display metadata for legacy job pages without projected titles.

use rusqlite::{params, Connection};

use super::errors::{sqlite_storage_error, ErrorCode, ImportError};
use super::jobs::JobItem;

const TITLE_SQL: &str = "SELECT item.candidate_id,
    CASE WHEN json_valid(item.result_json)
         THEN COALESCE(json_extract(item.result_json, '$.adapter.graph.title'),
                       json_extract(item.result_json, '$.adapter.graph.nativeSessionId'))
         ELSE NULL END
    FROM json_each(?2) AS requested
    CROSS JOIN job_items AS item
      ON item.job_id=?1 AND item.candidate_id=requested.value";

fn unavailable(error: impl std::fmt::Display) -> ImportError {
    ImportError::new(ErrorCode::SnapshotUnavailable, error.to_string())
}

pub(super) fn hydrate(
    conn: &Connection,
    job_id: &str,
    page: &mut [JobItem],
) -> Result<(), ImportError> {
    // Current jobs already project their display titles. Do not read/parse
    // durable adapter bodies for those rows on every progress refresh.
    let missing = page
        .iter()
        .filter(|item| item.title.is_none())
        .map(|item| item.candidate_id.as_str())
        .collect::<Vec<_>>();
    if missing.is_empty() {
        return Ok(());
    }
    let ids = serde_json::to_string(&missing).map_err(unavailable)?;
    // CROSS JOIN fixes the small requested page as the outer loop. Otherwise
    // SQLite can scan every item in the batch against every requested ID.
    let mut stmt = conn.prepare(TITLE_SQL).map_err(sqlite_storage_error)?;
    let rows = stmt
        .query_map(params![job_id, ids], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
        })
        .map_err(sqlite_storage_error)?;
    let mut titles = std::collections::HashMap::new();
    for row in rows {
        let (candidate_id, title) = row.map_err(sqlite_storage_error)?;
        if let Some(title) = title.filter(|title| !title.is_empty()) {
            titles.insert(candidate_id, title);
        }
    }
    for item in page {
        if item.title.is_none() {
            item.title = titles.remove(&item.candidate_id);
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "job_title_projection_tests.rs"]
mod tests;
