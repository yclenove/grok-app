//! Optional catalog extension, created only by explicit association. Read paths
//! validate its version and exact constraints; they never repair a damaged store.
use super::errors::{sqlite_storage_error, ErrorCode, ImportError};
use super::{hash::options_hash, sources, types::Provider};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::json;

const VERSION_KEY: &str = "source_location_schema_version";
const OWNER_SQL: &str = "CREATE TABLE source_location_owners (
  source_id TEXT PRIMARY KEY NOT NULL,
  primary_stamp TEXT NOT NULL CHECK(length(primary_stamp)=64)
)";
const LOCATION_SQL: &str = "CREATE TABLE source_locations (
  seq INTEGER PRIMARY KEY AUTOINCREMENT CHECK(seq>0),
  location_id TEXT NOT NULL UNIQUE CHECK(length(location_id)=36),
  source_id TEXT NOT NULL CHECK(length(source_id)=36),
  provider TEXT NOT NULL CHECK(provider='cursor'),
  host TEXT NOT NULL CHECK(host='local'),
  root_path TEXT NOT NULL CHECK(length(CAST(root_path AS BLOB))<=8192),
  path_key TEXT NOT NULL CHECK(length(CAST(path_key AS BLOB))<=8192),
  evidence TEXT NOT NULL CHECK(evidence='explicit-association-v1')
)";
const PATH_SQL: &str =
    "CREATE UNIQUE INDEX source_locations_path ON source_locations(provider,host,path_key)";
const PAGE_SQL: &str = "CREATE INDEX source_locations_page ON source_locations(source_id,seq)";
const OBJECTS: [(&str, &str); 4] = [
    ("source_location_owners", OWNER_SQL),
    ("source_locations", LOCATION_SQL),
    ("source_locations_path", PATH_SQL),
    ("source_locations_page", PAGE_SQL),
];

fn incompatible() -> ImportError {
    ImportError::new(
        ErrorCode::SchemaUnsupported,
        "source location catalog extension is incompatible",
    )
}

/// Missing extension is the legacy state. Orphan objects/markers are not.
pub(super) fn verify_schema(conn: &Connection) -> Result<bool, ImportError> {
    // A first association can commit between the marker and object queries.
    // Inspect legacy/extension state in one snapshot, including catalog reopen.
    if conn.is_autocommit() {
        let tx = conn.unchecked_transaction().map_err(sqlite_storage_error)?;
        return verify_schema(&tx);
    }
    let version: Option<String> = conn
        .query_row(
            "SELECT value FROM meta WHERE key=?1",
            [VERSION_KEY],
            |row| row.get(0),
        )
        .optional()
        .map_err(sqlite_storage_error)?;
    if version.as_deref().is_some_and(|v| v != "1") {
        return Err(
            match version.as_deref().and_then(|v| v.parse::<u32>().ok()) {
                Some(v) if v > 1 => ImportError::new(
                    ErrorCode::ImportSchemaTooNew,
                    "source location schema is newer than this app",
                ),
                _ => incompatible(),
            },
        );
    }
    for (name, expected) in OBJECTS {
        let sql: Option<String> = conn
            .query_row(
                "SELECT sql FROM sqlite_master WHERE name=?1",
                [name],
                |row| row.get(0),
            )
            .optional()
            .map_err(sqlite_storage_error)?;
        if version.is_none() {
            if sql.is_some() {
                return Err(incompatible());
            }
        } else if sql.as_deref().map(str::trim) != Some(expected) {
            return Err(incompatible());
        }
    }
    let has_trigger: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='trigger'
        AND tbl_name IN ('source_location_owners','source_locations'))",
            [],
            |row| row.get(0),
        )
        .map_err(sqlite_storage_error)?;
    if has_trigger {
        return Err(incompatible());
    }
    Ok(version.is_some())
}

/// Caller owns Immediate; DDL, marker, owner and location commit atomically.
pub(super) fn initialize(conn: &Connection) -> Result<(), ImportError> {
    if conn.is_autocommit() {
        return Err(incompatible());
    }
    if !verify_schema(conn)? {
        for (_, sql) in OBJECTS {
            conn.execute_batch(sql).map_err(sqlite_storage_error)?;
        }
        conn.execute("INSERT INTO meta(key,value) VALUES(?1,'1')", [VERSION_KEY])
            .map_err(sqlite_storage_error)?;
    }
    verify_schema(conn)?;
    Ok(())
}

