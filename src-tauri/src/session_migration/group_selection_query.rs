//! Snapshot-consistent bounded views; no source filesystem access or DDL.
use super::{
    mutate, store, Choice, GroupChoice, GroupSelection, GroupSelectionMembership,
    GroupSelectionPage,
};
use crate::session_migration::{
    budget,
    cursor::{decode_bound_cursor, encode_bound_cursor, CursorScope},
    errors::{sqlite_storage_error, ErrorCode, ImportError},
    scan::{self, CandidateRef},
};
use rusqlite::{params, Connection};

pub fn get(scan_id: &str, epoch: u64) -> Result<GroupSelection, ImportError> {
    let conn = scan::open_scan_readonly(scan_id, epoch)?;
    let tx = conn.unchecked_transaction().map_err(sqlite_storage_error)?;
    store::load(&tx, scan_id, epoch, None)
}

pub(crate) fn get_owned(
    scan_id: &str,
    epoch: u64,
    id: &str,
) -> Result<GroupSelection, ImportError> {
    let conn = scan::open_scan_readonly(scan_id, epoch)?;
    let tx = conn.unchecked_transaction().map_err(sqlite_storage_error)?;
    store::load(&tx, scan_id, epoch, Some(id))
}

fn checked_bytes(page: &impl serde::Serialize) -> Result<(), ImportError> {
    if serde_json::to_vec(page)
        .map_err(store::metadata_error)?
        .len()
        > budget::READ_PAGE_TEXT_BYTES
    {
        return Err(ImportError::new(
            ErrorCode::ResourceLimit,
            "fixed group page exceeds byte budget; request a smaller page",
        ));
    }
    Ok(())
}

fn fixed_page(
    scan_id: &str,
    epoch: u64,
    id: &str,
    cursor: Option<&str>,
    limit: Option<usize>,
    selected_only: bool,
) -> Result<(GroupSelection, Vec<Choice>, Option<String>), ImportError> {
    let conn = scan::open_scan_readonly(scan_id, epoch)?;
    let tx = conn.unchecked_transaction().map_err(sqlite_storage_error)?;
    let selection = store::load(&tx, scan_id, epoch, Some(id))?;
    let snapshot = format!("{epoch}:fixed-groups-v1:{}", selection.state_version);
    let scope = CursorScope {
        owner: id,
        snapshot: &snapshot,
        leaf: scan_id,
        filter: if selected_only {
            "selected-fixed-members"
        } else {
            "fixed-member-choices"
        },
    };
    let after = cursor
        .filter(|s| !s.is_empty())
        .map(|raw| decode_bound_cursor(raw, scope).map(|c| c.offset))
        .transpose()?
        .unwrap_or(0);
    let cap = budget::page_limit(limit);
    let mut query = tx.prepare("SELECT seq,group_id FROM group_selection_choices WHERE selection_id=?1 AND seq>?2 AND (?3=0 OR selected=1) ORDER BY seq LIMIT ?4").map_err(sqlite_storage_error)?;
    let mut rows = query
        .query(params![id, after, selected_only, cap + 1])
        .map_err(sqlite_storage_error)?;
    let mut choices = Vec::with_capacity(cap);
    let mut bytes = 0usize;
    let mut last = after;
    let mut more = false;
    while let Some(row) = rows.next().map_err(sqlite_storage_error)? {
        if choices.len() == cap {
            more = true;
            break;
        }
        let seq: u32 = row.get(0).map_err(sqlite_storage_error)?;
        let group: String = row.get(1).map_err(sqlite_storage_error)?;
        let choice = store::read_choice(&tx, &selection, &group)?
            .ok_or_else(|| store::stale("fixed group choice disappeared"))?;
        let size = serde_json::to_vec(&choice)
            .map_err(store::metadata_error)?
            .len()
            + 1;
        if bytes.saturating_add(size) > budget::READ_PAGE_TEXT_BYTES - 4096 {
            if choices.is_empty() {
                return Err(ImportError::new(
                    ErrorCode::ResourceLimit,
                    "fixed member cannot fit the bounded page",
                ));
            }
            more = true;
            break;
        }
        bytes += size;
        last = seq;
        choices.push(choice);
    }
    let next = if more {
        Some(encode_bound_cursor(scope, last, None)?)
    } else {
        None
    };
    Ok((selection, choices, next))
}

pub fn page(
    scan_id: &str,
    epoch: u64,
    id: &str,
    cursor: Option<&str>,
    limit: Option<usize>,
    selected_only: bool,
) -> Result<GroupSelectionPage, ImportError> {
    let (selection, choices, next_cursor) =
        fixed_page(scan_id, epoch, id, cursor, limit, selected_only)?;
    let page = GroupSelectionPage {
        selection,
        choices: choices.iter().map(Choice::public).collect(),
        next_cursor,
    };
    checked_bytes(&page)?;
    Ok(page)
}

pub(crate) fn page_refs(
    scan_id: &str,
    epoch: u64,
    id: &str,
    cursor: Option<&str>,
    limit: Option<usize>,
) -> Result<(Vec<CandidateRef>, Option<String>, u32), ImportError> {
    let (selection, choices, next) = fixed_page(scan_id, epoch, id, cursor, limit, true)?;
    let refs: Vec<_> = choices.into_iter().map(|choice| choice.reference).collect();
    checked_bytes(&refs)?;
    Ok((refs, next, selection.selected_count))
}

fn membership_choice(
    conn: &Connection,
    selection: &GroupSelection,
    group: &str,
) -> Result<GroupChoice, ImportError> {
    if let Some(choice) = store::read_choice(conn, selection, group)? {
        return Ok(choice.public());
    }
    let (seq, reference) = store::member(conn, selection, group, None)?;
    Ok(GroupChoice {
        group_id: group.into(),
        group_seq: seq,
        candidate: reference.candidate,
        selected: false,
        choice_version: 0,
    })
}

pub fn membership(
    scan_id: &str,
    epoch: u64,
    id: &str,
    groups: &[String],
) -> Result<GroupSelectionMembership, ImportError> {
    mutate::checked_ids(groups)?;
    let conn = scan::open_scan_readonly(scan_id, epoch)?;
    let tx = conn.unchecked_transaction().map_err(sqlite_storage_error)?;
    let selection = store::load(&tx, scan_id, epoch, Some(id))?;
    let choices = groups
        .iter()
        .map(|group| membership_choice(&tx, &selection, group))
        .collect::<Result<Vec<_>, _>>()?;
    let page = GroupSelectionMembership {
        selection_id: id.into(),
        state_version: selection.state_version,
        choices,
    };
    checked_bytes(&page)?;
    Ok(page)
}
