//! Read, recovery, and plan projection for durable prepare batches.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

#[cfg(test)]
use rusqlite::TransactionBehavior;
use rusqlite::{params, OptionalExtension};

use super::cursor::{decode_bound_cursor, encode_bound_cursor, CursorScope};
use super::errors::{ErrorCode, ImportError};
use super::prepare::{
    batch_root, db_failure, ensure_running, open_existing, status_raw, PrepareBatchItem,
    PrepareBatchItemPage, PrepareBatchState, PrepareItemState, PREPARE_BATCH_VERSION,
};
use super::scan::CandidateRef;
use super::selection::{PreparedSelectionIdentity, PreviewSelection, SelectionSet};

#[derive(Debug, Default)]
pub struct PrepareRecovery {
    pub status: Option<super::prepare::PrepareBatchStatus>,
    pub expired: bool,
    pub failed: Option<String>,
}

#[derive(Debug)]
pub(crate) struct ValidatedPreparePreview {
    pub source_member: Option<super::source_member::SourceMemberPin>,
    pub mapping_subject: super::mapping_scope::MappingSubject,
    pub preview_id: String,
    pub snapshot_digest: String,
    pub path: PathBuf,
}

#[derive(Debug)]
pub(crate) struct ValidatedPreparePlan {
    pub _operation: super::preview_lifecycle::OperationGuard,
    pub origin: super::prepare_plan::PreparePlanOrigin,
    pub preview_ids: Vec<String>,
    pub selection_set: SelectionSet,
    pub previews: Vec<ValidatedPreparePreview>,
}

pub(crate) struct ValidatedPrepareMember {
    pub _operation: super::preview_lifecycle::OperationGuard,
    pub selection: PreviewSelection,
}

struct PreparedRow {
    candidate_id: String,
    source_revision: String,
    input: String,
    preview_id: String,
    selection: String,
}

impl PreparedRow {
    fn read(row: &rusqlite::Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            candidate_id: row.get(0)?,
            source_revision: row.get(1)?,
            input: row.get(2)?,
            preview_id: row.get(3)?,
            selection: row.get(4)?,
        })
    }
}

fn validate_row(
    batch_id: &str,
    row: PreparedRow,
) -> Result<(PreviewSelection, ValidatedPreparePreview), ImportError> {
    let reference: CandidateRef = serde_json::from_str(&row.input).map_err(db_failure)?;
    let selection: PreviewSelection = serde_json::from_str(&row.selection).map_err(db_failure)?;
    let path = super::jobs::find_preview_dir(&row.preview_id)?
        .ok_or_else(|| ImportError::new(ErrorCode::PlanStale, "prepared preview is missing"))?;
    let (lease, _) = super::jobs::verify_disk_preview_integrity(&path, &row.preview_id)?;
    super::source_member::validate_reference_lease(&reference, &lease)?;
    if lease.prepare_batch_id.as_deref() != Some(batch_id)
        || lease.candidate_id != row.candidate_id
        || lease.source_revision != row.source_revision
        || reference.candidate.candidate_id != row.candidate_id
        || reference.candidate.source_revision != row.source_revision
    {
        return Err(ImportError::new(
            ErrorCode::PlanStale,
            "prepare batch item identity changed",
        ));
    }
    let validated = super::selection::validate_prepared_selection(
        &selection,
        &path,
        PreparedSelectionIdentity {
            preview_id: Some(&row.preview_id),
            candidate_id: Some(&row.candidate_id),
            source_id: Some(&reference.source_id),
            root_path: Some(&reference.root_path),
            legacy_id: Some(&reference.legacy_id),
            ..PreparedSelectionIdentity::default()
        },
    )?;
    Ok((
        selection,
        ValidatedPreparePreview {
            source_member: validated.source_member,
            mapping_subject: super::mapping_scope::MappingSubject::new(
                validated.source_id,
                validated.provider,
                validated.cwd,
            ),
            preview_id: row.preview_id,
            snapshot_digest: validated.snapshot_digest,
            path,
        },
    ))
}

/// A bounded lookup of the actual canonicalized member stored by prepare.
/// The same pin, lease and selection checks protect the full plan projection.
pub(crate) fn validated_member(
    batch_id: &str,
    expected_state_version: u64,
    candidate_id: &str,
    preview_id: &str,
) -> Result<ValidatedPrepareMember, ImportError> {
    let operation = super::preview_lifecycle::operation()?;
    let status = status_raw(batch_id)?;
    if !status.state.can_plan()
        || status.active > 0
        || status.state_version != expected_state_version
    {
        return Err(ImportError::new(
            ErrorCode::PlanStale,
            "prepare batch changed",
        ));
    }
    let conn = open_existing(batch_id)?;
    let row = conn
        .query_row(
            "SELECT candidate_id,source_revision,input_json,preview_id,selection_json
             FROM items WHERE state='prepared' AND candidate_id=?1 AND preview_id=?2",
            params![candidate_id, preview_id],
            PreparedRow::read,
        )
        .optional()
        .map_err(db_failure)?
        .ok_or_else(|| ImportError::new(ErrorCode::PlanStale, "prepared member changed"))?;
    let (selection, _) = validate_row(batch_id, row)?;
    Ok(ValidatedPrepareMember {
        _operation: operation,
        selection,
    })
}

