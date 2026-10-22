//! Bind/spawn checks use the exact reviewed roots, including Host path grants.
#[cfg(test)]
use super::is_bound_workspace_id;
use super::{
    canonicalize_workspace_root, get_workspace, local_paths::paths_equal, root_snapshot,
    validate_primary_location, WorkspaceRecord,
};
use crate::store::SessionMeta;

pub(crate) fn validate_session_workspace(
    meta: &SessionMeta,
    workspace: &WorkspaceRecord,
) -> Result<(), String> {
    if meta.project_id.as_deref() != Some(workspace.primary_project_id.as_str()) {
        return Err("workspace primary project does not match the session project".into());
    }
    if meta.workspace_root_snapshot.as_deref() != Some(root_snapshot(&workspace.roots).as_str()) {
        return Err("workspace roots changed; review the workspace again".into());
    }
    validate_primary_location(workspace)
}

#[derive(Clone)]
pub(crate) struct WorkspaceSpawnContext {
    pub sandbox: String,
    pub roots: Vec<String>,
}

#[cfg(test)]
pub(crate) fn resolve_session_spawn(
    global: &str,
    project_override: Option<&str>,
    session_data_mode: &str,
    meta: &SessionMeta,
) -> Result<WorkspaceSpawnContext, String> {
    let fallback = crate::store::resolve_sandbox_profile(global, project_override);
    let Some(id) = meta
        .workspace_id
        .as_deref()
        .map(str::trim)
        .filter(|id| is_bound_workspace_id(Some(id)))
    else {
        return Ok(WorkspaceSpawnContext {
            sandbox: fallback,
            roots: vec![],
        });
    };
    let _locks = crate::store_lock::lock_exclusive_many(&[
        crate::paths::projects_file(),
        crate::paths::workspaces_file(),
    ])?;
    resolve_bound_spawn(&fallback, session_data_mode, meta, id)
}

/// Caller holds the projects/workspaces store locks.
pub(crate) fn resolve_bound_spawn(
    fallback: &str,
    session_data_mode: &str,
    meta: &SessionMeta,
    id: &str,
) -> Result<WorkspaceSpawnContext, String> {
    let workspace =
        get_workspace(id)?.ok_or("bound workspace not found; choose or detach the workspace")?;
    validate_session_workspace(meta, &workspace)?;
    let mut roots = Vec::new();
    for root in &workspace.roots {
        let (canonical, available) = canonicalize_workspace_root(&root.path)?;
        if !available || !paths_equal(&root.path, &canonical) {
            return Err(
                "workspace folder changed or is unavailable; review the workspace again".into(),
            );
        }
        roots.push(canonical);
    }
    let plan = crate::workspace_sandbox::plan_for_session_mode(session_data_mode, &workspace)?;
    Ok(WorkspaceSpawnContext {
        sandbox: plan.spawn_sandbox.unwrap_or_else(|| fallback.to_string()),
        roots,
    })
}
