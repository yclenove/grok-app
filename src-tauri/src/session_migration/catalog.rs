//! Import catalog. Independent schema; never writes native sessions_index.

use crate::session_migration::sqlite_open;
use std::path::{Path, PathBuf};

use rusqlite::{params, Connection, OptionalExtension};

use crate::paths::app_data_root;
use crate::session_migration::cursor::{decode_bound_cursor, encode_bound_cursor, CursorScope};
use crate::session_migration::errors::{sqlite_storage_error, ErrorCode, ImportError};
use crate::session_migration::types::CATALOG_SCHEMA_VERSION;
use crate::session_migration::workspace::{SourceWorkspace, WorkspaceMapping};

#[path = "catalog_retention_epoch.rs"]
mod retention_epoch;
pub(super) use retention_epoch::read as retention_catalog_epoch;
pub(super) use retention_epoch::read_text as retention_metadata_text;

const SCHEMA_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS meta (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS sources (
  installation_id TEXT PRIMARY KEY,
  provider TEXT NOT NULL,
  profile TEXT,
  host TEXT,
  root_path TEXT NOT NULL,
  created_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS entries (
  import_id TEXT PRIMARY KEY,
  installation_id TEXT NOT NULL,
  provider TEXT NOT NULL,
  native_session_id TEXT NOT NULL,
  title TEXT,
  alias TEXT,
  hidden INTEGER NOT NULL DEFAULT 0,
  current_revision TEXT,
  created_at TEXT NOT NULL,
  UNIQUE (installation_id, provider, native_session_id)
);
CREATE TABLE IF NOT EXISTS revisions (
  revision_id TEXT PRIMARY KEY,
  import_id TEXT NOT NULL,
  source_snapshot_id TEXT NOT NULL,
  normalizer_version TEXT NOT NULL,
  options_hash TEXT NOT NULL,
  outcome TEXT NOT NULL,
  inventory_json TEXT NOT NULL,
  dir_path TEXT NOT NULL,
  created_at TEXT NOT NULL,
  UNIQUE (import_id, revision_id)
);
CREATE TABLE IF NOT EXISTS workspaces (
  source_workspace_id TEXT PRIMARY KEY,
  display_name TEXT NOT NULL,
  json TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS mappings (
  root_id TEXT PRIMARY KEY,
  json TEXT NOT NULL
);
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
CREATE TABLE IF NOT EXISTS jobs (
  job_id TEXT PRIMARY KEY,
  request_id TEXT NOT NULL UNIQUE,
  plan_hash TEXT NOT NULL,
  state TEXT NOT NULL,
  state_version INTEGER NOT NULL DEFAULT 0,
  json TEXT NOT NULL,
  created_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS job_items (
  job_id TEXT NOT NULL,
  candidate_id TEXT NOT NULL,
  phase TEXT NOT NULL,
  result_json TEXT,
  PRIMARY KEY (job_id, candidate_id)
);
CREATE TABLE IF NOT EXISTS receipts (
  receipt_id TEXT PRIMARY KEY,
  job_id TEXT NOT NULL,
  candidate_id TEXT NOT NULL,
  phase TEXT NOT NULL,
  import_id TEXT,
  revision_id TEXT,
  json TEXT NOT NULL,
  created_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS continuation_refs (
  app_session_id TEXT PRIMARY KEY,
  import_id TEXT NOT NULL,
  revision_id TEXT NOT NULL,
  branch_leaf TEXT NOT NULL,
  allowed_selection_hash TEXT NOT NULL,
  handoff_hash TEXT NOT NULL,
  request_id TEXT NOT NULL UNIQUE,
  json TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS tombstones (
  installation_id TEXT NOT NULL,
  provider TEXT NOT NULL,
  native_session_id TEXT NOT NULL,
  deleted_at TEXT NOT NULL,
  PRIMARY KEY (installation_id, provider, native_session_id)
);
CREATE TABLE IF NOT EXISTS migration_log (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  from_version INTEGER NOT NULL,
  to_version INTEGER NOT NULL,
  applied_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS body_search_chunks (
  import_id TEXT NOT NULL,
  revision_id TEXT NOT NULL,
  chunk_ordinal INTEGER NOT NULL,
  text TEXT NOT NULL,
  PRIMARY KEY (import_id, revision_id, chunk_ordinal)
);
"#;

pub fn imports_dir() -> PathBuf {
    app_data_root().join("imports")
}

pub fn catalog_path() -> PathBuf {
    imports_dir().join("catalog.sqlite")
}

pub fn catalog_exists() -> Result<bool, ImportError> {
    catalog_path()
        .try_exists()
        .map_err(|error| ImportError::new(ErrorCode::HistoryAccessUnavailable, error.to_string()))
}

pub fn entries_dir() -> PathBuf {
    imports_dir().join("entries")
}

pub fn staging_dir() -> PathBuf {
    imports_dir().join("staging")
}

pub fn open_catalog() -> Result<Connection, ImportError> {
    let dir = imports_dir();
    std::fs::create_dir_all(&dir).map_err(|e| {
        ImportError::new(ErrorCode::SnapshotUnavailable, format!("imports dir: {e}"))
    })?;
    std::fs::create_dir_all(entries_dir()).ok();
    std::fs::create_dir_all(staging_dir()).ok();
    let path = catalog_path();
    let conn = sqlite_open::open(&path).map_err(|e| {
        ImportError::new(ErrorCode::SnapshotUnavailable, format!("open catalog: {e}"))
    })?;
    init_schema(&conn)?;
    ensure_plans_table(&conn)?;
    retention_epoch::ensure(&conn)?;
    Ok(conn)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatalogSort {
    Newest,
    Oldest,
}

impl CatalogSort {
    pub fn parse(raw: Option<&str>) -> Self {
        match raw.map(|s| s.trim().to_ascii_lowercase()).as_deref() {
            Some("oldest") => Self::Oldest,
            _ => Self::Newest,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Newest => "newest",
            Self::Oldest => "oldest",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct EntryPageFilter<'a> {
    pub provider: Option<&'a str>,
    pub query: Option<&'a str>,
    pub sort: CatalogSort,
    pub workspace: Option<&'a super::workspace_catalog::WorkspaceFilter>,
}

impl<'a> EntryPageFilter<'a> {
    #[cfg(test)]
    pub fn default_newest() -> Self {
        Self {
            provider: None,
            query: None,
            sort: CatalogSort::Newest,
            workspace: None,
        }
    }

    fn cursor_filter(self) -> String {
        serde_json::json!([
            "entries-v2",
            self.provider.unwrap_or("").trim(),
            self.query.unwrap_or("").trim().to_lowercase(),
            self.sort.as_str(),
            self.workspace
        ])
        .to_string()
    }
}

pub fn open_catalog_readonly() -> Result<Connection, ImportError> {
    let conn =
        sqlite_open::open_with_flags(catalog_path(), rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|error| match error.sqlite_error_code() {
                Some(
                    rusqlite::ErrorCode::DatabaseBusy
                    | rusqlite::ErrorCode::DatabaseLocked
                    | rusqlite::ErrorCode::DiskFull,
                ) => sqlite_storage_error(error),
                _ => ImportError::new(ErrorCode::HistoryAccessUnavailable, error.to_string()),
            })?;
    // An initializing writer exposes an empty SQLite file before its schema
    // transaction commits. Only this empty state may be retried; an unversioned
    // populated catalog or unsupported schema remains a hard error.
    let deadline =
        std::time::Instant::now() + std::time::Duration::from_millis(super::budget::DB_BUSY_MS);
    loop {
        match verify_catalog_schema(&conn) {
            Ok(()) => return Ok(conn),
            Err(error)
                if error.code == ErrorCode::HistoryAccessUnavailable
                    && error.message == "catalog is not initialized"
                    && std::time::Instant::now() < deadline =>
            {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            Err(error) => return Err(error),
        }
    }
}

pub(crate) fn read_schema_version(conn: &Connection) -> Result<Option<u32>, ImportError> {
    let has_meta: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='meta')",
            [],
            |r| r.get(0),
        )
        .map_err(sqlite_storage_error)?;
    if !has_meta {
        return Ok(None);
    }
    let ver: Option<String> = conn
        .query_row(
            "SELECT value FROM meta WHERE key = 'schema_version'",
            [],
            |r| r.get(0),
        )
        .optional()
        .map_err(sqlite_storage_error)?;
    match ver {
        None => Ok(None),
        Some(v) => {
            let n: u32 = v.parse().map_err(|_| {
                ImportError::new(
                    ErrorCode::SnapshotUnavailable,
                    "catalog schema version is corrupt",
                )
            })?;
            Ok(Some(n))
        }
    }
}

pub fn verify_catalog_schema(conn: &Connection) -> Result<(), ImportError> {
    conn.busy_timeout(std::time::Duration::from_millis(super::budget::DB_BUSY_MS))
        .map_err(sqlite_storage_error)?;
    // Table existence and version must belong to one snapshot. Otherwise a
    // concurrent schema commit can appear as a populated but unversioned store.
    if conn.is_autocommit() {
        let tx = conn.unchecked_transaction().map_err(sqlite_storage_error)?;
        verify_schema_without_timeout_change(&tx)
    } else {
        verify_schema_without_timeout_change(conn)
    }
}

/// Existing stores only. Never wait for a catalog writer while holding a JSON
/// store lock: workspace writers reserve SQLite before acquiring that lock.
pub(super) fn open_catalog_under_store_lock() -> Result<Connection, ImportError> {
    let conn =
        sqlite_open::open_with_flags(catalog_path(), rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE)
            .map_err(super::errors::storage_error)?;
    conn.busy_timeout(std::time::Duration::ZERO)
        .map_err(super::errors::storage_error)?;
    verify_schema_without_timeout_change(&conn).map_err(|error| {
        if error.message.contains("database is locked")
            || error.message.contains("database table is locked")
        {
            super::errors::storage_error(error)
        } else {
            error
        }
    })?;
    Ok(conn)
}

/// Under a JSON store lock, never wait to acquire SQLite's writer reservation:
/// the existing writer may be waiting for that JSON lock. Once we own Immediate,
/// no other writer can hold that reservation; bounded waits for readers at commit
/// are safe and avoid rejecting normal concurrent metadata reads.
pub(super) fn write_under_store_lock<T>(
    conn: &Connection,
    write: impl FnOnce(&Connection) -> Result<T, ImportError>,
) -> Result<T, ImportError> {
    if !conn.is_autocommit() {
        return write(conn);
    }
    conn.busy_timeout(std::time::Duration::ZERO)
        .map_err(sqlite_storage_error)?;
    let tx = rusqlite::Transaction::new_unchecked(conn, rusqlite::TransactionBehavior::Immediate)
        .map_err(sqlite_storage_error)?;
    let result = (|| {
        conn.busy_timeout(std::time::Duration::from_millis(super::budget::DB_BUSY_MS))
            .map_err(sqlite_storage_error)?;
        let value = write(&tx)?;
        tx.commit().map_err(sqlite_storage_error)?;
        Ok(value)
    })();
    conn.busy_timeout(std::time::Duration::ZERO)
        .map_err(sqlite_storage_error)?;
    result
}

fn verify_schema_without_timeout_change(conn: &Connection) -> Result<(), ImportError> {
    match read_schema_version(conn)? {
        Some(n) if n == CATALOG_SCHEMA_VERSION => verify_body_search_schema(conn),
        Some(n) if n > CATALOG_SCHEMA_VERSION => Err(ImportError::new(
            ErrorCode::ImportSchemaTooNew,
            format!("catalog schema {n} is newer than this app"),
        )),
        Some(n) => Err(ImportError::new(
            ErrorCode::SchemaUnsupported,
            format!("catalog schema {n} requires an explicit index rebuild"),
        )),
        None => {
            let tables: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'",
                    [],
                    |r| r.get(0),
                )
                .map_err(sqlite_storage_error)?;
            if tables == 0 {
                Err(ImportError::new(
                    ErrorCode::HistoryAccessUnavailable,
                    "catalog is not initialized",
                ))
            } else {
                Err(ImportError::new(
                    ErrorCode::SnapshotUnavailable,
                    "existing catalog has no schema version",
                ))
            }
        }
    }
}

fn init_schema(conn: &Connection) -> Result<(), ImportError> {
    conn.busy_timeout(std::time::Duration::from_millis(super::budget::DB_BUSY_MS))
        .map_err(|e| ImportError::new(ErrorCode::SnapshotUnavailable, e.to_string()))?;
    if let Some(n) = read_schema_version(conn)? {
        if n > CATALOG_SCHEMA_VERSION {
            return Err(ImportError::new(
                ErrorCode::ImportSchemaTooNew,
                format!("catalog schema {n} is newer than this app"),
            ));
        }
        if n != CATALOG_SCHEMA_VERSION {
            return Err(ImportError::new(
                ErrorCode::SchemaUnsupported,
                format!("catalog schema {n} requires an explicit index rebuild"),
            ));
        }
        return verify_body_search_schema(conn);
    }
    let tables: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'",
            [],
            |r| r.get(0),
        )
        .map_err(|e| ImportError::new(ErrorCode::SnapshotUnavailable, e.to_string()))?;
    if tables != 0 {
        return Err(ImportError::new(
            ErrorCode::SnapshotUnavailable,
            "existing catalog has no schema version",
        ));
    }
    let tx = conn.unchecked_transaction().map_err(sqlite_storage_error)?;
    tx.execute_batch(SCHEMA_SQL).map_err(sqlite_storage_error)?;
    tx.execute(
        "INSERT INTO meta(key, value) VALUES ('schema_version', ?1)",
        params![CATALOG_SCHEMA_VERSION.to_string()],
    )
    .map_err(sqlite_storage_error)?;
    retention_epoch::initialize_new(&tx)?;
    tx.commit().map_err(sqlite_storage_error)?;
    verify_body_search_schema(conn)
}

pub(crate) fn verify_body_search_schema(conn: &Connection) -> Result<(), ImportError> {
    let has_legacy: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='body_search')",
            [],
            |row| row.get(0),
        )
        .map_err(sqlite_storage_error)?;
    if has_legacy {
        return Err(ImportError::new(
            ErrorCode::SchemaUnsupported,
            "legacy catalog body search schema is unsupported",
        ));
    }

    let mut stmt = conn
        .prepare("PRAGMA table_info(body_search_chunks)")
        .map_err(sqlite_storage_error)?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?.to_ascii_uppercase(),
                row.get::<_, i64>(3)? != 0,
                row.get::<_, i64>(5)?,
            ))
        })
        .map_err(sqlite_storage_error)?;
    let columns = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(sqlite_storage_error)?;
    let expected = vec![
        ("import_id".to_string(), "TEXT".to_string(), true, 1),
        ("revision_id".to_string(), "TEXT".to_string(), true, 2),
        ("chunk_ordinal".to_string(), "INTEGER".to_string(), true, 3),
        ("text".to_string(), "TEXT".to_string(), true, 0),
    ];
    if columns != expected {
        return Err(ImportError::new(
            ErrorCode::SchemaUnsupported,
            "catalog body search schema is incompatible",
        ));
    }
    super::source_location_store::verify_schema(conn)?;
    super::registration_provenance_store::verify_schema(conn)?;
    Ok(())
}

fn ensure_plans_table(conn: &Connection) -> Result<(), ImportError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS plans (
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
);",
    )
    .map_err(|e| ImportError::new(ErrorCode::SnapshotUnavailable, e.to_string()))
}

