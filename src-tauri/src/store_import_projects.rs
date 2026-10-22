//! Explicit import project creation: stable request identity, no trust upgrade,
//! no migrations, and no changes to reused rows.
use super::{projects_io, Project};
use serde::Serialize;
use std::path::Path;

#[path = "store_import_publication_projects.rs"]
pub(crate) mod publication;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnsuredProject {
    pub project: Project,
    pub created: bool,
}

fn local_path(raw: &str) -> bool {
    Path::new(raw).is_absolute()
        && raw.len() <= 8192
        && !raw.chars().any(char::is_control)
        && !raw.starts_with("//")
        && !(raw.starts_with(r"\\") && !raw.starts_with(r"\\?\"))
        && !raw
            .get(..8)
            .is_some_and(|head| head.eq_ignore_ascii_case(r"\\?\UNC\"))
}

pub(crate) fn ensure_import_project(id: &str, path: &str) -> Result<EnsuredProject, String> {
    projects_io::update(|projects| {
        let ensured = resolve_import_project(projects, id, path)?;
        if ensured.created {
            projects.push(ensured.project.clone());
        }
        Ok(ensured)
    })
    .map(|(result, _)| result)
}

/// Pure selection under the caller's project lock. The caller adds a genuinely
/// new row, optionally with its atomic creation provenance, after this returns.
fn resolve_import_project(
    projects: &[Project],
    id: &str,
    path: &str,
) -> Result<EnsuredProject, String> {
    if id.is_empty() || id.len() > 256 || id == super::GENERAL_PROJECT_ID || !local_path(path) {
        return Err("invalid import project identity or local target path".into());
    }
    if let Some(existing) = projects.iter().find(|row| row.id == id) {
        if existing.is_ssh_remote() || existing.is_legacy_general() || existing.path != path {
            return Err("project request identity is already bound to a different target".into());
        }
        return Ok(EnsuredProject {
            project: existing.clone(),
            created: false,
        });
    }
    if let Some(existing) = projects
        .iter()
        .find(|row| !row.is_ssh_remote() && !row.is_legacy_general() && row.path == path)
    {
        return Ok(EnsuredProject {
            project: existing.clone(),
            created: false,
        });
    }
    let (canonical, available) = crate::workspace_store::canonicalize_workspace_root(path)?;
    if !available {
        return Err("target project directory is unavailable".into());
    }
    // Match other local spellings without changing their saved path or any
    // user settings. Remote/UNC records are never probed by this action.
    if let Some(existing) = projects.iter().find(|row| {
        !row.is_ssh_remote()
            && !row.is_legacy_general()
            && local_path(&row.path)
            && crate::workspace_store::canonicalize_workspace_root(&row.path)
                .is_ok_and(|(resolved, exists)| exists && resolved == canonical)
    }) {
        return Ok(EnsuredProject {
            project: existing.clone(),
            created: false,
        });
    }
    let project = Project {
        id: id.into(),
        name: Path::new(path)
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| path.into()),
        path: path.into(),
        trusted: false,
        last_opened_at: chrono::Utc::now(),
        path_ok: true,
        pinned: false,
        system: false,
        model_id: None,
        effort: None,
        mode: None,
        permission_policy: None,
        sandbox_profile: None,
        color: None,
        ssh_alias: None,
    };
    Ok(EnsuredProject {
        project,
        created: true,
    })
}

/// Remove only the exact row inspected by undo. Related file locks serialize
/// the session-reference check with native session writers across processes.
/// Historical store tests only; production Undo requires creation provenance.
#[cfg(test)]
pub(crate) fn remove_import_project(expected: &Project) -> Result<(), String> {
    {
        let sessions_path = crate::paths::sessions_index_file();
        let _locks = crate::store_lock::lock_exclusive_many(&[
            crate::paths::projects_file(),
            sessions_path.clone(),
            crate::paths::workspaces_file(),
        ])?;
        let mut snapshot = projects_io::Snapshot::read()?;
        let Some(current) = snapshot.projects.iter().find(|row| row.id == expected.id) else {
            return Ok(());
        };
        if serde_json::to_value(current).map_err(|e| e.to_string())?
            != serde_json::to_value(expected).map_err(|e| e.to_string())?
        {
            return Err("project changed during undo".into());
        }
        // Imported projects start with known fields only. An older App cannot
        // safely undo a row customized by a newer version it cannot interpret.
        if snapshot.has_unknown_fields(&expected.id) {
            return Err("project has additional settings; cannot undo".into());
        }
        if crate::workspace_store::list_workspaces()?
            .iter()
            .any(|workspace| workspace.primary_project_id == expected.id)
        {
            return Err("project still has workspaces".into());
        }
        if super::load_sessions_snapshot()?
            .iter()
            .any(|row| row.project_id.as_deref() == Some(&expected.id))
        {
            return Err("project still has sessions".into());
        }
        snapshot.projects.retain(|row| row.id != expected.id);
        snapshot.save()?;
    }
    crate::path_scope::refresh_from_store();
    Ok(())
}
