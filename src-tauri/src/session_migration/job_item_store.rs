//! Mutable display rows, separate from authenticated original job_items input.
//! Only new acceptance opts in; old embedded jobs remain readable without DDL.
use rusqlite::{params, Connection, OptionalExtension};
use serde::{de::DeserializeOwned, Deserialize};

use super::JobRecord;
use crate::session_migration::errors::{sqlite_storage_error, ErrorCode, ImportError};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Identity {
    candidate_id: String,
}

const MAX_CANDIDATE_BYTES: usize = 16384;

fn candidate_bytes<'a>(value: rusqlite::types::ValueRef<'a>) -> Result<&'a [u8], ImportError> {
    match value {
        rusqlite::types::ValueRef::Text(bytes) if bytes.len() <= MAX_CANDIDATE_BYTES => Ok(bytes),
        _ => Err(invalid()),
    }
}

fn require_candidate(json: &str, candidate: &str) -> Result<(), ImportError> {
    let item: Identity = serde_json::from_str(json).map_err(|_| invalid())?;
    if candidate != item.candidate_id {
        return Err(invalid());
    }
    Ok(())
}

pub(in crate::session_migration) fn invalid() -> ImportError {
    ImportError::new(
        ErrorCode::SnapshotUnavailable,
        "invalid durable job item state",
    )
}

pub(in crate::session_migration) fn ensure(conn: &Connection) -> Result<(), ImportError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS job_item_states (
           job_id TEXT NOT NULL,
           item_index INTEGER NOT NULL CHECK(item_index>=0),
           candidate_id TEXT NOT NULL,
           json TEXT NOT NULL,
           PRIMARY KEY(job_id,item_index)
         );
         CREATE INDEX IF NOT EXISTS job_item_states_candidate
           ON job_item_states(job_id,candidate_id);",
    )
    .map_err(sqlite_storage_error)
}

pub(in crate::session_migration) fn uses_rows(
    conn: &Connection,
    job: &str,
) -> Result<bool, ImportError> {
    let storage: Option<(Option<String>, String, Option<u32>)> = conn
        .query_row(
            "SELECT json_extract(json,'$.itemStorage'),json_type(json,'$.items'),
                    json_array_length(json,'$.items') FROM jobs WHERE job_id=?1",
            [job],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()
        .map_err(sqlite_storage_error)?;
    match storage {
        None | Some((None, _, _)) => {
            // A downgrade writer can discard the unknown header marker. Never
            // reinterpret its surviving display rows as an empty embedded job.
            let table: bool = conn
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='job_item_states')",
                    [], |r| r.get(0),
                )
                .map_err(sqlite_storage_error)?;
            if table {
                let orphaned: bool = conn
                    .query_row(
                        "SELECT EXISTS(SELECT 1 FROM job_item_states WHERE job_id=?1)",
                        [job],
                        |r| r.get(0),
                    )
                    .map_err(sqlite_storage_error)?;
                if orphaned {
                    // Conflicting layouts cannot establish Undo ownership.
                    return Err(ImportError::new(
                        ErrorCode::HistoryAccessUnavailable,
                        "durable job item storage conflicts with its header",
                    ));
                }
            }
            Ok(false)
        }
        Some((Some(storage), kind, Some(0))) if storage == "rows-v1" && kind == "array" => Ok(true),
        _ => Err(invalid()),
    }
}

pub(in crate::session_migration) fn encode_header(
    job: &JobRecord,
    rows: bool,
) -> Result<String, ImportError> {
    if !rows {
        return serde_json::to_string(job).map_err(|_| invalid());
    }
    // Do not serialize or clone the full item vector even temporarily.
    let mut value = serde_json::to_value(job_header(job)).map_err(|_| invalid())?;
    value["itemStorage"] = "rows-v1".into();
    serde_json::to_string(&value).map_err(|_| invalid())
}

fn job_header(job: &JobRecord) -> JobRecord {
    JobRecord {
        job_id: job.job_id.clone(),
        request_id: job.request_id.clone(),
        plan_hash: job.plan_hash.clone(),
        plan_id: job.plan_id.clone(),
        state: job.state,
        state_version: job.state_version,
        items: Vec::new(),
        created_at: job.created_at.clone(),
        item_count: job.item_count,
        next_item_cursor: None,
        progress: None,
    }
}

