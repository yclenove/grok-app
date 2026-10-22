//! Bounded CJK n-gram / literal index over imported events.

use crate::session_migration::sqlite_open;
#[cfg(test)]
use std::io::{BufWriter, Write};
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use uuid::Uuid;

use rusqlite::{params, Connection, OpenFlags, OptionalExtension};

use super::budget::{read_byte_limit, search_limit};
use super::cursor::{
    decode_bound_cursor, decode_optional_offset, encode_bound_cursor, CursorScope,
};
use super::errors::{ErrorCode, ImportError};
use super::jsonl::open_jsonl;
use super::selection::AllowedSelection;
use super::types::{EventRelation, ImportedEvent};

pub const INDEX_SCHEMA_VERSION: i32 = 4;

#[path = "search_owner.rs"]
mod owner;
pub(crate) use owner::LOCK_NAME as INDEX_LOCK_FILE;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexedEventMetadata {
    pub event_id: String,
    pub ordinal: u32,
    pub kind: String,
    pub json_len: usize,
    pub model_visible: bool,
    pub asset_ids: Vec<String>,
}

#[cfg(test)]
thread_local! {
    static EVENT_BODY_READS: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn take_event_body_reads() -> u32 {
    EVENT_BODY_READS.with(|count| count.replace(0))
}

pub fn index_path(revision_dir: &Path) -> std::path::PathBuf {
    revision_dir.join("event-index.sqlite")
}

pub fn archive_events_path(revision_dir: &Path) -> std::path::PathBuf {
    revision_dir.join("events.jsonl")
}

pub fn revision_token(revision_dir: &Path) -> String {
    revision_dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("revision")
        .to_string()
}

#[cfg(test)]
pub fn encode_page_cursor(revision_dir: &Path, offset: u32) -> Result<String, ImportError> {
    let token = revision_token(revision_dir);
    encode_bound_cursor(
        CursorScope {
            owner: &token,
            snapshot: &token,
            leaf: "",
            filter: "events",
        },
        offset,
        None,
    )
}

#[cfg(test)]
pub fn decode_page_cursor(revision_dir: &Path, cursor: Option<&str>) -> Result<u32, ImportError> {
    let token = revision_token(revision_dir);
    decode_optional_offset(
        cursor,
        CursorScope {
            owner: &token,
            snapshot: &token,
            leaf: "",
            filter: "events",
        },
    )
}

#[cfg(test)]
pub(crate) fn load_archive_events(
    revision_dir: &Path,
    cancel: &dyn Fn() -> bool,
) -> Result<Vec<ImportedEvent>, ImportError> {
    let path = archive_events_path(revision_dir);
    let mut stream = open_jsonl(&path)?;
    let mut events = Vec::new();
    for rec in stream.by_ref() {
        if cancel() {
            return Err(ImportError::new(
                ErrorCode::PlanStale,
                "index rebuild cancelled",
            ));
        }
        let rec = rec?;
        events.push(
            serde_json::from_value::<ImportedEvent>(rec.value).map_err(|_| {
                ImportError::new(
                    ErrorCode::SnapshotUnavailable,
                    "archived event_json is corrupt",
                )
            })?,
        );
    }
    let summary = stream.finish()?;
    if !summary.issues.is_empty() {
        return Err(ImportError::new(
            ErrorCode::SnapshotUnavailable,
            "archived event stream is incomplete",
        ));
    }
    Ok(events)
}

fn index_user_version(path: &Path) -> Result<Option<i32>, ImportError> {
    if !path.exists() {
        return Ok(None);
    }
    let conn = sqlite_open::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(index_error)?;
    let version: i32 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .map_err(index_error)?;
    // A future schema need not contain the current events table.
    if version > INDEX_SCHEMA_VERSION {
        return Ok(Some(version));
    }
    let ok: Result<i64, _> = conn.query_row("SELECT COUNT(*) FROM events", [], |r| r.get(0));
    if ok.is_err() {
        drop(conn);
        return Err(ImportError::new(
            ErrorCode::HistoryAccessUnavailable,
            "event index is corrupt",
        ));
    }
    drop(conn);
    Ok(Some(version))
}

/// Rebuild the derived index from immutable archive files.
#[cfg(test)]
pub fn rebuild_index(revision_dir: &Path, cancel: &dyn Fn() -> bool) -> Result<(), ImportError> {
    rebuild_index_streaming(revision_dir, cancel)
}

/// Stream JSONL into a temporary index and atomically replace the live file.
pub fn rebuild_index_streaming(
    revision_dir: &Path,
    cancel: &dyn Fn() -> bool,
) -> Result<(), ImportError> {
    owner::with(revision_dir, cancel, || {
        rebuild_index_owned(revision_dir, cancel)
    })?;
    project_body_search_from_archive(revision_dir)
}

fn rebuild_index_owned(revision_dir: &Path, cancel: &dyn Fn() -> bool) -> Result<(), ImportError> {
    recover_index_files_owned(revision_dir)?;
    let live = index_path(revision_dir);
    let tmp = revision_dir.join("event-index.sqlite.tmp");
    let _ = std::fs::remove_file(&tmp);
    fill_sqlite_from_jsonl(revision_dir, &tmp, cancel)?;
    replace_index_file_owned(&tmp, &live)
}

fn sqlite_sidecars(path: &Path) -> [std::path::PathBuf; 2] {
    let name = path.as_os_str().to_string_lossy();
    [
        std::path::PathBuf::from(format!("{name}-wal")),
        std::path::PathBuf::from(format!("{name}-shm")),
    ]
}

fn is_sharing_violation(err: &std::io::Error) -> bool {
    matches!(err.raw_os_error(), Some(32) | Some(5))
        || err.kind() == std::io::ErrorKind::PermissionDenied
}

fn index_backup_path(live: &Path) -> std::path::PathBuf {
    let mut name = live
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_else(|| "event-index.sqlite".into());
    name.push(".bak");
    match live.parent() {
        Some(parent) => parent.join(name),
        None => std::path::PathBuf::from(name),
    }
}

fn sync_regular_file(path: &Path) -> Result<(), ImportError> {
    for attempt in 0..3 {
        match std::fs::File::open(path).and_then(|file| file.sync_all()) {
            Ok(()) => return Ok(()),
            Err(e) if is_sharing_violation(&e) => {
                std::thread::sleep(std::time::Duration::from_millis(20 * (attempt + 1) as u64));
            }
            Err(e) => {
                return Err(ImportError::new(
                    ErrorCode::SnapshotUnavailable,
                    format!("sync index file {}: {e}", path.display()),
                ));
            }
        }
    }
    Err(ImportError::new(
        ErrorCode::SnapshotUnavailable,
        format!("sync index file still locked: {}", path.display()),
    ))
}

fn sync_parent_dir(dir: &Path) -> Result<(), ImportError> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
        match std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
            .open(dir)
        {
            Ok(file) => {
                if let Err(e) = file.sync_all() {
                    if !is_sharing_violation(&e) {
                        return Err(ImportError::new(
                            ErrorCode::SnapshotUnavailable,
                            format!("sync index directory: {e}"),
                        ));
                    }
                }
            }
            Err(e) if is_sharing_violation(&e) => {
                // Windows may refuse a directory handle without backup privilege.
                // The replacement file has already been renamed into place.
            }
            Err(e) => {
                return Err(ImportError::new(
                    ErrorCode::SnapshotUnavailable,
                    format!("sync index directory: {e}"),
                ));
            }
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let file = std::fs::File::open(dir).map_err(|e| {
            ImportError::new(
                ErrorCode::SnapshotUnavailable,
                format!("sync index directory: {e}"),
            )
        })?;
        file.sync_all().map_err(|e| {
            ImportError::new(
                ErrorCode::SnapshotUnavailable,
                format!("sync index directory: {e}"),
            )
        })?;
        Ok(())
    }
}

