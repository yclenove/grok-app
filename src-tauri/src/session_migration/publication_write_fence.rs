//! In-transaction write fence for the publication worker connection.
//!
//! The worker owns its catalog connection and holds the write lock inside each
//! publication transaction, so another row can only change during that
//! transaction through SQL the transaction itself runs (for example a trigger
//! fired by a receipt insert). The fence records every row the current
//! transaction changes (update hook; cleared on commit and rollback) and lets
//! a per-candidate owner check refuse when another item's original input or
//! this job's signed acceptance rows changed. Changes made outside the
//! transaction are proved by each item's own input check and by the complete
//! snapshot when the job starts and before it turns terminal.
//!
//! The same hook also stamps every row change with a sequence number that
//! survives commits. Together with SQLite's `data_version` (which changes when
//! any other connection commits) a `RowStamp` proves a row is byte-for-byte
//! what it was when a caller verified it, so an immutable verified fact can be
//! reused without re-reading the row.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use rusqlite::hooks::Action;
use rusqlite::{Connection, OptionalExtension};

use crate::session_migration::errors::{sqlite_storage_error, ErrorCode, ImportError};

#[derive(Default)]
struct State {
    generation: u64,
    /// Rows changed by the open transaction (cleared on commit/rollback).
    open: Vec<(String, i64)>,
    sequence: u64,
    /// Last change sequence per row on this connection, across transactions.
    last: HashMap<(String, i64), u64>,
}

type Changes = Arc<Mutex<State>>;

fn lock(state: &Changes) -> std::sync::MutexGuard<'_, State> {
    state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn fences() -> &'static Mutex<HashMap<usize, Changes>> {
    static FENCES: OnceLock<Mutex<HashMap<usize, Changes>>> = OnceLock::new();
    FENCES.get_or_init(Default::default)
}

/// Transactions dereference to the same `Connection`, so its address names
/// the connection for as long as the installation guard borrows it.
fn key(conn: &Connection) -> usize {
    conn as *const Connection as usize
}

pub(in crate::session_migration) struct Installed<'connection> {
    conn: &'connection Connection,
}

static GENERATION: AtomicU64 = AtomicU64::new(1);
/// Bound the per-row map; exceeding it starts a new generation, which only
/// makes every earlier stamp stale (a later re-verification), never valid.
const MAX_TRACKED_ROWS: usize = 1_000_000;

pub(in crate::session_migration) fn install(conn: &Connection) -> Installed<'_> {
    let changes: Changes = Arc::new(Mutex::new(State {
        generation: GENERATION.fetch_add(1, Ordering::Relaxed),
        ..State::default()
    }));
    let (updated, committed, rolled_back) = (changes.clone(), changes.clone(), changes.clone());
    conn.update_hook(Some(
        move |_action: Action, _db: &str, table: &str, rowid: i64| {
            let mut state = lock(&updated);
            if state.last.len() >= MAX_TRACKED_ROWS {
                state.last.clear();
                state.generation = GENERATION.fetch_add(1, Ordering::Relaxed);
            }
            state.sequence += 1;
            let sequence = state.sequence;
            state.last.insert((table.to_owned(), rowid), sequence);
            state.open.push((table.to_owned(), rowid));
        },
    ));
    conn.commit_hook(Some(move || {
        lock(&committed).open.clear();
        false
    }));
    conn.rollback_hook(Some(move || {
        lock(&rolled_back).open.clear();
    }));
    fences()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .insert(key(conn), changes);
    Installed { conn }
}

impl Drop for Installed<'_> {
    fn drop(&mut self) {
        fences()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(&key(self.conn));
        self.conn.update_hook(None::<fn(Action, &str, &str, i64)>);
        self.conn.commit_hook(None::<fn() -> bool>);
        self.conn.rollback_hook(None::<fn()>);
    }
}

