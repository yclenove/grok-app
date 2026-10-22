//! In-transaction write fence for the publication worker connection.
//!
//! The worker owns its catalog connection and holds the write lock inside each
//! publication transaction, so another row can only change during that
//! transaction through SQL the transaction itself runs (for example a trigger
//! fired by a receipt insert). The fence records every row the current
//! transaction changes (update hook; cleared on commit and rollback) and lets
//! a per-candidate owner check refuse when another item's input/display state,
//! this job's signed acceptance, or an unapproved job header changed. Changes
//! made outside the transaction are proved by each item's own input check and
//! by the complete snapshot when the job starts and before it turns terminal.
//!
//! The same hook also stamps every row change with a sequence number that
//! survives commits. Together with SQLite's `data_version` (which changes when
//! any other connection commits) a `RowStamp` proves a row is byte-for-byte
//! what it was when a caller verified it, so an immutable verified fact can be
//! reused without re-reading the row.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use rusqlite::hooks::{Action, AuthAction, AuthContext, Authorization};
use rusqlite::{Connection, OptionalExtension};

use crate::session_migration::errors::{sqlite_storage_error, ErrorCode, ImportError};

#[derive(Default)]
struct State {
    generation: u64,
    /// Rows changed by the open transaction (cleared on commit/rollback).
    open: Vec<(String, i64, u64)>,
    /// Exact hook events emitted by the worker's own progress-header UPDATE.
    job_acks: Vec<u64>,
    selected: HashMap<(String, String), SelectedRows>,
    sequence: u64,
    /// Last change sequence per row on this connection, across transactions.
    last: HashMap<(String, i64), u64>,
}

type Changes = Arc<Mutex<State>>;

