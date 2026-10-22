// Multi-root workspace IPC (#1194 MVP-0).
// Included into `commands` — do not re-import Arc/State/SessionManager.

use crate::workspace_store::{
    self, root_snapshot, WorkspaceCapability, WorkspaceRecord, WorkspaceRoot,
};

#[tauri::command]
pub fn workspaces_list() -> Result<Vec<WorkspaceRecord>, String> {
    workspace_store::list_workspaces()
}

#[tauri::command]
pub fn workspaces_for_project(project_id: String) -> Result<Vec<WorkspaceRecord>, String> {
    workspace_store::workspaces_for_project(&project_id)
}

#[tauri::command]
pub fn workspace_get(id: String) -> Result<Option<WorkspaceRecord>, String> {
    workspace_store::get_workspace(&id)
}

#[tauri::command]
pub fn workspace_upsert(
    id: Option<String>,
    name: String,
    primary_project_id: String,
    roots: Vec<WorkspaceRoot>,
) -> Result<WorkspaceRecord, String> {
    workspace_store::upsert_workspace(id, name, primary_project_id, roots)
}

#[tauri::command]
pub fn workspace_delete(id: String) -> Result<(), String> {
    workspace_store::delete_workspace(&id)
}

#[tauri::command]
pub fn workspace_validate_root(path: String) -> Result<WorkspaceRoot, String> {
    let (canon, path_ok) = workspace_store::canonicalize_workspace_root(&path)?;
    Ok(WorkspaceRoot {
        path: canon,
        role: workspace_store::WorkspaceRootRole::Extra,
        access: workspace_store::WorkspaceRootAccess::Read,
        path_ok: Some(path_ok),
    })
}

/// Refresh capability plans for all workspaces (Doctor / settings).
#[tauri::command]
pub fn workspaces_diagnose() -> Result<Vec<WorkspaceRecord>, String> {
    workspace_store::refresh_all_capabilities()
}

/// Bind (or clear) a workspace on a session. Grants extra roots into Host
/// path_scope for App file APIs; does not change CLI OS sandbox (MVP-0).
#[tauri::command]
pub async fn session_set_workspace(
    app: tauri::AppHandle,
    mgr: State<'_, Arc<SessionManager>>,
    id: String,
    workspace_id: Option<String>,
) -> Result<SessionMeta, String> {
    let meta = bind_session_workspace(&id, workspace_id)?;

    // Roots change → next turn should not resume a process started without them.
    mgr.invalidate_spawn_flags_for_session(&app, &meta.id, "session_workspace")
        .await;
    Ok(meta)
}

#[tauri::command]
pub async fn session_workspace_runtime_preview(
    id: String,
) -> Result<crate::session_manager::WorkspaceRuntimeReview, String> {
    tokio::task::spawn_blocking(move || crate::session_manager::preview_workspace_runtime(&id))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn session_workspace_runtime_confirm(
    app: tauri::AppHandle,
    mgr: State<'_, Arc<SessionManager>>,
    id: String,
    request_hash: String,
) -> Result<(), String> {
    mgr.confirm_workspace_runtime(&app, &id, &request_hash)
        .await
}

pub(crate) fn bind_session_workspace(
    id: &str,
    workspace_id: Option<String>,
) -> Result<SessionMeta, String> {
    crate::session_migration::reject_imported_history_id(Some(id))?;
    let wid = workspace_id
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty() && s != workspace_store::UNBOUND_WORKSPACE_ID);

    let mut extra_roots = Vec::new();
    let (stored_id, snapshot, capability) = if let Some(ref wid) = wid {
        let ws = workspace_store::get_workspace(wid)?
            .ok_or_else(|| "workspace not found".to_string())?;
        for root in &ws.roots {
            if root.role == workspace_store::WorkspaceRootRole::Extra {
                extra_roots.push(root.path.clone());
            }
        }
        (
            Some(wid.clone()),
            Some(root_snapshot(&ws.roots)),
            Some(match ws.capability {
                WorkspaceCapability::None => "none".into(),
                WorkspaceCapability::ContextOnly => "context_only".into(),
                WorkspaceCapability::EnforcedRead => "enforced_read".into(),
                WorkspaceCapability::ExtraWriteActive => "extra_write_active".into(),
                WorkspaceCapability::Blocked => "blocked".into(),
            }),
        )
    } else {
        // Explicit detach: persist a sentinel so connect does not re-inherit
        // the project's default workspace (#1233).
        (
            Some(workspace_store::UNBOUND_WORKSPACE_ID.to_string()),
            None,
            None,
        )
    };

    let mut meta = store::set_session_workspace(id, stored_id, snapshot, capability)?;
    // A rejected or failed binding must not widen the process-wide file scope.
    for root in extra_roots {
        crate::path_scope::grant_path(std::path::Path::new(&root));
    }
    // The detach sentinel is an internal persistence detail that prevents a
    // project default from being inherited on the next connect.  Keep the
    // command contract as an ordinary unbound workspace for callers/UI.
    if meta.workspace_id.as_deref() == Some(workspace_store::UNBOUND_WORKSPACE_ID) {
        meta.workspace_id = None;
    }
    Ok(meta)
}
