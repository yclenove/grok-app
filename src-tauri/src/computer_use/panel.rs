//! Forward side-panel notices. The timeline itself lives in `gcu-host-protocol`.

use gcu_host_protocol::panel::{Notice, Panel};
use tauri::{AppHandle, Emitter};

/// Subscribe before the engine can emit, then forward each notice to the main window.
pub(crate) fn forward_notices(app: AppHandle, panel: &Panel) {
    let mut notes = panel.subscribe();
    tauri::async_runtime::spawn(async move {
        loop {
            match notes.recv().await {
                Ok(notice) => emit(&app, &notice),
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });
}

fn emit(app: &AppHandle, notice: &Notice) {
    let _ = app.emit("cu://timeline", notice);
}
