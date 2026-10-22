//! Read-only SQLite with committed WAL. Never checkpoint, VACUUM, or repair a source DB.

use crate::session_migration::sqlite_open;
use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use rusqlite::{
    backup::{Backup, StepResult},
    Connection, OpenFlags,
};

use super::budget::{DB_BUSY_MS, DB_BUSY_RETRIES};
use super::errors::{ErrorCode, ImportError};
use super::hash::sha256_file;

#[derive(Debug, Clone)]
pub struct SqliteSnapshot {
    // The production backup caller needs success/failure; these verified
    // before/after facts remain available for diagnostics and audit tests.
    #[allow(dead_code)]
    pub source_path: PathBuf,
    #[allow(dead_code)]
    pub copy_path: PathBuf,
    #[allow(dead_code)]
    pub before: BTreeMap<String, FileStamp>,
    #[allow(dead_code)]
    pub after: BTreeMap<String, FileStamp>,
    #[allow(dead_code)]
    pub source_unchanged: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileStamp {
    pub exists: bool,
    pub len: u64,
    pub sha256: Option<String>,
}

fn stamp(path: &Path) -> Result<FileStamp, ImportError> {
    match fs::metadata(path) {
        Ok(m) => Ok(FileStamp {
            exists: true,
            len: m.len(),
            sha256: Some(
                sha256_file(path)
                    .map_err(|e| ImportError::new(ErrorCode::SnapshotUnavailable, e))?,
            ),
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(FileStamp {
            exists: false,
            len: 0,
            sha256: None,
        }),
        Err(e) => Err(classify_io_err(e)),
    }
}

fn related_paths(db: &Path) -> Vec<PathBuf> {
    let mut out = vec![db.to_path_buf()];
    let mut wal = db.as_os_str().to_os_string();
    wal.push("-wal");
    out.push(PathBuf::from(wal));
    let mut shm = db.as_os_str().to_os_string();
    shm.push("-shm");
    out.push(PathBuf::from(shm));
    let mut journal = db.as_os_str().to_os_string();
    journal.push("-journal");
    out.push(PathBuf::from(journal));
    out
}

fn stamp_set(db: &Path) -> Result<BTreeMap<String, FileStamp>, ImportError> {
    related_paths(db)
        .into_iter()
        .map(|p| Ok((p.to_string_lossy().into_owned(), stamp(&p)?)))
        .collect()
}

/// Owns only a freshly-created private directory. Never a directory in the source tree.
struct SnapshotFiles(PathBuf);

impl SnapshotFiles {
    fn new() -> Result<Self, ImportError> {
        let path =
            std::env::temp_dir().join(format!("grok-import-sqlite-{}", uuid::Uuid::new_v4()));
        let builder = fs::DirBuilder::new();
        #[cfg(unix)]
        let builder = {
            use std::os::unix::fs::DirBuilderExt;
            let mut builder = builder;
            builder.mode(0o700);
            builder
        };
        builder.create(&path).map_err(classify_io_err)?;
        Ok(Self(path))
    }
}

impl Drop for SnapshotFiles {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// SQLite only opens our private copy. Even a READ_ONLY source connection can
/// create/update SHM, so it is never used against another application's files.
/// Field order closes SQLite before deleting the owned directory on Windows.
pub struct ReadonlySnapshot {
    connection: Connection,
    _files: SnapshotFiles,
    before: BTreeMap<String, FileStamp>,
    after: BTreeMap<String, FileStamp>,
}

impl Deref for ReadonlySnapshot {
    type Target = Connection;
    fn deref(&self) -> &Connection {
        &self.connection
    }
}

impl ReadonlySnapshot {
    /// Fingerprint of DB + WAL bytes, excluding ephemeral shared-memory locks.
    pub fn source_revision(&self) -> String {
        let stamps: Vec<_> = self
            .before
            .iter()
            .filter(|(name, _)| !name.ends_with("-shm") && !name.ends_with("-journal"))
            .map(|(_, stamp)| (&stamp.sha256, stamp.len, stamp.exists))
            .collect();
        format!(
            "sqlite:{}",
            super::hash::sha256_bytes(
                serde_json::to_string(&stamps)
                    .expect("file stamps serialize")
                    .as_bytes()
            )
        )
    }
}

fn classify_io_err(e: std::io::Error) -> ImportError {
    let code = match e.kind() {
        std::io::ErrorKind::NotFound => ErrorCode::SourceNotFound,
        std::io::ErrorKind::PermissionDenied => ErrorCode::SourcePermissionDenied,
        _ if cfg!(windows) && matches!(e.raw_os_error(), Some(32 | 33)) => ErrorCode::SourceBusy,
        _ => ErrorCode::SnapshotUnavailable,
    };
    ImportError::new(code, e.to_string())
}

fn copy_stable_set(src: &Path, files: SnapshotFiles) -> Result<ReadonlySnapshot, ImportError> {
    let before = stamp_set(src)?;
    if !before[&src.to_string_lossy().into_owned()].exists {
        return Err(ImportError::new(
            ErrorCode::SourceNotFound,
            "source database is missing",
        ));
    }
    if before
        .iter()
        .any(|(p, s)| p.ends_with("-journal") && s.exists && s.len > 0)
    {
        return Err(ImportError::new(
            ErrorCode::SourceBusy,
            "source has a rollback journal; close the source or provide a safe copy",
        ));
    }
    let copy = files.0.join("source.sqlite");
    for (index, path) in related_paths(src).into_iter().take(2).enumerate() {
        let expected = &before[&path.to_string_lossy().into_owned()];
        if !expected.exists {
            continue;
        }
        if expected.len > super::budget::BATCH_NEW_BYTES_MAX {
            return Err(ImportError::new(
                ErrorCode::ResourceLimit,
                "source database exceeds snapshot budget",
            ));
        }
        let target = if index == 0 {
            copy.clone()
        } else {
            files.0.join("source.sqlite-wal")
        };
        let input = fs::File::open(&path).map_err(classify_io_err)?;
        let mut output = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target)
            .map_err(classify_io_err)?;
        let copied =
            std::io::copy(&mut input.take(expected.len), &mut output).map_err(classify_io_err)?;
        drop(output);
        if copied != expected.len {
            return Err(ImportError::new(
                ErrorCode::SourceChanged,
                "source was truncated while copying",
            ));
        }
        if stamp(&target)? != *expected {
            return Err(ImportError::new(
                ErrorCode::SourceChanged,
                "source changed while copying database/WAL",
            ));
        }
    }
    let after = stamp_set(src)?;
    if before != after {
        return Err(ImportError::new(
            ErrorCode::SourceChanged,
            "source files changed during snapshot; retry or provide a closed copy",
        ));
    }
    // Private copy may create its own SHM and recover committed WAL. No immutable
    // flag: ignoring WAL here would silently discard the newest conversations.
    let conn = sqlite_open::open_with_flags(&copy, OpenFlags::SQLITE_OPEN_READ_WRITE)
        .map_err(|e| classify_open_err(src, &e))?;
    conn.busy_timeout(Duration::from_millis(DB_BUSY_MS))
        .map_err(|e| ImportError::new(ErrorCode::SourceBusy, e.to_string()))?;
    conn.pragma_update(None, "query_only", true)
        .map_err(|e| classify_query_err(src, &e))?;
    // Validate the copied view before exposing any rows. Failures concern the
    // snapshot, never attempt source repair.
    let check: String = conn
        .query_row("PRAGMA quick_check(1)", [], |r| r.get(0))
        .map_err(|e| classify_query_err(src, &e))?;
    if check != "ok" {
        return Err(ImportError::new(
            ErrorCode::SnapshotUnavailable,
            "inconsistent database snapshot",
        ));
    }
    Ok(ReadonlySnapshot {
        connection: conn,
        _files: files,
        before,
        after,
    })
}

pub fn open_readonly(path: &Path) -> Result<ReadonlySnapshot, ImportError> {
    let mut last_error = None;
    for _ in 0..=DB_BUSY_RETRIES {
        let files = SnapshotFiles::new()?;
        match copy_stable_set(path, files) {
            Ok(snapshot) => return Ok(snapshot),
            Err(e) if matches!(e.code, ErrorCode::SourceChanged | ErrorCode::SourceBusy) => {
                last_error = Some(e)
            }
            Err(e) => return Err(e),
        }
    }
    Err(last_error.expect("at least one snapshot attempt"))
}

fn classify_open_err(path: &Path, e: &rusqlite::Error) -> ImportError {
    classify_sqlite_err(path, e, true)
}

/// Map a rusqlite failure on an already-open connection (schema listing, kv scan).
pub(crate) fn classify_query_err(path: &Path, e: &rusqlite::Error) -> ImportError {
    classify_sqlite_err(path, e, false)
}

fn classify_sqlite_err(path: &Path, e: &rusqlite::Error, open: bool) -> ImportError {
    let msg = e.to_string();
    let sql_code = match e {
        rusqlite::Error::SqliteFailure(f, _) => Some(f.code),
        _ => None,
    };
    use rusqlite::ErrorCode as SqlCode;
    match sql_code {
        Some(SqlCode::DatabaseBusy | SqlCode::DatabaseLocked) => {
            return ImportError::new(
                ErrorCode::SourceBusy,
                format!("{} is locked ({msg})", path.display()),
            );
        }
        Some(SqlCode::PermissionDenied) => {
            return ImportError::new(
                ErrorCode::SourcePermissionDenied,
                format!("cannot open {} ({msg})", path.display()),
            );
        }
        Some(SqlCode::NotADatabase | SqlCode::DatabaseCorrupt | SqlCode::CannotOpen) => {
            return ImportError::new(
                ErrorCode::SnapshotUnavailable,
                format!("cannot open {} ({msg})", path.display()),
            );
        }
        _ => {}
    }
    let lower = msg.to_ascii_lowercase();
    if lower.contains("locked") || lower.contains("busy") {
        ImportError::new(
            ErrorCode::SourceBusy,
            format!("{} is locked ({msg})", path.display()),
        )
    } else if lower.contains("permission") || lower.contains("denied") {
        ImportError::new(
            ErrorCode::SourcePermissionDenied,
            format!("cannot open {} ({msg})", path.display()),
        )
    } else if open
        || lower.contains("not a database")
        || lower.contains("malformed")
        || lower.contains("unable to open")
    {
        ImportError::new(
            ErrorCode::SnapshotUnavailable,
            format!("cannot open {} ({msg})", path.display()),
        )
    } else {
        ImportError::new(
            ErrorCode::SchemaUnsupported,
            format!("{} ({msg})", path.display()),
        )
    }
}

/// Copy committed pages (including WAL) into `dest` using the backup API.
/// `dest` must be inside the App imports directory, never the source tree.
pub fn backup_readonly_to(src: &Path, dest: &Path) -> Result<SqliteSnapshot, ImportError> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|e| {
            ImportError::new(ErrorCode::SnapshotUnavailable, format!("staging dir: {e}"))
        })?;
    }
    let src_conn = open_readonly(src)?;
    // Exclusive creation prevents accidentally replacing a source or an earlier
    // completed snapshot, even when a caller supplies the wrong destination.
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(dest)
        .map_err(classify_io_err)?;
    let mut dest_conn = sqlite_open::open(dest)
        .map_err(|e| ImportError::new(ErrorCode::SnapshotUnavailable, format!("open dest: {e}")))?;
    let mut last_err = None;
    for attempt in 0..=DB_BUSY_RETRIES {
        match Backup::new(&src_conn, &mut dest_conn) {
            Ok(backup) => {
                let deadline = Instant::now() + Duration::from_millis(DB_BUSY_MS);
                let result = loop {
                    match backup.step(256) {
                        Ok(StepResult::Done) => break Ok(()),
                        Ok(StepResult::More) => continue,
                        Ok(_) if Instant::now() < deadline => {
                            std::thread::sleep(Duration::from_millis(10))
                        }
                        Ok(_) => break Err("snapshot backup remained busy".to_string()),
                        Err(e) => break Err(e.to_string()),
                    }
                };
                match result {
                    Ok(()) => {
                        last_err = None;
                        break;
                    }
                    Err(e) => last_err = Some(e.to_string()),
                }
            }
            Err(e) => last_err = Some(e.to_string()),
        }
        if attempt < DB_BUSY_RETRIES {
            std::thread::sleep(Duration::from_millis(DB_BUSY_MS));
        }
    }
    if let Some(e) = last_err {
        return Err(ImportError::new(
            ErrorCode::SourceBusy,
            format!("sqlite backup failed: {e}"),
        ));
    }
    drop(dest_conn);
    Ok(SqliteSnapshot {
        source_path: src.to_path_buf(),
        copy_path: dest.to_path_buf(),
        before: src_conn.before.clone(),
        after: src_conn.after.clone(),
        source_unchanged: true,
    })
}

