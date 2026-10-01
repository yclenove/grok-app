// Prevents additional console window on Windows in release
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    #[cfg(target_os = "windows")]
    if let Some(code) = tauri_plugin_updater::windows_update_dispatch_entrypoint(|| {
        if !cfg!(grok_updater_enabled) || cfg!(debug_assertions) {
            return Err("Independent updater dispatch is disabled in this build".into());
        }
        // generate_context! embeds the release build's policy. Do not load a
        // sidecar/env/IPC public key or start Tauri before this private entry.
        let context: tauri::Context<tauri::Wry> = tauri::generate_context!();
        let config = context
            .config()
            .plugins
            .0
            .get("updater")
            .ok_or("Compiled updater configuration missing")?
            .clone();
        Ok(tauri_plugin_updater::WindowsDispatchTrust {
            config: serde_json::from_value(config).map_err(|e| e.to_string())?,
            app_name: context.package_info().name.clone(),
            source_version: context.package_info().version.to_string(),
            target: "windows".into(),
        })
    }) {
        std::process::exit(code);
    }
    #[cfg(target_os = "windows")]
    if let Some(code) = tauri_plugin_updater::windows_update_witness_entrypoint() {
        std::process::exit(code);
    }
    #[cfg(target_os = "linux")]
    if let Some(code) = grok_computer_use_x11::clipboard::keeper_entrypoint() {
        std::process::exit(code);
    }
    grok_app_lib::run();
}