#[cfg(test)]
thread_local! {
    static INDEX_SWAP_FAULT: std::cell::Cell<Option<&'static str>> =
        const { std::cell::Cell::new(None) };
}

#[cfg(test)]
pub(crate) fn set_index_swap_fault(fault: Option<&'static str>) {
    INDEX_SWAP_FAULT.with(|cell| cell.set(fault));
}

fn take_index_swap_fault() -> Option<&'static str> {
    #[cfg(test)]
    {
        INDEX_SWAP_FAULT.with(|cell| cell.take())
    }
    #[cfg(not(test))]
    {
        None
    }
}

fn live_index_ready(path: &Path) -> bool {
    matches!(index_user_version(path), Ok(Some(v)) if v == INDEX_SCHEMA_VERSION)
}

fn remove_backup_files(backup: &Path) -> Result<(), ImportError> {
    if backup.exists() {
        std::fs::remove_file(backup).map_err(|e| {
            ImportError::new(
                ErrorCode::SnapshotUnavailable,
                format!("remove event index backup: {e}"),
            )
        })?;
    }
    for extra in sqlite_sidecars(backup) {
        let _ = std::fs::remove_file(extra);
    }
    Ok(())
}

fn rollback_backup(backup: &Path, live: &Path) -> Result<(), ImportError> {
    if backup.exists() && !live.exists() {
        std::fs::rename(backup, live).map_err(|e| {
            ImportError::new(
                ErrorCode::SnapshotUnavailable,
                format!("restore event index backup: {e}"),
            )
        })?;
    }
    Ok(())
}

