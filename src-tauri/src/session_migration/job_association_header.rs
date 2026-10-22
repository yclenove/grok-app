//! Metadata-first association job projection; never a grant or a body rewrite.
use std::io::{BufReader, Read};

use rusqlite::{params, Connection, DatabaseName};
use serde::{de::IgnoredAny, Deserialize};

use super::{stale, STORED_JOB_SQL};
use crate::session_migration::{
    errors::{ErrorCode, ImportError},
    job_control::input_reader::HeaderReader,
    jobs::{JobRecord, JobState},
};

const JOB_BYTES: usize = 16 * 1024 * 1024;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Header {
    job_id: String,
    request_id: String,
    plan_hash: String,
    plan_id: String,
    state: JobState,
    state_version: u32,
    created_at: String,
    #[serde(rename = "items")]
    _items: IgnoredAny,
    #[serde(default)]
    item_count: u32,
    // Mutable display data is explicitly known but never immutable authority.
    // IgnoredAny skips bodies without owning their strings or item metadata.
    #[serde(default, rename = "nextItemCursor")]
    _cursor: Option<IgnoredAny>,
    #[serde(default, rename = "progress")]
    _progress: Option<IgnoredAny>,
}

pub(super) fn parse(reader: impl Read) -> Result<JobRecord, ImportError> {
    let header: Header = serde_json::from_reader(HeaderReader::for_job(reader))
        .map_err(|_| stale("association durable job header is invalid"))?;
    Ok(project(header))
}

fn project(header: Header) -> JobRecord {
    JobRecord {
        job_id: header.job_id,
        request_id: header.request_id,
        plan_hash: header.plan_hash,
        plan_id: header.plan_id,
        state: header.state,
        state_version: header.state_version,
        items: Vec::new(),
        created_at: header.created_at,
        item_count: header.item_count,
        next_item_cursor: None,
        progress: None,
    }
}

pub(super) fn load(conn: &Connection, job_id: &str) -> Result<JobRecord, ImportError> {
    load_with_budget(conn, job_id, Some(JOB_BYTES))
}

/// Same header codec without imposing association's whole-record/request cap
/// on genuine ordinary jobs. IgnoredAny never constructs their JobItem bodies.
pub(super) fn ordinary_metadata(conn: &Connection, job_id: &str) -> Result<JobRecord, ImportError> {
    // Every publication step decodes this header (about 43 times per item).
    // On the fenced worker connection reuse a decode only while the job row is
    // unchanged on this connection and no other connection has committed.
    use crate::session_migration::publication_write_fence::{row_stamp, row_unchanged, RowStamp};
    use rusqlite::OptionalExtension;
    thread_local! {
        static DECODED: std::cell::RefCell<Option<(String, i64, RowStamp, JobRecord)>> =
            const { std::cell::RefCell::new(None) };
    }
    let stamp = row_stamp(conn)?;
    let rowid: Option<i64> = conn
        .query_row("SELECT rowid FROM jobs WHERE job_id=?1", [job_id], |row| {
            row.get(0)
        })
        .optional()
        .map_err(|_| stale("association input has no durable job context"))?;
    if let Some(rowid) = rowid {
        let hit = DECODED.with(|cached| {
            cached
                .borrow()
                .as_ref()
                .filter(|(job, row, _, _)| job == job_id && *row == rowid)
                .map(|(_, _, stamp, record)| (*stamp, record.clone()))
        });
        if let Some((verified, record)) = hit {
            if row_unchanged(conn, &verified, "jobs", rowid)? {
                return Ok(record);
            }
        }
    }
    let job = load_with_budget(conn, job_id, None)?;
    if let (Some(stamp), Some(rowid)) = (stamp, rowid) {
        DECODED
            .with(|cached| *cached.borrow_mut() = Some((job_id.into(), rowid, stamp, job.clone())));
    }
    Ok(job)
}

fn load_with_budget(
    conn: &Connection,
    job_id: &str,
    budget: Option<usize>,
) -> Result<JobRecord, ImportError> {
    // Types, byte lengths, SQL columns and JSON must all see the same row.
    // Reuse a caller's snapshot/writer without committing or replacing it.
    if conn.is_autocommit() {
        let tx = conn
            .unchecked_transaction()
            .map_err(|_| stale("association job snapshot is unavailable"))?;
        return load_with_budget(&tx, job_id, budget);
    }
    let (row_id, types): (i64, [String; 6]) = conn
        .query_row(STORED_JOB_SQL, [job_id], |row| {
            Ok((
                row.get(0)?,
                [
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                ],
            ))
        })
        .map_err(|_| stale("association input has no durable job context"))?;
    if types != ["text", "text", "text", "integer", "text", "text"] {
        return Err(stale("association durable job column types are invalid"));
    }
    // Incremental BLOB metadata checks TEXT byte lengths without a SQL CAST,
    // JSON function or full owned body. Never open a writable BLOB handle.
    for (column, bound) in [
        ("json", budget),
        ("request_id", budget),
        ("plan_hash", Some(64)),
        ("state", Some(32)),
        ("created_at", Some(1024)),
    ] {
        let blob = conn
            .blob_open(DatabaseName::Main, "jobs", column, row_id, true)
            .map_err(|_| stale("association durable job bytes are unavailable"))?;
        if bound.is_some_and(|bound| blob.len() > bound) {
            return Err(ImportError::new(
                ErrorCode::ResourceLimit,
                "association durable job exceeds record budget",
            ));
        }
    }
    let (request, hash, state, version, created): (String, String, String, u32, String) = conn
        .query_row(
            "SELECT request_id,plan_hash,state,state_version,created_at
             FROM jobs WHERE job_id=?1 AND rowid=?2",
            params![job_id, row_id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .map_err(|_| stale("association durable job columns are invalid"))?;
    let blob = conn
        .blob_open(DatabaseName::Main, "jobs", "json", row_id, true)
        .map_err(|_| stale("association durable job bytes are unavailable"))?;
    #[cfg(test)]
    let _codec =
        crate::session_migration::codec_profile::Span::start("job-header-decode", blob.len());
    let reader = BufReader::with_capacity(64 * 1024, blob);
    let job = if budget.is_some() {
        parse(reader)?
    } else {
        project(
            serde_json::from_reader(HeaderReader::new(reader))
                .map_err(|_| stale("ordinary durable job header is invalid"))?,
        )
    };
    if job.job_id != job_id
        || job.request_id != request
        || job.plan_hash != hash
        || job.state.as_str() != state
        || job.state_version != version
        || job.created_at != created
    {
        return Err(stale("association durable job row/header disagree"));
    }
    Ok(job)
}
