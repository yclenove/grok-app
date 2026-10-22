//! Read-only navigation metadata. Selecting a row still uses the durable job reader.
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use super::{
    catalog,
    cursor::{decode_bound_cursor, encode_bound_cursor, CursorScope},
    errors::{sqlite_storage_error, ErrorCode, ImportError},
    jobs::JobState,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobSummary {
    pub job_id: String,
    pub state: Option<JobState>,
    pub created_at: Option<String>,
    pub item_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobHistoryPage {
    pub items: Vec<JobSummary>,
    pub next_cursor: Option<String>,
}

const SCOPE: CursorScope<'_> = CursorScope {
    owner: "import-jobs",
    snapshot: "history-v1",
    leaf: "",
    filter: "",
};

pub fn page(cursor: Option<&str>, limit: Option<usize>) -> Result<JobHistoryPage, ImportError> {
    let mark = cursor
        .filter(|value| !value.is_empty())
        .map(|raw| {
            let bound = decode_bound_cursor(raw, SCOPE)?;
            let mark: (String, i64) = bound
                .mark
                .as_deref()
                .and_then(|text| serde_json::from_str(text).ok())
                .ok_or_else(|| {
                    ImportError::new(ErrorCode::PlanStale, "invalid job history cursor")
                })?;
            if mark.0.len() > 1024 || mark.1 <= 0 || bound.offset != 0 {
                return Err(ImportError::new(
                    ErrorCode::PlanStale,
                    "invalid job history mark",
                ));
            }
            Ok(mark)
        })
        .transpose()?;
    if !catalog::catalog_exists()? {
        return Ok(JobHistoryPage {
            items: Vec::new(),
            next_cursor: None,
        });
    }
    let conn = catalog::open_catalog_readonly()?;
    let tx = conn.unchecked_transaction().map_err(sqlite_storage_error)?;
    read_page(&tx, mark, super::budget::page_limit(limit))
}

fn read_page(
    conn: &Connection,
    mark: Option<(String, i64)>,
    cap: usize,
) -> Result<JobHistoryPage, ImportError> {
    // Never fetch jobs.json, job_items.result_json, previews, keys or original
    // bodies for this list. SQL columns are display metadata, not a grant.
    // Corrupt optional display metadata stays unknown without hiding other jobs.
    let mut stmt = conn.prepare(
        "WITH headers AS (
            SELECT rowid AS sequence, job_id,
                CASE WHEN typeof(state)='text' AND length(CAST(state AS BLOB))<=32 THEN state ELSE '' END AS display_state,
                CASE WHEN typeof(created_at)='text' AND length(CAST(created_at AS BLOB))<=1024 THEN created_at ELSE '' END AS sort_created
            FROM jobs WHERE typeof(job_id)='text' AND length(CAST(job_id AS BLOB)) BETWEEN 1 AND 256
        )
        SELECT sequence,job_id,display_state,sort_created,
            (SELECT COUNT(*) FROM job_items i WHERE i.job_id=h.job_id)
        FROM headers h WHERE ?1=0 OR sort_created<?2 OR (sort_created=?2 AND sequence<?3)
        ORDER BY sort_created DESC,sequence DESC LIMIT ?4"
    ).map_err(sqlite_storage_error)?;
    let rows = stmt
        .query_map(
            params![
                mark.is_some(),
                mark.as_ref().map(|m| m.0.as_str()).unwrap_or(""),
                mark.as_ref().map(|m| m.1).unwrap_or(0),
                (cap + 1) as i64
            ],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, u32>(4)?,
                ))
            },
        )
        .map_err(sqlite_storage_error)?;
    let mut records = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(sqlite_storage_error)?;
    let next_cursor = if records.len() > cap {
        records.pop();
        records
            .last()
            .map(|row| {
                encode_bound_cursor(
                    SCOPE,
                    0,
                    Some(serde_json::json!([row.3, row.0]).to_string()),
                )
            })
            .transpose()?
    } else {
        None
    };
    let items = records
        .into_iter()
        .map(|(_, job_id, state, created, item_count)| JobSummary {
            job_id,
            state: serde_json::from_value(serde_json::Value::String(state)).ok(),
            created_at: chrono::DateTime::parse_from_rfc3339(&created)
                .ok()
                .map(|_| created),
            item_count,
        })
        .collect();
    Ok(JobHistoryPage { items, next_cursor })
}

#[cfg(test)]
#[path = "job_history_tests.rs"]
mod tests;