pub fn persist_plan_json(
    conn: &Connection,
    plan_id: &str,
    plan_hash: &str,
    json: &str,
    created_at: &str,
    expires_at: &str,
) -> Result<(), ImportError> {
    conn.execute(
        "INSERT OR REPLACE INTO plans(plan_id, plan_hash, json, created_at, expires_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![plan_id, plan_hash, json, created_at, expires_at],
    )
    .map_err(sqlite_storage_error)?;
    Ok(())
}

pub fn load_plan_json(conn: &Connection, plan_id: &str) -> Result<(String, String), ImportError> {
    conn.query_row(
        "SELECT plan_hash, json FROM plans WHERE plan_id = ?1",
        params![plan_id],
        |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
    )
    .optional()
    .map_err(|e| ImportError::new(ErrorCode::SnapshotUnavailable, e.to_string()))?
    .ok_or_else(|| ImportError::new(ErrorCode::PlanStale, "plan not found"))
}

#[cfg(test)]
pub fn replace_plan_json(conn: &Connection, plan_id: &str, json: &str) -> Result<(), ImportError> {
    let n = conn
        .execute(
            "UPDATE plans SET json = ?1 WHERE plan_id = ?2",
            params![json, plan_id],
        )
        .map_err(|e| ImportError::new(ErrorCode::SnapshotUnavailable, e.to_string()))?;
    if n == 0 {
        return Err(ImportError::new(ErrorCode::PlanStale, "plan not found"));
    }
    Ok(())
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogEntry {
    pub import_id: String,
    pub installation_id: String,
    pub provider: String,
    pub native_session_id: String,
    pub title: String,
    pub alias: Option<String>,
    pub hidden: bool,
    pub current_revision: Option<String>,
    pub created_at: String,
}

pub fn find_entry(
    conn: &Connection,
    installation_id: &str,
    provider: &str,
    native_session_id: &str,
) -> Result<Option<CatalogEntry>, ImportError> {
    conn.query_row(
        "SELECT import_id, installation_id, provider, native_session_id, title, alias, hidden, current_revision, created_at
         FROM entries WHERE installation_id=?1 AND provider=?2 AND native_session_id=?3",
        params![installation_id, provider, native_session_id],
        row_entry,
    )
    .optional()
    .map_err(|e| ImportError::new(ErrorCode::SnapshotUnavailable, e.to_string()))
}

pub fn get_entry(conn: &Connection, import_id: &str) -> Result<Option<CatalogEntry>, ImportError> {
    conn.query_row(
        "SELECT import_id, installation_id, provider, native_session_id, title, alias, hidden, current_revision, created_at
         FROM entries WHERE import_id=?1",
        params![import_id],
        row_entry,
    )
    .optional()
    .map_err(|e| ImportError::new(ErrorCode::SnapshotUnavailable, e.to_string()))
}

fn row_entry(row: &rusqlite::Row<'_>) -> rusqlite::Result<CatalogEntry> {
    Ok(CatalogEntry {
        import_id: row.get(0)?,
        installation_id: row.get(1)?,
        provider: row.get(2)?,
        native_session_id: row.get(3)?,
        title: row.get::<_, Option<String>>(4)?.unwrap_or_default(),
        alias: row.get(5)?,
        hidden: row.get::<_, i64>(6)? != 0,
        current_revision: row.get(7)?,
        created_at: row.get(8)?,
    })
}

#[cfg(test)]
pub fn list_entries(conn: &Connection) -> Result<Vec<CatalogEntry>, ImportError> {
    Ok(list_entries_page(
        conn,
        None,
        Some(crate::session_migration::budget::PAGE_SIZE_MAX),
    )?
    .0)
}

pub fn list_entry_providers(conn: &Connection) -> Result<Vec<String>, ImportError> {
    let mut stmt = conn
        .prepare(
            "SELECT DISTINCT provider FROM entries WHERE hidden=0 AND provider <> ''
             AND NOT EXISTS (SELECT 1 FROM tombstones t WHERE t.installation_id=entries.installation_id
               AND t.provider=entries.provider AND t.native_session_id=entries.native_session_id) ORDER BY provider",
        )
        .map_err(|e| ImportError::new(ErrorCode::SnapshotUnavailable, e.to_string()))?;
    let rows = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|e| ImportError::new(ErrorCode::SnapshotUnavailable, e.to_string()))?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(|e| ImportError::new(ErrorCode::SnapshotUnavailable, e.to_string()))?);
    }
    Ok(out)
}