fn foreign() -> ImportError {
    ImportError::new(
        ErrorCode::HistoryAccessUnavailable,
        "another item's input or the signed acceptance changed inside this publication",
    )
}

/// `Ok(true)`: this connection is fenced and the open transaction changed no
/// other item's input and no acceptance row of `job_id`. `Ok(false)`: no fence,
/// or a deleted row whose key cannot be classified; the caller must run the
/// complete snapshot check instead.
pub(in crate::session_migration) fn check(
    conn: &Connection,
    job_id: &str,
    candidate: &str,
) -> Result<bool, ImportError> {
    let Some(changes) = fences()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(&key(conn))
        .cloned()
    else {
        return Ok(false);
    };
    let seen = lock(&changes).open.clone();
    if seen.is_empty() {
        return Ok(true);
    }
    let header = format!("publication_acceptance:v1:{job_id}:header");
    let members = format!("publication_acceptance:v1:{job_id}:member");
    for (table, rowid) in seen {
        match table.as_str() {
            "job_items" => {
                // Other jobs' rows may legitimately change (deduplication,
                // re-import); only this job's other items are fenced.
                let row: Option<(String, String)> = conn
                    .query_row(
                        "SELECT job_id,candidate_id FROM job_items WHERE rowid=?1",
                        [rowid],
                        |row| Ok((row.get(0)?, row.get(1)?)),
                    )
                    .optional()
                    .map_err(sqlite_storage_error)?;
                match row {
                    None => return Ok(false),
                    Some((job, item)) if job == job_id && item != candidate => {
                        return Err(foreign())
                    }
                    Some(_) => {}
                }
            }
            "meta" => {
                let key: Option<String> = conn
                    .query_row("SELECT key FROM meta WHERE rowid=?1", [rowid], |row| {
                        row.get(0)
                    })
                    .optional()
                    .map_err(sqlite_storage_error)?;
                match key {
                    None => return Ok(false),
                    Some(key) if key == header || key.starts_with(&members) => {
                        return Err(foreign())
                    }
                    Some(_) => {}
                }
            }
            _ => {}
        }
    }
    Ok(true)
}

/// Proof point for an immutable verified row: install generation, change
/// sequence and the database's `data_version` when the caller started reading.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::session_migration) struct RowStamp {
    generation: u64,
    sequence: u64,
    data_version: i64,
}

fn data_version(conn: &Connection) -> Result<i64, ImportError> {
    conn.query_row("PRAGMA data_version", [], |row| row.get(0))
        .map_err(sqlite_storage_error)
}

fn state(conn: &Connection) -> Option<Changes> {
    fences()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(&key(conn))
        .cloned()
}

/// Take BEFORE reading the row to be verified. `None` on unfenced connections.
pub(in crate::session_migration) fn row_stamp(
    conn: &Connection,
) -> Result<Option<RowStamp>, ImportError> {
    let Some(state) = state(conn) else {
        return Ok(None);
    };
    let (generation, sequence) = {
        let state = lock(&state);
        (state.generation, state.sequence)
    };
    Ok(Some(RowStamp {
        generation,
        sequence,
        data_version: data_version(conn)?,
    }))
}

/// True only when this fenced connection has not changed `table`/`rowid` since
/// the stamp and no other connection has committed anything since then.
pub(in crate::session_migration) fn row_unchanged(
    conn: &Connection,
    stamp: &RowStamp,
    table: &str,
    rowid: i64,
) -> Result<bool, ImportError> {
    let Some(state) = state(conn) else {
        return Ok(false);
    };
    {
        let state = lock(&state);
        if state.generation != stamp.generation
            || state
                .last
                .get(&(table.to_owned(), rowid))
                .is_some_and(|changed| *changed > stamp.sequence)
        {
            return Ok(false);
        }
    }
    Ok(data_version(conn)? == stamp.data_version)
}

#[cfg(test)]
#[path = "publication_write_fence_tests.rs"]
mod tests;
