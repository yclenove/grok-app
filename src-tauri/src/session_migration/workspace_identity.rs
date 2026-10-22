//! Explicit source installation identity. A locator is provenance, not a machine
//! identity or an execution grant. Legacy workspace/root IDs survive adoption.

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use super::{
    errors::{ErrorCode, ImportError},
    hash::sha256_bytes,
    sources,
    workspace::SourceWorkspace,
    workspace_import::{db_error, stored_json},
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceSourceIdentity {
    pub source_id: String,
    pub provider: String,
    pub locator: String,
}

fn conflict(message: &str) -> ImportError {
    ImportError::new(ErrorCode::MappingConflict, message)
}

fn identity(
    conn: &Connection,
    workspace: &SourceWorkspace,
    source_id: &str,
) -> Result<WorkspaceSourceIdentity, ImportError> {
    let source = sources::get(conn, source_id)?;
    let (kind, path) = match workspace.workspace_file.as_deref() {
        Some(path) => ("file", path),
        None => (
            "folder",
            workspace
                .roots
                .first()
                .map(|root| root.original_path.as_str())
                .ok_or_else(|| conflict("workspace source locator is missing"))?,
        ),
    };
    if path.is_empty() || path.len() > 8192 || path.contains('\0') {
        return Err(conflict("invalid workspace source locator"));
    }
    Ok(WorkspaceSourceIdentity {
        source_id: source.source_id,
        provider: source.provider.as_str().into(),
        locator: format!("{kind}:{path}"),
    })
}

/// At most two identities are materialized; malformed catalog JSON is an error.
/// The write caller holds an Immediate reservation across lookup and mutation.
fn find(
    conn: &Connection,
    identity: &WorkspaceSourceIdentity,
) -> Result<Option<String>, ImportError> {
    let mut statement = conn
        .prepare(
            "SELECT source_workspace_id FROM workspaces
         WHERE json_extract(json,'$.sourceIdentity.sourceId')=?1
           AND json_extract(json,'$.sourceIdentity.locator')=?2 LIMIT 2",
        )
        .map_err(db_error)?;
    let ids = statement
        .query_map(params![identity.source_id, identity.locator], |row| {
            row.get::<_, String>(0)
        })
        .map_err(db_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(db_error)?;
    if ids.len() > 1 {
        return Err(conflict(
            "multiple workspaces claim the same source identity",
        ));
    }
    Ok(ids.into_iter().next())
}

/// Preview only: never registers the provider or writes to the catalog.
pub fn scope(
    conn: &Connection,
    workspace: &mut SourceWorkspace,
    source_id: &str,
) -> Result<(), ImportError> {
    let identity = identity(conn, workspace, source_id)?;
    workspace.source_workspace_id = find(conn, &identity)?.unwrap_or_else(|| {
        format!(
            "ws:{}",
            sha256_bytes(
                format!("source-v1\0{}\0{}", identity.source_id, identity.locator).as_bytes()
            )
        )
    });
    workspace.source_identity = Some(identity);
    Ok(())
}

/// A one-record, user-confirmed upgrade. Do not rekey roots or existing references.
pub fn adopt(
    conn: &Connection,
    workspace: &mut SourceWorkspace,
    source_id: &str,
) -> Result<(), ImportError> {
    let next = identity(conn, workspace, source_id)?;
    if workspace
        .source_identity
        .as_ref()
        .is_some_and(|previous| previous != &next)
    {
        return Err(conflict("workspace already belongs to another source"));
    }
    workspace.source_identity = Some(next);
    Ok(())
}

pub fn validate_write(conn: &Connection, workspace: &SourceWorkspace) -> Result<(), ImportError> {
    if let Some(json) = stored_json(conn, &workspace.source_workspace_id)? {
        let previous: SourceWorkspace = serde_json::from_str(&json).map_err(db_error)?;
        if previous.source_workspace_id != workspace.source_workspace_id {
            return Err(conflict("stored workspace identity mismatch"));
        }
        if locator(&previous) != locator(workspace) {
            return Err(conflict(
                "use explicit relocation to change a workspace locator",
            ));
        }
        if previous.source_identity.is_some()
            && previous.source_identity != workspace.source_identity
        {
            return Err(conflict(
                "cannot change an assigned workspace source identity",
            ));
        }
    }
    let Some(assigned) = &workspace.source_identity else {
        if find_unassigned(conn, workspace)?.is_some_and(|id| id != workspace.source_workspace_id) {
            return Err(conflict(
                "this workspace locator is already saved under another ID",
            ));
        }
        return Ok(());
    };
    if identity(conn, workspace, &assigned.source_id)? != *assigned {
        return Err(conflict("workspace source provider or locator mismatch"));
    }
    if find(conn, assigned)?.is_some_and(|id| id != workspace.source_workspace_id) {
        return Err(conflict(
            "this source workspace is already saved under another ID",
        ));
    }
    Ok(())
}

fn locator(workspace: &SourceWorkspace) -> (bool, Option<&str>) {
    (
        workspace.workspace_file.is_some(),
        workspace
            .workspace_file
            .as_deref()
            .or_else(|| workspace.roots.first().map(|r| r.original_path.as_str())),
    )
}

fn find_unassigned(
    conn: &Connection,
    workspace: &SourceWorkspace,
) -> Result<Option<String>, ImportError> {
    let (file, path) = locator(workspace);
    let Some(path) = path else {
        return Ok(None);
    };
    let mut stmt = conn.prepare("SELECT source_workspace_id FROM workspaces
        WHERE json_extract(json,'$.sourceIdentity') IS NULL
        AND (json_extract(json,'$.workspaceFile') IS NOT NULL)=?1
        AND COALESCE(json_extract(json,'$.workspaceFile'),json_extract(json,'$.roots[0].originalPath'))=?2 LIMIT 2").map_err(db_error)?;
    let ids = stmt
        .query_map(params![file, path], |row| row.get::<_, String>(0))
        .map_err(db_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(db_error)?;
    if ids.len() > 1 {
        return Err(conflict(
            "multiple unassigned workspaces claim this locator",
        ));
    }
    Ok(ids.into_iter().next())
}

/// Reopening an unassigned relocated source resolves its stable ID. Never infer
/// an installation from a path shared by several registered sources.
pub(super) fn resolve_unassigned(
    conn: &Connection,
    workspace: &mut SourceWorkspace,
) -> Result<(), ImportError> {
    // Keep the legacy stale-preview error for an invalid record at the derived
    // ID, before the locator query asks SQLite to inspect other JSON rows.
    if let Some(json) = stored_json(conn, &workspace.source_workspace_id)? {
        serde_json::from_str::<SourceWorkspace>(&json).map_err(|error| {
            ImportError::new(
                ErrorCode::PlanStale,
                format!("saved workspace is invalid: {error}"),
            )
        })?;
    }
    if let Some(id) = find_unassigned(conn, workspace)? {
        workspace.source_workspace_id = id;
    }
    if let Some(json) = stored_json(conn, &workspace.source_workspace_id)? {
        let previous: SourceWorkspace = serde_json::from_str(&json).map_err(db_error)?;
        if previous.source_workspace_id != workspace.source_workspace_id
            || locator(&previous) != locator(workspace)
        {
            return Err(conflict(
                "saved workspace moved; reopen its current location",
            ));
        }
        workspace.source_identity = previous.source_identity;
    }
    Ok(())
}

/// Only the explicit relocation flow may change a locator. Installation,
/// provider and workspace identity stay fixed, including for offline sources.
pub(super) fn validate_relocation(
    conn: &Connection,
    before: &SourceWorkspace,
    after: &SourceWorkspace,
) -> Result<(), ImportError> {
    if before.source_workspace_id != after.source_workspace_id
        || before.workspace_file.is_some() != after.workspace_file.is_some()
    {
        return Err(conflict(
            "relocation cannot replace workspace identity or kind",
        ));
    }
    match (&before.source_identity, &after.source_identity) {
        (Some(old), Some(new))
            if old.source_id == new.source_id && old.provider == new.provider =>
        {
            if identity(conn, after, &new.source_id)? != *new {
                return Err(conflict("relocated source locator is inconsistent"));
            }
            if find(conn, new)?.is_some_and(|id| id != before.source_workspace_id) {
                return Err(conflict("destination already belongs to a saved workspace"));
            }
        }
        (None, None) => {}
        _ => return Err(conflict("relocation cannot change the source installation")),
    }
    // Unscoped legacy records still cannot silently take over each other.
    if after.source_identity.is_none()
        && find_unassigned(conn, after)?.is_some_and(|id| id != before.source_workspace_id)
    {
        return Err(conflict("destination already belongs to a saved workspace"));
    }
    Ok(())
}

#[cfg(test)]
#[path = "workspace_identity_tests.rs"]
mod tests;
