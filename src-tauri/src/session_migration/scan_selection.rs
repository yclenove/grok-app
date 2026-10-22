//! Host-owned candidate selections bound to one immutable scan epoch.

use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};

use super::cursor::{decode_bound_cursor, encode_bound_cursor, CursorScope};
use super::errors::{ErrorCode, ImportError};
use super::scan::CandidateRef;

pub const CANDIDATE_SELECTION_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CandidateSelectionMode {
    Explicit,
    FrozenScan,
}

impl CandidateSelectionMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Explicit => "explicit",
            Self::FrozenScan => "frozen-scan",
        }
    }

    fn parse(value: &str) -> Result<Self, ImportError> {
        match value {
            "explicit" => Ok(Self::Explicit),
            "frozen-scan" => Ok(Self::FrozenScan),
            _ => Err(failure("candidate selection mode is invalid")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CandidateSelection {
    pub version: u32,
    pub selection_id: String,
    pub scan_id: String,
    pub epoch: u64,
    pub mode: CandidateSelectionMode,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frozen_through: Option<u32>,
    pub selected_count: u32,
    pub state_version: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CandidateSelectionPage {
    pub candidates: Vec<super::types::SourceCandidate>,
    pub next_cursor: Option<String>,
    pub selected_count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CandidateSelectionMembership {
    pub selection_id: String,
    pub state_version: u64,
    pub selected_ids: Vec<String>,
}

fn failure(message: impl Into<String>) -> ImportError {
    ImportError::new(ErrorCode::SnapshotUnavailable, message)
}

fn db_failure(error: impl std::fmt::Display) -> ImportError {
    failure(error.to_string())
}

pub(crate) fn initialize(conn: &Connection, scan_id: &str, epoch: u64) -> Result<(), ImportError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS candidate_selections(
            selection_id TEXT PRIMARY KEY,
            scan_id TEXT NOT NULL UNIQUE,
            epoch INTEGER NOT NULL,
            mode TEXT NOT NULL,
            frozen_through INTEGER,
            state_version INTEGER NOT NULL
         );
         CREATE TABLE IF NOT EXISTS candidate_selection_overrides(
            selection_id TEXT NOT NULL,
            candidate_id TEXT NOT NULL,
            selected INTEGER NOT NULL CHECK(selected IN (0,1)),
            PRIMARY KEY(selection_id,candidate_id),
            FOREIGN KEY(selection_id) REFERENCES candidate_selections(selection_id) ON DELETE CASCADE
         );",
    )
    .map_err(db_failure)?;
    conn.execute(
        "INSERT INTO candidate_selections(selection_id,scan_id,epoch,mode,frozen_through,state_version)
         SELECT ?1,?2,?3,'explicit',NULL,1
         WHERE NOT EXISTS(
           SELECT 1 FROM candidate_selections WHERE scan_id=?2 AND epoch=?3
         )",
        params![uuid::Uuid::new_v4().to_string(), scan_id, epoch],
    )
    .map_err(db_failure)?;
    Ok(())
}

fn load_row(
    conn: &Connection,
    scan_id: &str,
    epoch: u64,
    selection_id: Option<&str>,
) -> Result<(String, CandidateSelectionMode, Option<u32>, u64), ImportError> {
    let row: Option<(String, String, Option<u32>, u64)> = conn
        .query_row(
            "SELECT selection_id,mode,frozen_through,state_version
             FROM candidate_selections
             WHERE scan_id=?1 AND epoch=?2 AND (?3 IS NULL OR selection_id=?3)",
            params![scan_id, epoch, selection_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()
        .map_err(db_failure)?;
    let (id, mode, frozen, version) = row.ok_or_else(|| {
        ImportError::new(
            ErrorCode::PlanStale,
            "candidate selection does not belong to this scan epoch",
        )
    })?;
    Ok((id, CandidateSelectionMode::parse(&mode)?, frozen, version))
}

fn selected_count(
    conn: &Connection,
    selection_id: &str,
    mode: CandidateSelectionMode,
    frozen_through: Option<u32>,
) -> Result<u32, ImportError> {
    conn.query_row(
        "SELECT COUNT(*)
         FROM candidates c
         LEFT JOIN candidate_selection_overrides o
           ON o.selection_id=?1 AND o.candidate_id=c.candidate_id
         WHERE COALESCE(
           o.selected,
           CASE WHEN ?2='frozen-scan' AND c.seq<=COALESCE(?3,0) THEN 1 ELSE 0 END
         )=1",
        params![selection_id, mode.as_str(), frozen_through],
        |row| row.get(0),
    )
    .map_err(db_failure)
}

pub(crate) fn descriptor_from(
    conn: &Connection,
    scan_id: &str,
    epoch: u64,
    selection_id: Option<&str>,
) -> Result<CandidateSelection, ImportError> {
    let (selection_id, mode, frozen_through, state_version) =
        load_row(conn, scan_id, epoch, selection_id)?;
    Ok(CandidateSelection {
        version: CANDIDATE_SELECTION_VERSION,
        selected_count: selected_count(conn, &selection_id, mode, frozen_through)?,
        selection_id,
        scan_id: scan_id.to_string(),
        epoch,
        mode,
        frozen_through,
        state_version,
    })
}

pub fn get(scan_id: &str, epoch: u64) -> Result<CandidateSelection, ImportError> {
    let conn = super::scan::open_scan(scan_id, epoch)?;
    descriptor_from(&conn, scan_id, epoch, None)
}

pub fn freeze_current(scan_id: &str, epoch: u64) -> Result<CandidateSelection, ImportError> {
    let mut conn = super::scan::open_scan(scan_id, epoch)?;
    let tx = conn.transaction().map_err(db_failure)?;
    let (selection_id, _, _, _) = load_row(&tx, scan_id, epoch, None)?;
    tx.execute(
        "UPDATE meta
         SET frozen=COALESCE(frozen,(SELECT COALESCE(MAX(seq),0) FROM candidates)),
             version=version+1
         WHERE id=1",
        [],
    )
    .map_err(db_failure)?;
    let frozen: u32 = tx
        .query_row(
            "SELECT COALESCE(frozen,0) FROM meta WHERE id=1",
            [],
            |row| row.get(0),
        )
        .map_err(db_failure)?;
    tx.execute(
        "UPDATE candidate_selections
         SET mode='frozen-scan',frozen_through=?2,state_version=state_version+1
         WHERE selection_id=?1",
        params![selection_id, frozen],
    )
    .map_err(db_failure)?;
    tx.execute(
        "DELETE FROM candidate_selection_overrides WHERE selection_id=?1",
        params![selection_id],
    )
    .map_err(db_failure)?;
    tx.commit().map_err(db_failure)?;
    descriptor_from(&conn, scan_id, epoch, Some(&selection_id))
}

pub fn clear(
    scan_id: &str,
    epoch: u64,
    selection_id: &str,
) -> Result<CandidateSelection, ImportError> {
    let mut conn = super::scan::open_scan(scan_id, epoch)?;
    let tx = conn.transaction().map_err(db_failure)?;
    load_row(&tx, scan_id, epoch, Some(selection_id))?;
    tx.execute(
        "DELETE FROM candidate_selection_overrides WHERE selection_id=?1",
        params![selection_id],
    )
    .map_err(db_failure)?;
    tx.execute(
        "UPDATE candidate_selections
         SET mode='explicit',frozen_through=NULL,state_version=state_version+1
         WHERE selection_id=?1",
        params![selection_id],
    )
    .map_err(db_failure)?;
    tx.commit().map_err(db_failure)?;
    descriptor_from(&conn, scan_id, epoch, Some(selection_id))
}

fn base_selected(mode: CandidateSelectionMode, frozen: Option<u32>, seq: u32) -> bool {
    mode == CandidateSelectionMode::FrozenScan && seq <= frozen.unwrap_or(0)
}

fn candidate_state(
    tx: &Transaction<'_>,
    selection_id: &str,
    candidate_id: &str,
    mode: CandidateSelectionMode,
    frozen: Option<u32>,
) -> Result<(bool, bool), ImportError> {
    let row: Option<(u32, Option<i64>)> = tx
        .query_row(
            "SELECT c.seq,o.selected
             FROM candidates c
             LEFT JOIN candidate_selection_overrides o
               ON o.selection_id=?1 AND o.candidate_id=c.candidate_id
             WHERE c.candidate_id=?2",
            params![selection_id, candidate_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(db_failure)?;
    let (seq, override_value) = row.ok_or_else(|| {
        ImportError::new(
            ErrorCode::PlanStale,
            "candidate does not belong to this scan",
        )
    })?;
    let base = base_selected(mode, frozen, seq);
    Ok((override_value.map(|value| value != 0).unwrap_or(base), base))
}

fn write_state(
    tx: &Transaction<'_>,
    selection_id: &str,
    candidate_id: &str,
    selected: bool,
    base: bool,
) -> Result<(), ImportError> {
    if selected == base {
        tx.execute(
            "DELETE FROM candidate_selection_overrides WHERE selection_id=?1 AND candidate_id=?2",
            params![selection_id, candidate_id],
        )
        .map_err(db_failure)?;
    } else {
        tx.execute(
            "INSERT INTO candidate_selection_overrides(selection_id,candidate_id,selected)
             VALUES(?1,?2,?3)
             ON CONFLICT(selection_id,candidate_id) DO UPDATE SET selected=excluded.selected",
            params![selection_id, candidate_id, i64::from(selected)],
        )
        .map_err(db_failure)?;
    }
    Ok(())
}

fn mutate_many(
    scan_id: &str,
    epoch: u64,
    selection_id: &str,
    candidate_ids: &[String],
    operation: impl Fn(bool) -> bool,
) -> Result<CandidateSelection, ImportError> {
    if candidate_ids.len() > super::budget::PAGE_SIZE_MAX {
        return Err(ImportError::new(
            ErrorCode::ResourceLimit,
            "candidate selection mutation exceeds one bounded page",
        ));
    }
    let mut unique = std::collections::HashSet::with_capacity(candidate_ids.len());
    if candidate_ids
        .iter()
        .any(|id| id.trim().is_empty() || !unique.insert(id.as_str()))
    {
        return Err(ImportError::new(
            ErrorCode::PlanStale,
            "candidate selection mutation contains invalid or duplicate IDs",
        ));
    }
    let mut conn = super::scan::open_scan(scan_id, epoch)?;
    let tx = conn.transaction().map_err(db_failure)?;
    let (_, mode, frozen, _) = load_row(&tx, scan_id, epoch, Some(selection_id))?;
    let mut changed = false;
    for candidate_id in candidate_ids {
        let (current, base) = candidate_state(&tx, selection_id, candidate_id, mode, frozen)?;
        let next = operation(current);
        if next != current {
            write_state(&tx, selection_id, candidate_id, next, base)?;
            changed = true;
        }
    }
    if changed {
        tx.execute(
            "UPDATE candidate_selections SET state_version=state_version+1 WHERE selection_id=?1",
            params![selection_id],
        )
        .map_err(db_failure)?;
    }
    tx.commit().map_err(db_failure)?;
    descriptor_from(&conn, scan_id, epoch, Some(selection_id))
}

pub fn set_many(
    scan_id: &str,
    epoch: u64,
    selection_id: &str,
    candidate_ids: &[String],
    selected: bool,
) -> Result<CandidateSelection, ImportError> {
    mutate_many(scan_id, epoch, selection_id, candidate_ids, |_| selected)
}

pub fn invert_many(
    scan_id: &str,
    epoch: u64,
    selection_id: &str,
    candidate_ids: &[String],
) -> Result<CandidateSelection, ImportError> {
    mutate_many(scan_id, epoch, selection_id, candidate_ids, |selected| {
        !selected
    })
}

pub(crate) fn page_refs(
    scan_id: &str,
    epoch: u64,
    selection_id: &str,
    cursor: Option<&str>,
    limit: Option<usize>,
) -> Result<(Vec<CandidateRef>, Option<String>, u32), ImportError> {
    let conn = super::scan::open_scan(scan_id, epoch)?;
    let selection = descriptor_from(&conn, scan_id, epoch, Some(selection_id))?;
    let snapshot = selection.state_version.to_string();
    let scope = CursorScope {
        owner: &selection.selection_id,
        snapshot: &snapshot,
        leaf: "",
        filter: "selected-candidates",
    };
    let after = match cursor.filter(|value| !value.is_empty()) {
        None => 0,
        Some(raw) => decode_bound_cursor(raw, scope)?.offset,
    };
    let limit = super::budget::page_limit(limit);
    let mut statement = conn
        .prepare(
            "SELECT c.seq,c.json
             FROM candidates c
             LEFT JOIN candidate_selection_overrides o
               ON o.selection_id=?1 AND o.candidate_id=c.candidate_id
             WHERE c.seq>?2 AND COALESCE(
               o.selected,
               CASE WHEN ?3='frozen-scan' AND c.seq<=COALESCE(?4,0) THEN 1 ELSE 0 END
             )=1
             ORDER BY c.seq LIMIT ?5",
        )
        .map_err(db_failure)?;
    let rows = statement
        .query_map(
            params![
                selection.selection_id,
                after,
                selection.mode.as_str(),
                selection.frozen_through,
                limit + 1
            ],
            |row| Ok((row.get::<_, u32>(0)?, row.get::<_, String>(1)?)),
        )
        .map_err(db_failure)?;
    let mut found = Vec::with_capacity(limit + 1);
    for row in rows {
        let (seq, raw) = row.map_err(db_failure)?;
        let reference: CandidateRef = serde_json::from_str(&raw).map_err(db_failure)?;
        found.push((seq, reference));
    }
    let more = found.len() > limit;
    found.truncate(limit);
    let last = found.last().map(|(seq, _)| *seq).unwrap_or(after);
    let next = if more {
        Some(encode_bound_cursor(scope, last, None)?)
    } else {
        None
    };
    Ok((
        found.into_iter().map(|(_, reference)| reference).collect(),
        next,
        selection.selected_count,
    ))
}

pub fn page(
    scan_id: &str,
    epoch: u64,
    selection_id: &str,
    cursor: Option<&str>,
    limit: Option<usize>,
) -> Result<CandidateSelectionPage, ImportError> {
    let (references, next_cursor, selected_count) =
        page_refs(scan_id, epoch, selection_id, cursor, limit)?;
    Ok(CandidateSelectionPage {
        candidates: references
            .into_iter()
            .map(|reference| reference.candidate)
            .collect(),
        next_cursor,
        selected_count,
    })
}

pub fn selected_ids_for_candidates(
    scan_id: &str,
    epoch: u64,
    selection_id: &str,
    candidate_ids: &[String],
) -> Result<CandidateSelectionMembership, ImportError> {
    if candidate_ids.len() > super::budget::PAGE_SIZE_MAX {
        return Err(ImportError::new(
            ErrorCode::ResourceLimit,
            "candidate membership query exceeds one bounded page",
        ));
    }
    let mut unique = std::collections::HashSet::with_capacity(candidate_ids.len());
    if candidate_ids
        .iter()
        .any(|id| id.trim().is_empty() || !unique.insert(id.as_str()))
    {
        return Err(ImportError::new(
            ErrorCode::PlanStale,
            "candidate membership query contains invalid or duplicate IDs",
        ));
    }
    let mut conn = super::scan::open_scan(scan_id, epoch)?;
    let tx = conn.transaction().map_err(db_failure)?;
    let (selection_id, mode, frozen, state_version) =
        load_row(&tx, scan_id, epoch, Some(selection_id))?;
    let mut selected = Vec::new();
    for candidate_id in candidate_ids {
        if candidate_state(&tx, &selection_id, candidate_id, mode, frozen)?.0 {
            selected.push(candidate_id.clone());
        }
    }
    tx.commit().map_err(db_failure)?;
    Ok(CandidateSelectionMembership {
        selection_id,
        state_version,
        selected_ids: selected,
    })
}
