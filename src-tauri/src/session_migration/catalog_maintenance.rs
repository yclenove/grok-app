use crate::session_migration::sqlite_open;
use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{LazyLock, Mutex};

use fs2::FileExt;
use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::*;
use crate::session_migration::errors::{ErrorCode, ImportError};
use crate::session_migration::jsonl::open_jsonl;
use crate::session_migration::types::{ImportedEvent, CATALOG_SCHEMA_VERSION};

const CATALOG_REBUILD_LOCK: &str = "catalog-rebuild.lock";
const CATALOG_REBUILD_BACKUP: &str = "catalog.sqlite.rebuild-backup";
const STAGING_TABLE: &str = "body_search_chunks_v2_staging";

const STAGING_SCHEMA_SQL: &str = r#"
CREATE TABLE body_search_chunks_v2_staging (
  import_id TEXT NOT NULL,
  revision_id TEXT NOT NULL,
  chunk_ordinal INTEGER NOT NULL,
  text TEXT NOT NULL,
  PRIMARY KEY (import_id, revision_id, chunk_ordinal)
);
"#;

const V2_SUPPORT_SCHEMA_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS plans (
  plan_id TEXT PRIMARY KEY,
  plan_hash TEXT NOT NULL,
  json TEXT NOT NULL,
  created_at TEXT NOT NULL,
  expires_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS drafts (
  draft_id TEXT PRIMARY KEY,
  draft_hash TEXT NOT NULL,
  import_id TEXT NOT NULL,
  json TEXT NOT NULL,
  created_at TEXT NOT NULL,
  expires_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS migration_log (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  from_version INTEGER NOT NULL,
  to_version INTEGER NOT NULL,
  applied_at TEXT NOT NULL
);
"#;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CatalogRebuildPhase {
    Idle,
    Checking,
    BackingUp,
    Rebuilding,
    Verifying,
    Committing,
    Complete,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogRebuildReport {
    pub from_version: u32,
    pub to_version: u32,
    pub revisions_scanned: u32,
    pub revisions_rebuilt: u32,
    pub chunks_written: u64,
    pub searchable_bytes: u64,
    pub projection_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogRebuildStatus {
    pub running: bool,
    pub phase: CatalogRebuildPhase,
    pub completed_revisions: u32,
    pub total_revisions: u32,
    pub report: Option<CatalogRebuildReport>,
    pub error_code: Option<ErrorCode>,
}

impl Default for CatalogRebuildStatus {
    fn default() -> Self {
        Self {
            running: false,
            phase: CatalogRebuildPhase::Idle,
            completed_revisions: 0,
            total_revisions: 0,
            report: None,
            error_code: None,
        }
    }
}

static REBUILD_RUNNING: AtomicBool = AtomicBool::new(false);
static REBUILD_CANCEL: AtomicBool = AtomicBool::new(false);
static REBUILD_STATUS: LazyLock<Mutex<CatalogRebuildStatus>> =
    LazyLock::new(|| Mutex::new(CatalogRebuildStatus::default()));

struct RebuildRunGuard;

impl Drop for RebuildRunGuard {
    fn drop(&mut self) {
        REBUILD_RUNNING.store(false, Ordering::Release);
        let mut state = REBUILD_STATUS.lock().unwrap_or_else(|p| p.into_inner());
        state.running = false;
    }
}

pub fn catalog_rebuild_status() -> CatalogRebuildStatus {
    REBUILD_STATUS
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .clone()
}

pub fn cancel_catalog_rebuild() -> bool {
    if !REBUILD_RUNNING.load(Ordering::Acquire) {
        return false;
    }
    REBUILD_CANCEL.store(true, Ordering::Release);
    true
}

pub fn run_catalog_rebuild() -> Result<CatalogRebuildReport, ImportError> {
    if REBUILD_RUNNING
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Err(ImportError::new(
            ErrorCode::SourceBusy,
            "an imported-history index rebuild is already running",
        ));
    }
    let _guard = RebuildRunGuard;
    REBUILD_CANCEL.store(false, Ordering::Release);
    {
        let mut state = REBUILD_STATUS.lock().unwrap_or_else(|p| p.into_inner());
        *state = CatalogRebuildStatus {
            running: true,
            phase: CatalogRebuildPhase::Checking,
            ..CatalogRebuildStatus::default()
        };
    }

    let result = rebuild_catalog_index_with_hooks(
        &|| REBUILD_CANCEL.load(Ordering::Acquire),
        &mut |phase, completed, total| {
            let mut state = REBUILD_STATUS.lock().unwrap_or_else(|p| p.into_inner());
            state.phase = phase;
            state.completed_revisions = completed;
            state.total_revisions = total;
        },
    );
    let mut state = REBUILD_STATUS.lock().unwrap_or_else(|p| p.into_inner());
    match &result {
        Ok(report) => {
            state.phase = CatalogRebuildPhase::Complete;
            state.completed_revisions = report.revisions_rebuilt;
            state.total_revisions = report.revisions_scanned;
            state.report = Some(report.clone());
            state.error_code = None;
        }
        Err(error) => {
            state.phase = if REBUILD_CANCEL.load(Ordering::Acquire) {
                CatalogRebuildPhase::Cancelled
            } else {
                CatalogRebuildPhase::Failed
            };
            state.error_code = Some(error.code);
        }
    }
    drop(state);
    result
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PublishedRevision {
    import_id: String,
    revision_id: String,
    revision_dir: PathBuf,
}

pub(crate) fn rebuild_catalog_index_with_hooks(
    cancel: &dyn Fn() -> bool,
    progress: &mut dyn FnMut(CatalogRebuildPhase, u32, u32),
) -> Result<CatalogRebuildReport, ImportError> {
    let catalog = catalog_path();
    if !catalog.is_file() && !backup_path().is_file() {
        return Err(ImportError::new(
            ErrorCode::HistoryAccessUnavailable,
            "the imported-history catalog does not exist",
        ));
    }
    let lock = acquire_rebuild_lock()?;
    recover_catalog_backup_if_needed(&catalog)?;
    if !catalog.is_file() {
        return Err(ImportError::new(
            ErrorCode::HistoryAccessUnavailable,
            "the imported-history catalog could not be recovered",
        ));
    }
    check_cancel(cancel)?;

    progress(CatalogRebuildPhase::Checking, 0, 0);
    let preflight = open_catalog_for_maintenance(&catalog)?;
    let from_version = supported_rebuild_version(&preflight)?;
    crate::session_migration::source_location_store::verify_owners(&preflight)?;
    crate::session_migration::registration_provenance_store::verify_schema(&preflight)?;
    let revisions = collect_published_revisions(&preflight)?;
    let total = revisions.len().try_into().unwrap_or(u32::MAX);
    let estimated_bytes = estimate_rebuild_bytes(&catalog, &revisions);
    ensure_rebuild_disk_space(&catalog, estimated_bytes)?;
    drop(preflight);

    check_cancel(cancel)?;
    progress(CatalogRebuildPhase::BackingUp, 0, total);
    let backup = backup_path();
    crate::session_migration::sqlite_ro::backup_readonly_to(&catalog, &backup)?;

    let outcome = (|| {
        let mut conn = open_catalog_for_maintenance(&catalog)?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Exclusive)
            .map_err(|error| catalog_db_error("start catalog rebuild", error))?;
        let locked_version = supported_rebuild_version(&tx)?;
        crate::session_migration::source_location_store::verify_owners(&tx)?;
        crate::session_migration::registration_provenance_store::verify_schema(&tx)?;
        let locked_revisions = collect_published_revisions(&tx)?;
        if locked_version != from_version || locked_revisions != revisions {
            return Err(ImportError::new(
                ErrorCode::SourceChanged,
                "the imported-history catalog changed during rebuild setup",
            ));
        }

        rebuild_catalog_transaction(tx, from_version, &locked_revisions, cancel, progress)
    })();

    match outcome {
        Ok(report) => {
            // A committed v2 catalog is already authoritative. A backup that
            // cannot be removed is harmless and is cleared after the next
            // successful integrity check; do not report a completed rebuild as
            // failed only because antivirus briefly held the backup file.
            let _ = fs::remove_file(&backup);
            drop(lock);
            Ok(report)
        }
        Err(error) => {
            // The SQLite transaction rolls back atomically. Keep the preflight
            // backup so a killed process or later integrity failure is recoverable.
            drop(lock);
            Err(error)
        }
    }
}

fn rebuild_catalog_transaction(
    tx: Transaction<'_>,
    from_version: u32,
    revisions: &[PublishedRevision],
    cancel: &dyn Fn() -> bool,
    progress: &mut dyn FnMut(CatalogRebuildPhase, u32, u32),
) -> Result<CatalogRebuildReport, ImportError> {
    drop_schema_object(&tx, STAGING_TABLE)?;
    tx.execute_batch(STAGING_SCHEMA_SQL)
        .map_err(|error| catalog_db_error("create catalog projection staging table", error))?;

    let total = revisions.len().try_into().unwrap_or(u32::MAX);
    let mut expected_hash = Sha256::new();
    let mut chunks_written = 0u64;
    let mut searchable_bytes = 0u64;
    for (index, revision) in revisions.iter().enumerate() {
        check_cancel(cancel)?;
        progress(CatalogRebuildPhase::Rebuilding, index as u32, total);
        verify_revision(revision, &tx)?;
        let (chunks, bytes) = project_revision(&tx, revision, cancel, &mut expected_hash)?;
        chunks_written = chunks_written.saturating_add(chunks);
        searchable_bytes = searchable_bytes.saturating_add(bytes);
        progress(
            CatalogRebuildPhase::Rebuilding,
            (index + 1).try_into().unwrap_or(u32::MAX),
            total,
        );
    }

    check_cancel(cancel)?;
    progress(CatalogRebuildPhase::Verifying, total, total);
    let expected_hash = hex::encode(expected_hash.finalize());
    let (actual_revisions, actual_chunks, actual_bytes, actual_hash) =
        verify_staged_projection(&tx)?;
    if actual_revisions != revisions.len() as u64
        || actual_chunks != chunks_written
        || actual_bytes != searchable_bytes
        || actual_hash != expected_hash
    {
        return Err(ImportError::new(
            ErrorCode::SnapshotUnavailable,
            "rebuilt imported-history projection did not verify",
        ));
    }

    check_cancel(cancel)?;
    progress(CatalogRebuildPhase::Committing, total, total);
    drop_schema_object(&tx, "body_search_chunks")?;
    tx.execute_batch(&format!(
        "ALTER TABLE {STAGING_TABLE} RENAME TO body_search_chunks;"
    ))
    .map_err(|error| catalog_db_error("activate rebuilt catalog projection", error))?;
    drop_schema_object(&tx, "body_search")?;
    tx.execute_batch(V2_SUPPORT_SCHEMA_SQL)
        .map_err(|error| catalog_db_error("create catalog v2 support tables", error))?;
    tx.execute(
        "UPDATE meta SET value=?1 WHERE key='schema_version'",
        params![CATALOG_SCHEMA_VERSION.to_string()],
    )
    .map_err(|error| catalog_db_error("update catalog schema version", error))?;
    if from_version != CATALOG_SCHEMA_VERSION {
        tx.execute(
            "INSERT INTO migration_log(from_version, to_version, applied_at) VALUES (?1, ?2, ?3)",
            params![
                from_version,
                CATALOG_SCHEMA_VERSION,
                chrono::Utc::now().to_rfc3339()
            ],
        )
        .map_err(|error| catalog_db_error("record catalog migration", error))?;
    }
    verify_body_search_schema(&tx)?;
    crate::session_migration::source_location_store::verify_owners(&tx)?;
    tx.commit()
        .map_err(|error| catalog_db_error("commit catalog rebuild", error))?;

    Ok(CatalogRebuildReport {
        from_version,
        to_version: CATALOG_SCHEMA_VERSION,
        revisions_scanned: total,
        revisions_rebuilt: total,
        chunks_written,
        searchable_bytes,
        projection_sha256: expected_hash,
    })
}

fn acquire_rebuild_lock() -> Result<File, ImportError> {
    let path = imports_dir().join(CATALOG_REBUILD_LOCK);
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .map_err(|error| ImportError::new(ErrorCode::SnapshotUnavailable, error.to_string()))?;
    file.try_lock_exclusive().map_err(|_| {
        ImportError::new(
            ErrorCode::SourceBusy,
            "another imported-history catalog maintenance operation is running",
        )
    })?;
    Ok(file)
}

fn backup_path() -> PathBuf {
    imports_dir().join(CATALOG_REBUILD_BACKUP)
}

fn recover_catalog_backup_if_needed(catalog: &Path) -> Result<(), ImportError> {
    let backup = backup_path();
    if !backup.exists() {
        return Ok(());
    }
    if !catalog.exists() {
        fs::rename(&backup, catalog).map_err(|error| {
            ImportError::new(
                ErrorCode::SnapshotUnavailable,
                format!("restore catalog rebuild backup: {error}"),
            )
        })?;
        return Ok(());
    }
    if catalog_integrity_ok(catalog) {
        fs::remove_file(&backup).map_err(|error| {
            ImportError::new(
                ErrorCode::SnapshotUnavailable,
                format!("remove stale catalog rebuild backup: {error}"),
            )
        })?;
        return Ok(());
    }
    Err(ImportError::new(
        ErrorCode::SnapshotUnavailable,
        "the catalog failed integrity checking; its rebuild backup was preserved",
    ))
}

fn catalog_integrity_ok(path: &Path) -> bool {
    let Ok(conn) = sqlite_open::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
    else {
        return false;
    };
    conn.query_row("PRAGMA quick_check", [], |row| row.get::<_, String>(0))
        .is_ok_and(|result| result.eq_ignore_ascii_case("ok"))
}

fn open_catalog_for_maintenance(path: &Path) -> Result<Connection, ImportError> {
    let conn = sqlite_open::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE)
        .map_err(|error| catalog_db_error("open catalog for maintenance", error))?;
    conn.busy_timeout(std::time::Duration::from_millis(
        crate::session_migration::budget::DB_BUSY_MS,
    ))
    .map_err(|error| catalog_db_error("set catalog maintenance timeout", error))?;
    Ok(conn)
}

fn supported_rebuild_version(conn: &Connection) -> Result<u32, ImportError> {
    match read_schema_version(conn).map_err(normalize_maintenance_error)? {
        Some(version) if version == 1 || version == CATALOG_SCHEMA_VERSION => Ok(version),
        Some(version) if version > CATALOG_SCHEMA_VERSION => Err(ImportError::new(
            ErrorCode::ImportSchemaTooNew,
            format!("catalog schema {version} is newer than this app"),
        )),
        Some(version) => Err(ImportError::new(
            ErrorCode::SchemaUnsupported,
            format!("catalog schema {version} cannot be rebuilt by this app"),
        )),
        None => Err(ImportError::new(
            ErrorCode::SchemaUnsupported,
            "the imported-history catalog has no supported schema version",
        )),
    }
}

fn collect_published_revisions(conn: &Connection) -> Result<Vec<PublishedRevision>, ImportError> {
    let mut stmt = conn
        .prepare(
            "SELECT e.import_id, e.current_revision, r.dir_path
             FROM entries e
             LEFT JOIN revisions r ON r.import_id=e.import_id
              AND r.revision_id=e.current_revision
             WHERE e.current_revision IS NOT NULL
             ORDER BY e.import_id ASC, e.current_revision ASC",
        )
        .map_err(|error| catalog_db_error("read catalog revisions", error))?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
            ))
        })
        .map_err(|error| catalog_db_error("read catalog revisions", error))?;
    let mut revisions = Vec::new();
    for row in rows {
        let (import_id, revision_id, stored_dir) =
            row.map_err(|error| catalog_db_error("read catalog revision", error))?;
        let stored_dir = stored_dir.ok_or_else(|| {
            ImportError::new(
                ErrorCode::SnapshotUnavailable,
                "a current imported-history revision is missing from the catalog",
            )
        })?;
        let expected_dir = revision_dir(&import_id, &revision_id)?;
        let stored = PathBuf::from(stored_dir);
        let stored = stored.canonicalize().map_err(|_| {
            ImportError::new(
                ErrorCode::HistoryAccessUnavailable,
                "a current imported-history revision storage path is unavailable",
            )
        })?;
        let expected = expected_dir.canonicalize().map_err(|_| {
            ImportError::new(
                ErrorCode::HistoryAccessUnavailable,
                "a current imported-history revision directory is unavailable",
            )
        })?;
        if stored != expected {
            return Err(ImportError::new(
                ErrorCode::HistoryAccessUnavailable,
                "a current imported-history revision has an invalid storage path",
            ));
        }
        revisions.push(PublishedRevision {
            import_id,
            revision_id,
            revision_dir: expected_dir,
        });
    }
    Ok(revisions)
}