/// Replace live with a fully closed/synced temp file in the same directory.
/// Never removes live before the new file is in place: live → backup, temp → live, then drop backup.
fn replace_index_file_owned(tmp: &Path, live: &Path) -> Result<(), ImportError> {
    let parent = live.parent().ok_or_else(|| {
        ImportError::new(
            ErrorCode::SnapshotUnavailable,
            "event index parent directory missing",
        )
    })?;
    if tmp.parent() != Some(parent) {
        return Err(ImportError::new(
            ErrorCode::SnapshotUnavailable,
            "temp event index must share the live directory",
        ));
    }
    if !tmp.exists() {
        return Err(ImportError::new(
            ErrorCode::SnapshotUnavailable,
            "temp event index missing",
        ));
    }
    let backup = index_backup_path(live);
    if live.exists() && backup.exists() {
        return Err(ImportError::new(
            ErrorCode::SnapshotUnavailable,
            "event index backup already exists; refusing to replace",
        ));
    }
    if !live.exists() && backup.exists() && !tmp.exists() {
        std::fs::rename(&backup, live).map_err(|e| {
            ImportError::new(
                ErrorCode::SnapshotUnavailable,
                format!("restore event index backup: {e}"),
            )
        })?;
        sync_parent_dir(parent)?;
        return Ok(());
    }

    let fault = take_index_swap_fault();
    if fault == Some("fail_rename_backup") && live.exists() {
        return Err(ImportError::new(
            ErrorCode::SnapshotUnavailable,
            "injected backup rename failure",
        ));
    }

    let mut backed_up = !live.exists() && backup.exists();
    for attempt in 0..16 {
        for extra in sqlite_sidecars(tmp)
            .iter()
            .chain(sqlite_sidecars(live).iter())
        {
            let _ = std::fs::remove_file(extra);
        }
        if live.exists() && !backed_up {
            if fault == Some("sharing_backup") && attempt == 0 {
                std::thread::sleep(std::time::Duration::from_millis(20));
                continue;
            }
            match std::fs::rename(live, &backup) {
                Ok(()) => backed_up = true,
                Err(e) if is_sharing_violation(&e) => {
                    std::thread::sleep(std::time::Duration::from_millis(20 * (attempt + 1) as u64));
                    continue;
                }
                Err(e) => {
                    return Err(ImportError::new(
                        ErrorCode::SnapshotUnavailable,
                        format!("backup event index: {e}"),
                    ));
                }
            }
            #[cfg(test)]
            owner_tests::after_backup();
            if fault == Some("interrupt_after_backup") {
                return Err(ImportError::new(
                    ErrorCode::SnapshotUnavailable,
                    "injected interrupt after backup",
                ));
            }
            if fault == Some("pause_after_backup") {
                std::thread::sleep(std::time::Duration::from_secs(60));
            }
        }
        if fault == Some("fail_rename_tmp") {
            rollback_backup(&backup, live)?;
            return Err(ImportError::new(
                ErrorCode::SnapshotUnavailable,
                "injected temp rename failure",
            ));
        }
        if fault == Some("sharing_tmp") && attempt == 0 {
            std::thread::sleep(std::time::Duration::from_millis(20));
            continue;
        }
        match std::fs::rename(tmp, live) {
            Ok(()) => {
                if fault == Some("fail_sync_live") {
                    return Err(ImportError::new(
                        ErrorCode::SnapshotUnavailable,
                        "injected live sync failure",
                    ));
                }
                match sync_regular_file(live) {
                    Ok(()) => {}
                    Err(error) if cfg!(windows) && error.message.contains("still locked") => {
                        // Same-volume rename is the atomic publish. Windows may
                        // refuse FlushFileBuffers while SQLite handles linger;
                        // opening the new live file is the remaining barrier.
                    }
                    Err(error) => return Err(error),
                }
                if !live_index_ready(live) {
                    return Err(ImportError::new(
                        ErrorCode::SnapshotUnavailable,
                        "new event index failed schema check",
                    ));
                }
                remove_backup_files(&backup)?;
                sync_parent_dir(parent)?;
                return Ok(());
            }
            Err(e) if is_sharing_violation(&e) => {
                std::thread::sleep(std::time::Duration::from_millis(20 * (attempt + 1) as u64));
            }
            Err(e) => {
                rollback_backup(&backup, live)?;
                return Err(ImportError::new(
                    ErrorCode::SnapshotUnavailable,
                    format!("publish event index: {e}"),
                ));
            }
        }
    }
    rollback_backup(&backup, live)?;
    Err(ImportError::new(
        ErrorCode::SnapshotUnavailable,
        "publish event index: file still locked after retries",
    ))
}

