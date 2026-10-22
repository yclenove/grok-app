//! Link imported history to Grok project records via receipts.
//! Existing project name/path/pin/trust/space are never overwritten.

use crate::session_migration::catalog;
#[cfg(test)]
use crate::session_migration::catalog::get_entry;
use crate::session_migration::errors::{ErrorCode, ImportError};
use crate::session_migration::jobs::{CommitItem, Receipt};
use crate::session_migration::workspace::WorkspaceMapping;
use crate::store::{self, Project};

/// Freeze mapping identity on the receipt. Returns true when the receipt
/// gained a newly allocated project id and must be persisted before store I/O.
pub fn prepare_link(item: &CommitItem, rec: &mut Receipt) -> bool {
    if rec.mapping_project_id.is_none() {
        rec.mapping_project_id = item.mapping_project_id.clone();
    }
    if item.create_project && rec.mapping_project_id.is_none() {
        rec.mapping_project_id = Some(uuid::Uuid::new_v4().to_string());
        rec.created_project = true;
        rec.project_decision = Some("create".into());
        return true;
    }
    if rec.project_decision.is_none() {
        rec.project_decision = Some(
            if item.create_project {
                "create"
            } else if rec.mapping_project_id.is_some() {
                "match"
            } else {
                "unassigned"
            }
            .into(),
        );
    }
    if item.create_project {
        rec.created_project = true;
    }
    false
}

/// Create or reuse a project record. Must not run inside a catalog transaction.
pub fn apply_store_link(item: &CommitItem, rec: &mut Receipt) -> Result<(), ImportError> {
    if rec.project_decision.as_deref() == Some("unassigned") && !item.create_project {
        rec.mapping_project_id = None;
        rec.created_project = false;
        return Ok(());
    }
    if item.create_project {
        let path = item
            .explicit_target
            .as_deref()
            .filter(|s| !s.is_empty())
            .unwrap_or(item.root_path.as_str());
        let planned = rec.mapping_project_id.clone().ok_or_else(|| {
            ImportError::new(
                ErrorCode::SnapshotUnavailable,
                "create link has no project id",
            )
        })?;
        match store::ensure_import_project(&planned, path)
            .map_err(|error| ImportError::new(ErrorCode::MappingConflict, error))
        {
            Ok(ensured) => {
                rec.mapping_project_id = Some(ensured.project.id.clone());
                if ensured.created {
                    rec.created_project = true;
                    rec.project_decision = Some("create".into());
                } else if ensured.project.id != planned {
                    rec.created_project = false;
                    rec.project_decision = Some("match".into());
                }
                snapshot_project(rec, &ensured.project);
                return Ok(());
            }
            Err(_) if item.import_unassigned_anyway => {
                rec.mapping_project_id = None;
                rec.created_project = false;
                rec.project_decision = Some("unassigned-reported".into());
                return Ok(());
            }
            Err(err) => return Err(err),
        }
    }
    let Some(id) = rec.mapping_project_id.clone() else {
        rec.project_decision = Some("unassigned".into());
        rec.created_project = false;
        return Ok(());
    };
    if let Some(project) = store::load_projects_snapshot()
        .map_err(|error| ImportError::new(ErrorCode::SnapshotUnavailable, error))?
        .into_iter()
        .find(|p| p.id == id)
    {
        rec.created_project = false;
        rec.project_decision = Some("match".into());
        snapshot_project(rec, &project);
        Ok(())
    } else if item.import_unassigned_anyway {
        rec.mapping_project_id = None;
        rec.created_project = false;
        rec.project_decision = Some("unassigned-reported".into());
        Ok(())
    } else {
        rec.created_project = false;
        rec.project_decision = Some("pending".into());
        Ok(())
    }
}

pub fn remember_entry_mapping(
    conn: &rusqlite::Connection,
    rec: &Receipt,
) -> Result<(), ImportError> {
    let Some(import_id) = rec.import_id.as_deref() else {
        return Ok(());
    };
    let mapping = WorkspaceMapping {
        source: None,
        root_id: format!("import:{import_id}"),
        target_project_id: rec.mapping_project_id.clone(),
        explicit_target: rec.project_path_at_link.clone(),
        unassigned: rec.mapping_project_id.is_none(),
        user_choice_version: 1,
        primary: true,
        suggested_target: None,
        create_project: rec.created_project,
        import_unassigned_anyway: rec.project_decision.as_deref() == Some("unassigned-reported"),
    };
    catalog::upsert_mapping(conn, &mapping)
}