pub fn items_page(
    batch_id: &str,
    cursor: Option<&str>,
    limit: Option<usize>,
) -> Result<PrepareBatchItemPage, ImportError> {
    let conn = open_existing(batch_id)?;
    let scope = CursorScope {
        owner: batch_id,
        snapshot: batch_id,
        leaf: "",
        filter: "prepare-items",
    };
    let after = cursor
        .filter(|value| !value.is_empty())
        .map(|value| decode_bound_cursor(value, scope).map(|cursor| cursor.offset))
        .transpose()?
        .unwrap_or(0);
    let limit = super::budget::page_limit(limit);
    let mut statement = conn
        .prepare(
            "SELECT seq,candidate_id,title,provider,source_revision,state,attempt,preview_id,error_json
             FROM items WHERE seq>?1 ORDER BY seq LIMIT ?2",
        )
        .map_err(db_failure)?;
    let rows = statement
        .query_map(params![after, limit + 1], |row| {
            Ok((
                row.get::<_, u32>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, u32>(6)?,
                row.get::<_, Option<String>>(7)?,
                row.get::<_, Option<String>>(8)?,
            ))
        })
        .map_err(db_failure)?;
    let mut found = Vec::with_capacity(limit + 1);
    for row in rows {
        found.push(row.map_err(db_failure)?);
    }
    let more = found.len() > limit;
    found.truncate(limit);
    let last = found.last().map(|row| row.0).unwrap_or(after);
    let next_cursor = more
        .then(|| encode_bound_cursor(scope, last, None))
        .transpose()?;
    let items = found
        .into_iter()
        .map(
            |(
                _,
                candidate_id,
                title,
                provider,
                source_revision,
                state,
                attempt,
                preview_id,
                error,
            )| {
                Ok(PrepareBatchItem {
                    candidate_id,
                    title,
                    provider,
                    source_revision,
                    state: PrepareItemState::parse(&state)?,
                    attempt,
                    preview_id,
                    error: error
                        .map(|raw| serde_json::from_str(&raw).map_err(db_failure))
                        .transpose()?,
                })
            },
        )
        .collect::<Result<Vec<_>, ImportError>>()?;
    Ok(PrepareBatchItemPage { items, next_cursor })
}

#[cfg(test)]
pub fn plan_inputs(batch_id: &str) -> Result<(Vec<String>, SelectionSet), ImportError> {
    let validated = validated_plan_inputs(batch_id)?;
    Ok((validated.preview_ids, validated.selection_set))
}

pub(crate) fn validated_plan_inputs(batch_id: &str) -> Result<ValidatedPreparePlan, ImportError> {
    let operation = super::preview_lifecycle::operation()?;
    let status = status_raw(batch_id)?;
    if !status.state.can_plan() || status.active > 0 {
        return Err(ImportError::new(
            ErrorCode::PlanStale,
            "prepare batch is not ready to plan",
        ));
    }
    let conn = open_existing(batch_id)?;
    let mut statement = conn
        .prepare(
            "SELECT candidate_id,source_revision,input_json,preview_id,selection_json
             FROM items WHERE state='prepared' ORDER BY seq",
        )
        .map_err(db_failure)?;
    let rows = statement
        .query_map([], PreparedRow::read)
        .map_err(db_failure)?;
    let mut preview_ids = Vec::new();
    let mut entries = Vec::new();
    let mut previews = Vec::new();
    for row in rows {
        let (selection, preview) = validate_row(batch_id, row.map_err(db_failure)?)?;
        preview_ids.push(preview.preview_id.clone());
        previews.push(preview);
        entries.push(selection);
    }
    if preview_ids.is_empty() {
        return Err(ImportError::new(
            ErrorCode::HistoryAccessUnavailable,
            "prepare batch has no successful previews",
        ));
    }
    Ok(ValidatedPreparePlan {
        _operation: operation,
        origin: super::prepare_plan::PreparePlanOrigin {
            version: 1,
            batch_id: status.batch_id,
            state_version: status.state_version,
        },
        preview_ids,
        selection_set: SelectionSet {
            entries,
            ..SelectionSet::default()
        },
        previews,
    })
}

