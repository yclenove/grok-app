//! Policy file for persistent Always grants. Corrupt files grant nothing.

use std::path::PathBuf;

use gcu_host_protocol::approval::Office;
use tauri::{AppHandle, Manager};

/// Load the App policy, or the baseline when the file is missing or corrupt.
pub(crate) fn open_office(app: &AppHandle) -> Office {
    Office::open(policy_file(app))
}

fn policy_file(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .app_config_dir()
        .ok()
        .map(|dir| dir.join("computer-use").join("policy.json"))
}