#[cfg(test)]
fn recover_index_files(revision_dir: &Path) -> Result<(), ImportError> {
    owner::with(revision_dir, &|| false, || {
        recover_index_files_owned(revision_dir)
    })
}

fn recover_index_files_owned(revision_dir: &Path) -> Result<(), ImportError> {
    owner::validate_artifacts(revision_dir)?;
    let live = index_path(revision_dir);
    let tmp = revision_dir.join("event-index.sqlite.tmp");
    let backup = index_backup_path(&live);
    if live.exists() {
        if backup.exists() {
            if live_index_ready(&live) {
                remove_backup_files(&backup)?;
            } else if live_index_ready(&backup) {
                for extra in sqlite_sidecars(&live) {
                    let _ = std::fs::remove_file(extra);
                }
                std::fs::remove_file(&live).map_err(|e| {
                    ImportError::new(
                        ErrorCode::SnapshotUnavailable,
                        format!("remove corrupt event index: {e}"),
                    )
                })?;
                std::fs::rename(&backup, &live).map_err(|e| {
                    ImportError::new(
                        ErrorCode::SnapshotUnavailable,
                        format!("restore event index backup: {e}"),
                    )
                })?;
                if let Some(parent) = live.parent() {
                    sync_parent_dir(parent)?;
                }
            } else {
                remove_backup_files(&backup)?;
                for extra in sqlite_sidecars(&live) {
                    let _ = std::fs::remove_file(extra);
                }
                std::fs::remove_file(&live).map_err(|e| {
                    ImportError::new(
                        ErrorCode::SnapshotUnavailable,
                        format!("remove dual-invalid event index: {e}"),
                    )
                })?;
            }
        }
        return Ok(());
    }
    if tmp.exists() {
        return replace_index_file_owned(&tmp, &live);
    }
    if backup.exists() {
        std::fs::rename(&backup, &live).map_err(|e| {
            ImportError::new(
                ErrorCode::SnapshotUnavailable,
                format!("restore event index backup: {e}"),
            )
        })?;
        if let Some(parent) = live.parent() {
            sync_parent_dir(parent)?;
        }
    }
    Ok(())
}

pub fn ensure_index(revision_dir: &Path) -> Result<(), ImportError> {
    let rebuilt = owner::with(revision_dir, &|| false, || {
        ensure_index_owned(revision_dir, &|| false)
    })?;
    if rebuilt {
        project_body_search_from_archive(revision_dir)?;
    }
    Ok(())
}

