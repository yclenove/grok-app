// Main local WebView only. Never registered as a model or mirror action.
#[tauri::command]
pub(crate) fn computer_use_extension_info(
    webview: tauri::Webview,
) -> Result<crate::computer_use::extension_bundle::ExtensionInfo, String> {
    computer_task_main_webview(&webview)?;
    crate::computer_use::extension_bundle::info()
}
#[tauri::command]
pub(crate) async fn computer_use_extension_export(
    webview: tauri::Webview,
) -> Result<crate::computer_use::extension_bundle::ExportedExtension, String> {
    computer_task_main_webview(&webview)?;
    cu_blocking(crate::computer_use::extension_bundle::export).await
}
#[tauri::command]
pub(crate) async fn computer_use_extension_open(
    webview: tauri::Webview,
) -> Result<(), String> {
    computer_task_main_webview(&webview)?;
    cu_blocking(|| {
        let path = crate::computer_use::extension_bundle::published_path()?;
        use std::process::Stdio;
        #[cfg(target_os = "windows")]
        let mut command = crate::process_util::command("explorer.exe");
        #[cfg(target_os = "macos")]
        let mut command = crate::process_util::command("open");
        #[cfg(target_os = "linux")]
        let mut command = crate::process_util::command("xdg-open");
        command.arg(path).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null())
            .spawn().map_err(|_| "computer_use_extension_open_failed".to_owned())?;
        Ok(())
    }).await
}
