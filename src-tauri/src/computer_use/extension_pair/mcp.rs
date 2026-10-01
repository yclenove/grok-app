//! Private fixture controls. These are not HTTP endpoints or model authorization tools.
use grok_computer_use_core::{adapter::SurfaceKind, broker::ComputerUseBroker};
use serde_json::{json, Value};

const SESSION: &str = "extension-mcp-owner";
const RUN: &str = "extension-mcp-run";

pub(super) fn control(
    broker: &ComputerUseBroker,
    endpoint: &str,
    value: &Value,
) -> Result<Value, String> {
    match value["command"].as_str().unwrap_or_default() {
        "mcp-open" => {
            let secondary = value["secondary"].as_bool().unwrap_or(false);
            let (session, run) = if secondary {
                ("extension-mcp-other-owner", "extension-mcp-other-run")
            } else {
                (SESSION, RUN)
            };
            broker
                .open_run(session, run)
                .map_err(|_| "fixture run unavailable")?;
            if secondary {
                // Production does not issue MCP credentials before a target grant.
                let issued = super::super::ipc::endpoint()
                    .ok_or("fixture IPC unavailable")?
                    .credential_for_session(session, run)
                    .is_ok();
                return Ok(json!({"issued": issued}));
            }
            if !secondary {
                let selector = value["selector"]
                    .as_str()
                    .ok_or("fixture candidate missing")?;
                broker
                    .authorize_on_surface(
                        session,
                        run,
                        SurfaceKind::ExistingTab,
                        &format!("existing-tab:{selector}"),
                    )
                    .map_err(|_| "fixture authorization failed")?;
            }
            let token = super::super::ipc::endpoint()
                .ok_or("fixture IPC unavailable")?
                .credential_for_session(session, run)
                .map_err(|_| "fixture credential issuance failed")?;
            // Private stdin pipe only; the child forwards these as env, never args or logs.
            Ok(json!({"endpoint": endpoint, "token": token, "session": session, "run": run}))
        }
        "mcp-preview" => Ok(json!({"ok": broker.observe_preview(RUN).is_ok()})),
        "mcp-state" => Ok(json!({
            "snapshot": broker.act_defaults(RUN).ok().map(|defaults| defaults.2),
            "borrowed": broker.tabs().list_for_run(RUN).len(),
            "idle": broker.tabs().existing_operations_idle(RUN),
            "stop": broker.stop_state(RUN).map_err(|_| "fixture run missing")?.as_str(),
        })),
        _ => Err("unsupported MCP fixture request".into()),
    }
}
