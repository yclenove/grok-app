//! Shared foreground/background handler, before replay and generic policy.
use super::*;

impl SessionManager {
    pub(crate) async fn respond_computer_use_permission(
        &self,
        app_session: &str,
        process: &str,
        rpc_id: u64,
        raw: &Value,
        options: &Value,
    ) -> bool {
        let Some(endpoint) = self.mcp_endpoint(app_session) else {
            return false;
        };
        if endpoint.process_id != process {
            return false;
        }
        if !endpoint
            .acp
            .is_registered_computer_use_call(app_session, raw)
        {
            return false;
        }
        let turn = self
            .with_session_mut(app_session, |session| {
                (session.process_id == process && session.prompt_in_flight)
                    .then(|| session.active_turn_id.clone())
                    .flatten()
            })
            .flatten();
        let active = turn.is_some();
        let acp = endpoint.acp.clone();
        let app_id = app_session.to_string();
        let agent = endpoint.agent_session_id.clone();
        let raw = raw.clone();
        let options = options.clone();
        let outcome = tauri::async_runtime::spawn_blocking(move || {
            acp.computer_use_permission(&app_id, &agent, &raw, &options, active)
        })
        .await;
        // Once identified as Host CU, a retired connection or failed worker
        // must never fall through to replay/YOLO/general allow policy.
        let mut outcome = outcome
            .ok()
            .flatten()
            .unwrap_or(crate::acp_client::PermissionOutcome::Cancelled);
        let still_active = turn.is_some()
            && self.endpoint_is_current(app_session, &endpoint)
            && self
                .with_session_mut(app_session, |session| {
                    session.prompt_in_flight && session.active_turn_id == turn
                })
                .unwrap_or(false);
        if !still_active {
            outcome = crate::acp_client::PermissionOutcome::Cancelled;
        }
        let _ = endpoint.acp.respond_permission(rpc_id, outcome).await;
        true
    }
}