fn ensure_index_owned(revision_dir: &Path, cancel: &dyn Fn() -> bool) -> Result<bool, ImportError> {
    recover_index_files_owned(revision_dir)?;
    let path = index_path(revision_dir);
    match index_user_version(&path) {
        Ok(Some(v)) if v == INDEX_SCHEMA_VERSION => Ok(false),
        Ok(Some(v)) if v > INDEX_SCHEMA_VERSION => Err(ImportError::new(
            ErrorCode::ImportSchemaTooNew,
            format!("event index schema {v} is newer than this app"),
        )),
        Ok(_) => rebuild_index_owned(revision_dir, cancel).map(|()| true),
        Err(error) => Err(error),
    }
}

/// Only the derived index is held stable during a publication copy. Callers
/// copy immutable bodies/blobs separately and must not project the catalog here.
pub(crate) fn with_index_snapshot<T>(
    revision_dir: &Path,
    cancel: &dyn Fn() -> bool,
    copy: impl FnOnce(&Path) -> Result<T, ImportError>,
) -> Result<T, ImportError> {
    owner::with(revision_dir, cancel, || {
        ensure_index_owned(revision_dir, cancel)?;
        copy(&index_path(revision_dir))
    })
}

fn create_index_schema(conn: &Connection) -> Result<(), ImportError> {
    conn.execute_batch(
        "DROP TABLE IF EXISTS events;
         DROP TABLE IF EXISTS grams;
         CREATE TABLE events (
            event_id TEXT PRIMARY KEY,
            ordinal INTEGER NOT NULL,
            kind TEXT NOT NULL,
            json_off INTEGER NOT NULL,
            json_len INTEGER NOT NULL,
            model_visible INTEGER NOT NULL,
            asset_ids_json TEXT NOT NULL
         );
         CREATE INDEX events_order ON events(ordinal, event_id);",
    )
    .map_err(|e| ImportError::new(ErrorCode::SnapshotUnavailable, e.to_string()))?;
    conn.execute(&format!("PRAGMA user_version={INDEX_SCHEMA_VERSION}"), [])
        .map_err(|e| ImportError::new(ErrorCode::SnapshotUnavailable, e.to_string()))?;
    let _ = conn.execute_batch("PRAGMA journal_mode=DELETE;");
    Ok(())
}

fn insert_index_event(
    conn: &Connection,
    ev: &ImportedEvent,
    json_off: i64,
    json_len: i64,
) -> Result<(), ImportError> {
    if json_off < 0 || json_len <= 0 {
        return Err(ImportError::new(
            ErrorCode::SnapshotUnavailable,
            "indexed event span is invalid",
        ));
    }
    let asset_ids = event_asset_ids(ev);
    let asset_ids_json = serde_json::to_string(&asset_ids).map_err(index_error)?;
    conn.prepare_cached(
        "INSERT INTO events(event_id, ordinal, kind, json_off, json_len, model_visible, asset_ids_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
    )
    .map_err(index_error)?
    .execute(params![
        ev.id(),
        ev.base().ordinal as i64,
        ev.kind_name(),
        json_off,
        json_len,
        i64::from(!super::selection::is_never_model_visible(ev)),
        asset_ids_json,
    ])
    .map_err(|e| ImportError::new(ErrorCode::SnapshotUnavailable, e.to_string()))?;
    Ok(())
}

fn event_asset_ids(event: &ImportedEvent) -> Vec<String> {
    let blocks = match event {
        ImportedEvent::Message { blocks, .. }
        | ImportedEvent::ToolResult { blocks, .. }
        | ImportedEvent::Compaction {
            summary: blocks, ..
        } => blocks.as_slice(),
        _ => return Vec::new(),
    };
    blocks
        .iter()
        .filter_map(|block| match block {
            super::types::ContentBlock::Asset { asset_id, .. } => Some(asset_id.clone()),
            _ => None,
        })
        .collect()
}

