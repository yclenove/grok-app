//! Atomic user mutations. Discovery never edits an existing fixed choice.
use super::{store, Choice, GroupMemberChange, GroupSelection, GROUP_SELECTION_VERSION};
use crate::session_migration::{
    budget,
    errors::{sqlite_storage_error, ErrorCode, ImportError},
    scan,
};
use rusqlite::{params, Connection, TransactionBehavior};

pub(super) fn checked_ids(ids: &[String]) -> Result<(), ImportError> {
    if ids.len() > budget::PAGE_SIZE_MAX {
        return Err(ImportError::new(
            ErrorCode::ResourceLimit,
            "group mutation exceeds one bounded page",
        ));
    }
    let mut unique = std::collections::HashSet::with_capacity(ids.len());
    if ids
        .iter()
        .any(|id| id.trim().is_empty() || !unique.insert(id.as_str()))
    {
        return Err(store::stale(
            "group mutation contains invalid or duplicate IDs",
        ));
    }
    Ok(())
}

fn expected(selection: &GroupSelection, version: u64) -> Result<(), ImportError> {
    if selection.state_version != version {
        return Err(store::stale(
            "group selection changed; reload before editing",
        ));
    }
    Ok(())
}

fn advance(selection: &mut GroupSelection, changed: &mut bool) -> Result<(), ImportError> {
    if !*changed {
        selection.state_version = selection.state_version.checked_add(1).ok_or_else(|| {
            ImportError::new(
                ErrorCode::ResourceLimit,
                "group selection version exhausted",
            )
        })?;
        *changed = true;
    }
    Ok(())
}

fn first(
    conn: &Connection,
    selection: &GroupSelection,
    group: &str,
) -> Result<Choice, ImportError> {
    let (seq, reference) = store::member(conn, selection, group, None)?;
    Ok(Choice {
        version: GROUP_SELECTION_VERSION,
        group_id: group.into(),
        group_seq: seq,
        reference,
        selected: false,
        choice_version: 0,
        seal: String::new(),
    })
}

/// Re-selecting keeps preferred members, including deselected choices. Only
/// newly captured groups use first discovery; this is not a best-body claim.
pub fn freeze_current(
    scan_id: &str,
    epoch: u64,
    id: &str,
    expected_version: u64,
) -> Result<GroupSelection, ImportError> {
    let mut conn = scan::open_scan_group_writer(scan_id, epoch)?;
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sqlite_storage_error)?;
    let mut selection = store::load(&tx, scan_id, epoch, Some(id))?;
    expected(&selection, expected_version)?;
    let through: u32 = tx
        .query_row(
            "SELECT COALESCE(MAX(seq),0) FROM source_groups",
            [],
            |row| row.get(0),
        )
        .map_err(sqlite_storage_error)?;
    let mut after = 0u32;
    let mut changed = false;
    loop {
        let groups: Vec<(u32, String)> = tx.prepare("SELECT seq,group_id FROM source_groups WHERE seq>?1 AND seq<=?2 ORDER BY seq LIMIT ?3").map_err(sqlite_storage_error)?
            .query_map(params![after, through, budget::PAGE_SIZE_MAX], |row| Ok((row.get(0)?,row.get(1)?))).map_err(sqlite_storage_error)?
            .collect::<Result<_, _>>().map_err(sqlite_storage_error)?;
        if groups.is_empty() {
            break;
        }
        for (seq, group) in groups {
            let existing = store::read_choice(&tx, &selection, &group)?;
            let mut choice = match existing {
                Some(choice) => choice,
                None => first(&tx, &selection, &group)?,
            };
            if !choice.selected {
                advance(&mut selection, &mut changed)?;
                choice.selected = true;
                store::write_choice(&tx, &selection, choice)?;
            }
            after = seq;
        }
    }
    if changed {
        selection = store::persist(&tx, selection)?;
    }
    tx.commit().map_err(sqlite_storage_error)?;
    Ok(selection)
}