fn estimate_rebuild_bytes(catalog: &Path, revisions: &[PublishedRevision]) -> u64 {
    let catalog_bytes = fs::metadata(catalog).map(|meta| meta.len()).unwrap_or(0);
    let archive_bytes = revisions.iter().fold(0u64, |total, revision| {
        total.saturating_add(
            fs::metadata(revision.revision_dir.join("events.jsonl"))
                .map(|meta| meta.len())
                .unwrap_or(0),
        )
    });
    catalog_bytes
        .saturating_mul(2)
        .saturating_add(archive_bytes)
}

fn ensure_rebuild_disk_space(path: &Path, estimated_bytes: u64) -> Result<(), ImportError> {
    let available = fs2::available_space(path).map_err(|error| {
        ImportError::new(
            ErrorCode::SnapshotUnavailable,
            format!("read catalog volume capacity: {error}"),
        )
    })?;
    validate_rebuild_disk_space(available, estimated_bytes)
}

pub(crate) fn validate_rebuild_disk_space(
    available: u64,
    estimated_bytes: u64,
) -> Result<(), ImportError> {
    crate::session_migration::jobs::require_disk_space(available, estimated_bytes)
}

fn verify_revision(revision: &PublishedRevision, conn: &Connection) -> Result<(), ImportError> {
    let receipt = crate::session_migration::jobs::latest_receipt_for_revision(
        conn,
        &revision.import_id,
        &revision.revision_id,
    )?
    .ok_or_else(|| {
        ImportError::new(
            ErrorCode::HistoryAccessUnavailable,
            "a current imported-history revision has no committed receipt",
        )
    })?;
    if receipt.phase != "committed" {
        return Err(ImportError::new(
            ErrorCode::HistoryAccessUnavailable,
            "a current imported-history revision is not committed",
        ));
    }
    crate::session_migration::jobs::verify_published(&revision.revision_dir, &receipt)?;
    Ok(())
}

