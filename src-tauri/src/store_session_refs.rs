//! Validate new bindings under the projects/workspaces/sessions writer locks.
//! Existing orphaned legacy rows can still be renamed, archived, or detached.
use std::collections::HashMap;

use super::SessionMeta;

#[derive(PartialEq, Eq)]
pub(super) struct Binding {
    project: Option<String>,
    workspace: Option<String>,
    roots: Option<String>,
    capability: Option<String>,
}

impl From<&SessionMeta> for Binding {
    fn from(row: &SessionMeta) -> Self {
        Self {
            project: row.project_id.clone(),
            workspace: row.workspace_id.clone(),
            roots: row.workspace_root_snapshot.clone(),
            capability: row.workspace_capability.clone(),
        }
    }
}

pub(super) fn snapshot(rows: &[SessionMeta]) -> HashMap<String, Binding> {
    rows.iter()
        .map(|row| (row.id.clone(), Binding::from(row)))
        .collect()
}

pub(super) fn validate(
    before: &HashMap<String, Binding>,
    rows: &[SessionMeta],
) -> Result<(), String> {
    let mut projects: Option<Vec<super::Project>> = None;
    let mut workspaces: Option<Vec<crate::workspace_store::WorkspaceRecord>> = None;
    for row in rows {
        let next = Binding::from(row);
        let previous = before.get(&row.id);
        if previous == Some(&next) {
            continue;
        }
        if let Some(project_id) = row.project_id.as_deref() {
            if previous.and_then(|binding| binding.project.as_deref()) != Some(project_id)
                || crate::workspace_store::is_bound_workspace_id(row.workspace_id.as_deref())
            {
                let projects = match &projects {
                    Some(projects) => projects,
                    None => projects.insert(super::load_projects_snapshot()?),
                };
                if !projects
                    .iter()
                    .any(|project| project.id == project_id && !project.is_legacy_general())
                {
                    return Err(format!("project not found: {project_id}"));
                }
            }
        }
        if let Some(workspace_id) = row
            .workspace_id
            .as_deref()
            .filter(|id| crate::workspace_store::is_bound_workspace_id(Some(id)))
        {
            let workspaces = match &workspaces {
                Some(workspaces) => workspaces,
                None => workspaces.insert(crate::workspace_store::list_workspaces()?),
            };
            let workspace = workspaces
                .iter()
                .find(|workspace| workspace.id == workspace_id)
                .ok_or("workspace not found")?;
            crate::workspace_store::validate_session_workspace(row, workspace)?;
        } else if row.workspace_root_snapshot.is_some() || row.workspace_capability.is_some() {
            return Err("workspace metadata without a workspace binding".into());
        }
    }
    Ok(())
}

pub(super) fn record_removals(
    before: &HashMap<String, Binding>,
    rows: &[SessionMeta],
) -> Result<(), String> {
    let remaining: std::collections::HashSet<_> = rows.iter().map(|row| &row.id).collect();
    for id in before.keys().filter(|id| !remaining.contains(id)) {
        crate::session_migration::record_published_before_removal(id)?;
    }
    Ok(())
}
