//! Native creation recovery/removal from a verified V3 allocation. This API
//! performs no key loading and never upgrades a legacy/reused project's owner.
use super::*;
use crate::session_migration::ImportProjectCreation;

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Disposition {
    Created,
    Recovered,
    Matched,
}

pub(crate) struct PublicationProject {
    pub project: Project,
    pub disposition: Disposition,
}

#[derive(Debug)]
pub(crate) enum EnsureError {
    Target(String),
    Provenance(crate::session_migration::errors::ImportError),
    Store(String),
}

/// Origin is sealed only inside the native lock, for a new row, and saved in
/// the same atomic replacement. A crash cannot persist one without the other.
pub(crate) fn ensure(
    creation: &ImportProjectCreation<'_>,
    path: &str,
) -> Result<PublicationProject, EnsureError> {
    creation
        .require_path(path)
        .map_err(EnsureError::Provenance)?;
    let mut failure = None;
    let result = projects_io::update_snapshot(|snapshot| {
        let result = resolve_import_project(&snapshot.projects, creation.project_id(), path)
            .inspect_err(|message| {
                failure = Some(EnsureError::Target(message.clone()));
            })?;
        let disposition = if result.created {
            snapshot.push_import_project(result.project.clone(), creation)?;
            Disposition::Created
        } else if result.project.id == creation.project_id() {
            let owned = match snapshot.import_origin(&result.project.id)? {
                Some(origin) => creation.owns(origin, &result.project).map_err(|error| {
                    let message = error.code.as_str().to_string();
                    failure = Some(EnsureError::Provenance(error));
                    message
                })?,
                None => false,
            };
            if owned {
                Disposition::Recovered
            } else {
                Disposition::Matched
            }
        } else {
            Disposition::Matched
        };
        Ok(PublicationProject {
            project: result.project,
            disposition,
        })
    });
    result
        .map(|(result, _)| result)
        .map_err(|message| failure.unwrap_or(EnsureError::Store(message)))
}

/// The caller must first verify acceptance and the complete committed outcome
/// in its catalog transaction. This exact native recheck happens after COMMIT,
/// with reference writers serialized. It never deletes a project directory.
pub(crate) fn remove(
    creation: &ImportProjectCreation<'_>,
    expected: &Project,
) -> Result<(), String> {
    let sessions_path = crate::paths::sessions_index_file();
    {
        let _locks = crate::store_lock::lock_exclusive_many(&[
            crate::paths::projects_file(),
            sessions_path,
            crate::paths::workspaces_file(),
        ])?;
        let mut snapshot = projects_io::Snapshot::read()?;
        let Some(current) = snapshot.projects.iter().find(|row| row.id == expected.id) else {
            return Ok(());
        };
        if serde_json::to_value(current).map_err(|error| error.to_string())?
            != serde_json::to_value(expected).map_err(|error| error.to_string())?
        {
            return Err("project changed during undo".into());
        }
        let origin = snapshot
            .import_origin(&current.id)?
            .ok_or("project has no creation provenance; kept")?;
        if !creation
            .owns(origin, current)
            .map_err(|error| error.code.as_str().to_string())?
        {
            return Err("project does not belong to this import allocation; kept".into());
        }
        // Only this successfully verified exact field is exempted. All other
        // unknown/new-version customization still prevents removal.
        if snapshot.has_unknown_fields_except_origin(&current.id) {
            return Err("project has additional settings; cannot undo".into());
        }
        if crate::workspace_store::list_workspaces()?
            .iter()
            .any(|workspace| workspace.primary_project_id == current.id)
        {
            return Err("project still has workspaces".into());
        }
        if super::super::load_sessions_snapshot()?
            .iter()
            .any(|row| row.project_id.as_deref() == Some(current.id.as_str()))
        {
            return Err("project still has sessions".into());
        }
        snapshot.projects.retain(|row| row.id != expected.id);
        snapshot.save()?;
    }
    crate::path_scope::refresh_from_store();
    Ok(())
}