pub(super) fn event_from_jsonl(
    revision_dir: &Path,
    json_off: i64,
    json_len: i64,
) -> Result<ImportedEvent, ImportError> {
    #[cfg(test)]
    EVENT_BODY_READS.with(|count| count.set(count.get().saturating_add(1)));
    if json_off < 0 || json_len <= 0 || json_len > super::budget::JSON_RECORD_MAX_BYTES as i64 {
        return Err(ImportError::new(
            ErrorCode::SnapshotUnavailable,
            "indexed event span is invalid",
        ));
    }
    let mut file = std::fs::File::open(archive_events_path(revision_dir)).map_err(|_| {
        ImportError::new(
            ErrorCode::SnapshotUnavailable,
            "archived event stream is incomplete",
        )
    })?;
    file.seek(SeekFrom::Start(json_off as u64))
        .map_err(index_error)?;
    let mut buf = vec![0u8; json_len as usize];
    file.read_exact(&mut buf).map_err(|_| {
        ImportError::new(
            ErrorCode::SnapshotUnavailable,
            "indexed event span is invalid",
        )
    })?;
    serde_json::from_slice(&buf).map_err(|_| {
        ImportError::new(
            ErrorCode::SnapshotUnavailable,
            "indexed event_json is corrupt",
        )
    })
}

#[path = "search_build.rs"]
mod index_build;
use index_build::fill_sqlite_from_jsonl;

#[cfg(test)]
pub fn build_index(revision_dir: &Path, events: &[ImportedEvent]) -> Result<(), ImportError> {
    index_build::build_index(revision_dir, events)
}

fn import_ids_from_revision_dir(dir: &Path) -> Option<(String, String)> {
    let revision_id = dir.file_name()?.to_str()?.to_string();
    Uuid::parse_str(&revision_id).ok()?;
    let parent = dir.parent()?.file_name()?.to_str()?;
    let import_id = parent.replacen('_', ":", 1);
    super::ids::parse_import_uuid(&import_id).ok()?;
    Some((import_id, revision_id))
}

#[cfg(test)]
pub fn project_body_search(
    revision_dir: &Path,
    events: &[ImportedEvent],
) -> Result<(), ImportError> {
    let Some((import_id, revision_id)) = import_ids_from_revision_dir(revision_dir) else {
        return Ok(());
    };
    let conn = super::catalog::open_catalog()?;
    let mut projection =
        super::catalog::BodySearchProjection::begin(&conn, &import_id, &revision_id)?;
    for event in events {
        projection.push_text(&event.searchable_text())?;
    }
    projection.commit()
}

fn project_body_search_from_archive(revision_dir: &Path) -> Result<(), ImportError> {
    let Some((import_id, revision_id)) = import_ids_from_revision_dir(revision_dir) else {
        return Ok(());
    };
    let conn = super::catalog::open_catalog()?;
    project_body_search_from_archive_with_conn(&conn, revision_dir, &import_id, &revision_id)
}

fn project_body_search_from_archive_with_conn(
    conn: &Connection,
    revision_dir: &Path,
    import_id: &str,
    revision_id: &str,
) -> Result<(), ImportError> {
    let mut stream = open_jsonl(&archive_events_path(revision_dir))?;
    let mut projection = super::catalog::BodySearchProjection::begin(conn, import_id, revision_id)?;
    for rec in stream.by_ref() {
        let rec = rec?;
        let ev: ImportedEvent = serde_json::from_value(rec.value).map_err(|_| {
            ImportError::new(
                ErrorCode::SnapshotUnavailable,
                "archived event_json is corrupt",
            )
        })?;
        projection.push_text(&ev.searchable_text())?;
    }
    let summary = stream.finish()?;
    if !summary.issues.is_empty() {
        return Err(ImportError::new(
            ErrorCode::SnapshotUnavailable,
            "archived event stream is incomplete",
        ));
    }
    projection.commit()
}

/// Ensure the immutable published archive has a complete catalog projection.
/// Call this before making a revision current; a crash can leave only an
/// unreferenced projection, never a visible partial one.
pub fn ensure_body_search_projection(
    conn: &Connection,
    revision_dir: &Path,
) -> Result<(), ImportError> {
    let (import_id, revision_id) = import_ids_from_revision_dir(revision_dir).ok_or_else(|| {
        ImportError::new(
            ErrorCode::SnapshotUnavailable,
            "published revision path has no import identity",
        )
    })?;
    if super::catalog::body_search_projection_exists(conn, &import_id, &revision_id)? {
        return Ok(());
    }
    project_body_search_from_archive_with_conn(conn, revision_dir, &import_id, &revision_id)
}