/// After an unassigned import, attach a project without republishing content.
#[cfg(test)]
pub fn apply_late_mapping(
    conn: &rusqlite::Connection,
    import_id: &str,
    mapping: &WorkspaceMapping,
) -> Result<WorkspaceMapping, ImportError> {
    get_entry(conn, import_id)?
        .ok_or_else(|| ImportError::new(ErrorCode::SourceNotFound, "import not found"))?;
    match mapping_for_import(conn, import_id)? {
        Some(current)
            if !current.unassigned
                && current.target_project_id.is_some()
                && current.target_project_id != mapping.target_project_id =>
        {
            return Err(ImportError::new(
                ErrorCode::MappingConflict,
                "import already linked to a different project",
            ));
        }
        _ => {}
    }
    let mut stored = mapping.clone();
    stored.root_id = format!("import:{import_id}");
    catalog::upsert_mapping(conn, &stored)?;
    Ok(stored)
}

#[cfg(test)]
pub fn mapping_for_import(
    conn: &rusqlite::Connection,
    import_id: &str,
) -> Result<Option<WorkspaceMapping>, ImportError> {
    let key = format!("import:{import_id}");
    Ok(catalog::list_mappings(conn)?
        .into_iter()
        .find(|m| m.root_id == key))
}

/// Remove a project record this job created. Never deletes the directory.
pub fn undo_created_project(
    conn: &rusqlite::Connection,
    rec: &Receipt,
    creation: &crate::session_migration::ImportProjectCreation<'_>,
) -> Result<(), ImportError> {
    undo_created_project_checked(conn, rec, creation, |_| Ok(()))
}

pub(super) fn undo_created_project_checked(
    conn: &rusqlite::Connection,
    rec: &Receipt,
    creation: &crate::session_migration::ImportProjectCreation<'_>,
    check: impl FnOnce(&rusqlite::Transaction<'_>) -> Result<(), ImportError>,
) -> Result<(), ImportError> {
    if !rec.created_project {
        return Ok(());
    }
    let Some(id) = rec.mapping_project_id.as_deref() else {
        return Ok(());
    };
    let tx =
        rusqlite::Transaction::new_unchecked(conn, rusqlite::TransactionBehavior::Immediate)
            .map_err(|error| ImportError::new(ErrorCode::SnapshotUnavailable, error.to_string()))?;
    check(&tx)?;
    super::project_references::ensure_unreferenced(&tx, id)?;
    let Some(project) = store::load_projects_snapshot()
        .map_err(|error| ImportError::new(ErrorCode::SnapshotUnavailable, error))?
        .into_iter()
        .find(|p| p.id == id)
    else {
        return Ok(());
    };
    let renamed = rec
        .project_name_at_link
        .as_deref()
        .is_some_and(|n| n != project.name);
    let moved = rec
        .project_path_at_link
        .as_deref()
        .is_some_and(|path| path != project.path);
    let pin_changed = rec
        .project_pinned_at_link
        .is_some_and(|v| v != project.pinned);
    let trust_changed = rec
        .project_trusted_at_link
        .is_some_and(|v| v != project.trusted);
    let customized = project.model_id.is_some()
        || project.effort.is_some()
        || project.mode.is_some()
        || project.permission_policy.is_some()
        || project.sandbox_profile.is_some()
        || project.color.is_some()
        || project.ssh_alias.is_some();
    if renamed || moved || pin_changed || trust_changed || customized {
        return Err(ImportError::new(
            ErrorCode::MappingConflict,
            "project was modified by the user",
        ));
    }
    store::import_publication_projects::remove(creation, &project)
        .map_err(|e| ImportError::new(ErrorCode::MappingConflict, e))?;
    Ok(())
}

fn snapshot_project(rec: &mut Receipt, project: &Project) {
    if rec.project_name_at_link.is_none() {
        rec.project_name_at_link = Some(project.name.clone());
    }
    if rec.project_path_at_link.is_none() {
        rec.project_path_at_link = Some(project.path.clone());
    }
    if rec.project_pinned_at_link.is_none() {
        rec.project_pinned_at_link = Some(project.pinned);
    }
    if rec.project_trusted_at_link.is_none() {
        rec.project_trusted_at_link = Some(project.trusted);
    }
}