pub(in crate::session_migration) fn save_new(
    tx: &rusqlite::Transaction<'_>,
    job: &JobRecord,
) -> Result<(), ImportError> {
    super::save_job_header(tx, job, true)?;
    save_items(tx, job)
}

pub(super) fn save_items(conn: &Connection, job: &JobRecord) -> Result<(), ImportError> {
    conn.execute(
        "DELETE FROM job_item_states WHERE job_id=?1 AND item_index>=?2",
        params![job.job_id, job.items.len() as i64],
    )
    .map_err(sqlite_storage_error)?;
    for (index, item) in job.items.iter().enumerate() {
        conn.execute(
            "INSERT INTO job_item_states(job_id,item_index,candidate_id,json) VALUES(?1,?2,?3,?4)
             ON CONFLICT(job_id,item_index) DO UPDATE SET candidate_id=excluded.candidate_id,json=excluded.json",
            params![job.job_id, index as i64, item.candidate_id, serde_json::to_string(item).map_err(|_| invalid())?],
        ).map_err(sqlite_storage_error)?;
    }
    Ok(())
}

/// Used by action journals to reject trigger writes as well as missing rows.
pub(in crate::session_migration) fn save_changes(
    conn: &Connection,
    job: &JobRecord,
) -> Result<u64, ImportError> {
    Ok(1 + job.items.len() as u64 * if uses_rows(conn, &job.job_id)? { 2 } else { 1 })
}

pub(in crate::session_migration) fn item_json(
    conn: &Connection,
    job: &str,
    index: usize,
) -> Result<String, ImportError> {
    if uses_rows(conn, job)? {
        let mut statement = conn
            .prepare(
                "SELECT candidate_id,json FROM job_item_states WHERE job_id=?1 AND item_index=?2",
            )
            .map_err(sqlite_storage_error)?;
        let mut stored = statement
            .query(params![job, index as i64])
            .map_err(sqlite_storage_error)?;
        let row = stored
            .next()
            .map_err(sqlite_storage_error)?
            .ok_or_else(invalid)?;
        let candidate = candidate_bytes(row.get_ref(0).map_err(sqlite_storage_error)?)?;
        let candidate = std::str::from_utf8(candidate).map_err(|_| invalid())?;
        let json: String = row.get(1).map_err(sqlite_storage_error)?;
        require_candidate(&json, candidate)?;
        Ok(json)
    } else {
        conn.query_row(
            "SELECT json_extract(json,?2) FROM jobs WHERE job_id=?1",
            params![job, format!("$.items[{index}]")],
            |r| r.get(0),
        )
        .map_err(sqlite_storage_error)
    }
}

pub(in crate::session_migration) fn index(
    conn: &Connection,
    job: &str,
    candidate: &str,
) -> Result<usize, ImportError> {
    let sql = if uses_rows(conn, job)? {
        "SELECT MIN(item_index),COUNT(*) FROM job_item_states WHERE job_id=?1 AND candidate_id=?2"
    } else {
        "SELECT MIN(CAST(i.key AS INTEGER)),COUNT(*) FROM jobs j,json_each(j.json,'$.items') i
         WHERE j.job_id=?1 AND json_type(j.json,'$.items')='array' AND json_extract(i.value,'$.candidateId')=?2"
    };
    let (index, count): (Option<usize>, u64) = conn
        .query_row(sql, params![job, candidate], |r| Ok((r.get(0)?, r.get(1)?)))
        .map_err(sqlite_storage_error)?;
    if count != 1 {
        return Err(invalid());
    }
    index.ok_or_else(invalid)
}