pub fn last_indexed_event_id(revision_dir: &Path) -> Result<Option<String>, ImportError> {
    ensure_index(revision_dir)?;
    let conn = open_index(revision_dir)?;
    let id = conn
        .query_row(
            "SELECT event_id FROM events ORDER BY ordinal DESC, event_id DESC LIMIT 1",
            [],
            |r| r.get::<_, String>(0),
        )
        .optional()
        .map_err(index_error)?;
    Ok(id)
}

fn index_error(error: impl std::fmt::Display) -> ImportError {
    ImportError::new(ErrorCode::HistoryAccessUnavailable, error.to_string())
}

fn open_index(dir: &Path) -> Result<Connection, ImportError> {
    owner::with(dir, &|| false, || {
        sqlite_open::open_with_flags(index_path(dir), OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(index_error)
    })
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub event_id: String,
    pub ordinal: u32,
    pub kind: String,
    pub snippet: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchDirection {
    Forward,
    Backward,
}

impl SearchDirection {
    pub fn parse(value: Option<&str>) -> Result<Self, ImportError> {
        match value.filter(|value| !value.trim().is_empty()) {
            None | Some("forward") => Ok(Self::Forward),
            Some("backward") => Ok(Self::Backward),
            Some(_) => Err(ImportError::new(
                ErrorCode::PlanStale,
                "invalid search page direction",
            )),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchWindow {
    pub hits: Vec<SearchHit>,
    pub previous_cursor: Option<String>,
    pub next_cursor: Option<String>,
}

fn search_cursor_scope<'a>(
    token: &'a str,
    snapshot: &'a str,
    filter: &'a str,
    leaf: &'a str,
) -> CursorScope<'a> {
    CursorScope {
        owner: token,
        snapshot,
        leaf,
        filter,
    }
}

fn decode_search_cursor(
    revision_dir: &Path,
    snapshot: &str,
    filter: &str,
    leaf: &str,
    cursor: Option<&str>,
) -> Result<(i64, String), ImportError> {
    let token = revision_token(revision_dir);
    match cursor.filter(|raw| !raw.is_empty()) {
        None => Ok((-1, String::new())),
        Some(raw) => {
            let bound =
                decode_bound_cursor(raw, search_cursor_scope(&token, snapshot, filter, leaf))?;
            Ok((i64::from(bound.offset), bound.mark.unwrap_or_default()))
        }
    }
}

fn encode_search_cursor(
    revision_dir: &Path,
    snapshot: &str,
    filter: &str,
    leaf: &str,
    ordinal: u32,
    event_id: &str,
) -> Result<String, ImportError> {
    let token = revision_token(revision_dir);
    encode_bound_cursor(
        search_cursor_scope(&token, snapshot, filter, leaf),
        ordinal,
        Some(event_id.to_string()),
    )
}

fn decode_read_offset(
    revision_dir: &Path,
    snapshot: &str,
    filter: &str,
    cursor: Option<&str>,
) -> Result<usize, ImportError> {
    let token = revision_token(revision_dir);
    Ok(decode_optional_offset(cursor, search_cursor_scope(&token, snapshot, filter, ""))? as usize)
}

fn encode_read_cursor(
    revision_dir: &Path,
    snapshot: &str,
    filter: &str,
    offset: usize,
) -> Result<String, ImportError> {
    let token = revision_token(revision_dir);
    let offset = u32::try_from(offset).map_err(index_error)?;
    encode_bound_cursor(
        search_cursor_scope(&token, snapshot, filter, ""),
        offset,
        None,
    )
}

#[path = "search_query.rs"]
mod search_query;
pub use search_query::*;

#[cfg(test)]
mod tests {
    use super::*;
    include!("search_tests.rs");
}

#[cfg(test)]
#[path = "search_recovery_schema_tests.rs"]
mod recovery_schema_tests;

#[cfg(test)]
#[path = "search_owner_tests.rs"]
mod owner_tests;

#[cfg(test)]
pub(crate) use owner::observe_revision_owner_wait;
#[cfg(test)]
pub(crate) use owner_tests::PausedBuilder as PausedIndexBuilder;
