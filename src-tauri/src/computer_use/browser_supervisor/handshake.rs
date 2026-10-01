//! Supervisor handshake: protocol/build/runtime/page, never GROK_CU_BROWSER_IPC.

use std::path::PathBuf;
use std::time::Duration;

use uuid::Uuid;

use super::process::{test_node_exe, BrowserSupervisor, SpawnRequest};
use super::process_tree::process_alive;

/// Probe/E2: Host-owned spawn using an explicit node+script, never PATH.
pub fn run_supervisor_gate() -> Result<(), String> {
    if std::env::var_os("GROK_CU_BROWSER_IPC").is_some() {
        return Err("GROK_CU_BROWSER_IPC must not be required; unset it for this gate".into());
    }
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("tools")
        .join("computer-use-browser")
        .join("server.mjs");
    if !script.is_file() {
        return Err(format!("missing {}", script.display()));
    }
    let Some(node) = test_node_exe() else {
        println!("managed browser supervisor: not_run (js-runtime pack missing)");
        return Ok(());
    };
    let profile = std::env::temp_dir().join(format!(
        "grok-cu-browser-{}-{}",
        std::process::id(),
        Uuid::new_v4()
    ));
    let supervisor = BrowserSupervisor::spawn(SpawnRequest {
        node,
        script,
        profile_root: profile,
        browser: None,
    })?;
    let pid = supervisor.pid();
    let health = supervisor.health()?;
    if health.get("ok") != Some(&serde_json::json!(true)) {
        return Err(format!("health {health}"));
    }
    if health.get("protocol") != Some(&serde_json::json!(1)) {
        return Err(format!("handshake protocol missing: {health}"));
    }
    if health
        .get("build")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .is_empty()
    {
        return Err(format!("handshake build missing: {health}"));
    }
    if health
        .get("runtime")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .is_empty()
    {
        return Err(format!("handshake runtime missing: {health}"));
    }
    if health.get("page").and_then(|v| v.get("observe")) != Some(&serde_json::json!(true)) {
        return Err(format!("handshake page capability missing: {health}"));
    }
    // Missing bearer must fail closed.
    let denied = reqwest::blocking::Client::new()
        .post(format!("{}/health", supervisor.base_url()))
        .json(&serde_json::json!({}))
        .send()
        .map_err(|e| e.to_string())?;
    if denied.status().as_u16() != 401 {
        return Err(format!("missing token must 401, got {}", denied.status()));
    }
    supervisor.shutdown()?;
    std::thread::sleep(Duration::from_millis(200));
    if process_alive(pid) {
        return Err(format!("worker pid {pid} still alive after shutdown"));
    }
    println!("gate: managed_browser_host_owned_spawn_stop pid={pid}");
    Ok(())
}
