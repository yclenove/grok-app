//! Named broker contract tests migrated from x_rounds.rs.
use super::gates::{click_req, enabled_opts};
use super::*;
use crate::browser::RecordingBrowserWorker;
use crate::fake::FakeAdapter;

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

fn x1_wait_timeout_keeps_snapshot() -> Result<(), String> {
    let (broker, fake, gen, obs) = ready()?;
    let before = broker
        .model_snapshot_id("run-1")
        .ok_or("missing snapshot")?;
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
        return Err("wait miss must error".into());
    }
    let after = broker
        .model_snapshot_id("run-1")
        .ok_or("snapshot dropped")?;
    if after != before {
        return Err(format!("failed wait advanced snapshot {before} -> {after}"));
    }
    let out = broker.act(click_req(
        "run-1",
        &fake.fixture_id(),
        gen,
        &obs.snapshot_id,
        obs.geometry_revision,
        "after-wait",
    ));
    if !out.executed {
        return Err(format!(
            "pre-wait snapshot must still act, {:?}",
            out.reason
        ));
    }
    println!("gate: x1_wait_timeout_keeps_snapshot");
    Ok(())
}

fn x2_model_list_only_authorized() -> Result<(), String> {
    let fake = Arc::new(FakeAdapter::new());
    fake.set_include_png(true);
    fake.set_include_decoy(true);
    let broker = ComputerUseBroker::new(fake.clone(), enabled_opts(400));
    broker.open_run("s", "run-1").map_err(|e| e.to_string())?;
    let binding = crate::ipc::SessionBinding {
        session_id: "s".into(),
        run_id: "run-1".into(),
    };
    let empty = crate::tools::dispatch(
        &broker,
        &binding,
        "computer_list_targets",
        serde_json::json!({}),
    );
    let empty_text = empty["content"][0]["text"].as_str().unwrap_or("");
    if empty_text != "[]" && !empty_text.contains("[]") {
        return Err(format!("unauthorized list must be empty, got {empty_text}"));
    }
    let listed = broker.list_targets("run-1").map_err(|e| e.to_string())?;
    if listed.len() < 2 {
        return Err("host picker must still see the decoy".into());
    }
    broker
        .authorize_target("run-1", &fake.fixture_id())
        .map_err(|e| e.to_string())?;
    let result = crate::tools::dispatch(
        &broker,
        &binding,
        "computer_list_targets",
        serde_json::json!({}),
    );
    let text = result["content"][0]["text"].as_str().unwrap_or("");
    let rows: Vec<serde_json::Value> =
        serde_json::from_str(text).map_err(|e| format!("list json {e} {text}"))?;
    if rows.len() != 1 {
        return Err(format!(
            "model list must be the authorized target only, got {rows:?}"
        ));
    }
    if rows[0]["targetId"].as_str() != Some(fake.fixture_id().as_str()) {
        return Err(format!("model list leaked {:?}", rows[0]));
    }
    if text.contains("OtherApp") || text.contains("fake:decoy") {
        return Err("decoy leaked to the model".into());
    }
    println!("gate: x2_model_list_only_authorized");
    Ok(())
}

fn x3_sec_fetch_forbidden() -> Result<(), String> {
    let (server, token) = super::test_support::ipc_fixture()?;
    let response = reqwest::blocking::Client::new()
        .post(format!("{}/cu/tool", server.url))
        .bearer_auth(&token)
        .header("Sec-Fetch-Site", "cross-site")
        .json(&serde_json::json!({"name":"computer_status"}))
        .send()
        .map_err(|e| e.to_string())?;
    if response.status().as_u16() != 403 {
        return Err(format!(
            "sec-fetch-site must be 403, got {}",
            response.status()
        ));
    }
    println!("gate: x3_sec_fetch_forbidden");
    Ok(())
}

fn x4_navigate_requires_tab() -> Result<(), String> {
    let (broker, _, _, _) = ready()?;
    let root = std::env::temp_dir().join(format!("cu-x4-{}", Uuid::new_v4()));
    let worker = Arc::new(RecordingBrowserWorker::new(root.clone()));
    broker.tabs().set_worker(worker.clone());
    let binding = crate::ipc::SessionBinding {
        session_id: "s".into(),
        run_id: "run-1".into(),
    };
    let result = crate::tools::dispatch(
        &broker,
        &binding,
        "computer_navigate",
        serde_json::json!({"url": "https://example.test/"}),
    );
    if result.get("isError") != Some(&serde_json::json!(true)) {
        return Err("navigate without tabId must error".into());
    }
    if !worker.gotos.lock().is_empty() {
        return Err("navigate without tabId reached the worker".into());
    }
    let _ = std::fs::remove_dir_all(root);
    println!("gate: x4_navigate_requires_tab");
    Ok(())
}

fn x5_download_requires_tab() -> Result<(), String> {
    let (broker, _, _, _) = ready()?;
    let root = std::env::temp_dir().join(format!("cu-x5-{}", Uuid::new_v4()));
    let worker = Arc::new(RecordingBrowserWorker::new(root.clone()));
    broker.tabs().set_worker(worker.clone());
    let binding = crate::ipc::SessionBinding {
        session_id: "s".into(),
        run_id: "run-1".into(),
    };
    let result = crate::tools::dispatch(
        &broker,
        &binding,
        "computer_download",
        serde_json::json!({"filename": "report.bin"}),
    );
    if result.get("isError") != Some(&serde_json::json!(true)) {
        return Err("download without tabId must error".into());
    }
    if *worker.downloads.lock() != 0 {
        return Err("download without tabId reached the worker".into());
    }
    let _ = std::fs::remove_dir_all(root);
    println!("gate: x5_download_requires_tab");
    Ok(())
}

fn x6_wait_match_still_works() -> Result<(), String> {
    let (broker, _, _, _) = ready()?;
    let binding = crate::ipc::SessionBinding {
        session_id: "s".into(),
        run_id: "run-1".into(),
    };
    let result = crate::tools::dispatch(
        &broker,
        &binding,
        "computer_wait",
        super::test_support::wait_args(&broker, "run-1", "Count", 400),
    );
    if result.get("isError") == Some(&serde_json::json!(true)) {
        return Err(format!("wait match must succeed, {result}"));
    }
    let text = result["content"][0]["text"].as_str().unwrap_or("");
    let payload: serde_json::Value = serde_json::from_str(text).unwrap_or(serde_json::json!({}));
    if payload.get("matched") != Some(&serde_json::json!(true)) {
        return Err(format!("wait match payload {text}"));
    }
    println!("gate: x6_wait_match_still_works");
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn wait_timeout_keeps_snapshot() {
        super::x1_wait_timeout_keeps_snapshot().expect("wait_timeout_keeps_snapshot");
    }
    #[test]
    fn model_list_only_authorized() {
        super::x2_model_list_only_authorized().expect("model_list_only_authorized");
    }
    #[test]
    fn sec_fetch_forbidden() {
        super::x3_sec_fetch_forbidden().expect("sec_fetch_forbidden");
    }
    #[test]
    fn navigate_requires_tab() {
        super::x4_navigate_requires_tab().expect("navigate_requires_tab");
    }
    #[test]
    fn download_requires_tab() {
        super::x5_download_requires_tab().expect("download_requires_tab");
    }
    #[test]
    fn wait_match_still_works() {
        super::x6_wait_match_still_works().expect("wait_match_still_works");
    }
}