pub fn mappings(
    batch_id: &str,
    fallback_project: Option<String>,
) -> Result<Vec<super::workspace::WorkspaceMapping>, ImportError> {
    let _operation = super::preview_lifecycle::operation()?;
    let status = status_raw(batch_id)?;
    if !status.state.can_plan() || status.active > 0 {
        return Err(ImportError::new(
            ErrorCode::PlanStale,
            "prepare batch is not ready to map",
        ));
    }
    let conn = open_existing(batch_id)?;
    let summaries = super::prepare_mapping::load(&conn)?;
    let mut statement = conn
        .prepare(
            "SELECT candidate_id,source_revision,input_json,preview_id,selection_json
             FROM items WHERE state='prepared' ORDER BY seq",
        )
        .map_err(db_failure)?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
            ))
        })
        .map_err(db_failure)?;
    let mut subjects = Vec::new();
    for row in rows {
        let (candidate_id, source_revision, input, preview_id, selection) =
            row.map_err(db_failure)?;
        let reference: CandidateRef = serde_json::from_str(&input).map_err(db_failure)?;
        if reference.candidate.candidate_id != candidate_id
            || reference.candidate.source_revision != source_revision
        {
            return Err(ImportError::new(
                ErrorCode::PlanStale,
                "prepare batch mapping identity changed",
            ));
        }
        subjects.push(super::prepare_mapping::subject_for(
            &summaries,
            batch_id,
            &reference,
            &preview_id,
            &selection,
        )?);
    }
    if subjects.is_empty() {
        return Err(ImportError::new(
            ErrorCode::HistoryAccessUnavailable,
            "prepare batch has no successful previews",
        ));
    }
    Ok(super::mapping_scope::propose(&subjects, fallback_project))
}

#[cfg(test)]
pub fn mark_planned(batch_id: &str) -> Result<(), ImportError> {
    let mut conn = open_existing(batch_id)?;
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(db_failure)?;
    let changed = tx
        .execute(
            "UPDATE meta SET state='planned',state_version=state_version+1,updated_at=?1
         WHERE id=1 AND state IN ('completed','completed-with-issues')
           AND cancel_requested=0
           AND NOT EXISTS(SELECT 1 FROM items WHERE state IN ('queued','preparing'))",
            params![chrono::Utc::now().to_rfc3339()],
        )
        .map_err(db_failure)?;
    if changed != 1 {
        return Err(ImportError::new(
            ErrorCode::PlanStale,
            "prepare batch is not ready to mark planned",
        ));
    }
    tx.commit().map_err(db_failure)?;
    Ok(())
}

pub(crate) fn preview_is_retained_checked(
    batch_id: &str,
    preview_id: &str,
) -> Result<bool, super::errors::ImportError> {
    let conn = open_existing(batch_id)?;
    conn.query_row(
        "SELECT EXISTS(
           SELECT 1 FROM meta m JOIN items i
           WHERE m.id=1 AND m.state!='planned' AND m.retention_expires_unix>?1
             AND i.state='prepared' AND i.preview_id=?2
         )",
        params![chrono::Utc::now().timestamp(), preview_id],
        |row| row.get::<_, bool>(0),
    )
    .map_err(db_failure)
}

/// Best-effort callers such as `Drop` must fail closed. An unreadable batch
/// cannot prove that its preview is disposable, so retain it for recovery.
pub(crate) fn preview_is_retained(batch_id: &str, preview_id: &str) -> bool {
    preview_is_retained_checked(batch_id, preview_id).unwrap_or(true)
}

pub(crate) fn retained_preview_ids(
    batch_ids: &BTreeSet<String>,
) -> Result<HashMap<String, HashSet<String>>, ImportError> {
    let root = batch_root()?;
    let now = chrono::Utc::now().timestamp();
    let mut retained = HashMap::new();
    for batch_id in batch_ids {
        let Ok(parsed) = uuid::Uuid::parse_str(batch_id) else {
            continue;
        };
        if parsed.to_string() != *batch_id {
            continue;
        }
        let path = root.join(format!("{batch_id}.sqlite"));
        let metadata = match std::fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(db_failure(error)),
        };
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(db_failure("prepare batch is not a regular file"));
        }
        let conn = open_existing(batch_id)?;
        let (schema_version, stored_batch_id, state, retention_expires_unix): (
            u32,
            String,
            String,
            i64,
        ) = conn
            .query_row(
                "SELECT schema_version,batch_id,state,retention_expires_unix FROM meta WHERE id=1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .map_err(db_failure)?;
        if schema_version != PREPARE_BATCH_VERSION || stored_batch_id != *batch_id {
            return Err(db_failure("prepare batch identity is invalid"));
        }
        if state == "planned" || retention_expires_unix <= now {
            continue;
        }
        let mut stmt = conn
            .prepare(
                "SELECT preview_id FROM items WHERE state='prepared' AND preview_id IS NOT NULL",
            )
            .map_err(db_failure)?;
        let rows = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(db_failure)?;
        let mut ids = HashSet::new();
        for row in rows {
            ids.insert(row.map_err(db_failure)?);
        }
        if !ids.is_empty() {
            retained.insert(batch_id.clone(), ids);
        }
    }
    Ok(retained)
}