fn project_revision(
    conn: &Connection,
    revision: &PublishedRevision,
    cancel: &dyn Fn() -> bool,
    hasher: &mut Sha256,
) -> Result<(u64, u64), ImportError> {
    let mut insert = conn
        .prepare(&format!(
            "INSERT INTO {STAGING_TABLE}(import_id, revision_id, chunk_ordinal, text)
             VALUES (?1, ?2, ?3, ?4)"
        ))
        .map_err(|error| catalog_db_error("prepare rebuilt catalog projection", error))?;
    let mut chunks = 0u64;
    let mut bytes = 0u64;
    let mut emit = |ordinal: i64, text: &str| -> Result<(), ImportError> {
        insert
            .execute(params![
                revision.import_id,
                revision.revision_id,
                ordinal,
                text
            ])
            .map_err(|error| catalog_db_error("write rebuilt catalog projection", error))?;
        feed_projection_hash(
            hasher,
            &revision.import_id,
            &revision.revision_id,
            ordinal,
            text,
        );
        chunks = chunks.saturating_add(1);
        bytes = bytes.saturating_add(text.len() as u64);
        Ok(())
    };

    let mut stream = open_jsonl(&revision.revision_dir.join("events.jsonl"))?;
    let mut buffer = ProjectionBuffer::default();
    for record in stream.by_ref() {
        check_cancel(cancel)?;
        let event: ImportedEvent = serde_json::from_value(record?.value).map_err(|_| {
            ImportError::new(
                ErrorCode::SnapshotUnavailable,
                "a published imported-history event is corrupt",
            )
        })?;
        buffer.push_text(&event.searchable_text(), &mut emit)?;
    }
    let summary = stream.finish()?;
    if !summary.issues.is_empty() {
        return Err(ImportError::new(
            ErrorCode::SnapshotUnavailable,
            "a published imported-history event stream is incomplete",
        ));
    }
    buffer.finish(&mut emit)?;
    Ok((chunks, bytes))
}