pub fn clear(
    scan_id: &str,
    epoch: u64,
    id: &str,
    expected_version: u64,
) -> Result<GroupSelection, ImportError> {
    let mut conn = scan::open_scan_group_writer(scan_id, epoch)?;
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sqlite_storage_error)?;
    let mut selection = store::load(&tx, scan_id, epoch, Some(id))?;
    expected(&selection, expected_version)?;
    if selection.choice_count != 0 {
        let mut changed = false;
        advance(&mut selection, &mut changed)?;
        tx.execute(
            "DELETE FROM group_selection_choices WHERE selection_id=?1",
            [id],
        )
        .map_err(sqlite_storage_error)?;
        selection = store::persist(&tx, selection)?;
    }
    tx.commit().map_err(sqlite_storage_error)?;
    Ok(selection)
}

fn mutate_many(
    scan_id: &str,
    epoch: u64,
    id: &str,
    expected_version: u64,
    groups: &[String],
    operation: impl Fn(bool) -> bool,
) -> Result<GroupSelection, ImportError> {
    checked_ids(groups)?;
    let mut conn = scan::open_scan_group_writer(scan_id, epoch)?;
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sqlite_storage_error)?;
    let mut selection = store::load(&tx, scan_id, epoch, Some(id))?;
    expected(&selection, expected_version)?;
    let mut changed = false;
    for group in groups {
        let mut choice = match store::read_choice(&tx, &selection, group)? {
            Some(choice) => choice,
            None => first(&tx, &selection, group)?,
        };
        let selected = operation(choice.selected);
        if choice.selected != selected {
            advance(&mut selection, &mut changed)?;
            choice.selected = selected;
            store::write_choice(&tx, &selection, choice)?;
        }
    }
    if changed {
        selection = store::persist(&tx, selection)?;
    }
    tx.commit().map_err(sqlite_storage_error)?;
    Ok(selection)
}

pub fn set_many(
    scan_id: &str,
    epoch: u64,
    id: &str,
    expected_version: u64,
    groups: &[String],
    selected: bool,
) -> Result<GroupSelection, ImportError> {
    mutate_many(scan_id, epoch, id, expected_version, groups, |_| selected)
}

pub fn invert_many(
    scan_id: &str,
    epoch: u64,
    id: &str,
    expected_version: u64,
    groups: &[String],
) -> Result<GroupSelection, ImportError> {
    mutate_many(scan_id, epoch, id, expected_version, groups, |selected| {
        !selected
    })
}

pub fn choose_member(
    scan_id: &str,
    epoch: u64,
    id: &str,
    expected_version: u64,
    group: &str,
    candidate: &str,
) -> Result<GroupMemberChange, ImportError> {
    let mut conn = scan::open_scan_group_writer(scan_id, epoch)?;
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sqlite_storage_error)?;
    let mut selection = store::load(&tx, scan_id, epoch, Some(id))?;
    expected(&selection, expected_version)?;
    let (seq, reference) = store::member(&tx, &selection, group, Some(candidate))?;
    let existing = store::read_choice(&tx, &selection, group)?;
    let previous = existing
        .as_ref()
        .map(|c| c.reference.candidate.candidate_id.clone());
    let selected = existing.as_ref().is_some_and(|c| c.selected);
    if previous.as_deref() != Some(candidate) {
        let mut changed = false;
        advance(&mut selection, &mut changed)?;
        store::write_choice(
            &tx,
            &selection,
            Choice {
                version: GROUP_SELECTION_VERSION,
                group_id: group.into(),
                group_seq: seq,
                reference,
                selected,
                choice_version: 0,
                seal: String::new(),
            },
        )?;
        selection = store::persist(&tx, selection)?;
    }
    tx.commit().map_err(sqlite_storage_error)?;
    Ok(GroupMemberChange {
        selection,
        group_id: group.into(),
        previous_candidate_id: previous,
        candidate_id: candidate.into(),
        selected,
    })
}