pub(in crate::session_migration) fn aggregate(
    conn: &Connection,
    job: &str,
) -> Result<(u32, u32, u32), ImportError> {
    let source = if uses_rows(conn, job)? {
        "SELECT json AS value FROM job_item_states WHERE job_id=?1"
    } else {
        "SELECT value FROM json_each((SELECT json FROM jobs WHERE job_id=?1),'$.items')"
    };
    conn.query_row(&format!(
        "SELECT COUNT(*),
         COALESCE(SUM(CASE WHEN json_extract(value,'$.phase') IN ('committed','failed','cancelled','tombstoned')
            OR COALESCE(json_extract(value,'$.importId'),'')!='' THEN 1 ELSE 0 END),0),
         COALESCE(SUM(CASE WHEN json_extract(value,'$.phase') IN ('failed','cancelled','recovery_needed')
            THEN 1 ELSE 0 END),0) FROM ({source})"),
        [job], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    ).map_err(sqlite_storage_error)
}

pub(in crate::session_migration) fn page<T: DeserializeOwned>(
    conn: &Connection,
    job: &str,
    start: usize,
    limit: usize,
) -> Result<Vec<T>, ImportError> {
    let rows = uses_rows(conn, job)?;
    let sql = if rows {
        "SELECT item_index,candidate_id,json FROM job_item_states
         WHERE job_id=?1 AND item_index>=?2 ORDER BY item_index LIMIT ?3"
    } else {
        "SELECT CAST(key AS INTEGER),json_extract(value,'$.candidateId'),value
         FROM json_each((SELECT json FROM jobs WHERE job_id=?1),'$.items')
         WHERE CAST(key AS INTEGER)>=?2 ORDER BY CAST(key AS INTEGER) LIMIT ?3"
    };
    let mut stmt = conn.prepare(sql).map_err(sqlite_storage_error)?;
    let mut items = stmt
        .query(params![
            job,
            start as i64,
            limit.min(i64::MAX as usize) as i64
        ])
        .map_err(sqlite_storage_error)?;
    let mut page = Vec::new();
    while let Some(item) = items.next().map_err(sqlite_storage_error)? {
        let index: usize = item.get(0).map_err(sqlite_storage_error)?;
        // Borrow and bound the candidate column before owning either string.
        let candidate = candidate_bytes(item.get_ref(1).map_err(sqlite_storage_error)?)?;
        let candidate = std::str::from_utf8(candidate).map_err(|_| invalid())?;
        let json: String = item.get(2).map_err(sqlite_storage_error)?;
        if index != start + page.len() {
            return Err(invalid());
        }
        require_candidate(&json, candidate)?;
        page.push(serde_json::from_str(&json).map_err(|_| invalid())?);
    }
    Ok(page)
}

/// Association's old whole-record cap still covers the split display bytes.
pub(in crate::session_migration) fn require_budget(
    conn: &Connection,
    job: &str,
    budget: usize,
) -> Result<(), ImportError> {
    if uses_rows(conn, job)? {
        let row: i64 = conn
            .query_row("SELECT rowid FROM jobs WHERE job_id=?1", [job], |r| {
                r.get(0)
            })
            .map_err(sqlite_storage_error)?;
        let mut bytes = conn
            .blob_open(rusqlite::DatabaseName::Main, "jobs", "json", row, true)
            .map_err(sqlite_storage_error)?
            .len();
        if bytes > budget {
            return Err(budget_exceeded());
        }
        let mut stmt = conn
            .prepare(
                "SELECT typeof(candidate_id),length(CAST(candidate_id AS BLOB)),
                        typeof(json),length(CAST(json AS BLOB))
                 FROM job_item_states WHERE job_id=?1",
            )
            .map_err(sqlite_storage_error)?;
        let mut items = stmt.query([job]).map_err(sqlite_storage_error)?;
        while let Some(item) = items.next().map_err(sqlite_storage_error)? {
            let candidate_kind: String = item.get(0).map_err(sqlite_storage_error)?;
            let candidate_len: usize = item.get(1).map_err(sqlite_storage_error)?;
            let json_kind: String = item.get(2).map_err(sqlite_storage_error)?;
            let json_len: usize = item.get(3).map_err(sqlite_storage_error)?;
            if candidate_kind != "text" || json_kind != "text" {
                return Err(invalid());
            }
            bytes = bytes
                .saturating_add(candidate_len)
                .saturating_add(json_len)
                .saturating_add(1);
            if candidate_len > MAX_CANDIDATE_BYTES || bytes > budget {
                return Err(budget_exceeded());
            }
        }
    }
    Ok(())
}

fn budget_exceeded() -> ImportError {
    ImportError::new(
        ErrorCode::ResourceLimit,
        "association durable job exceeds record budget",
    )
}
