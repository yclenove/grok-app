// Group queries may wait for SQLite locks; keep them off the IPC/UI thread.
#[tauri::command]
pub async fn session_import_scan_group_member(
    scan_id: String,
    epoch: u64,
    group_id: String,
    candidate_id: String,
) -> Result<crate::session_migration::scan_groups::SourceGroupMember, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::session_migration::scan_groups::member(&scan_id, epoch, &group_id, &candidate_id)
            .map_err(|e| e.into_command_err())
    }).await.map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn session_import_scan_groups(
    scan_id: String,
    epoch: u64,
    cursor: Option<String>,
    limit: Option<usize>,
    filter: Option<crate::session_migration::scan_groups::SourceGroupFilter>,
) -> Result<crate::session_migration::scan_groups::SourceGroupPage, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::session_migration::scan_groups::page(&scan_id, epoch, cursor.as_deref(), limit, &filter.unwrap_or_default()).map_err(|e| e.into_command_err())
    }).await.map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn session_import_scan_group_members(
    scan_id: String,
    epoch: u64,
    group_id: String,
    cursor: Option<String>,
    limit: Option<usize>,
) -> Result<crate::session_migration::scan_groups::SourceGroupMemberPage, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::session_migration::scan_groups::members(&scan_id, epoch, &group_id, cursor.as_deref(), limit).map_err(|e| e.into_command_err())
    }).await.map_err(|e| e.to_string())?
}