fn sidecar_path(path: &Path, suffix: &str) -> PathBuf {
    let mut value = OsString::from(path.as_os_str());
    value.push(suffix);
    PathBuf::from(value)
}

fn remove_file_if_present(path: &Path) -> Result<(), ImportError> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(db_failure(error)),
    }
}

fn remove_expired_batch(path: &Path, batch_id: &str) -> Result<(), ImportError> {
    let root = batch_root()?;
    if path.parent() != Some(root.as_path())
        || path.file_stem().and_then(|value| value.to_str()) != Some(batch_id)
        || uuid::Uuid::parse_str(batch_id).is_err()
    {
        return Err(db_failure("invalid prepare batch cleanup target"));
    }
    remove_file_if_present(&sidecar_path(path, "-wal"))?;
    remove_file_if_present(&sidecar_path(path, "-shm"))?;
    remove_file_if_present(path)
}

fn cleanup_orphan_sidecars(root: &Path) -> Result<(), ImportError> {
    for entry in std::fs::read_dir(root).map_err(db_failure)? {
        let entry = entry.map_err(db_failure)?;
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        let Some(main_name) = name
            .strip_suffix("-wal")
            .or_else(|| name.strip_suffix("-shm"))
        else {
            continue;
        };
        let Some(batch_id) = main_name.strip_suffix(".sqlite") else {
            continue;
        };
        if uuid::Uuid::parse_str(batch_id).is_err() {
            continue;
        }
        if !root.join(main_name).exists() {
            remove_file_if_present(&path)?;
        }
    }
    Ok(())
}

fn recovery_failure(batch_id: &str, error: &ImportError) -> String {
    ImportError::new(
        ErrorCode::SnapshotUnavailable,
        format!("prepare batch {batch_id} restore failed: {}", error.message),
    )
    .into_command_err()
}

pub fn latest_recoverable() -> Result<PrepareRecovery, ImportError> {
    let root = batch_root()?;
    let mut latest = None;
    let mut expired = false;
    let mut failed = None;
    let mut removed = false;
    cleanup_orphan_sidecars(&root)?;
    for entry in std::fs::read_dir(&root).map_err(db_failure)? {
        let path = entry.map_err(db_failure)?.path();
        if path.extension().and_then(|value| value.to_str()) != Some("sqlite") {
            continue;
        }
        let Some(batch_id) = path.file_stem().and_then(|value| value.to_str()) else {
            continue;
        };
        if uuid::Uuid::parse_str(batch_id).is_err() {
            continue;
        }
        let metadata = std::fs::symlink_metadata(&path).map_err(db_failure)?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            failed.get_or_insert_with(|| {
                recovery_failure(batch_id, &db_failure("prepare batch is not a regular file"))
            });
            continue;
        }
        let conn = match open_existing(batch_id) {
            Ok(conn) => conn,
            Err(error) => {
                failed.get_or_insert_with(|| recovery_failure(batch_id, &error));
                continue;
            }
        };
        let (retention_expires_unix, state): (i64, String) = match conn.query_row(
            "SELECT retention_expires_unix,state FROM meta WHERE id=1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        ) {
            Ok(value) => value,
            Err(error) => {
                failed.get_or_insert_with(|| recovery_failure(batch_id, &db_failure(error)));
                continue;
            }
        };
        drop(conn);
        if retention_expires_unix <= chrono::Utc::now().timestamp()
            && !super::prepare::batch_is_running(batch_id)
        {
            remove_expired_batch(&path, batch_id)?;
            // A planned batch has handed ownership to a durable plan/job. Its
            // cache cleanup is not an expired unfinished operation.
            expired |= state != "planned";
            removed = true;
            continue;
        }
        let status = match status_raw(batch_id) {
            Ok(status) => status,
            Err(error) => {
                failed.get_or_insert_with(|| recovery_failure(batch_id, &error));
                continue;
            }
        };
        if status.state == PrepareBatchState::Planned {
            continue;
        }
        if latest
            .as_ref()
            .is_none_or(|current: &super::prepare::PrepareBatchStatus| {
                status.created_at > current.created_at
            })
        {
            latest = Some(status);
        }
    }
    if let Some(status) = &latest {
        if status.state.runnable() {
            ensure_running(&status.batch_id)?;
        }
    }
    if removed {
        if let Err(error) = super::jobs::cleanup_unreferenced_preview_dirs() {
            failed.get_or_insert_with(|| recovery_failure("preview-cleanup", &error));
        }
    }
    Ok(PrepareRecovery {
        status: latest,
        expired,
        failed,
    })
}
