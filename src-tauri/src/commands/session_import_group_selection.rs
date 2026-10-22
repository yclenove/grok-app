// SQLite/snapshot work belongs on blocking workers, not the UI IPC thread.
use crate::session_migration::group_selection as fixed_groups;

async fn group_selection_call<T: Send + 'static>(
    operation: impl FnOnce() -> Result<T, crate::session_migration::errors::ImportError> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(operation)
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.into_command_err())
}

#[tauri::command]
pub async fn session_import_group_selection_get(scan_id: String, epoch: u64) -> Result<fixed_groups::GroupSelection, String> {
    group_selection_call(move || fixed_groups::get(&scan_id, epoch)).await
}

#[tauri::command]
pub async fn session_import_group_selection_freeze(scan_id: String, epoch: u64, selection_id: String, expected_state_version: u64) -> Result<fixed_groups::GroupSelection, String> {
    group_selection_call(move || fixed_groups::freeze_current(&scan_id, epoch, &selection_id, expected_state_version)).await
}

#[tauri::command]
pub async fn session_import_group_selection_clear(scan_id: String, epoch: u64, selection_id: String, expected_state_version: u64) -> Result<fixed_groups::GroupSelection, String> {
    group_selection_call(move || fixed_groups::clear(&scan_id, epoch, &selection_id, expected_state_version)).await
}

#[tauri::command]
pub async fn session_import_group_selection_set(scan_id: String, epoch: u64, selection_id: String, expected_state_version: u64, group_ids: Vec<String>, selected: bool) -> Result<fixed_groups::GroupSelection, String> {
    group_selection_call(move || fixed_groups::set_many(&scan_id, epoch, &selection_id, expected_state_version, &group_ids, selected)).await
}

#[tauri::command]
pub async fn session_import_group_selection_invert(scan_id: String, epoch: u64, selection_id: String, expected_state_version: u64, group_ids: Vec<String>) -> Result<fixed_groups::GroupSelection, String> {
    group_selection_call(move || fixed_groups::invert_many(&scan_id, epoch, &selection_id, expected_state_version, &group_ids)).await
}

#[tauri::command]
pub async fn session_import_group_selection_choose(scan_id: String, epoch: u64, selection_id: String, expected_state_version: u64, group_id: String, candidate_id: String) -> Result<fixed_groups::GroupMemberChange, String> {
    group_selection_call(move || fixed_groups::choose_member(&scan_id, epoch, &selection_id, expected_state_version, &group_id, &candidate_id)).await
}

#[tauri::command]
pub async fn session_import_group_selection_page(scan_id: String, epoch: u64, selection_id: String, cursor: Option<String>, limit: Option<usize>, selected_only: Option<bool>) -> Result<fixed_groups::GroupSelectionPage, String> {
    group_selection_call(move || fixed_groups::page(&scan_id, epoch, &selection_id, cursor.as_deref(), limit, selected_only.unwrap_or(true))).await
}

#[tauri::command]
pub async fn session_import_group_selection_membership(scan_id: String, epoch: u64, selection_id: String, group_ids: Vec<String>) -> Result<fixed_groups::GroupSelectionMembership, String> {
    group_selection_call(move || fixed_groups::membership(&scan_id, epoch, &selection_id, &group_ids)).await
}
