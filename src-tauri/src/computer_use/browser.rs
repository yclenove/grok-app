//! Built-in browser commands. L1 stays closed: no content webview is created.
//!
//! Chrome identity is the webview label Tauri reports. A page-supplied name is
//! not an argument. Network destination limits are `not_run` on every OS.

use std::path::Path;

use gcu_host_protocol::clear_profile;
use gcu_protocol::builtin_browser::{
    classify, current_platform, l1_open, UrlClass, UrlRole, CHROME_LABEL,
};
use tauri::{AppHandle, Manager, WebviewWindow};

/// Address-bar snapshot. `open` is false while platform proof is `not_run`.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserChromeState {
    /// False on every current OS.
    pub open: bool,
    /// `not_run` until a platform proves destination limits.
    pub proof: &'static str,
    /// Committed origin. Empty while closed.
    pub origin: String,
}

/// Refuse to open a window. This function does not build a webview.
#[tauri::command]
pub fn cu_browser_open() -> Result<(), String> {
    Err("browser_unavailable".into())
}

/// Closed state for the chrome webview only.
#[tauri::command]
pub fn cu_browser_state(window: WebviewWindow) -> Result<BrowserChromeState, String> {
    require_chrome(window.label())?;
    Ok(BrowserChromeState {
        open: false,
        proof: "not_run",
        origin: String::new(),
    })
}

/// Classify `url`, then refuse. A public URL is still unavailable while L1 is closed.
#[tauri::command]
pub fn cu_browser_navigate(window: WebviewWindow, url: String) -> Result<(), String> {
    decide_navigate(window.label(), &url).map_err(str::to_owned)
}

/// History is not available while L1 is closed.
#[tauri::command]
pub fn cu_browser_back(window: WebviewWindow) -> Result<(), String> {
    decide_history(window.label()).map_err(str::to_owned)
}

/// History is not available while L1 is closed.
#[tauri::command]
pub fn cu_browser_forward(window: WebviewWindow) -> Result<(), String> {
    decide_history(window.label()).map_err(str::to_owned)
}

/// Reload is not available while L1 is closed.
#[tauri::command]
pub fn cu_browser_reload(window: WebviewWindow) -> Result<(), String> {
    decide_history(window.label()).map_err(str::to_owned)
}

/// Delete only the app's `cu-browser-profile`. No path argument. Failure is not success.
#[tauri::command]
pub fn cu_browser_clear(app: AppHandle) -> Result<(), String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|_| "browser_clear_failed".to_owned())?;
    clear_app_data(&dir)
}

pub(crate) fn decide_navigate(label: &str, url: &str) -> Result<(), &'static str> {
    require_chrome(label)?;
    if !matches!(classify(url, UrlRole::Navigation), UrlClass::Public(_)) {
        return Err("browser_url_blocked");
    }
    // Unproven platforms stay closed. A proven platform still has no content webview here.
    let _proven = l1_open(current_platform());
    Err("browser_unavailable")
}

pub(crate) fn decide_history(label: &str) -> Result<(), &'static str> {
    require_chrome(label)?;
    Err("browser_unavailable")
}

pub(crate) fn clear_app_data(app_data: &Path) -> Result<(), String> {
    clear_profile(app_data).map_err(|_| "browser_clear_failed".to_owned())
}

fn require_chrome(label: &str) -> Result<(), &'static str> {
    if label == CHROME_LABEL {
        Ok(())
    } else {
        Err("untrusted")
    }
}

#[cfg(test)]
#[path = "tests/browser.rs"]
mod tests;
