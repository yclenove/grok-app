//! Optional catalog metadata. Reading legacy catalogs never creates the table.
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use super::workspace_association::WorkspaceChoice;
use super::{
    errors::{ErrorCode, ImportError},
    workspace_import::db_error,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Record {
    pub schema_version: u32,
    pub import_id: String,
    pub version: u64,
    pub source_id: String,
    pub provider: String,
    pub choice: Option<WorkspaceChoice>,
    pub request_id: String,
    pub expected_version: u64,
}

pub(super) fn available(conn: &Connection) -> Result<bool, ImportError> {
    let table: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='entry_workspaces' AND type='table')",
        [], |row| row.get(0),
    ).map_err(db_error)?;
    let version: Option<String> = conn
        .query_row(
            "SELECT value FROM meta WHERE key='entry_workspaces_version'",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(db_error)?;
    match (table, version.as_deref()) {
        (false, None) => Ok(false),
        (true, Some("1")) => Ok(true),
        _ => Err(ImportError::new(
            ErrorCode::SnapshotUnavailable,
            "workspace association schema is missing or unsupported",
        )),
    }
}

pub(super) fn load(conn: &Connection, import_id: &str) -> Result<Option<Record>, ImportError> {
    if !available(conn)? {
        return Ok(None);
    }
    let raw: Option<String> = conn
        .query_row(
            "SELECT json FROM entry_workspaces WHERE import_id=?1",
            [import_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(db_error)?;
    raw.map(|raw| decode(import_id, &raw)).transpose()
}

pub(super) fn decode(import_id: &str, raw: &str) -> Result<Record, ImportError> {
    if raw.len() > 65536 {
        return Err(db_error("workspace association exceeds metadata limit"));
    }
    let record: Record = serde_json::from_str(raw).map_err(db_error)?;
    if record.schema_version != 1
        || record.import_id != import_id
        || record.version == 0
        || record.version > 9_007_199_254_740_991
        || record.source_id.is_empty()
        || record.provider.is_empty()
        || record.request_id.is_empty()
        || record.expected_version.checked_add(1) != Some(record.version)
    {
        return Err(db_error("invalid workspace association record"));
    }
    if let Some(choice) = &record.choice {
        choice.validate()?;
    }
    Ok(record)
}

/// Caller holds a catalog write transaction across version checks and this write.
pub(super) fn save(conn: &Connection, record: &Record) -> Result<(), ImportError> {
    if conn.is_autocommit() {
        return Err(db_error("association write requires a transaction"));
    }
    let json = serde_json::to_string(record).map_err(db_error)?;
    if json.len() > 65536 {
        return Err(db_error("workspace association exceeds metadata limit"));
    }
    if !available(conn)? {
        conn.execute_batch(
            "CREATE TABLE entry_workspaces(import_id TEXT PRIMARY KEY, json TEXT NOT NULL);
            INSERT INTO meta(key,value) VALUES('entry_workspaces_version','1');",
        )
        .map_err(db_error)?;
    }
    conn.execute(
        "INSERT INTO entry_workspaces(import_id,json) VALUES(?1,?2)
        ON CONFLICT(import_id) DO UPDATE SET json=excluded.json",
        params![record.import_id, json],
    )
    .map_err(db_error)?;
    Ok(())
}

pub(super) fn revoke(conn: &Connection, import_id: &str) -> Result<(), ImportError> {
    let Some(entry) = super::catalog::get_entry(conn, import_id)? else {
        return Ok(());
    };
    // A window may still hold version zero even when no association was saved yet.
    // Persist the revocation version so an explicit reimport cannot revive that draft.
    let previous = load(conn, import_id)?;
    let expected_version = previous.map_or(0, |record| record.version);
    let version = expected_version
        .checked_add(1)
        .filter(|n| *n <= 9_007_199_254_740_991)
        .ok_or_else(|| db_error("association version exhausted"))?;
    save(
        conn,
        &Record {
            schema_version: 1,
            import_id: import_id.into(),
            version,
            source_id: entry.installation_id,
            provider: entry.provider,
            choice: None,
            request_id: uuid::Uuid::new_v4().to_string(),
            expected_version,
        },
    )
}
