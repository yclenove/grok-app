//! Explicit imported-history membership; never changes native execution bindings.
use rusqlite::{params, Connection, TransactionBehavior};
use serde::{Deserialize, Serialize};

use super::{
    catalog::{self, CatalogEntry},
    errors::{ErrorCode, ImportError},
    hash::sha256_bytes,
    workspace::SourceWorkspace,
    workspace_association_store::{self as storage, Record},
    workspace_import::{db_error, stored_json},
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceChoice {
    pub source_workspace_id: String,
    pub revision: String,
    pub display_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_locator: Option<String>,
}

impl WorkspaceChoice {
    pub(super) fn validate(&self) -> Result<(), ImportError> {
        if self.source_workspace_id.is_empty()
            || self.source_workspace_id.len() > 256
            || self.revision.len() != 64
            || !self.revision.bytes().all(|b| b.is_ascii_hexdigit())
            || self.display_name.len() > 8192
            || self
                .source_locator
                .as_ref()
                .is_some_and(|path| path.len() > 16384)
        {
            return Err(db_error("invalid association workspace reference"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Association {
    pub import_id: String,
    pub version: u64,
    pub source_id: String,
    pub provider: String,
    pub source_ready: bool,
    pub choice: Option<WorkspaceChoice>,
    pub status: AssociationStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum AssociationStatus {
    Unassigned,
    Assigned,
    Stale,
    Missing,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateRequest {
    pub import_id: String,
    pub expected_version: u64,
    pub request_id: String,
    pub choice: Option<WorkspaceChoice>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspacePage {
    pub items: Vec<WorkspaceChoice>,
    pub next_cursor: Option<String>,
}

fn conflict(message: &str) -> ImportError {
    ImportError::new(ErrorCode::MappingConflict, message)
}
fn stale() -> ImportError {
    ImportError::new(
        ErrorCode::PlanStale,
        "workspace association changed; refresh before saving",
    )
}

fn entry(conn: &Connection, import_id: &str) -> Result<CatalogEntry, ImportError> {
    if !super::ids::is_import_id(import_id) {
        return Err(conflict("invalid imported history identity"));
    }
    let entry = catalog::get_entry(conn, import_id)?
        .ok_or_else(|| ImportError::new(ErrorCode::SourceNotFound, "imported history not found"))?;
    if entry.hidden
        || catalog::is_tombstoned(
            conn,
            &entry.installation_id,
            &entry.provider,
            &entry.native_session_id,
        )?
    {
        return Err(ImportError::new(
            ErrorCode::HistoryAccessUnavailable,
            "imported history is revoked",
        ));
    }
    Ok(entry)
}

fn source_ready(conn: &Connection, entry: &CatalogEntry) -> Result<bool, ImportError> {
    if uuid::Uuid::parse_str(&entry.installation_id).is_err() {
        return Ok(false);
    }
    let exists: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sources WHERE installation_id=?1)",
            [&entry.installation_id],
            |row| row.get(0),
        )
        .map_err(db_error)?;
    if !exists {
        return Ok(false);
    }
    let source = super::sources::get(conn, &entry.installation_id)?;
    Ok(source.provider.as_str() == entry.provider)
}

pub(super) fn workspace(
    conn: &Connection,
    id: &str,
) -> Result<Option<(WorkspaceChoice, SourceWorkspace)>, ImportError> {
    stored_json(conn, id)?
        .map(|raw| {
            if raw.len() > 8 * 1024 * 1024 {
                return Err(db_error("workspace metadata exceeds limit"));
            }
            let workspace: SourceWorkspace = serde_json::from_str(&raw).map_err(db_error)?;
            if workspace.source_workspace_id != id {
                return Err(db_error("workspace identity mismatch"));
            }
            let choice = WorkspaceChoice {
                source_workspace_id: id.into(),
                revision: sha256_bytes(raw.as_bytes()),
                display_name: workspace.display_name.clone(),
                source_locator: workspace
                    .source_identity
                    .as_ref()
                    .map(|identity| identity.locator.clone()),
            };
            choice.validate()?;
            Ok((choice, workspace))
        })
        .transpose()
}

fn belongs(workspace: &SourceWorkspace, entry: &CatalogEntry) -> bool {
    workspace.source_identity.as_ref().is_some_and(|identity| {
        identity.source_id == entry.installation_id && identity.provider == entry.provider
    })
}

fn describe(
    conn: &Connection,
    entry: CatalogEntry,
    record: Option<Record>,
) -> Result<Association, ImportError> {
    let source_ready = source_ready(conn, &entry)?;
    let version = record.as_ref().map_or(0, |row| row.version);
    let choice = record.as_ref().and_then(|row| row.choice.clone());
    let status = match &choice {
        None => AssociationStatus::Unassigned,
        Some(choice) => match workspace(conn, &choice.source_workspace_id)? {
            None => AssociationStatus::Missing,
            Some((current, workspace)) => {
                if source_ready
                    && belongs(&workspace, &entry)
                    && current.revision == choice.revision
                    && record.as_ref().is_some_and(|row| {
                        row.source_id == entry.installation_id && row.provider == entry.provider
                    })
                {
                    AssociationStatus::Assigned
                } else {
                    AssociationStatus::Stale
                }
            }
        },
    };
    Ok(Association {
        import_id: entry.import_id,
        version,
        source_id: entry.installation_id,
        provider: entry.provider,
        source_ready,
        choice,
        status,
    })
}

pub fn get(import_id: &str) -> Result<Association, ImportError> {
    let conn = catalog::open_catalog_readonly()?;
    let tx = conn.unchecked_transaction().map_err(db_error)?;
    describe(&tx, entry(&tx, import_id)?, storage::load(&tx, import_id)?)
}

pub fn update(request: UpdateRequest) -> Result<Association, ImportError> {
    if uuid::Uuid::parse_str(&request.request_id).is_err()
        || request.expected_version >= 9_007_199_254_740_991
    {
        return Err(conflict("invalid association request identity or version"));
    }
    if let Some(choice) = &request.choice {
        choice.validate()?;
    }
    // Validate the existing catalog first. Invalid requests never create a new catalog.
    get(&request.import_id)?;
    let mut conn = catalog::open_catalog()?;
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(db_error)?;
    let entry = entry(&tx, &request.import_id)?;
    let previous = storage::load(&tx, &request.import_id)?;
    if let Some(row) = &previous {
        if row.request_id == request.request_id {
            if row.choice != request.choice || row.expected_version != request.expected_version {
                return Err(stale());
            }
            return describe(&tx, entry, previous);
        }
    }
    if previous.as_ref().map_or(0, |row| row.version) != request.expected_version {
        return Err(stale());
    }
    let choice = if let Some(choice) = request.choice {
        if !source_ready(&tx, &entry)? {
            return Err(conflict(
                "confirm the imported source identity before assigning a workspace",
            ));
        }
        let (current, workspace) = workspace(&tx, &choice.source_workspace_id)?
            .ok_or_else(|| conflict("selected workspace is missing"))?;
        if !belongs(&workspace, &entry) {
            return Err(conflict("workspace belongs to another source"));
        }
        if current != choice {
            return Err(stale());
        }
        Some(current)
    } else {
        None
    };
    let record = Record {
        schema_version: 1,
        import_id: request.import_id,
        version: request.expected_version + 1,
        source_id: entry.installation_id.clone(),
        provider: entry.provider.clone(),
        choice,
        request_id: request.request_id,
        expected_version: request.expected_version,
    };
    storage::save(&tx, &record)?;
    let result = describe(&tx, entry, Some(record))?;
    tx.commit().map_err(db_error)?;
    Ok(result)
}

pub fn eligible(
    import_id: &str,
    cursor: Option<&str>,
    limit: Option<usize>,
) -> Result<WorkspacePage, ImportError> {
    use super::cursor::{decode_bound_cursor, encode_bound_cursor, CursorScope};
    let conn = catalog::open_catalog_readonly()?;
    let tx = conn.unchecked_transaction().map_err(db_error)?;
    let entry = entry(&tx, import_id)?;
    let scope = CursorScope {
        owner: import_id,
        snapshot: &entry.installation_id,
        leaf: &entry.provider,
        filter: "eligible-workspaces-v1",
    };
    let after = cursor
        .map(|cursor| decode_bound_cursor(cursor, scope))
        .transpose()?
        .and_then(|cursor| cursor.mark)
        .unwrap_or_default();
    if !source_ready(&tx, &entry)? {
        return Ok(WorkspacePage {
            items: Vec::new(),
            next_cursor: None,
        });
    }
    let limit = super::budget::page_limit(limit);
    let mut stmt = tx
        .prepare(
            "SELECT source_workspace_id FROM workspaces
        WHERE source_workspace_id>?1 AND json_extract(json,'$.sourceIdentity.sourceId')=?2
          AND json_extract(json,'$.sourceIdentity.provider')=?3
        ORDER BY source_workspace_id LIMIT ?4",
        )
        .map_err(db_error)?;
    let ids = stmt
        .query_map(
            params![after, entry.installation_id, entry.provider, limit + 1],
            |row| row.get::<_, String>(0),
        )
        .map_err(db_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(db_error)?;
    let more = ids.len() > limit;
    let mut items = Vec::new();
    for id in ids.into_iter().take(limit) {
        let (choice, workspace) = workspace(&tx, &id)?
            .ok_or_else(|| db_error("workspace disappeared in read snapshot"))?;
        if !belongs(&workspace, &entry) {
            return Err(db_error("workspace source identity mismatch"));
        }
        items.push(choice);
    }
    let next_cursor = if more {
        Some(encode_bound_cursor(
            scope,
            0,
            items.last().map(|item| item.source_workspace_id.clone()),
        )?)
    } else {
        None
    };
    Ok(WorkspacePage { items, next_cursor })
}

#[cfg(test)]
#[path = "workspace_association_tests.rs"]
mod tests;