#[derive(Default)]
struct ProjectionBuffer {
    buffer: String,
    next_ordinal: i64,
    has_text: bool,
    buffer_is_overlap_only: bool,
}

impl ProjectionBuffer {
    fn push_text(
        &mut self,
        text: &str,
        emit: &mut dyn FnMut(i64, &str) -> Result<(), ImportError>,
    ) -> Result<(), ImportError> {
        if text.is_empty() {
            return Ok(());
        }
        if self.has_text {
            self.append("\n", emit)?;
        }
        self.has_text = true;
        self.append(text, emit)
    }

    fn append(
        &mut self,
        mut text: &str,
        emit: &mut dyn FnMut(i64, &str) -> Result<(), ImportError>,
    ) -> Result<(), ImportError> {
        while !text.is_empty() {
            let room = BODY_SEARCH_CHUNK_BYTES.saturating_sub(self.buffer.len());
            let mut take = room.min(text.len());
            while take > 0 && !text.is_char_boundary(take) {
                take -= 1;
            }
            if take == 0 {
                self.flush_with_overlap(emit)?;
                continue;
            }
            self.buffer.push_str(&text[..take]);
            self.buffer_is_overlap_only = false;
            text = &text[take..];
            if self.buffer.len() == BODY_SEARCH_CHUNK_BYTES {
                self.flush_with_overlap(emit)?;
            }
        }
        Ok(())
    }