#[cfg(test)]
pub fn list_entries_page(
    conn: &Connection,
    cursor: Option<&str>,
    limit: Option<usize>,
) -> Result<(Vec<CatalogEntry>, Option<String>, u32), ImportError> {
    list_entries_page_filtered(conn, cursor, limit, EntryPageFilter::default_newest())
}

pub fn list_entries_page_filtered(
    conn: &Connection,
    cursor: Option<&str>,
    limit: Option<usize>,
    filter: EntryPageFilter<'_>,
) -> Result<(Vec<CatalogEntry>, Option<String>, u32), ImportError> {
    let owned = conn
        .is_autocommit()
        .then(|| {
            conn.unchecked_transaction()
                .map_err(|e| ImportError::new(ErrorCode::SnapshotUnavailable, e.to_string()))
        })
        .transpose()?;
    let conn = owned.as_deref().unwrap_or(conn);
    let cap = crate::session_migration::budget::page_limit(limit);
    let provider = filter.provider.unwrap_or("").trim();
    let query = filter.query.unwrap_or("").trim().to_lowercase();
    let (workspace_kind, workspace_id) = filter
        .workspace
        .map_or(Ok((0, "")), |filter| filter.sql_value())?;
    let has_associations =
        workspace_kind != 0 && super::workspace_catalog::validate_memberships(conn)?;
    let membership_table = if has_associations {
        "entry_workspaces"
    } else {
        "(SELECT NULL import_id, NULL json WHERE 0)"
    };
    let from = format!("FROM entries e LEFT JOIN {membership_table} a ON a.import_id=e.import_id");
    let conditions = "e.hidden=0
        AND NOT EXISTS (SELECT 1 FROM tombstones t WHERE t.installation_id=e.installation_id AND t.provider=e.provider AND t.native_session_id=e.native_session_id)
        AND (?1='' OR e.provider=?1)
        AND (?2='' OR instr(lower(ifnull(e.title,'') || ' ' || ifnull(e.alias,'') || ' ' || e.native_session_id),?2)>0)
        AND CASE ?6 WHEN 0 THEN 1 WHEN 1 THEN json_extract(a.json,'$.choice') IS NULL
            ELSE json_extract(a.json,'$.choice.sourceWorkspaceId')=?7 END";
    let filter_key = filter.cursor_filter();
    let scope = CursorScope {
        owner: "catalog",
        snapshot: "entries",
        leaf: "",
        filter: filter_key.as_str(),
    };
    let newest = filter.sort == CatalogSort::Newest;
    let total: u32 = conn
        .query_row(
            &format!("SELECT COUNT(*) {from} WHERE {conditions}"),
            params![provider, query, "", "", 0, workspace_kind, workspace_id],
            |r| r.get::<_, i64>(0),
        )
        .map_err(|e| ImportError::new(ErrorCode::SnapshotUnavailable, e.to_string()))?
        .try_into()
        .unwrap_or(0);
    let (created, import_id) = match cursor.filter(|s| !s.is_empty()) {
        None => {
            if newest {
                (String::from("~"), String::new())
            } else {
                (String::new(), String::new())
            }
        }
        Some(raw) => {
            let bound = decode_bound_cursor(raw, scope)?;
            let mark = bound.mark.ok_or_else(|| {
                ImportError::new(ErrorCode::PlanStale, "catalog cursor is missing its mark")
            })?;
            mark.split_once('\n')
                .map(|(created, import_id)| (created.to_string(), import_id.to_string()))
                .ok_or_else(|| ImportError::new(ErrorCode::PlanStale, "invalid catalog cursor"))?
        }
    };
    let (comparison, order) = if newest { ("<", "DESC") } else { (">", "ASC") };
    let sql = format!("SELECT e.import_id, e.installation_id, e.provider, e.native_session_id, e.title, e.alias, e.hidden, e.current_revision, e.created_at
        {from} WHERE {conditions} AND (e.created_at {comparison} ?3 OR (e.created_at=?3 AND e.import_id>?4))
        ORDER BY e.created_at {order}, e.import_id ASC LIMIT ?5");
    let mut stmt = conn
        .prepare(&sql)
        .map_err(|e| ImportError::new(ErrorCode::SnapshotUnavailable, e.to_string()))?;
    let rows = stmt
        .query_map(
            params![
                provider,
                query,
                created,
                import_id,
                (cap + 1) as i64,
                workspace_kind,
                workspace_id
            ],
            row_entry,
        )
        .map_err(|e| ImportError::new(ErrorCode::SnapshotUnavailable, e.to_string()))?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(|e| ImportError::new(ErrorCode::SnapshotUnavailable, e.to_string()))?);
    }
    let next = if out.len() > cap {
        out.pop();
        match out.last() {
            Some(entry) => {
                let mark = format!("{}\n{}", entry.created_at, entry.import_id);
                Some(encode_bound_cursor(scope, 0, Some(mark))?)
            }
            None => None,
        }
    } else {
        None
    };
    Ok((out, next, total))
}

