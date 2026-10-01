//! Named broker contract tests migrated from y_rounds.rs.
use super::gates::enabled_opts;
use super::*;
use crate::adapter::skip_desktop_target;
use crate::fake::FakeAdapter;

fn ready() -> Result<(ComputerUseBroker, Arc<FakeAdapter>), String> {
    let fake = Arc::new(FakeAdapter::new());
    fake.set_include_png(true);
    let broker = ComputerUseBroker::new(fake.clone(), enabled_opts(400));
    broker.open_run("s", "run-1").map_err(|e| e.to_string())?;
    broker
        .authorize_target("run-1", &fake.fixture_id())
        .map_err(|e| e.to_string())?;
    Ok((broker, fake))
}

fn y1_model_stop_state_is_snake_case() -> Result<(), String> {
    let (broker, _) = ready()?;
    let binding = crate::ipc::SessionBinding {
        session_id: "s".into(),
        run_id: "run-1".into(),
    };
    let result =
        crate::tools::dispatch(&broker, &binding, "computer_status", serde_json::json!({}));
    let text = result["content"][0]["text"].as_str().unwrap_or("");
    let payload: serde_json::Value =
        serde_json::from_str(text).map_err(|e| format!("status json {e} {text}"))?;
    if payload["stopState"].as_str() != Some("running") {
        return Err(format!("stopState must be running, got {text}"));
    }
    if text.contains("Some(") || text.contains("Running") {
        return Err(format!("stopState leaked Debug, {text}"));
    }
    println!("gate: y1_model_stop_state_is_snake_case");
    Ok(())
}

fn y2_wait_blank_name_rejected() -> Result<(), String> {
    let (broker, fake) = ready()?;
    let binding = crate::ipc::SessionBinding {
        session_id: "s".into(),
        run_id: "run-1".into(),
    };
    let result = crate::tools::dispatch(
        &broker,
        &binding,
        "computer_wait",
        super::test_support::wait_args(&broker, "run-1", "   ", 80),
    );
    if result.get("isError") != Some(&serde_json::json!(true)) {
        return Err("blank nameEquals must error".into());
    }
    if !fake.executions().is_empty() {
        return Err("blank wait executed".into());
    }
    println!("gate: y2_wait_blank_name_rejected");
    Ok(())
}

fn y3_ipc_get_not_allowed() -> Result<(), String> {
    let (server, token) = super::test_support::ipc_fixture()?;
    let response = reqwest::blocking::Client::new()
        .get(format!("{}/cu/tool", server.url))
        .bearer_auth(&token)
        .send()
        .map_err(|e| e.to_string())?;
    let status = response.status().as_u16();
    if status != 405 && status != 404 {
        return Err(format!("GET /cu/tool must not succeed, got {status}"));
    }
    println!("gate: y3_ipc_get_not_allowed");
    Ok(())
}

fn y4_overlay_not_listed_or_authorized() -> Result<(), String> {
    let fake = Arc::new(FakeAdapter::new());
    fake.set_include_overlay(true);
    let broker = ComputerUseBroker::new(fake.clone(), enabled_opts(400));
    broker.open_run("s", "run-1").map_err(|e| e.to_string())?;
    let listed = broker.discover_targets().map_err(|e| e.to_string())?;
    if listed.iter().any(|t| t.target_id == fake.overlay_id()) {
        return Err("overlay must not appear in the picker".into());
    }
    match broker.authorize_target("run-1", &fake.overlay_id()) {
        Err(BrokerError::TargetUnauthorized) | Err(BrokerError::DeadTarget) => {}
        other => return Err(format!("overlay authorize must fail, got {other:?}")),
    }
    println!("gate: y4_overlay_not_listed_or_authorized");
    Ok(())
}

fn y5_wait_blank_ref_rejected() -> Result<(), String> {
    let (broker, fake) = ready()?;
    let mut args = super::test_support::wait_args(&broker, "run-1", "Count", 80);
    args["elementRef"] = serde_json::json!("  ");
    let binding = crate::ipc::SessionBinding {
        session_id: "s".into(),
        run_id: "run-1".into(),
    };
    let result = crate::tools::dispatch(&broker, &binding, "computer_wait", args);
    if result.get("isError") != Some(&serde_json::json!(true)) {
        return Err("blank elementRef must error".into());
    }
    if !fake.executions().is_empty() {
        return Err("blank ref wait executed".into());
    }
    println!("gate: y5_wait_blank_ref_rejected");
    Ok(())
}

fn y6_chatgpt_app_is_not_auto_skipped() -> Result<(), String> {
    if skip_desktop_target("ChatGPT", "chrome") {
        return Err("ChatGPT window itself must remain user-selectable".into());
    }
    if !skip_desktop_target("ChatGPT is using your computer. Esc to cancel", "chrome") {
        return Err("CU overlay must be skipped".into());
    }
    if !skip_desktop_target("NVIDIA GeForce Overlay", "overlay") {
        return Err("GeForce overlay must be skipped".into());
    }
    if !skip_desktop_target("User Account Control", "consent") {
        return Err("UAC consent UI must be skipped".into());
    }
    if !skip_desktop_target("用户账户控制", "consent") {
        return Err("UAC consent UI (zh) must be skipped".into());
    }
    if !skip_desktop_target("Confirm", "consent") {
        return Err("consent.exe must be skipped even without UAC title".into());
    }
    println!("gate: y6_chatgpt_app_is_not_auto_skipped");
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn model_stop_state_is_snake_case() {
        super::y1_model_stop_state_is_snake_case().expect("model_stop_state_is_snake_case");
    }
    #[test]
    fn wait_blank_name_rejected() {
        super::y2_wait_blank_name_rejected().expect("wait_blank_name_rejected");
    }
    #[test]
    fn ipc_get_not_allowed() {
        super::y3_ipc_get_not_allowed().expect("ipc_get_not_allowed");
    }
    #[test]
    fn overlay_not_listed_or_authorized() {
        super::y4_overlay_not_listed_or_authorized().expect("overlay_not_listed_or_authorized");
    }
    #[test]
    fn wait_blank_ref_rejected() {
        super::y5_wait_blank_ref_rejected().expect("wait_blank_ref_rejected");
    }
    #[test]
    fn chatgpt_app_is_not_auto_skipped() {
        super::y6_chatgpt_app_is_not_auto_skipped().expect("chatgpt_app_is_not_auto_skipped");
    }
}