#[derive(Clone, Copy)]
struct SelectedRows {
    input: Option<i64>,
    display: Option<(i64, i64)>,
}

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
    // SQLITE_IGNORE on DELETE preserves the deletion but disables SQLite's
    // truncate optimization, which would otherwise bypass the update hook.
    conn.authorizer(Some(|context: AuthContext<'_>| match context.action {
        AuthAction::Delete {
            table_name: "jobs" | "job_items" | "job_item_states" | "meta",
        } => Authorization::Ignore,
        _ => Authorization::Allow,
    }));
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
            state.open.push((table.to_owned(), rowid, sequence));
        },
    ));
    conn.commit_hook(Some(move || {
        let mut state = lock(&committed);
        state.open.clear();
        state.job_acks.clear();
        state.selected.clear();
        false
    }));
    conn.rollback_hook(Some(move || {
        let mut state = lock(&rolled_back);
        state.open.clear();
        state.job_acks.clear();
        state.selected.clear();
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
        self.conn
            .authorizer(None::<fn(AuthContext<'_>) -> Authorization>);
    }
}

fn foreign() -> ImportError {
    ImportError::new(
        ErrorCode::HistoryAccessUnavailable,
        "another item's state or the job's authenticated context changed inside this publication",
    )
}

/// Capture only the selected row identities before publication writes. A
/// neighbor moved onto our candidate/slot cannot become our authorized row.
pub(in crate::session_migration) fn capture_candidate(
    conn: &Connection,
    job_id: &str,
    candidate: &str,
) -> Result<(), ImportError> {
    let Some(state) = state(conn) else {
        return Ok(());
    };
    if conn.is_autocommit() {
        return Err(foreign());
    }
    let key = (job_id.to_owned(), candidate.to_owned());
    {
        let state = lock(&state);
        if state.selected.contains_key(&key) {
            return Ok(());
        }
        if state
            .open
            .iter()
            .any(|(table, _, _)| matches!(table.as_str(), "jobs" | "job_items" | "job_item_states"))
        {
            return Err(foreign());
        }
    }
    let input: Option<i64> = conn
        .query_row(
            "SELECT rowid FROM job_items WHERE job_id=?1 AND candidate_id=?2",
            rusqlite::params![job_id, candidate],
            |r| r.get(0),
        )
        .optional()
        .map_err(sqlite_storage_error)?;
    let table: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='job_item_states')",
            [], |r| r.get(0),
        )
        .map_err(sqlite_storage_error)?;
    let display: Option<(i64, i64)> = if table {
        conn.query_row(
            "SELECT rowid,item_index FROM job_item_states WHERE job_id=?1 AND candidate_id=?2",
            rusqlite::params![job_id, candidate],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(sqlite_storage_error)?
    } else {
        None
    };
    lock(&state)
        .selected
        .insert(key, SelectedRows { input, display });
    Ok(())
}

/// `Ok(true)`: this connection is fenced and the open transaction changed no
/// other item's input/display state and no acceptance row of `job_id`.
/// `Ok(false)`: no fence, or an input/meta row whose old key cannot be
/// classified; the caller must run the complete snapshot check instead.
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
    if !conn.is_autocommit() {
        capture_candidate(conn, job_id, candidate)?;
    }
    let (seen, job_acks, selected) = {
        let state = lock(&changes);
        (
            state.open.clone(),
            state.job_acks.clone(),
            state
                .selected
                .get(&(job_id.to_owned(), candidate.to_owned()))
                .copied(),
        )
    };
    if seen.is_empty() {
        return Ok(true);
    }
    let header = format!("publication_acceptance:v1:{job_id}:header");
    let members = format!("publication_acceptance:v1:{job_id}:member");
    let mut complete_snapshot = false;
    let mut own_writes = HashSet::new();
    for (table, rowid, sequence) in seen {
        match table.as_str() {
            // Only the worker's one explicitly bracketed ACK may write a job
            // header. A trigger cannot switch layouts or forge embedded peers.
            "jobs" if !job_acks.contains(&sequence) => return Err(foreign()),
            "job_items" | "job_item_states" => {
                // A post-image in another job may be a row moved out of this
                // one. It requires complete membership validation, after ALL
                // hook events have been checked for foreign display writes.
                let sql = if table == "job_items" {
                    "SELECT job_id,candidate_id,0 FROM job_items WHERE rowid=?1"
                } else {
                    "SELECT job_id,candidate_id,item_index FROM job_item_states WHERE rowid=?1"
                };
                let row: Option<(String, String, i64)> = conn
                    .query_row(sql, [rowid], |row| {
                        Ok((row.get(0)?, row.get(1)?, row.get(2)?))
                    })
                    .optional()
                    .map_err(sqlite_storage_error)?;
                match row {
                    None if table == "job_item_states" => return Err(foreign()),
                    None => complete_snapshot = true,
                    Some((job, item, _)) if job == job_id && item != candidate => {
                        return Err(foreign())
                    }
                    Some((job, item, index)) if job == job_id && item == candidate => {
                        let selected = selected.ok_or_else(foreign)?;
                        let expected = if table == "job_items" {
                            selected.input.map(|row| (row, 0))
                        } else {
                            selected.display
                        };
                        if expected != Some((rowid, index)) || !own_writes.insert(table.clone()) {
                            return Err(foreign());
                        }
                        if table == "job_items" {
                            continue;
                        }
                        // The display candidate index is not a UNIQUE input
                        // key. A trigger cannot disguise a neighbor as our row.
                        let count: u64 = conn.query_row(
                            "SELECT COUNT(*) FROM job_item_states WHERE job_id=?1 AND candidate_id=?2",
                            rusqlite::params![job_id, candidate], |r| r.get(0),
                        ).map_err(sqlite_storage_error)?;
                        if count != 1 {
                            return Err(foreign());
                        }
                    }
                    Some((job, _, _)) if job != job_id => complete_snapshot = true,
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
                    None => complete_snapshot = true,
                    Some(key) if key == header || key.starts_with(&members) => {
                        return Err(foreign())
                    }
                    Some(_) => {}
                }
            }
            _ => {}
        }
    }
    Ok(!complete_snapshot)
}

/// Called immediately after the known progress-header UPDATE, using a stamp
/// taken immediately before it. Extra header events (including trigger writes
/// that restore the original bytes) are never whitelisted.
pub(in crate::session_migration) fn acknowledge_job_write(
    conn: &Connection,
    job_id: &str,
    before: Option<RowStamp>,
) -> Result<(), ImportError> {
    let Some(state) = state(conn) else {
        return Ok(());
    };
    let before = before.ok_or_else(foreign)?;
    if conn.is_autocommit() || data_version(conn)? != before.data_version {
        return Err(foreign());
    }
    let rowid: i64 = conn
        .query_row("SELECT rowid FROM jobs WHERE job_id=?1", [job_id], |r| {
            r.get(0)
        })
        .map_err(sqlite_storage_error)?;
    let mut state = lock(&state);
    if state.generation != before.generation {
        return Err(foreign());
    }
    let sequence = {
        let mut updates = state
            .open
            .iter()
            .filter(|(table, _, sequence)| table == "jobs" && *sequence > before.sequence);
        let Some((_, changed, sequence)) = updates.next() else {
            return Err(foreign());
        };
        if *changed != rowid || updates.next().is_some() {
            return Err(foreign());
        }
        *sequence
    };
    state.job_acks.push(sequence);
    Ok(())
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