pub fn persist_draft_json(
    conn: &Connection,
    draft_id: &str,
    draft_hash: &str,
    import_id: &str,
    json: &str,
    created_at: &str,
    expires_at: &str,
) -> Result<(), ImportError> {
    conn.execute(
        "DELETE FROM drafts WHERE import_id = ?1",
        params![import_id],
    )
    .map_err(sqlite_storage_error)?;
    conn.execute(
        "INSERT OR REPLACE INTO drafts(draft_id, draft_hash, import_id, json, created_at, expires_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![draft_id, draft_hash, import_id, json, created_at, expires_at],
    )
    .map_err(sqlite_storage_error)?;
    Ok(())
}

pub fn load_draft_json(conn: &Connection, draft_id: &str) -> Result<Option<String>, ImportError> {
    conn.query_row(
        "SELECT json FROM drafts WHERE draft_id = ?1",
        params![draft_id],
        |row| row.get::<_, String>(0),
    )
    .optional()
    .map_err(|e| ImportError::new(ErrorCode::SnapshotUnavailable, e.to_string()))
}

pub fn latest_draft_json(conn: &Connection) -> Result<Option<(String, String)>, ImportError> {
    conn.query_row(
        "SELECT json, expires_at FROM drafts ORDER BY created_at DESC LIMIT 1",
        [],
        |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
    )
    .optional()
    .map_err(|e| ImportError::new(ErrorCode::SnapshotUnavailable, e.to_string()))
}

