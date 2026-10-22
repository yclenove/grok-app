//! Immutable creation input; dispatch lifecycle remains in continuation_refs.
use super::{
    errors::{sqlite_storage_error, ImportError},
    hash::sha256_bytes,
};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Intent {
    pub version: u32,
    pub draft_id: String,
    pub draft_hash: String,
    pub request_id: String,
    pub meta: crate::store::SessionMeta,
    pub workspace: Option<crate::workspace_store::WorkspaceRecord>,
    pub text: String,
    pub published: bool,
    #[serde(default)]
    pub cancelled: bool,
}

pub(super) fn error(cause: impl std::fmt::Display) -> ImportError {
    super::errors::storage_error(cause)
}

/// Called under the native index writer lock, before removing a visible row.
/// An interrupted publication cannot later resurrect a deliberately deleted chat.
pub(crate) fn record_published_before_removal(sid: &str) -> Result<(), String> {
    let path = super::catalog::catalog_path();
    if !path.try_exists().map_err(|e| e.to_string())? {
        return Ok(());
    }
    let conn = super::catalog::open_catalog_readonly().map_err(|e| e.message)?;
    if !exists(&conn).map_err(|e| e.message)? {
        return Ok(());
    }
    let request: Option<String> = conn
        .query_row(
            "SELECT request_id FROM continuation_intents WHERE app_session_id=?1",
            [sid],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let Some(request) = request else {
        return Ok(());
    };
    let mut intent = load(&conn, &request)
        .map_err(|e| e.message)?
        .ok_or("creation intent disappeared")?;
    if intent.published {
        return Ok(());
    }
    drop(conn);
    let conn = super::catalog::open_catalog_under_store_lock().map_err(|e| e.into_command_err())?;
    conn.pragma_update(None, "synchronous", "FULL")
        .map_err(|e| e.to_string())?;
    intent.published = true;
    save(&conn, &intent).map_err(|e| e.message)
}

pub(super) fn exists(conn: &Connection) -> Result<bool, ImportError> {
    conn.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='continuation_intents')", [], |row| row.get(0)).map_err(sqlite_storage_error)
}

pub(super) fn load(conn: &Connection, request: &str) -> Result<Option<Intent>, ImportError> {
    if !exists(conn)? {
        return Ok(None);
    }
    let raw: Option<(String, String, String, String)> = conn.query_row(
        "SELECT CASE WHEN length(CAST(json AS BLOB))<=2097152 THEN json ELSE NULL END, digest, draft_id, app_session_id FROM continuation_intents WHERE request_id=?1",
        [request], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
    ).optional().map_err(sqlite_storage_error)?;
    raw.map(|(json, digest, draft_id, session_id)| {
        if sha256_bytes(json.as_bytes()) != digest {
            return Err(error("creation intent digest mismatch"));
        }
        let intent: Intent = serde_json::from_str(&json).map_err(error)?;
        if intent.version != 1
            || intent.request_id != request
            || uuid::Uuid::parse_str(&intent.meta.id).is_err()
            || intent.draft_id != draft_id
            || intent.meta.id != session_id
        {
            return Err(error("invalid continuation creation intent"));
        }
        Ok(intent)
    })
    .transpose()
}

pub(super) fn save(conn: &Connection, intent: &Intent) -> Result<(), ImportError> {
    let json = serde_json::to_string(intent).map_err(error)?;
    if json.len() > 2 * 1024 * 1024 {
        return Err(error("creation intent exceeds metadata limit"));
    }
    super::catalog::write_under_store_lock(conn, |conn| {
        conn.execute_batch("CREATE TABLE IF NOT EXISTS continuation_intents (
        request_id TEXT PRIMARY KEY, draft_id TEXT NOT NULL UNIQUE, app_session_id TEXT NOT NULL UNIQUE,
        json TEXT NOT NULL, digest TEXT NOT NULL);").map_err(sqlite_storage_error)?;
        conn.execute("INSERT INTO continuation_intents(request_id,draft_id,app_session_id,json,digest) VALUES(?1,?2,?3,?4,?5)
        ON CONFLICT(request_id) DO UPDATE SET json=excluded.json,digest=excluded.digest",
        params![intent.request_id, intent.draft_id, intent.meta.id, json, sha256_bytes(json.as_bytes())]).map_err(sqlite_storage_error)?;
        Ok(())
    })
}

pub(super) fn draft_claimed(conn: &Connection, draft_id: &str) -> Result<bool, ImportError> {
    if !exists(conn)? {
        return Ok(false);
    }
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM continuation_intents WHERE draft_id=?1)",
        [draft_id],
        |row| row.get(0),
    )
    .map_err(sqlite_storage_error)
}

pub(super) fn for_draft(conn: &Connection, draft_id: &str) -> Result<Option<Intent>, ImportError> {
    if !exists(conn)? {
        return Ok(None);
    }
    let request: Option<String> = conn
        .query_row(
            "SELECT request_id FROM continuation_intents WHERE draft_id=?1",
            [draft_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(sqlite_storage_error)?;
    request
        .map(|request| load(conn, &request))
        .transpose()
        .map(Option::flatten)
}

pub(super) fn hydrate(
    conn: &Connection,
    draft: &mut super::continuation::HandoffDraft,
) -> Result<bool, ImportError> {
    let Some(intent) = for_draft(conn, &draft.draft_id)? else {
        return Ok(false);
    };
    if intent.draft_hash != draft.draft_hash {
        return Err(error("creation draft identity mismatch"));
    }
    if intent.cancelled {
        return Err(error("continuation creation was cancelled"));
    }
    draft.text = intent.text;
    draft.creation_request_id = Some(intent.request_id);
    Ok(true)
}
