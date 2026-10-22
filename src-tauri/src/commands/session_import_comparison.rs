#[tauri::command]
pub async fn session_import_source_comparison(
    request: crate::session_migration::source_comparison::ComparisonRequest,
) -> Result<serde_json::Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::session_migration::source_comparison::page(request)
            .map_err(|error| error.into_command_err())
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn session_import_source_comparison_field(
    request: crate::session_migration::source_comparison_fields::FieldRequest,
) -> Result<serde_json::Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::session_migration::source_comparison_fields::page(request)
            .map_err(|error| error.into_command_err())
    })
    .await
    .map_err(|error| error.to_string())?
}
