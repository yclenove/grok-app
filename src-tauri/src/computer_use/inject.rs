//! Session-only MCP injection. Credentials never enter argv or global Grok config.
use crate::extensions::McpServerDef;
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;

use super::ipc::IpcServer;
use super::ComputerUseBroker;

pub const MCP_SERVER_NAME: &str = "grok-computer-use";
const SCRIPT: &str = include_str!("../../../tools/computer-use-mcp/server.mjs");
const PROTOCOL: &str = include_str!("../../../tools/computer-use-mcp/protocol.mjs");

fn check_inject_allowed(session: &str) -> Result<(), String> {
    if session.trim().is_empty() {
        return Err("select a local chat first".into());
    }
    if !super::should_inject_session_mcp(Some(session)) {
        return Err("Computer Use is not enabled for this session".into());
    }
    super::refuse_if_not_local(session)
}

fn refuse_path_node(node: &Path) -> Result<(), String> {
    if node.as_os_str().is_empty() || node.components().count() == 1 {
        return Err(
            "Computer Use MCP refuses PATH node. Repair Grok App runtime. System Node on PATH is not used."
                .into(),
        );
    }
    let root = super::runtime::runtime_root();
    if !node.starts_with(&root) {
        return Err(
            "Computer Use MCP command must be the Host private js-runtime. System Node on PATH is not used."
                .into(),
        );
    }
    Ok(())
}

/// ACP connect/update entry. Official-aux is not consulted.
pub fn mcp_acp_entry(session: &str, run: &str) -> Result<Value, String> {
    check_inject_allowed(session)?;
    let broker = super::ensure_host_runtime();
    let endpoint = super::ipc::ensure(broker.clone())?;
    mcp_acp_entry_with(&broker, endpoint, session, run)
}

/// Same builder ACP uses. Callers supply the Host broker and loopback IPC.
pub fn mcp_acp_entry_with(
    broker: &ComputerUseBroker,
    endpoint: &IpcServer,
    session: &str,
    run: &str,
) -> Result<Value, String> {
    check_inject_allowed(session)?;
    broker
        .require_owner(session, run)
        .map_err(|e| e.to_string())?;
    broker
        .authorized_target(run)
        .map_err(|_| "authorize a target in the Computer panel first".to_string())?;
    super::runtime::ensure_embedded_mcp(SCRIPT, PROTOCOL)?;
    let script = super::runtime::resolve(super::runtime::MCP_SERVER)?;
    let node = super::runtime::resolve(super::runtime::JS_RUNTIME)?;
    refuse_path_node(&node)?;
    // Catalog rebuilds and ambiguous ACP retries must keep the credential used
    // by the currently installed MCP child. Explicit security rotation has a
    // separate IPC API and must be coordinated with catalog replacement.
    let token = endpoint.credential_for_session(session, run)?;
    let env = HashMap::from([
        ("GROK_APP_CU_SESSION".into(), session.to_string()),
        ("GROK_APP_CU_TOKEN".into(), token),
        ("GROK_APP_CU_IPC".into(), endpoint.url.clone()),
    ]);
    let def = McpServerDef {
        name: MCP_SERVER_NAME.into(),
        command: Some(node.to_string_lossy().into_owned()),
        args: Some(vec![script.to_string_lossy().into_owned()]),
        env: Some(env),
        url: None,
        headers: None,
        transport: Some("stdio".into()),
        enabled: Some(true),
        scope: Some("session".into()),
    };
    crate::extensions::mcp_def_to_acp(&def)
        .ok_or_else(|| "Computer Use MCP entry could not be built".into())
}

#[cfg(feature = "computer-use-probe")]
fn env_value(entry: &Value, name: &str) -> Option<String> {
    entry.get("env")?.as_array()?.iter().find_map(|row| {
        if row.get("name").and_then(Value::as_str) == Some(name) {
            row.get("value").and_then(Value::as_str).map(str::to_string)
        } else {
            None
        }
    })
}

