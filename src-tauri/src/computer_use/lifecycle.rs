//! Thin ACP/App boundary adapter. The tested state machine lives in the protocol crate.
use super::{gateway, ComputerUseHost};
use gcu_host_protocol::lifecycle::Turn;
use tauri::{AppHandle, Emitter, Manager};

/// Capture immediately before the ACP prompt is sent, including background chats.
pub(crate) fn begin_turn(app: &AppHandle, session: &str) -> Option<Turn> {
    app.try_state::<ComputerUseHost>()?
        .lifecycle
        .begin_turn(session)
}

/// Every terminal prompt RPC path (Ok, error, cancel) converges here.
pub(crate) async fn turn_ended(app: &AppHandle, turn: Option<Turn>) {
    let (Some(host), Some(turn)) = (app.try_state::<ComputerUseHost>(), turn) else {
        return;
    };
    let session_id = turn.session_id().to_owned();
    if let Err(error) = host.lifecycle.end_turn(turn).await {
        tracing::debug!(%error, "computer use turn end failed");
    }
    let _ = app.emit(
        "cu://turn-ended",
        serde_json::json!({ "sessionId": session_id }),
    );
}

/// Close/delete/fork/model boundaries never restore Stop. Changing chat focus
/// only parks/demotes the ACP and deliberately does not call this hook.
pub(crate) async fn session_closed(app: &AppHandle, session: &str) {
    let Some(host) = app.try_state::<ComputerUseHost>() else {
        return;
    };
    gateway::revoke(&host, session).await;
    host.overlay.hide_session(session);
    host.panel.drop_session(session);
}

/// A replacement engine or revoked MCP namespace requires ACP MCP reinjection.
pub(crate) fn needs_reconnect(app: &AppHandle, session: &str) -> bool {
    app.try_state::<ComputerUseHost>()
        .is_some_and(|host| host.lifecycle.needs_reconnect(session))
}

/// Clear visible/cache derivatives for explicit closes and engine replacement.
pub(super) fn forward_closed(app: &AppHandle) {
    let host = app.state::<ComputerUseHost>();
    let mut notices = host.lifecycle.subscribe_closed();
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        while let Some(session) = notices.recv().await {
            let host = app.state::<ComputerUseHost>();
            host.overlay.hide_session(&session);
            host.panel.drop_session(&session);
            let _ = app.emit(
                "cu://session-closed",
                serde_json::json!({ "sessionId": session }),
            );
        }
    });
}

#[cfg(test)]
#[path = "tests/lifecycle.rs"]
mod tests;
