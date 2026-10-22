//! Catalog writers take SQLite's write reservation before the JSON store lock.
//! Undo uses the same order so new references cannot slip past its final check.
use rusqlite::{Connection, Transaction, TransactionBehavior};

use super::{
    catalog,
    errors::{ErrorCode, ImportError},
    workspace::{SourceWorkspace, WorkspaceMapping},
};

fn error(message: impl ToString) -> ImportError {
    ImportError::new(ErrorCode::MappingConflict, message.to_string())
}

/// Existing transactions must already hold a write reservation (Immediate).
/// Keep the project lock until an owned transaction commits, not just until SQL
/// execute returns. No project-store callback may acquire a catalog transaction.
pub(super) fn with_targets<T>(
    conn: &Connection,
    targets: &[&str],
    write: impl FnOnce(&Connection) -> Result<T, ImportError>,
) -> Result<T, ImportError> {
    if targets.is_empty() {
        return write(conn);
    }
    let owned = conn
        .is_autocommit()
        .then(|| Transaction::new_unchecked(conn, TransactionBehavior::Immediate).map_err(error))
        .transpose()?;
    let db = owned.as_deref().unwrap_or(conn);
    let _lock = crate::store_lock::lock_exclusive(&crate::paths::projects_file()).map_err(error)?;
    let projects = crate::store::load_projects_snapshot().map_err(error)?;
    if targets.iter().any(|id| {
        !projects
            .iter()
            .any(|p| p.id == *id && !p.is_legacy_general())
    }) {
        return Err(error("selected project no longer exists"));
    }
    let result = write(db)?;
    if let Some(tx) = owned {
        tx.commit().map_err(error)?;
    }
    Ok(result)
}

/// Caller holds an Immediate catalog transaction until native removal ends.
/// Stream individual rows with a byte limit; corrupt metadata cannot prove a
/// project is unused. Tombstoned histories may be imported again with new maps.
pub(super) fn ensure_unreferenced(conn: &Connection, project_id: &str) -> Result<(), ImportError> {
    let mut statement = conn
        .prepare("SELECT source_workspace_id, json FROM workspaces")
        .map_err(error)?;
    let mut rows = statement.query([]).map_err(error)?;
    while let Some(row) = rows.next().map_err(error)? {
        let id: String = row.get(0).map_err(error)?;
        let json = bounded_json(row)?;
        let workspace: SourceWorkspace = serde_json::from_str(json).map_err(error)?;
        if workspace.source_workspace_id != id {
            return Err(error("workspace identity mismatch"));
        }
        if workspace
            .mapping_choices
            .iter()
            .any(|choice| choice.target_project_id.as_deref() == Some(project_id))
        {
            return Err(error("project is used by a saved imported workspace"));
        }
    }
    let mut statement = conn
        .prepare("SELECT root_id, json FROM mappings")
        .map_err(error)?;
    let mut rows = statement.query([]).map_err(error)?;
    while let Some(row) = rows.next().map_err(error)? {
        let id: String = row.get(0).map_err(error)?;
        let mapping: WorkspaceMapping = serde_json::from_str(bounded_json(row)?).map_err(error)?;
        if mapping.root_id != id {
            return Err(error("mapping identity mismatch"));
        }
        if mapping.target_project_id.as_deref() != Some(project_id) {
            continue;
        }
        if let Some(import_id) = id.strip_prefix("import:") {
            if let Some(entry) = catalog::get_entry(conn, import_id)? {
                if entry.hidden
                    && catalog::is_tombstoned(
                        conn,
                        &entry.installation_id,
                        &entry.provider,
                        &entry.native_session_id,
                    )?
                {
                    continue;
                }
            }
        }
        return Err(error("project is used by an imported history mapping"));
    }
    Ok(())
}

fn bounded_json<'a>(row: &'a rusqlite::Row<'_>) -> Result<&'a str, ImportError> {
    let value = row.get_ref(1).map_err(error)?.as_str().map_err(error)?;
    if value.len() > 16 * 1024 * 1024 {
        return Err(error("reference metadata exceeds 16 MiB"));
    }
    Ok(value)
}

#[cfg(test)]
#[path = "project_reference_tests.rs"]
mod tests;