fn primary_stamp(conn: &Connection, id: &str) -> Result<String, ImportError> {
    let row: Option<(String, Option<String>, String, Option<String>)> = conn
        .query_row(
            "SELECT provider,host,root_path,profile FROM sources WHERE installation_id=?1
         AND length(CAST(root_path AS BLOB))<=8192
         AND (profile IS NULL OR length(CAST(profile AS BLOB))<=65536)",
            [id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()
        .map_err(sqlite_storage_error)?;
    let (provider, host, root, profile) = row.ok_or_else(|| {
        ImportError::new(
            ErrorCode::SourceChanged,
            "associated installation registration is missing or oversized",
        )
    })?;
    Ok(options_hash(&json!([
        "primary-registration-v1",
        id,
        provider,
        host,
        root,
        profile
    ])))
}

/// A legacy binary may rewrite sources/profile but cannot update our stamp.
/// Refuse that drift; do not silently legitimize it on the next registration.
pub(super) fn verify_owners(conn: &Connection) -> Result<bool, ImportError> {
    if conn.is_autocommit() {
        let tx = conn.unchecked_transaction().map_err(sqlite_storage_error)?;
        return verify_owners(&tx);
    }
    if !verify_schema(conn)? {
        return Ok(false);
    }
    let mut stmt = conn
        .prepare("SELECT source_id,primary_stamp FROM source_location_owners")
        .map_err(sqlite_storage_error)?;
    let mut rows = stmt.query([]).map_err(sqlite_storage_error)?;
    while let Some(row) = rows.next().map_err(sqlite_storage_error)? {
        let id: String = row.get(0).map_err(sqlite_storage_error)?;
        let stamp: String = row.get(1).map_err(sqlite_storage_error)?;
        if uuid::Uuid::parse_str(&id).is_err() || primary_stamp(conn, &id)? != stamp {
            return Err(ImportError::new(
                ErrorCode::SourceChanged,
                "primary source registration changed outside location-aware relocation",
            ));
        }
    }
    let orphan: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM source_locations l LEFT JOIN source_location_owners o
         ON o.source_id=l.source_id WHERE o.source_id IS NULL)",
            [],
            |row| row.get(0),
        )
        .map_err(sqlite_storage_error)?;
    if orphan {
        return Err(incompatible());
    }
    Ok(true)
}

pub(super) fn pin_owner(conn: &Connection, id: &str) -> Result<(), ImportError> {
    let stamp = primary_stamp(conn, id)?;
    conn.execute(
        "INSERT INTO source_location_owners(source_id,primary_stamp) VALUES(?1,?2)
        ON CONFLICT(source_id) DO UPDATE SET primary_stamp=excluded.primary_stamp",
        params![id, stamp],
    )
    .map_err(sqlite_storage_error)?;
    Ok(())
}

pub(super) fn sync_primary(conn: &Connection, id: &str) -> Result<(), ImportError> {
    if verify_schema(conn)? {
        let pinned: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM source_location_owners WHERE source_id=?1)",
                [id],
                |row| row.get(0),
            )
            .map_err(sqlite_storage_error)?;
        if pinned {
            pin_owner(conn, id)?;
        }
    }
    Ok(())
}

fn overlaps(a: &str, b: &str) -> bool {
    let a = sources::identity_key(a);
    let b = sources::identity_key(b);
    let separator = if cfg!(windows) { '\\' } else { '/' };
    a == b
        || a.strip_prefix(&b)
            .is_some_and(|tail| b.ends_with(separator) || tail.starts_with(separator))
        || b.strip_prefix(&a)
            .is_some_and(|tail| a.ends_with(separator) || tail.starts_with(separator))
}

/// Do not register/relocate a primary over an associated root, even implicitly.
pub(super) fn guard_primary_path(
    conn: &Connection,
    provider: Provider,
    root: &str,
) -> Result<(), ImportError> {
    if !verify_owners(conn)? {
        return Ok(());
    }
    let mut stmt = conn
        .prepare("SELECT root_path FROM source_locations WHERE provider=?1 AND host='local'")
        .map_err(sqlite_storage_error)?;
    let mut rows = stmt
        .query([provider.as_str()])
        .map_err(sqlite_storage_error)?;
    while let Some(row) = rows.next().map_err(sqlite_storage_error)? {
        let path: String = row.get(0).map_err(sqlite_storage_error)?;
        if overlaps(&path, root) {
            return Err(ImportError::new(
                ErrorCode::MappingConflict,
                "path overlaps an associated source location; select that location explicitly",
            ));
        }
    }
    Ok(())
}

/// Streaming registrations and bounded aliases, not an unbounded source array.
pub(super) fn guard_associated_path(
    conn: &Connection,
    provider: Provider,
    root: &str,
    allowed_location: Option<&str>,
) -> Result<(), ImportError> {
    let mut stmt = conn
        .prepare("SELECT installation_id FROM sources WHERE provider=?1 AND host='local'")
        .map_err(sqlite_storage_error)?;
    let mut rows = stmt
        .query([provider.as_str()])
        .map_err(sqlite_storage_error)?;
    while let Some(row) = rows.next().map_err(sqlite_storage_error)? {
        let id: String = row.get(0).map_err(sqlite_storage_error)?;
        if uuid::Uuid::parse_str(&id).is_err() {
            continue;
        }
        let source = sources::get(conn, &id)?;
        if sources::registered_paths(conn, &source)?
            .iter()
            .any(|p| overlaps(p, root))
        {
            return Err(ImportError::new(
                ErrorCode::MappingConflict,
                "path overlaps an existing primary source or relocation alias",
            ));
        }
    }
    if verify_schema(conn)? {
        let mut stmt = conn.prepare("SELECT location_id,root_path FROM source_locations WHERE provider=?1 AND host='local'").map_err(sqlite_storage_error)?;
        let mut rows = stmt
            .query([provider.as_str()])
            .map_err(sqlite_storage_error)?;
        while let Some(row) = rows.next().map_err(sqlite_storage_error)? {
            let id: String = row.get(0).map_err(sqlite_storage_error)?;
            let path: String = row.get(1).map_err(sqlite_storage_error)?;
            if allowed_location != Some(id.as_str()) && overlaps(&path, root) {
                return Err(ImportError::new(
                    ErrorCode::MappingConflict,
                    "path overlaps another associated source location",
                ));
            }
        }
    }
    Ok(())
}
