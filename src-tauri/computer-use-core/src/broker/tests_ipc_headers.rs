//! Named broker contract tests migrated from z_rounds.rs.
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

fn z1_lowercase_bearer_accepted() -> Result<(), String> {
    let (server, token) = super::test_support::ipc_fixture()?;
    let response = reqwest::blocking::Client::new()
        .post(format!("{}/cu/tool", server.url))
        .header("Authorization", format!("bearer {token}"))
        .json(&serde_json::json!({"name":"computer_status"}))
        .send()
        .map_err(|e| e.to_string())?;
    if response.status().as_u16() != 200 {
        return Err(format!(
            "lowercase bearer must be 200, got {}",
            response.status()
        ));
    }
    println!("gate: z1_lowercase_bearer_accepted");
    Ok(())
}

fn z2_cookie_header_forbidden() -> Result<(), String> {
    let (server, token) = super::test_support::ipc_fixture()?;
    let response = reqwest::blocking::Client::new()
        .post(format!("{}/cu/tool", server.url))
        .bearer_auth(&token)
        .header("Cookie", "sid=evil")
        .json(&serde_json::json!({"name":"computer_status"}))
        .send()
        .map_err(|e| e.to_string())?;
    if response.status().as_u16() != 403 {
        return Err(format!("cookie must be 403, got {}", response.status()));
    }
    println!("gate: z2_cookie_header_forbidden");
    Ok(())
}

fn z3_statusbar_not_listed() -> Result<(), String> {
    let fake = Arc::new(FakeAdapter::new());
    fake.set_include_statusbar(true);
    let broker = ComputerUseBroker::new(fake.clone(), enabled_opts(400));
    broker.open_run("s", "run-1").map_err(|e| e.to_string())?;
    let listed = broker.discover_targets().map_err(|e| e.to_string())?;
    if listed.iter().any(|t| t.target_id == fake.statusbar_id()) {
        return Err("StatusBarWnd must not appear in the picker".into());
    }
    if !skip_desktop_target("StatusBarWnd", "win32")
        || !skip_desktop_target("Program Manager", "win32")
        || !skip_desktop_target("Default IME", "win32")
    {
        return Err("helper window titles must be skipped".into());
    }
    println!("gate: z3_statusbar_not_listed");
    Ok(())
}

fn z4_wait_name_too_long_rejected() -> Result<(), String> {
    let (broker, fake) = ready()?;
    let binding = crate::ipc::SessionBinding {
        session_id: "s".into(),
        run_id: "run-1".into(),
    };
    let result = crate::tools::dispatch(
        &broker,
        &binding,
        "computer_wait",
        super::test_support::wait_args(&broker, "run-1", &"x".repeat(257), 80),
    );
    if result.get("isError") != Some(&serde_json::json!(true)) {
        return Err("overlong nameEquals must error".into());
    }
    if !fake.executions().is_empty() {
        return Err("overlong wait executed".into());
    }
    println!("gate: z4_wait_name_too_long_rejected");
    Ok(())
}

fn z5_ipc_put_not_allowed() -> Result<(), String> {
    let (server, token) = super::test_support::ipc_fixture()?;
    let response = reqwest::blocking::Client::new()
        .put(format!("{}/cu/tool", server.url))
        .bearer_auth(&token)
        .json(&serde_json::json!({"name":"computer_status"}))
        .send()
        .map_err(|e| e.to_string())?;
    let status = response.status().as_u16();
    if status != 405 && status != 404 {
        return Err(format!("PUT /cu/tool must not succeed, got {status}"));
    }
    println!("gate: z5_ipc_put_not_allowed");
    Ok(())
}

fn z6_status_paused_is_boolean() -> Result<(), String> {
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
    if payload["paused"] != serde_json::json!(false) {
        return Err(format!("paused must be boolean false, got {text}"));
    }
    println!("gate: z6_status_paused_is_boolean");
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn lowercase_bearer_accepted() {
        super::z1_lowercase_bearer_accepted().expect("lowercase_bearer_accepted");
    }
    #[test]
    fn cookie_header_forbidden() {
        super::z2_cookie_header_forbidden().expect("cookie_header_forbidden");
    }
    #[test]
    fn statusbar_not_listed() {
        super::z3_statusbar_not_listed().expect("statusbar_not_listed");
    }
    #[test]
    fn wait_name_too_long_rejected() {
        super::z4_wait_name_too_long_rejected().expect("wait_name_too_long_rejected");
    }
    #[test]
    fn ipc_put_not_allowed() {
        super::z5_ipc_put_not_allowed().expect("ipc_put_not_allowed");
    }
    #[test]
    fn status_paused_is_boolean() {
        super::z6_status_paused_is_boolean().expect("status_paused_is_boolean");
    }
}