pub fn delete_draft(conn: &Connection, draft_id: &str) -> Result<(), ImportError> {
    conn.execute("DELETE FROM drafts WHERE draft_id = ?1", params![draft_id])
        .map_err(|e| ImportError::new(ErrorCode::SnapshotUnavailable, e.to_string()))?;
    Ok(())
}

pub fn latest_plan_row(
    conn: &Connection,
) -> Result<Option<(String, String, String, String)>, ImportError> {
    conn.query_row(
        "SELECT plan_id, plan_hash, json, expires_at FROM plans
         ORDER BY created_at DESC, rowid DESC LIMIT 1",
        [],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        },
    )
    .optional()
    .map_err(|e| ImportError::new(ErrorCode::SnapshotUnavailable, e.to_string()))
}

pub fn latest_active_job_id(conn: &Connection) -> Result<Option<String>, ImportError> {
    conn.query_row(
        "SELECT job_id FROM jobs
         WHERE state IN ('queued','running','cancelling','recovery_needed')
         ORDER BY created_at DESC, rowid DESC LIMIT 1",
        [],
        |row| row.get::<_, String>(0),
    )
    .optional()
    .map_err(|e| ImportError::new(ErrorCode::SnapshotUnavailable, e.to_string()))
}

pub fn latest_plan_job_id(
    conn: &Connection,
    plan_id: &str,
    plan_hash: &str,
) -> Result<Option<String>, ImportError> {
    let row = conn
        .query_row(
            "SELECT job_id, plan_hash, state,
                    json_set(json_remove(json, '$.items'), '$.items', json('[]'))
             FROM jobs WHERE json_extract(json, '$.planId')=?1
             ORDER BY created_at DESC, rowid DESC LIMIT 1",
            params![plan_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            },
        )
        .optional()
        .map_err(|e| ImportError::new(ErrorCode::SnapshotUnavailable, e.to_string()))?;
    let Some((job_id, stored_hash, state, header)) = row else {
        return Ok(None);
    };
    let job: super::jobs::JobRecord = serde_json::from_str(&header)
        .map_err(|e| ImportError::new(ErrorCode::SnapshotUnavailable, e.to_string()))?;
    // A matching ID with a divergent hash/header is corruption, not permission
    // to submit the plan again. Only deserialize the bounded job header here.
    if job.job_id != job_id
        || job.plan_id != plan_id
        || job.plan_hash != plan_hash
        || stored_hash != plan_hash
        || job.state.as_str() != state
    {
        return Err(ImportError::new(
            ErrorCode::SnapshotUnavailable,
            "persisted plan/job identity is inconsistent",
        ));
    }
    Ok(Some(job_id))
}

#[path = "catalog_entries.rs"]
mod catalog_entries;
pub use catalog_entries::*;

#[path = "catalog_maintenance.rs"]
mod catalog_maintenance;
pub use catalog_maintenance::*;

#[cfg(test)]
mod tests {
    use super::*;
    include!("catalog_tests.rs");
}
