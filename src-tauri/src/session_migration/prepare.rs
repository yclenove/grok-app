//! Durable, bounded preview preparation for one Host-owned candidate selection.

use crate::session_migration::sqlite_open;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};

use super::errors::{sqlite_storage_error, ErrorCode, ImportError};
use super::scan::CandidateRef;
use super::selection::{AllowedSelection, PreparedSelectionIdentity, PreviewSelection};

pub use super::prepare_query::{items_page, latest_recoverable, mappings};
#[cfg(test)]
pub use super::prepare_query::{mark_planned, plan_inputs};
pub(crate) use super::prepare_query::{
    preview_is_retained, preview_is_retained_checked, retained_preview_ids, validated_member,
    validated_plan_inputs, ValidatedPreparePlan,
};

pub const PREPARE_BATCH_VERSION: u32 = 1;
pub(crate) const RETENTION_SECS: i64 = 24 * 60 * 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PrepareBatchState {
    Queued,
    Preparing,
    Cancelling,
    Completed,
    CompletedWithIssues,
    Failed,
    Cancelled,
    Planned,
}

impl PrepareBatchState {
    fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Preparing => "preparing",
            Self::Cancelling => "cancelling",
            Self::Completed => "completed",
            Self::CompletedWithIssues => "completed-with-issues",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Planned => "planned",
        }
    }

    fn parse(value: &str) -> Result<Self, ImportError> {
        match value {
            "queued" => Ok(Self::Queued),
            "preparing" => Ok(Self::Preparing),
            "cancelling" => Ok(Self::Cancelling),
            "completed" => Ok(Self::Completed),
            "completed-with-issues" => Ok(Self::CompletedWithIssues),
            "failed" => Ok(Self::Failed),
            "cancelled" => Ok(Self::Cancelled),
            "planned" => Ok(Self::Planned),
            _ => Err(db_failure("invalid prepare batch state")),
        }
    }

    pub(crate) fn runnable(self) -> bool {
        matches!(self, Self::Queued | Self::Preparing | Self::Cancelling)
    }

    pub(crate) fn can_plan(self) -> bool {
        matches!(self, Self::Completed | Self::CompletedWithIssues)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PrepareItemState {
    Queued,
    Preparing,
    Prepared,
    Failed,
    Skipped,
}

impl PrepareItemState {
    pub(crate) fn parse(value: &str) -> Result<Self, ImportError> {
        match value {
            "queued" => Ok(Self::Queued),
            "preparing" => Ok(Self::Preparing),
            "prepared" => Ok(Self::Prepared),
            "failed" => Ok(Self::Failed),
            "skipped" => Ok(Self::Skipped),
            _ => Err(db_failure("invalid prepare item state")),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PrepareOverride {
    pub candidate_id: String,
    pub preview_id: String,
    pub leaf_id: String,
    pub selection: AllowedSelection,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrepareBatchStatus {
    pub version: u32,
    pub batch_id: String,
    pub scan_id: String,
    pub epoch: u64,
    pub selection_id: String,
    pub selection_state_version: u64,
    pub state: PrepareBatchState,
    pub state_version: u64,
    pub total: u32,
    pub prepared: u32,
    pub failed: u32,
    pub skipped: u32,
    pub active: u32,
    pub cancel_requested: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrepareBatchItem {
    pub candidate_id: String,
    pub title: String,
    pub provider: String,
    pub source_revision: String,
    pub state: PrepareItemState,
    pub attempt: u32,
    pub preview_id: Option<String>,
    pub error: Option<ImportError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrepareBatchItemPage {
    pub items: Vec<PrepareBatchItem>,
    pub next_cursor: Option<String>,
}

struct Control {
    cancel: AtomicBool,
    running: AtomicBool,
}

struct RunningGuard {
    control: Arc<Control>,
}

impl Drop for RunningGuard {
    fn drop(&mut self) {
        self.control.running.store(false, Ordering::Release);
    }
}

static CONTROLS: OnceLock<Mutex<HashMap<String, Arc<Control>>>> = OnceLock::new();

#[cfg(test)]
thread_local! {
    static OPEN_EXISTING_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

fn controls() -> &'static Mutex<HashMap<String, Arc<Control>>> {
    CONTROLS.get_or_init(Default::default)
}

pub(crate) fn db_failure(error: impl std::fmt::Display) -> ImportError {
    ImportError::new(ErrorCode::SnapshotUnavailable, error.to_string())
}

pub(crate) fn batch_root() -> Result<PathBuf, ImportError> {
    let root = super::catalog::imports_dir().join("prepare-batches");
    super::catalog::ensure_inside_imports(&root)?;
    std::fs::create_dir_all(&root).map_err(db_failure)?;
    Ok(root)
}

fn batch_path(batch_id: &str) -> Result<PathBuf, ImportError> {
    uuid::Uuid::parse_str(batch_id)
        .map_err(|_| ImportError::new(ErrorCode::PlanStale, "invalid prepare batch id"))?;
    let path = batch_root()?.join(format!("{batch_id}.sqlite"));
    super::catalog::ensure_inside_imports(&path)?;
    Ok(path)
}

fn configure(conn: &Connection) -> Result<(), ImportError> {
    conn.busy_timeout(std::time::Duration::from_millis(super::budget::DB_BUSY_MS))
        .map_err(db_failure)?;
    conn.execute_batch("PRAGMA foreign_keys=ON;")
        .map_err(db_failure)?;
    // The mode lives in the database header. Setting it again runs
    // sqlite3BtreeSetVersion, which takes a write lock. On Windows that
    // returns SQLITE_BUSY ("database is locked") while another transaction
    // is open, and the busy handler is not always invoked. A new batch file
    // is still switched to WAL here; later opens must not repeat the write.
    let mode: String = conn
        .query_row("PRAGMA journal_mode", [], |row| row.get(0))
        .map_err(db_failure)?;
    if !mode.eq_ignore_ascii_case("wal") {
        conn.execute_batch("PRAGMA journal_mode=WAL;")
            .map_err(db_failure)?;
    }
    conn.execute_batch("PRAGMA synchronous=NORMAL;")
        .map_err(db_failure)
}

pub(crate) fn open_existing(batch_id: &str) -> Result<Connection, ImportError> {
    let path = batch_path(batch_id)?;
    if !path.is_file() {
        return Err(ImportError::new(
            ErrorCode::PlanStale,
            "prepare batch no longer exists",
        ));
    }
    let conn = sqlite_open::open(path).map_err(db_failure)?;
    #[cfg(test)]
    OPEN_EXISTING_COUNT.with(|count| count.set(count.get() + 1));
    configure(&conn)?;
    Ok(conn)
}

#[cfg(test)]
pub(crate) fn reset_open_existing_count() {
    OPEN_EXISTING_COUNT.with(|count| count.set(0));
}

#[cfg(test)]
pub(crate) fn open_existing_count() -> usize {
    OPEN_EXISTING_COUNT.with(std::cell::Cell::get)
}

fn create_schema(conn: &Connection) -> Result<(), ImportError> {
    conn.execute_batch(
        "CREATE TABLE meta(
           id INTEGER PRIMARY KEY CHECK(id=1),
           schema_version INTEGER NOT NULL,
           batch_id TEXT NOT NULL UNIQUE,
           scan_id TEXT NOT NULL,
           epoch INTEGER NOT NULL,
           selection_id TEXT NOT NULL,
           selection_version INTEGER NOT NULL,
           state TEXT NOT NULL,
           state_version INTEGER NOT NULL,
           cancel_requested INTEGER NOT NULL CHECK(cancel_requested IN (0,1)),
           created_at TEXT NOT NULL,
           updated_at TEXT NOT NULL,
           retention_expires_unix INTEGER NOT NULL
         );
         CREATE TABLE items(
           seq INTEGER PRIMARY KEY,
           candidate_id TEXT NOT NULL UNIQUE,
           title TEXT NOT NULL,
           provider TEXT NOT NULL,
           source_revision TEXT NOT NULL,
           input_json TEXT NOT NULL,
           state TEXT NOT NULL,
           attempt INTEGER NOT NULL DEFAULT 0,
           preview_id TEXT,
           selection_json TEXT,
           error_json TEXT
         );
         CREATE INDEX items_state_seq ON items(state,seq);",
    )
    .map_err(db_failure)?;
    super::prepare_mapping::create_schema(conn)
}

fn create_batch(
    path: &Path,
    batch_id: &str,
    scan_id: &str,
    epoch: u64,
    selection_id: &str,
    overrides: &[PrepareOverride],
) -> Result<(), ImportError> {
    let selection = super::prepare_selection::get(scan_id, epoch, selection_id)?;
    if selection.count() == 0 {
        return Err(ImportError::new(
            ErrorCode::HistoryAccessUnavailable,
            "prepare batch requires at least one selected candidate",
        ));
    }
    if overrides.len() > super::budget::PAGE_SIZE_MAX {
        return Err(ImportError::new(
            ErrorCode::ResourceLimit,
            "prepare overrides exceed one bounded page",
        ));
    }
    let mut override_ids = HashSet::with_capacity(overrides.len());
    if overrides.iter().any(|entry| {
        entry.candidate_id.trim().is_empty()
            || entry.preview_id.trim().is_empty()
            || entry.leaf_id.trim().is_empty()
            || !override_ids.insert(entry.candidate_id.as_str())
    }) {
        return Err(ImportError::new(
            ErrorCode::HistoryAccessUnavailable,
            "prepare overrides contain invalid or duplicate identities",
        ));
    }

    let mut conn = sqlite_open::open(path).map_err(db_failure)?;
    configure(&conn)?;
    create_schema(&conn)?;
    // Recovery must never observe a queued header without its complete frozen
    // membership and overrides. A killed initializer leaves no runnable batch.
    let tx = conn.transaction().map_err(db_failure)?;
    let now = chrono::Utc::now();
    tx.execute(
        "INSERT INTO meta(id,schema_version,batch_id,scan_id,epoch,selection_id,
          selection_version,state,state_version,cancel_requested,created_at,updated_at,
          retention_expires_unix)
         VALUES(1,?1,?2,?3,?4,?5,?6,'queued',1,0,?7,?7,?8)",
        params![
            PREPARE_BATCH_VERSION,
            batch_id,
            scan_id,
            epoch,
            selection_id,
            selection.state_version(),
            now.to_rfc3339(),
            now.timestamp() + RETENTION_SECS
        ],
    )
    .map_err(db_failure)?;
    #[cfg(test)]
    super::preview_lifecycle_tests::after_publish_point("prepare_header_written");
    let mut cursor = None;
    let mut seq = 0u32;
    loop {
        let (references, next, selected_count) = selection.page(
            scan_id,
            epoch,
            cursor.as_deref(),
            Some(super::budget::PAGE_SIZE_MAX),
        )?;
        if selected_count != selection.count() {
            return Err(ImportError::new(
                ErrorCode::PlanStale,
                "candidate selection changed while preparation was frozen",
            ));
        }
        for reference in references {
            super::source_member::validate_reference(&reference)?;
            seq += 1;
            let candidate = &reference.candidate;
            tx.execute(
                "INSERT INTO items(seq,candidate_id,title,provider,source_revision,input_json,state)
                 VALUES(?1,?2,?3,?4,?5,?6,'queued')",
                params![
                    seq,
                    candidate.candidate_id,
                    candidate.title,
                    candidate.provider.as_str(),
                    candidate.source_revision,
                    serde_json::to_string(&reference).map_err(db_failure)?
                ],
            )
            .map_err(db_failure)?;
        }
        cursor = next;
        if cursor.is_none() {
            break;
        }
    }
    if seq != selection.count() {
        return Err(ImportError::new(
            ErrorCode::PlanStale,
            "candidate selection snapshot count changed",
        ));
    }
    #[cfg(test)]
    super::preview_lifecycle_tests::after_publish_point("prepare_inputs_written");
    let latest = super::prepare_selection::get(scan_id, epoch, selection_id)?;
    if latest != selection {
        return Err(ImportError::new(
            ErrorCode::PlanStale,
            "candidate selection changed during preparation",
        ));
    }
    for entry in overrides {
        apply_override(&tx, batch_id, entry)?;
        #[cfg(test)]
        super::preview_lifecycle_tests::after_publish_point("prepare_override_written");
    }
    tx.commit().map_err(db_failure)
}

pub fn start(
    scan_id: &str,
    epoch: u64,
    selection_id: &str,
    overrides: Vec<PrepareOverride>,
) -> Result<PrepareBatchStatus, ImportError> {
    let batch_id = uuid::Uuid::new_v4().to_string();
    let path = batch_path(&batch_id)?;
    if let Err(error) = create_batch(&path, &batch_id, scan_id, epoch, selection_id, &overrides) {
        let _ = std::fs::remove_file(&path);
        return Err(error);
    }
    #[cfg(test)]
    super::preview_lifecycle_tests::after_publish_point("prepare_queued");
    ensure_running(&batch_id)?;
    status_raw(&batch_id)
}

fn apply_override(
    tx: &rusqlite::Transaction<'_>,
    batch_id: &str,
    entry: &PrepareOverride,
) -> Result<(), ImportError> {
    let _operation = super::preview_lifecycle::operation()?;
    let input: Option<String> = tx
        .query_row(
            "SELECT input_json FROM items WHERE candidate_id=?1",
            params![entry.candidate_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(db_failure)?;
    let Some(input) = input else {
        return Ok(());
    };
    let reference: CandidateRef = serde_json::from_str(&input).map_err(db_failure)?;
    let path = super::jobs::find_preview_dir(&entry.preview_id)?
        .ok_or_else(|| ImportError::new(ErrorCode::PlanStale, "preview expired"))?;
    let (lease, _) = super::jobs::verify_disk_preview_integrity(&path, &entry.preview_id)?;
    super::source_member::validate_reference_lease(&reference, &lease)?;
    if lease.expires_unix <= chrono::Utc::now().timestamp()
        || lease.candidate_id != reference.candidate.candidate_id
        || lease.source_revision != reference.candidate.source_revision
    {
        return Err(ImportError::new(
            ErrorCode::PlanStale,
            "prepare override does not match the selected source revision",
        ));
    }
    let selection = canonical_selection(PreviewSelection {
        preview_id: entry.preview_id.clone(),
        leaf_id: entry.leaf_id.clone(),
        selection: entry.selection.clone(),
    });
    let verified = super::selection::validate_prepared_selection(
        &selection,
        &path,
        PreparedSelectionIdentity {
            preview_id: Some(&entry.preview_id),
            candidate_id: Some(&reference.candidate.candidate_id),
            source_id: Some(&reference.source_id),
            root_path: Some(&reference.root_path),
            legacy_id: Some(&reference.legacy_id),
            ..PreparedSelectionIdentity::default()
        },
    )?;
    super::jobs::adopt_preview_for_batch(&path, batch_id)?;
    tx.execute(
        "UPDATE items SET state='prepared',preview_id=?2,selection_json=?3,error_json=NULL
         WHERE candidate_id=?1",
        params![
            entry.candidate_id,
            entry.preview_id,
            serde_json::to_string(&selection).map_err(db_failure)?
        ],
    )
    .map_err(db_failure)?;
    super::prepare_mapping::store(
        tx,
        &entry.candidate_id,
        &entry.preview_id,
        &super::mapping_scope::MappingSubject::new(
            verified.source_id,
            verified.provider,
            verified.cwd,
        ),
    )
}

pub(crate) fn status_raw(batch_id: &str) -> Result<PrepareBatchStatus, ImportError> {
    let conn = open_status(batch_id)?;
    conn.query_row(
        "SELECT schema_version,batch_id,scan_id,epoch,selection_id,selection_version,
          state,state_version,cancel_requested,created_at,updated_at,
          (SELECT COUNT(*) FROM items),
          (SELECT COUNT(*) FROM items WHERE state='prepared'),
          (SELECT COUNT(*) FROM items WHERE state='failed'),
          (SELECT COUNT(*) FROM items WHERE state='skipped'),
          (SELECT COUNT(*) FROM items WHERE state='preparing')
         FROM meta WHERE id=1",
        [],
        |row| {
            let raw_state: String = row.get(6)?;
            Ok((
                row.get::<_, u32>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, u64>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, u64>(5)?,
                raw_state,
                row.get::<_, u64>(7)?,
                row.get::<_, bool>(8)?,
                row.get::<_, String>(9)?,
                row.get::<_, String>(10)?,
                row.get::<_, u32>(11)?,
                row.get::<_, u32>(12)?,
                row.get::<_, u32>(13)?,
                row.get::<_, u32>(14)?,
                row.get::<_, u32>(15)?,
            ))
        },
    )
    .map_err(sqlite_storage_error)
    .and_then(
        |(
            version,
            batch_id,
            scan_id,
            epoch,
            selection_id,
            selection_state_version,
            state,
            state_version,
            cancel_requested,
            created_at,
            updated_at,
            total,
            prepared,
            failed,
            skipped,
            active,
        )| {
            if version != PREPARE_BATCH_VERSION {
                return Err(ImportError::new(
                    ErrorCode::SchemaUnsupported,
                    "unsupported prepare batch schema",
                ));
            }
            Ok(PrepareBatchStatus {
                version,
                batch_id,
                scan_id,
                epoch,
                selection_id,
                selection_state_version,
                state: PrepareBatchState::parse(&state)?,
                state_version,
                total,
                prepared,
                failed,
                skipped,
                active,
                cancel_requested,
                created_at,
                updated_at,
            })
        },
    )
}

#[path = "prepare_status.rs"]
mod status_read;
use status_read::open_status;
pub use status_read::status;

fn control_for(batch_id: &str) -> Arc<Control> {
    let mut guard = controls()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    Arc::clone(guard.entry(batch_id.to_string()).or_insert_with(|| {
        Arc::new(Control {
            cancel: AtomicBool::new(false),
            running: AtomicBool::new(false),
        })
    }))
}

pub(crate) fn ensure_running(batch_id: &str) -> Result<(), ImportError> {
    let current = status_raw(batch_id)?;
    if !current.state.runnable() {
        return Ok(());
    }
    let control = control_for(batch_id);
    control
        .cancel
        .store(current.cancel_requested, Ordering::Release);
    if control
        .running
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Ok(());
    }
    let running_guard = RunningGuard {
        control: Arc::clone(&control),
    };
    recover_abandoned(batch_id)?;
    let id = batch_id.to_string();
    std::thread::Builder::new()
        .name(format!("session-import-prepare-{batch_id}"))
        .spawn(move || {
            let _running_guard = running_guard;
            let _ = run_batch(&id, &control);
        })
        .map_err(db_failure)?;
    Ok(())
}

pub(crate) fn batch_is_running(batch_id: &str) -> bool {
    controls()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(batch_id)
        .is_some_and(|control| control.running.load(Ordering::Acquire))
}

fn recover_abandoned(batch_id: &str) -> Result<(), ImportError> {
    let conn = open_existing(batch_id)?;
    conn.execute(
        "UPDATE items SET state='queued' WHERE state='preparing'",
        [],
    )
    .map_err(db_failure)?;
    Ok(())
}

fn should_cancel(batch_id: &str, control: &Control) -> Result<bool, ImportError> {
    if control.cancel.load(Ordering::Acquire) {
        return Ok(true);
    }
    let conn = open_existing(batch_id)?;
    conn.query_row("SELECT cancel_requested FROM meta WHERE id=1", [], |row| {
        row.get(0)
    })
    .map_err(db_failure)
}

fn claim_next(batch_id: &str) -> Result<Option<(u32, CandidateRef)>, ImportError> {
    let mut conn = open_existing(batch_id)?;
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(db_failure)?;
    let (cancelled, raw_state): (bool, String) = tx
        .query_row(
            "SELECT cancel_requested,state FROM meta WHERE id=1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(db_failure)?;
    let state = PrepareBatchState::parse(&raw_state)?;
    if cancelled
        || !matches!(
            state,
            PrepareBatchState::Queued | PrepareBatchState::Preparing
        )
    {
        tx.commit().map_err(db_failure)?;
        return Ok(None);
    }
    let row: Option<(u32, String)> = tx
        .query_row(
            "SELECT seq,input_json FROM items WHERE state='queued' ORDER BY seq LIMIT 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(db_failure)?;
    let Some((seq, raw)) = row else {
        tx.commit().map_err(db_failure)?;
        return Ok(None);
    };
    let reference: CandidateRef = serde_json::from_str(&raw).map_err(db_failure)?;
    let claimed = tx
        .execute(
            "UPDATE items SET state='preparing',attempt=attempt+1 WHERE seq=?1 AND state='queued'",
            params![seq],
        )
        .map_err(db_failure)?;
    if claimed != 1 {
        return Err(db_failure("prepare item claim changed no rows"));
    }
    let advanced = tx
        .execute(
            "UPDATE meta SET state='preparing',state_version=state_version+1,updated_at=?1
         WHERE id=1 AND cancel_requested=0 AND state IN ('queued','preparing')",
            params![chrono::Utc::now().to_rfc3339()],
        )
        .map_err(db_failure)?;
    if advanced != 1 {
        return Err(db_failure("prepare batch rejected item claim"));
    }
    tx.commit().map_err(db_failure)?;
    Ok(Some((seq, reference)))
}

#[cfg(test)]
pub(crate) fn claim_next_for_test(
    batch_id: &str,
) -> Result<Option<(u32, CandidateRef)>, ImportError> {
    claim_next(batch_id)
}

fn persist_failure(batch_id: &str, seq: u32, error: &ImportError) -> Result<(), ImportError> {
    let conn = open_existing(batch_id)?;
    conn.execute(
        "UPDATE items SET state='failed',preview_id=NULL,selection_json=NULL,error_json=?2
         WHERE seq=?1 AND state='preparing'",
        params![seq, serde_json::to_string(error).map_err(db_failure)?],
    )
    .map_err(db_failure)?;
    conn.execute(
        "UPDATE meta SET state_version=state_version+1,updated_at=?1 WHERE id=1",
        params![chrono::Utc::now().to_rfc3339()],
    )
    .map_err(db_failure)?;
    Ok(())
}

fn worker_loop(batch_id: &str, control: &Control) -> Result<(), ImportError> {
    while !should_cancel(batch_id, control)? {
        let _operation = super::preview_lifecycle::operation()?;
        let Some((seq, reference)) = claim_next(batch_id)? else {
            break;
        };
        #[cfg(test)]
        super::preview_lifecycle_tests::after_publish_point("prepare_claimed");
        let source_revision = reference.candidate.source_revision.clone();
        let candidate_id = reference.candidate.candidate_id.clone();
        let source_id = reference.source_id.clone();
        let provider = reference.candidate.provider.as_str().to_string();
        match super::engine::preview_reference(reference, Some(&source_revision), Some(batch_id)) {
            Ok((preview_id, details)) => {
                #[cfg(test)]
                super::preview_lifecycle_tests::after_publish_point("prepare_preview_ready");
                let leaf_id = details.active_leaf.clone().ok_or_else(|| {
                    ImportError::new(ErrorCode::PartialHistory, "preview has no branch leaf")
                });
                match leaf_id {
                    Ok(leaf_id) => super::prepare_mapping::persist_success(
                        batch_id,
                        seq,
                        &preview_id,
                        &PreviewSelection {
                            preview_id: preview_id.clone(),
                            leaf_id,
                            selection: details.default_selection,
                        },
                        &candidate_id,
                        &super::mapping_scope::MappingSubject::new(
                            source_id,
                            provider,
                            details.cwd,
                        ),
                    )?,
                    Err(error) => persist_failure(batch_id, seq, &error)?,
                }
            }
            Err(error) => persist_failure(batch_id, seq, &error)?,
        }
        #[cfg(test)]
        super::preview_lifecycle_tests::after_publish_point("prepare_item_recorded");
    }
    Ok(())
}

fn canonical_selection(mut value: PreviewSelection) -> PreviewSelection {
    value.selection = value.selection.canonicalized();
    value
}

fn run_batch(batch_id: &str, control: &Arc<Control>) -> Result<(), ImportError> {
    let mut workers = Vec::new();
    for _ in 0..super::budget::READ_CONCURRENCY {
        let id = batch_id.to_string();
        let control = Arc::clone(control);
        workers.push(std::thread::spawn(move || worker_loop(&id, &control)));
    }
    let mut worker_error = None;
    for worker in workers {
        match worker.join() {
            Ok(Ok(())) => {}
            Ok(Err(error)) => worker_error = Some(error),
            Err(_) => {
                worker_error = Some(db_failure("prepare worker panicked"));
            }
        }
    }
    finalize(batch_id, worker_error.as_ref())?;
    #[cfg(test)]
    super::preview_lifecycle_tests::after_publish_point("prepare_batch_finished");
    Ok(())
}

fn finalize(batch_id: &str, worker_error: Option<&ImportError>) -> Result<(), ImportError> {
    let mut conn = open_existing(batch_id)?;
    let tx = conn.transaction().map_err(db_failure)?;
    let cancel_requested: bool = tx
        .query_row("SELECT cancel_requested FROM meta WHERE id=1", [], |row| {
            row.get(0)
        })
        .map_err(db_failure)?;
    if cancel_requested {
        tx.execute("UPDATE items SET state='skipped' WHERE state='queued'", [])
            .map_err(db_failure)?;
    }
    if let Some(error) = worker_error {
        tx.execute(
            "UPDATE items SET state='failed',error_json=?1 WHERE state='preparing'",
            params![serde_json::to_string(error).map_err(db_failure)?],
        )
        .map_err(db_failure)?;
    }
    let (total, prepared, failed, skipped, active, queued): (u32, u32, u32, u32, u32, u32) = tx
        .query_row(
            "SELECT COUNT(*),
              SUM(CASE WHEN state='prepared' THEN 1 ELSE 0 END),
              SUM(CASE WHEN state='failed' THEN 1 ELSE 0 END),
              SUM(CASE WHEN state='skipped' THEN 1 ELSE 0 END),
              SUM(CASE WHEN state='preparing' THEN 1 ELSE 0 END),
              SUM(CASE WHEN state='queued' THEN 1 ELSE 0 END)
             FROM items",
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            },
        )
        .map_err(db_failure)?;
    let state = if cancel_requested {
        PrepareBatchState::Cancelled
    } else if active > 0 || queued > 0 {
        PrepareBatchState::Failed
    } else if prepared == total {
        PrepareBatchState::Completed
    } else if prepared > 0 && failed + skipped > 0 {
        PrepareBatchState::CompletedWithIssues
    } else {
        PrepareBatchState::Failed
    };
    tx.execute(
        "UPDATE meta SET state=?1,state_version=state_version+1,updated_at=?2 WHERE id=1",
        params![state.as_str(), chrono::Utc::now().to_rfc3339()],
    )
    .map_err(db_failure)?;
    tx.commit().map_err(db_failure)?;
    Ok(())
}

pub fn cancel(batch_id: &str) -> Result<PrepareBatchStatus, ImportError> {
    let mut conn = open_existing(batch_id)?;
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(db_failure)?;
    let raw_state: String = tx
        .query_row("SELECT state FROM meta WHERE id=1", [], |row| row.get(0))
        .map_err(db_failure)?;
    let state = PrepareBatchState::parse(&raw_state)?;
    if !state.runnable() {
        tx.commit().map_err(db_failure)?;
        return status_raw(batch_id);
    }
    tx.execute("UPDATE items SET state='skipped' WHERE state='queued'", [])
        .map_err(db_failure)?;
    let active: u32 = tx
        .query_row(
            "SELECT COUNT(*) FROM items WHERE state='preparing'",
            [],
            |row| row.get(0),
        )
        .map_err(db_failure)?;
    let changed = tx
        .execute(
            "UPDATE meta SET cancel_requested=1,state=?1,state_version=state_version+1,
          updated_at=?2 WHERE id=1 AND state IN ('queued','preparing','cancelling')",
            params![
                if active > 0 {
                    "cancelling"
                } else {
                    "cancelled"
                },
                chrono::Utc::now().to_rfc3339()
            ],
        )
        .map_err(db_failure)?;
    if changed != 1 {
        return Err(db_failure("prepare cancellation lost its state transition"));
    }
    tx.commit().map_err(db_failure)?;
    if let Some(control) = controls()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(batch_id)
        .cloned()
    {
        control.cancel.store(true, Ordering::Release);
    }
    status_raw(batch_id)
}

pub fn retry(batch_id: &str) -> Result<PrepareBatchStatus, ImportError> {
    if controls()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(batch_id)
        .is_some_and(|control| control.running.load(Ordering::Acquire))
    {
        return Err(ImportError::new(
            ErrorCode::SourceBusy,
            "prepare batch is still running",
        ));
    }
    let mut conn = open_existing(batch_id)?;
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(db_failure)?;
    let raw_state: String = tx
        .query_row("SELECT state FROM meta WHERE id=1", [], |row| row.get(0))
        .map_err(db_failure)?;
    let state = PrepareBatchState::parse(&raw_state)?;
    if !matches!(
        state,
        PrepareBatchState::Failed
            | PrepareBatchState::Cancelled
            | PrepareBatchState::CompletedWithIssues
    ) {
        return Err(ImportError::new(
            ErrorCode::PlanStale,
            "prepare batch cannot be retried from its current state",
        ));
    }
    tx.execute(
        "UPDATE items SET state='queued',error_json=NULL WHERE state IN ('failed','skipped')",
        [],
    )
    .map_err(db_failure)?;
    let changed = tx
        .execute(
            "UPDATE meta SET cancel_requested=0,state='queued',state_version=state_version+1,
          updated_at=?1,retention_expires_unix=?2 WHERE id=1 AND state=?3",
            params![
                chrono::Utc::now().to_rfc3339(),
                chrono::Utc::now().timestamp() + RETENTION_SECS,
                raw_state
            ],
        )
        .map_err(db_failure)?;
    if changed != 1 {
        return Err(db_failure("prepare retry lost its state transition"));
    }
    tx.commit().map_err(db_failure)?;
    let control = control_for(batch_id);
    control.cancel.store(false, Ordering::Release);
    ensure_running(batch_id)?;
    status_raw(batch_id)
}

#[cfg(test)]
pub fn reset_runtime() {
    if let Some(controls) = CONTROLS.get() {
        controls
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clear();
    }
}

#[cfg(test)]
pub(crate) fn running_for_test(batch_id: &str) -> bool {
    batch_is_running(batch_id)
}

#[cfg(test)]
#[path = "capacity_tests.rs"]
mod capacity_tests;
