//! Returning event loop is limited to the isolated Windows debug probe entry.

pub(crate) fn validate() -> Result<(), &'static str> {
    let absolute_home =
        std::env::var("GROK_APP_HOME").is_ok_and(|home| std::path::Path::new(&home).is_absolute());
    if crate::paths::isolated_app_instance_id().is_none()
        || !absolute_home
        || std::env::var("GROK_CU_APP_HARNESS").ok().as_deref() != Some("1")
        || !matches!(
            std::env::var("GROK_CU_APP_HARNESS_MODE").ok().as_deref(),
            Some(
                "webview-download-close"
                    | "webview-exit-idle"
                    | "webview-download-error-unwind"
                    | "webview-download-before-start"
                    | "webview-download-buffer-retirement"
                    | "webview-download-buffer-crash"
                    | "webview-download-publication"
                    | "webview-upload"
                    | "webview-upload-task"
                    | "webview-upload-mcp"
                    | "native-download-maintenance"
            )
        )
    {
        return Err("App probe requires an isolated debug WebView harness");
    }
    Ok(())
}

/// A separate fixture process invokes the same explicit Host staging command.
/// It does not start a UI, native browser, model session or automatic recovery.
pub(crate) fn try_maintenance() -> Result<bool, &'static str> {
    use std::io::Write;

    if std::env::var("GROK_CU_APP_HARNESS_MODE").ok().as_deref()
        != Some("native-download-maintenance")
    {
        return Ok(false);
    }
    let settings = crate::store::load_settings();
    if settings.session_data_mode != "independent"
        || settings.close_to_tray
        || settings.keep_tray_for_schedules
    {
        return Err("maintenance probe requires independent isolated settings");
    }
    let home = crate::paths::app_data_root();
    let output = std::env::var_os("GROK_CU_APP_HARNESS_OUT")
        .map(std::path::PathBuf::from)
        .ok_or("maintenance probe report missing")?;
    if output.parent() != Some(home.as_path()) {
        return Err("maintenance probe report must belong to the isolated home");
    }
    let result = tauri::async_runtime::block_on(crate::commands::computer_use_clear_staging());
    let report = serde_json::json!({
        "ok":result.is_ok(),"mode":"native-download-maintenance",
        "realHostMaintenanceCommand":true,"error":result.as_ref().err(),
        "realModel":false,"fullDeliveryAcceptance":false
    });
    let bytes = serde_json::to_vec(&report).map_err(|_| "maintenance report invalid")?;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)
        .map_err(|_| "maintenance report creation failed")?;
    file.write_all(&bytes)
        .and_then(|()| file.sync_all())
        .map_err(|_| "maintenance report publication failed")?;
    result.map_err(|_| "native download maintenance retained original files")?;
    Ok(true)
}
