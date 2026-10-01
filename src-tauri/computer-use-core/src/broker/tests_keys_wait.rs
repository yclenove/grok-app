//! Named broker contract tests migrated from v_rounds.rs.
use super::gates::{click_req, enabled_opts};
use super::*;
use crate::fake::FakeAdapter;
use crate::protocol::{ActionKind, ActionTarget};

fn ready() -> Result<(ComputerUseBroker, Arc<FakeAdapter>, u64, Observation), String> {
    let fake = Arc::new(FakeAdapter::new());
    fake.set_include_png(true);
    let broker = ComputerUseBroker::new(fake.clone(), enabled_opts(400));
    broker.open_run("s", "run-1").map_err(|e| e.to_string())?;
    let tid = fake.fixture_id();
    let gen = broker
        .authorize_target("run-1", &tid)
        .map_err(|e| e.to_string())?;
    let obs = broker.observe("run-1").map_err(|e| e.to_string())?;
    Ok((broker, fake, gen, obs))
}

fn v1_unknown_key_rejected() -> Result<(), String> {
    let (broker, fake, gen, obs) = ready()?;
    let mut req = click_req(
        "run-1",
        &fake.fixture_id(),
        gen,
        &obs.snapshot_id,
        obs.geometry_revision,
        "alt-f4",
    );
    req.action = ActionKind::Key;
    req.parameters = serde_json::json!({"key": "alt+f4"});
    let out = broker.act(req);
    if out.executed {
        return Err("alt+f4 must not execute".into());
    }
    let reason = out.reason.unwrap_or_default();
    if !reason.contains("allowed set") {
        return Err(format!("expected allowlist denial, got {reason}"));
    }
    if !fake.executions().is_empty() {
        return Err("unknown key reached the adapter".into());
    }
    println!("gate: v1_unknown_key_rejected");
    Ok(())
}

fn v2_wait_timeout_is_error() -> Result<(), String> {
    let (broker, fake, _, _) = ready()?;
    let binding = crate::ipc::SessionBinding {
        session_id: "s".into(),
        run_id: "run-1".into(),
    };
    let result = crate::tools::dispatch(
        &broker,
        &binding,
        "computer_wait",
        super::test_support::wait_args(&broker, "run-1", "Never", 80),
    );
    if result.get("isError") != Some(&serde_json::json!(true)) {
        return Err("wait timeout must be an error, not success".into());
    }
    let text = result["content"][0]["text"].as_str().unwrap_or("");
    if !text.contains("timed out") {
        return Err(format!("wait timeout text, got {text}"));
    }
    if !fake.executions().is_empty() {
        return Err("wait timeout must not dispatch desktop input".into());
    }
    println!("gate: v2_wait_timeout_is_error");
    Ok(())
}

fn v3_ipc_referer_forbidden() -> Result<(), String> {
    let (server, token) = super::test_support::ipc_fixture()?;
    let response = reqwest::blocking::Client::new()
        .post(format!("{}/cu/tool", server.url))
        .bearer_auth(&token)
        .header("Referer", "https://evil.example/app")
        .json(&serde_json::json!({"name":"computer_status"}))
        .send()
        .map_err(|e| e.to_string())?;
    if response.status().as_u16() != 403 {
        return Err(format!("referer must be 403, got {}", response.status()));
    }
    println!("gate: v3_ipc_referer_forbidden");
    Ok(())
}

fn v4_wait_zero_timeout_rejected() -> Result<(), String> {
    let (broker, fake, _, _) = ready()?;
    let binding = crate::ipc::SessionBinding {
        session_id: "s".into(),
        run_id: "run-1".into(),
    };
    let result = crate::tools::dispatch(
        &broker,
        &binding,
        "computer_wait",
        super::test_support::wait_args(&broker, "run-1", "Count", 0),
    );
    if result.get("isError") != Some(&serde_json::json!(true)) {
        return Err("timeoutMs 0 must error".into());
    }
    if !fake.executions().is_empty() {
        return Err("zero wait executed".into());
    }
    println!("gate: v4_wait_zero_timeout_rejected");
    Ok(())
}

fn v5_alt_f4_zero_execution() -> Result<(), String> {
    let (broker, fake, gen, obs) = ready()?;
    for key in ["win+l", "alt+f4", "ctrl+alt+delete", "a"] {
        let mut req = click_req(
            "run-1",
            &fake.fixture_id(),
            gen,
            &obs.snapshot_id,
            obs.geometry_revision,
            &format!("bad-key-{key}"),
        );
        req.action = ActionKind::Key;
        req.target = ActionTarget::Element {
            element_ref: "n1".into(),
        };
        req.parameters = serde_json::json!({ "key": key });
        let out = broker.act(req);
        if out.executed {
            return Err(format!("{key} must not execute"));
        }
    }
    if !fake.executions().is_empty() || !fake.last_key().is_empty() {
        return Err("forbidden keys reached the adapter".into());
    }
    println!("gate: v5_alt_f4_zero_execution");
    Ok(())
}

fn v6_dead_authorized_target_not_alive() -> Result<(), String> {
    let (broker, fake, _, _) = ready()?;
    if !broker.authorized_target_alive("run-1") {
        return Err("live fixture must report alive".into());
    }
    fake.set_alive(false);
    if broker.authorized_target_alive("run-1") {
        return Err("dead fixture must not report alive".into());
    }
    println!("gate: v6_dead_authorized_target_not_alive");
    Ok(())
}

fn v7_allowed_key_reaches_adapter() -> Result<(), String> {
    let (broker, fake, gen, obs) = ready()?;
    let mut req = click_req(
        "run-1",
        &fake.fixture_id(),
        gen,
        &obs.snapshot_id,
        obs.geometry_revision,
        "key-down",
    );
    req.action = ActionKind::Key;
    req.parameters = serde_json::json!({"key": "down"});
    let out = broker.act(req);
    if !out.executed {
        return Err(format!("down must execute, {:?}", out.reason));
    }
    if fake.last_key() != "down" {
        return Err(format!("adapter key {}, expected down", fake.last_key()));
    }
    println!("gate: v7_allowed_key_reaches_adapter");
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn unknown_key_rejected() {
        super::v1_unknown_key_rejected().expect("unknown_key_rejected");
    }
    #[test]
    fn wait_timeout_is_error() {
        super::v2_wait_timeout_is_error().expect("wait_timeout_is_error");
    }
    #[test]
    fn ipc_referer_forbidden() {
        super::v3_ipc_referer_forbidden().expect("ipc_referer_forbidden");
    }
    #[test]
    fn wait_zero_timeout_rejected() {
        super::v4_wait_zero_timeout_rejected().expect("wait_zero_timeout_rejected");
    }
    #[test]
    fn alt_f4_zero_execution() {
        super::v5_alt_f4_zero_execution().expect("alt_f4_zero_execution");
    }
    #[test]
    fn dead_authorized_target_not_alive() {
        super::v6_dead_authorized_target_not_alive().expect("dead_authorized_target_not_alive");
    }
    #[test]
    fn allowed_key_reaches_adapter() {
        super::v7_allowed_key_reaches_adapter().expect("allowed_key_reaches_adapter");
    }
}