pub fn table_names(conn: &Connection) -> Result<Vec<String>, ImportError> {
    table_names_at(conn, Path::new("<memory>"))
}

pub(crate) fn table_names_at(conn: &Connection, path: &Path) -> Result<Vec<String>, ImportError> {
    let mut stmt = conn
        .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
        .map_err(|e| classify_query_err(path, &e))?;
    let rows = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|e| classify_query_err(path, &e))?;
    let mut names = Vec::new();
    for r in rows {
        names.push(r.map_err(|e| classify_query_err(path, &e))?);
    }
    Ok(names)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;

    #[test]
    fn committed_wal_is_visible_and_source_files_do_not_change() {
        let root = std::env::temp_dir().join(format!("grok-wal-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let db = root.join("state.vscdb");
        let dest = root.join("copy.sqlite");
        let writer = Connection::open(&db).unwrap();
        writer
            .execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0;")
            .unwrap();
        writer
            .execute_batch("CREATE TABLE ItemTable (key TEXT PRIMARY KEY, value BLOB);")
            .unwrap();
        writer
            .execute(
                "INSERT INTO ItemTable(key, value) VALUES (?1, ?2)",
                params!["composerData:wal-test", b"visible-in-wal"],
            )
            .unwrap();
        // Keep the writer open so WAL is not checkpointed away.
        let before = stamp_set(&db).unwrap();
        let snap = backup_readonly_to(&db, &dest).expect("backup");
        assert!(snap.source_unchanged);
        assert_eq!(
            before,
            stamp_set(&db).unwrap(),
            "including SHM existence and bytes"
        );
        assert_eq!(snap.before, snap.after);
        let reader = Connection::open(&dest).unwrap();
        let val: Vec<u8> = reader
            .query_row(
                "SELECT value FROM ItemTable WHERE key = ?1",
                params!["composerData:wal-test"],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(val, b"visible-in-wal");
        drop(reader);
        drop(writer);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn offline_wal_without_shm_is_read_without_creating_source_files() {
        let root = std::env::temp_dir().join(format!("grok-offline-wal-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let db = root.join("writer.sqlite");
        let writer = Connection::open(&db).unwrap();
        writer
            .execute_batch(
                "PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0;
            CREATE TABLE ItemTable(key TEXT PRIMARY KEY, value TEXT);
            INSERT INTO ItemTable VALUES('id', 'committed WAL only');",
            )
            .unwrap();
        let source = root.join("离线 # data.sqlite");
        fs::copy(&db, &source).unwrap();
        fs::copy(
            related_paths(&db)[1].clone(),
            related_paths(&source)[1].clone(),
        )
        .unwrap();
        drop(writer);
        let before = stamp_set(&source).unwrap();
        let snapshot = open_readonly(&source).unwrap();
        let value: String = snapshot
            .query_row("SELECT value FROM ItemTable", [], |r| r.get(0))
            .unwrap();
        assert_eq!(value, "committed WAL only");
        assert!(snapshot.execute("DELETE FROM ItemTable", []).is_err());
        let private_dir = snapshot._files.0.clone();
        drop(snapshot);
        assert!(!private_dir.exists());
        assert_eq!(before, stamp_set(&source).unwrap());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn snapshot_cannot_overwrite_an_existing_destination() {
        let root = std::env::temp_dir().join(format!("grok-dest-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let db = root.join("source.sqlite");
        let source = Connection::open(&db).unwrap();
        source.execute_batch("CREATE TABLE t(value TEXT);").unwrap();
        drop(source);
        let before = stamp_set(&db).unwrap();
        assert!(backup_readonly_to(&db, &db).is_err());
        assert_eq!(before, stamp_set(&db).unwrap());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn exclusive_lock_query_is_busy_not_schema_unsupported() {
        let root = std::env::temp_dir().join(format!("grok-lock-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let db = root.join("state.vscdb");
        let writer = Connection::open(&db).unwrap();
        writer
            .execute_batch("CREATE TABLE ItemTable (key TEXT PRIMARY KEY, value BLOB);")
            .unwrap();
        writer
            .execute_batch("PRAGMA locking_mode=EXCLUSIVE; BEGIN EXCLUSIVE;")
            .unwrap();
        match open_readonly(&db) {
            Err(e) => assert!(
                matches!(
                    e.code,
                    ErrorCode::SourceBusy | ErrorCode::SnapshotUnavailable
                ),
                "unexpected open {:?}",
                e.code
            ),
            Ok(conn) => {
                if let Err(e) = table_names_at(&conn, &db) {
                    assert_ne!(
                        e.code,
                        ErrorCode::SchemaUnsupported,
                        "busy lock must not look like an unsupported schema: {e:?}"
                    );
                }
            }
        }
        drop(writer);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn rollback_journal_is_busy_and_does_not_repair_source() {
        let root = std::env::temp_dir().join(format!("grok-journal-busy-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let db = root.join("state.vscdb");
        let writer = Connection::open(&db).unwrap();
        writer
            .execute_batch("CREATE TABLE ItemTable (key TEXT PRIMARY KEY, value BLOB);")
            .unwrap();
        drop(writer);
        let mut journal = db.as_os_str().to_os_string();
        journal.push("-journal");
        fs::write(&journal, b"rollback-journal-busy").unwrap();
        let before = stamp_set(&db).unwrap();
        let err = match open_readonly(&db) {
            Err(e) => e,
            Ok(_) => panic!("rollback journal must not open as a usable snapshot"),
        };
        assert_eq!(err.code, ErrorCode::SourceBusy);
        assert_eq!(
            before,
            stamp_set(&db).unwrap(),
            "busy snapshot must not repair or rewrite the source"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[cfg(windows)]
    #[test]
    fn owned_private_snapshot_beyond_default_vfs_buffer_preserves_wal_source() {
        use std::os::windows::ffi::OsStrExt;
        let home = crate::session_migration::test_home::IsolatedAppHome::acquire("private-long");
        let db = home.path.join("source.sqlite");
        let writer = Connection::open(&db).unwrap();
        writer.execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0; CREATE TABLE payload (value TEXT); INSERT INTO payload VALUES ('private-long-wal');").unwrap();
        let before = stamp_set(&db).unwrap();
        let mut private = home.path.join("private");
        while private.as_os_str().encode_wide().count() < 1200 {
            private.push("private-copy-component-0123456789-abcdefghijklmnopqrstuvwxyz");
        }
        fs::create_dir_all(&private).unwrap();
        assert!(private.as_os_str().encode_wide().count() > 1040);
        let snapshot = copy_stable_set(&db, SnapshotFiles(private.clone())).unwrap();
        let value: String = snapshot
            .query_row("SELECT value FROM payload", [], |row| row.get(0))
            .unwrap();
        assert_eq!(value, "private-long-wal");
        assert!(snapshot.execute_batch("DELETE FROM payload").is_err());
        assert_eq!(stamp_set(&db).unwrap(), before);
        drop(snapshot);
        assert!(
            !private.exists(),
            "owned copy must close before cleanup on Windows"
        );
    }
}
