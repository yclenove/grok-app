// Desktop settings only: no model/MCP or mirror method is registered.
fn cu_helper_settings_window(webview: &tauri::Webview) -> Result<(), String> {
    // Use Tauri's actual invoking view, never a renderer-supplied label. The
    // embedded browsing surface must not manage a desktop user's extension.
    if webview.label() != "main" {
        return Err("computer_use_helper_settings_window_required".into());
    }
    Ok(())
}
#[tauri::command]
pub async fn computer_use_helper_status(
    webview: tauri::Webview,
) -> Result<crate::computer_use::gnome_helper::HelperStatus, String> {
    cu_helper_settings_window(&webview)?;
    crate::computer_use::gnome_helper::status().await
}

#[tauri::command]
pub async fn computer_use_helper_action(
    webview: tauri::Webview,
    action: crate::computer_use::gnome_helper::HelperAction,
) -> Result<crate::computer_use::gnome_helper::HelperStatus, String> {
    cu_helper_settings_window(&webview)?;
    crate::computer_use::gnome_helper::act(action).await
}