    fn flush_with_overlap(
        &mut self,
        emit: &mut dyn FnMut(i64, &str) -> Result<(), ImportError>,
    ) -> Result<(), ImportError> {
        if self.buffer.is_empty() {
            return Ok(());
        }
        let mut overlap_start = self
            .buffer
            .len()
            .saturating_sub(BODY_SEARCH_CHUNK_OVERLAP_BYTES);
        while overlap_start < self.buffer.len() && !self.buffer.is_char_boundary(overlap_start) {
            overlap_start += 1;
        }
        let overlap = self.buffer[overlap_start..].to_string();
        emit(self.next_ordinal, &self.buffer)?;
        self.next_ordinal += 1;
        self.buffer.clear();
        self.buffer.push_str(&overlap);
        self.buffer_is_overlap_only = true;
        Ok(())
    }

    fn finish(
        self,
        emit: &mut dyn FnMut(i64, &str) -> Result<(), ImportError>,
    ) -> Result<(), ImportError> {
        if self.next_ordinal == 0 || (!self.buffer.is_empty() && !self.buffer_is_overlap_only) {
            emit(self.next_ordinal, &self.buffer)?;
        }
        Ok(())
    }
}

fn verify_staged_projection(conn: &Connection) -> Result<(u64, u64, u64, String), ImportError> {
    let revisions: i64 = conn
        .query_row(
            &format!(
                "SELECT COUNT(*) FROM (
                   SELECT import_id, revision_id FROM {STAGING_TABLE}
                   GROUP BY import_id, revision_id
                 )"
            ),
            [],
            |row| row.get(0),
        )
        .map_err(|error| catalog_db_error("verify rebuilt catalog revisions", error))?;
    let (chunks, bytes): (i64, i64) = conn
        .query_row(
            &format!(
                "SELECT COUNT(*), COALESCE(SUM(length(CAST(text AS BLOB))), 0)
                 FROM {STAGING_TABLE}"
            ),
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|error| catalog_db_error("verify rebuilt catalog size", error))?;
    let mut stmt = conn
        .prepare(&format!(
            "SELECT import_id, revision_id, chunk_ordinal, text
             FROM {STAGING_TABLE}
             ORDER BY import_id ASC, revision_id ASC, chunk_ordinal ASC"
        ))
        .map_err(|error| catalog_db_error("verify rebuilt catalog hash", error))?;
    let mut rows = stmt
        .query([])
        .map_err(|error| catalog_db_error("verify rebuilt catalog hash", error))?;
    let mut hasher = Sha256::new();
    while let Some(row) = rows
        .next()
        .map_err(|error| catalog_db_error("verify rebuilt catalog hash", error))?
    {
        let import_id: String = row
            .get(0)
            .map_err(|error| catalog_db_error("read rebuilt catalog hash", error))?;
        let revision_id: String = row
            .get(1)
            .map_err(|error| catalog_db_error("read rebuilt catalog hash", error))?;
        let ordinal: i64 = row
            .get(2)
            .map_err(|error| catalog_db_error("read rebuilt catalog hash", error))?;
        let text: String = row
            .get(3)
            .map_err(|error| catalog_db_error("read rebuilt catalog hash", error))?;
        feed_projection_hash(&mut hasher, &import_id, &revision_id, ordinal, &text);
    }
    Ok((
        revisions.try_into().unwrap_or(0),
        chunks.try_into().unwrap_or(0),
        bytes.try_into().unwrap_or(0),
        hex::encode(hasher.finalize()),
    ))
}

