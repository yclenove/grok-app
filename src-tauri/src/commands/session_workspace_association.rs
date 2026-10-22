// Workspace membership for imported history; independent of native execution bindings.

#[tauri::command]
pub async fn session_import_workspace_association_get(
    import_id: String,
) -> Result<crate::session_migration::workspace_association::Association, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::session_migration::workspace_association::get(&import_id).map_err(map_err)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn session_import_workspace_association_update(
    request: crate::session_migration::workspace_association::UpdateRequest,
) -> Result<crate::session_migration::workspace_association::Association, String> {
    let result = tauri::async_runtime::spawn_blocking(move || {
        crate::session_migration::workspace_association::update(request).map_err(map_err)
    })
    .await
    .map_err(|error| error.to_string())??;
    crate::session_migration::job_control::workspace_changed();
    Ok(result)
}

#[tauri::command]
pub async fn session_import_workspace_groups(
    cursor: Option<String>,
    limit: Option<usize>,
    selected_id: Option<String>,
) -> Result<crate::session_migration::workspace_catalog::WorkspaceGroupsPage, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::session_migration::workspace_catalog::groups(
            cursor.as_deref(),
            limit,
            selected_id.as_deref(),
        )
        .map_err(map_err)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn session_import_workspace_association_options(
    import_id: String,
    cursor: Option<String>,
    limit: Option<usize>,
) -> Result<crate::session_migration::workspace_association::WorkspacePage, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::session_migration::workspace_association::eligible(
            &import_id,
            cursor.as_deref(),
            limit,
        )
        .map_err(map_err)
    })
    .await
    .map_err(|error| error.to_string())?
}