#[cfg(feature = "computer-use-probe")]
pub fn run_session_mcp_inject_gates() -> Result<(), String> {
    use grok_computer_use_core::broker::BrokerOptions;
    use grok_computer_use_core::fake::FakeAdapter;
    use grok_computer_use_core::runtime::{NewComponent, RuntimeStore, JS_RUNTIME};
    use grok_computer_use_core::surface::ComputerUseSurface;
    use serde_json::json;
    use std::sync::Arc;

    let _home = crate::paths::APP_HOME_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let prev_home = std::env::var("GROK_APP_HOME").ok();
    let tmp = std::env::temp_dir().join(format!(
        "cu-s101-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    let _ = std::fs::create_dir_all(&tmp);
    std::env::set_var("GROK_APP_HOME", &tmp);

    let restore = |prev: Option<String>| {
        super::set_feature_enabled(false);
        super::set_session_enabled("s101-local", false);
        super::set_session_enabled("s101-im", false);
        match prev {
            Some(v) => std::env::set_var("GROK_APP_HOME", v),
            None => std::env::remove_var("GROK_APP_HOME"),
        }
        let _ = std::fs::remove_dir_all(&tmp);
    };

    let fail = |prev: Option<String>, msg: String| {
        restore(prev);
        msg
    };

    let fake = Arc::new(FakeAdapter::new());
    let fixture = fake.fixture_id();
    let broker = Arc::new(ComputerUseBroker::new(
        fake,
        BrokerOptions {
            feature_enabled: true,
            lease_path: tmp.join("desktop.lease"),
            ..BrokerOptions::default()
        },
    ));
    let endpoint = grok_computer_use_core::ipc::spawn(broker.clone())
        .map_err(|e| fail(prev_home.clone(), e))?;

    super::set_feature_enabled(true);
    super::set_session_enabled("s101-local", true);
    super::set_session_surface("s101-local", ComputerUseSurface::LocalInteractive);
    broker
        .open_run("s101-local", "run-bare")
        .map_err(|e| fail(prev_home.clone(), e.to_string()))?;
    match mcp_acp_entry_with(&broker, &endpoint, "s101-local", "run-bare") {
        Err(e) if e.contains("authorize a target") => {}
        other => {
            return Err(fail(
                prev_home,
                format!("inject without authorized target must fail, got {other:?}"),
            ))
        }
    }
    println!("gate: session_mcp_inject_unauthorized");

    super::set_session_enabled("s101-im", true);
    super::set_session_surface("s101-im", ComputerUseSurface::RemoteIm);
    match mcp_acp_entry_with(&broker, &endpoint, "s101-im", "run-bare") {
        Err(_) => {}
        Ok(_) => {
            return Err(fail(
                prev_home,
                "non-local session must not inject Computer Use MCP".into(),
            ))
        }
    }
    println!("gate: session_mcp_inject_nonlocal");

    super::set_feature_enabled(false);
    match mcp_acp_entry_with(&broker, &endpoint, "s101-local", "run-bare") {
        Err(_) => {}
        Ok(_) => {
            return Err(fail(
                prev_home,
                "disabled flag must not inject Computer Use MCP".into(),
            ))
        }
    }
    match mcp_acp_entry("s101-local", "run-bare") {
        Err(_) => {}
        Ok(_) => {
            return Err(fail(
                prev_home,
                "mcp_acp_entry must fail when Computer Use is off".into(),
            ))
        }
    }
    println!("gate: session_mcp_inject_disabled");

    super::set_feature_enabled(true);
    super::set_session_enabled("s101-local", true);
    super::set_session_surface("s101-local", ComputerUseSurface::LocalInteractive);
    broker
        .open_run("s101-local", "run-auth")
        .map_err(|e| fail(prev_home.clone(), e.to_string()))?;
    broker
        .authorize_target("run-auth", &fixture)
        .map_err(|e| fail(prev_home.clone(), e.to_string()))?;
    match mcp_acp_entry_with(&broker, &endpoint, "s101-local", "run-auth") {
        Err(e) if e.contains("PATH is not used") || e.contains("js-runtime") => {}
        other => {
            return Err(fail(
                prev_home,
                format!("missing private runtime must not fall back to PATH, got {other:?}"),
            ))
        }
    }

    let store = RuntimeStore::new(super::runtime::runtime_root());
    store
        .install_pack(
            "s101",
            vec![NewComponent {
                id: JS_RUNTIME.into(),
                version: "1".into(),
                relpath: "bin/js-runtime".into(),
                bytes: b"runtime-private-pack\n".to_vec(),
            }],
        )
        .map_err(|e| fail(prev_home.clone(), e))?;
    store
        .activate_pack("s101")
        .map_err(|e| fail(prev_home.clone(), e))?;

    let entry = mcp_acp_entry_with(&broker, &endpoint, "s101-local", "run-auth")
        .map_err(|e| fail(prev_home.clone(), e))?;
    if entry.get("name").and_then(Value::as_str) != Some(MCP_SERVER_NAME) {
        return Err(fail(
            prev_home,
            format!("injected server name {:?}", entry.get("name")),
        ));
    }
    let command = entry
        .get("command")
        .and_then(Value::as_str)
        .ok_or_else(|| fail(prev_home.clone(), "inject command missing".into()))?;
    let command_path = Path::new(command);
    refuse_path_node(command_path).map_err(|e| fail(prev_home.clone(), e))?;
    if command_path
        .file_name()
        .is_some_and(|n| n == "node" || n == "node.exe")
    {
        return Err(fail(
            prev_home,
            "injected command must not be system node".into(),
        ));
    }
    let token = env_value(&entry, "GROK_APP_CU_TOKEN")
        .ok_or_else(|| fail(prev_home.clone(), "token must live in env, not argv".into()))?;
    if token.len() < 32 {
        return Err(fail(prev_home, "issued token is too short".into()));
    }
    let args = entry
        .get("args")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if args.iter().any(|v| v.as_str() == Some(token.as_str())) {
        return Err(fail(prev_home, "token must not appear in argv".into()));
    }
    let rebuilt = mcp_acp_entry_with(&broker, &endpoint, "s101-local", "run-auth")
        .map_err(|e| fail(prev_home.clone(), e))?;
    let rebuilt_token = env_value(&rebuilt, "GROK_APP_CU_TOKEN")
        .ok_or_else(|| fail(prev_home.clone(), "rebuilt token missing".into()))?;
    if rebuilt_token != token {
        return Err(fail(
            prev_home,
            "catalog rebuild rotated the live session credential".into(),
        ));
    }
    println!("gate: session_mcp_catalog_rebuild_reuses_token");
    println!(
        "gate: session_mcp_inject_private_runtime command={}",
        command
    );
    println!("gate: session_mcp_inject_authorized");
    println!("gate: session_mcp_inject_independent_of_official_aux");

    let ipc = env_value(&entry, "GROK_APP_CU_IPC")
        .ok_or_else(|| fail(prev_home.clone(), "IPC url missing from env".into()))?;
    let live = reqwest::blocking::Client::new()
        .post(format!("{ipc}/cu/tool"))
        .header("Authorization", format!("Bearer {token}"))
        .json(&json!({"name": "computer_status"}))
        .send()
        .map_err(|e| fail(prev_home.clone(), e.to_string()))?;
    if !live.status().is_success() {
        return Err(fail(
            prev_home,
            format!("authorized token must call IPC, status {}", live.status()),
        ));
    }
    endpoint.revoke_session("s101-local");
    let closed = reqwest::blocking::Client::new()
        .post(format!("{ipc}/cu/tool"))
        .header("Authorization", format!("Bearer {token}"))
        .json(&json!({"name": "computer_status"}))
        .send()
        .map_err(|e| fail(prev_home.clone(), e.to_string()))?;
    if closed.status() != reqwest::StatusCode::UNAUTHORIZED {
        return Err(fail(
            prev_home,
            format!("revoked token must fail closed, status {}", closed.status()),
        ));
    }
    println!("gate: session_mcp_token_revoked");
    println!(
        "gate: session_mcp_inject authorized=ok disabled=ok nonlocal=ok unauthorized=ok private_runtime=ok revoke=ok"
    );

    restore(prev_home);
    Ok(())
}