fn feed_projection_hash(
    hasher: &mut Sha256,
    import_id: &str,
    revision_id: &str,
    ordinal: i64,
    text: &str,
) {
    for value in [
        import_id.as_bytes(),
        revision_id.as_bytes(),
        text.as_bytes(),
    ] {
        hasher.update((value.len() as u64).to_le_bytes());
        hasher.update(value);
    }
    hasher.update(ordinal.to_le_bytes());
}

fn drop_schema_object(conn: &Connection, name: &str) -> Result<(), ImportError> {
    let kind: Option<String> = conn
        .query_row(
            "SELECT type FROM sqlite_master WHERE name=?1 AND type IN ('table','view')",
            params![name],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| catalog_db_error("inspect catalog schema object", error))?;
    match kind.as_deref() {
        Some("table") => conn
            .execute_batch(&format!("DROP TABLE {name};"))
            .map_err(|error| catalog_db_error("drop incompatible catalog table", error)),
        Some("view") => conn
            .execute_batch(&format!("DROP VIEW {name};"))
            .map_err(|error| catalog_db_error("drop incompatible catalog view", error)),
        _ => Ok(()),
    }
}

fn check_cancel(cancel: &dyn Fn() -> bool) -> Result<(), ImportError> {
    if cancel() {
        Err(ImportError::new(
            ErrorCode::PlanStale,
            "imported-history catalog rebuild cancelled",
        ))
    } else {
        Ok(())
    }
}

fn catalog_db_error(context: &str, error: rusqlite::Error) -> ImportError {
    let code = match error.sqlite_error_code() {
        Some(rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked) => {
            ErrorCode::SourceBusy
        }
        Some(rusqlite::ErrorCode::DiskFull) => ErrorCode::DiskSpaceLow,
        _ => ErrorCode::SnapshotUnavailable,
    };
    ImportError::new(code, format!("{context}: {error}"))
}

fn normalize_maintenance_error(error: ImportError) -> ImportError {
    if error.code != ErrorCode::SnapshotUnavailable {
        return error;
    }
    let message = error.message.to_ascii_lowercase();
    if message.contains("locked") || message.contains("busy") {
        ImportError::new(
            ErrorCode::SourceBusy,
            "the imported-history catalog is busy or locked",
        )
    } else if message.contains("disk") && message.contains("full") {
        ImportError::new(
            ErrorCode::DiskSpaceLow,
            "the imported-history catalog volume is full",
        )
    } else {
        error
    }
}
