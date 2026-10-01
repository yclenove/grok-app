//! Named broker contract tests migrated from aa_rounds.rs.
use super::gates::enabled_opts;
use super::*;
use crate::adapter::skip_desktop_target;
use crate::browser::RecordingBrowserWorker;
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

fn aa1_model_status_omits_notes() -> Result<(), String> {
    let (broker, _) = ready()?;
    let binding = crate::ipc::SessionBinding {
        session_id: "s".into(),
        run_id: "run-1".into(),
    };
    let result =
        crate::tools::dispatch(&broker, &binding, "computer_status", serde_json::json!({}));
    let text = result["content"][0]["text"].as_str().unwrap_or("");
    if text.contains("notes") || text.contains("BitBlt") || text.contains("test adapter") {
        return Err(format!("model status must omit adapter notes, got {text}"));
    }
    let payload: serde_json::Value =
        serde_json::from_str(text).map_err(|e| format!("status json {e} {text}"))?;
    if payload.get("notes").is_some() {
        return Err("notes field must not be in model status".into());
    }
    if payload["stopState"].as_str() != Some("running") {
        return Err(format!("status still required, got {text}"));
    }
    println!("gate: aa1_model_status_omits_notes");
    Ok(())
}

fn aa2_metadata_url_never_reaches_worker() -> Result<(), String> {
    let (broker, _) = ready()?;
    let root = std::env::temp_dir().join(format!("cu-aa2-{}", Uuid::new_v4()));
    let worker = Arc::new(RecordingBrowserWorker::new(root.clone()));
    broker.tabs().set_worker(worker.clone());
    let binding = crate::ipc::SessionBinding {
        session_id: "s".into(),
        run_id: "run-1".into(),
    };
    for url in [
        "http://169.254.169.254/latest/meta-data/",
        "http://metadata.google.internal/",
        "http://100.100.100.200/latest/meta-data/",
    ] {
        let result = crate::tools::dispatch(
            &broker,
            &binding,
            "computer_navigate",
            serde_json::json!({"tabId": "tab-nav", "url": url}),
        );
        if result.get("isError") != Some(&serde_json::json!(true)) {
            return Err(format!("{url} must be rejected"));
        }
        if broker
            .browser_navigate("run-1", "tab-nav", url, "nav-meta")
            .is_ok()
        {
            return Err(format!("broker navigate leaked {url}"));
        }
    }
    if !worker.gotos.lock().is_empty() {
        return Err("metadata url reached the managed worker".into());
    }
    let _ = std::fs::remove_dir_all(root);
    println!("gate: aa2_metadata_url_never_reaches_worker");
    Ok(())
}

fn aa3_tab_id_too_long_rejected() -> Result<(), String> {
    let (broker, _) = ready()?;
    let root = std::env::temp_dir().join(format!("cu-aa3-{}", Uuid::new_v4()));
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
        serde_json::json!({"tabId": "t".repeat(257), "url": "https://example.test/"}),
    );
    if result.get("isError") != Some(&serde_json::json!(true)) {
        return Err("overlong tabId must error".into());
    }
    if !worker.gotos.lock().is_empty() {
        return Err("overlong tabId reached the worker".into());
    }
    let _ = std::fs::remove_dir_all(root);
    println!("gate: aa3_tab_id_too_long_rejected");
    Ok(())
}

fn aa4_basic_auth_unauthorized() -> Result<(), String> {
    let (server, _token) = super::test_support::ipc_fixture()?;
    let response = reqwest::blocking::Client::new()
        .post(format!("{}/cu/tool", server.url))
        .header("Authorization", "Basic dXNlcjpwYXNz")
        .json(&serde_json::json!({"name":"computer_status"}))
        .send()
        .map_err(|e| e.to_string())?;
    if response.status().as_u16() != 401 {
        return Err(format!("Basic auth must be 401, got {}", response.status()));
    }
    println!("gate: aa4_basic_auth_unauthorized");
    Ok(())
}

fn aa5_ole_window_skipped() -> Result<(), String> {
    if !skip_desktop_target("OleMainThreadWndName", "win32") {
        return Err("OLE helper window must be skipped".into());
    }
    if skip_desktop_target("ChatGPT", "chrome") {
        return Err("ChatGPT must remain selectable".into());
    }
    println!("gate: aa5_ole_window_skipped");
    Ok(())
}

fn aa6_ipc_patch_not_allowed() -> Result<(), String> {
    let (server, token) = super::test_support::ipc_fixture()?;
    let response = reqwest::blocking::Client::new()
        .patch(format!("{}/cu/tool", server.url))
        .bearer_auth(&token)
        .json(&serde_json::json!({"name":"computer_status"}))
        .send()
        .map_err(|e| e.to_string())?;
    let status = response.status().as_u16();
    if status != 405 && status != 404 {
        return Err(format!("PATCH /cu/tool must not succeed, got {status}"));
    }
    println!("gate: aa6_ipc_patch_not_allowed");
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn model_status_omits_notes() {
        super::aa1_model_status_omits_notes().expect("model_status_omits_notes");
    }
    #[test]
    fn metadata_url_never_reaches_worker() {
        super::aa2_metadata_url_never_reaches_worker().expect("metadata_url_never_reaches_worker");
    }
    #[test]
    fn tab_id_too_long_rejected() {
        super::aa3_tab_id_too_long_rejected().expect("tab_id_too_long_rejected");
    }
    #[test]
    fn basic_auth_unauthorized() {
        super::aa4_basic_auth_unauthorized().expect("basic_auth_unauthorized");
    }
    #[test]
    fn ole_window_skipped() {
        super::aa5_ole_window_skipped().expect("ole_window_skipped");
    }
    #[test]
    fn ipc_patch_not_allowed() {
        super::aa6_ipc_patch_not_allowed().expect("ipc_patch_not_allowed");
    }
}
