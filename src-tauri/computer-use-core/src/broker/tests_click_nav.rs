//! Named broker contract tests migrated from u_rounds.rs.
use super::gates::{click_req, enabled_opts};
use super::*;
use crate::browser::{ExistingTabHost, RecordingBrowserWorker};
use crate::fake::FakeAdapter;
use std::io::{Read, Write};
use std::time::{Duration, Instant};

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

fn u1_yolo_never_grants_desktop() -> Result<(), String> {
    let (broker, fake, gen, obs) = ready()?;
    let mut req = click_req(
        "run-1",
        &fake.fixture_id(),
        gen,
        &obs.snapshot_id,
        obs.geometry_revision,
        "yolo-act",
    );
    req.parameters = serde_json::json!({"yolo": true, "acceptEdits": true});
    let out = broker.act(req);
    if out.executed || out.kind != crate::protocol::OutcomeKind::Rejected {
        return Err("yolo must not execute".into());
    }
    let reason = out.reason.unwrap_or_default();
    if !reason.contains("never grants desktop control") {
        return Err(format!("expected explicit yolo denial, got {reason}"));
    }
    if !fake.executions().is_empty() {
        return Err("adapter must not run under yolo params".into());
    }
    println!("gate: u1_yolo_never_grants_desktop");
    Ok(())
}

fn u2_click_button_reaches_adapter() -> Result<(), String> {
    let (broker, fake, gen, obs) = ready()?;
    let mut req = click_req(
        "run-1",
        &fake.fixture_id(),
        gen,
        &obs.snapshot_id,
        obs.geometry_revision,
        "right-click",
    );
    req.parameters = serde_json::json!({"button": "right", "count": 2});
    let out = broker.act(req);
    if !out.executed {
        return Err(format!("right-click must execute, got {:?}", out.reason));
    }
    if fake.last_click_button() != "right" {
        return Err(format!(
            "adapter button {}, expected right",
            fake.last_click_button()
        ));
    }
    if fake.last_click_count() != 2 {
        return Err(format!(
            "adapter count {}, expected 2",
            fake.last_click_count()
        ));
    }
    println!("gate: u2_click_button_reaches_adapter");
    Ok(())
}

fn managed_tab() -> Result<(ExistingTabHost, Arc<RecordingBrowserWorker>, String), String> {
    let host = ExistingTabHost::new();
    let root = std::env::temp_dir().join(format!("cu-u-nav-{}", Uuid::new_v4()));
    let worker = Arc::new(RecordingBrowserWorker::new(root.clone()));
    host.set_profile_root(root.join("profiles"));
    host.set_worker(worker.clone());
    let info = host
        .open_managed_profile("sess", "run-a", "p1")
        .map_err(|e| e.to_string())?;
    Ok((host, worker, info.tab_id))
}

fn u3_javascript_url_never_reaches_worker() -> Result<(), String> {
    let (host, worker, tab) = managed_tab()?;
    let err = host
        .navigate("run-a", &tab, "javascript:alert(1)", "nav-js")
        .err()
        .ok_or("javascript: must fail")?;
    if !err.to_string().contains("not allowed") {
        return Err(format!("javascript denial, got {err}"));
    }
    if !worker.gotos.lock().is_empty() {
        return Err("javascript: must not reach the managed worker".into());
    }
    println!("gate: u3_javascript_url_never_reaches_worker");
    Ok(())
}

fn u4_file_url_rejected() -> Result<(), String> {
    let (host, worker, tab) = managed_tab()?;
    for url in [
        "file:///C:/Windows/win.ini",
        "data:text/html,hi",
        "http://user:pass@example.test/",
        "",
    ] {
        if host.navigate("run-a", &tab, url, "nav-bad").is_ok() {
            return Err(format!("{url:?} must be rejected"));
        }
    }
    if !worker.gotos.lock().is_empty() {
        return Err("dangerous urls must not reach the worker".into());
    }
    host.navigate("run-a", &tab, "https://example.test/ok", "nav-ok")
        .map_err(|e| e.to_string())?;
    if worker.gotos.lock().is_empty() {
        return Err("https navigate must invoke the worker".into());
    }
    println!("gate: u4_file_url_rejected");
    Ok(())
}

fn ipc_raw(
    server: &crate::ipc::IpcServer,
    token: &str,
    host: &str,
    body: &str,
) -> Result<String, String> {
    let mut socket = std::net::TcpStream::connect(server.url.trim_start_matches("http://"))
        .map_err(|e| e.to_string())?;
    socket
        .set_read_timeout(Some(Duration::from_secs(2)))
        .map_err(|e| e.to_string())?;
    write!(
        socket,
        "POST /cu/tool HTTP/1.1\r\nHost: {host}\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
    .map_err(|e| e.to_string())?;
    let mut response = String::new();
    socket
        .read_to_string(&mut response)
        .map_err(|e| e.to_string())?;
    Ok(response)
}

fn u5_ipc_non_loopback_host_forbidden() -> Result<(), String> {
    if crate::ipc::host_is_loopback("127.0.0.1:9")
        && crate::ipc::host_is_loopback("localhost")
        && crate::ipc::host_is_loopback("[::1]:9")
        && !crate::ipc::host_is_loopback("evil.example")
        && !crate::ipc::host_is_loopback("192.168.1.8")
        && !crate::ipc::host_is_loopback("")
    {
    } else {
        return Err("host_is_loopback predicate failed".into());
    }
    let (server, token) = super::test_support::ipc_fixture()?;
    let body = r#"{"name":"computer_status"}"#;
    let denied = ipc_raw(&server, &token, "evil.example", body)?;
    if !denied.starts_with("HTTP/1.1 403") {
        return Err(format!("non-loopback Host must be 403, got {denied}"));
    }
    let ok = ipc_raw(&server, &token, "127.0.0.1", body)?;
    if !ok.starts_with("HTTP/1.1 200") {
        return Err(format!("loopback Host must be 200, got {ok}"));
    }
    println!("gate: u5_ipc_non_loopback_host_forbidden");
    Ok(())
}

fn u6_ipc_rate_limit_429() -> Result<(), String> {
    let now = Instant::now();
    let mut hits = Vec::new();
    for _ in 0..8 {
        if !crate::ipc::admit_rate(&mut hits, now, 8, Duration::from_millis(800)) {
            return Err("first 8 admits must succeed".into());
        }
    }
    if crate::ipc::admit_rate(&mut hits, now, 8, Duration::from_millis(800)) {
        return Err("9th admit in the window must fail".into());
    }
    let (server, token) = super::test_support::ipc_fixture()?;
    let client = reqwest::blocking::Client::new();
    let url = format!("{}/cu/tool", server.url);
    let mut saw_429 = false;
    for _ in 0..12 {
        let status = client
            .post(&url)
            .bearer_auth(&token)
            .json(&serde_json::json!({"name":"computer_status"}))
            .send()
            .map_err(|e| e.to_string())?
            .status()
            .as_u16();
        if status == 429 {
            saw_429 = true;
            break;
        }
        if status != 200 {
            return Err(format!("unexpected ipc status {status}"));
        }
    }
    if !saw_429 {
        return Err("token flood must return 429 rate limited".into());
    }
    println!("gate: u6_ipc_rate_limit_429");
    Ok(())
}

fn reserved_name_on_disk(root: &std::path::Path) -> Result<Option<String>, String> {
    if !root.exists() {
        return Ok(None);
    }
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = std::fs::read_dir(&dir).map_err(|e| e.to_string())?;
        for entry in entries {
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let name = path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or_default()
                .to_ascii_uppercase();
            let stem = name.split('.').next().unwrap_or(&name);
            if matches!(stem, "CON" | "PRN" | "AUX" | "NUL" | "COM1" | "LPT1") {
                return Ok(Some(path.display().to_string()));
            }
        }
    }
    Ok(None)
}

fn u7_reserved_download_name_rejected() -> Result<(), String> {
    let fake = Arc::new(FakeAdapter::new());
    let broker = ComputerUseBroker::new(fake, enabled_opts(400));
    broker
        .open_run("sess-a", "run-a")
        .map_err(|e| e.to_string())?;
    let root = std::env::temp_dir().join(format!("cu-u7-{}", Uuid::new_v4()));
    let worker = Arc::new(RecordingBrowserWorker::new(root.clone()));
    broker.tabs().set_profile_root(root.join("profiles"));
    broker.tabs().set_worker(worker.clone());
    broker.tabs().set_staging_root(root.clone());
    let tab = broker
        .tabs()
        .open_managed_profile("sess-a", "run-a", "downloads")
        .map_err(|e| e.to_string())?
        .tab_id;
    for name in ["NUL", "con.txt", "AUX", "COM1"] {
        match broker.browser_download("run-a", &tab, name, None, "dl-bad", "", None) {
            Err(_) => {}
            Ok(path) => {
                return Err(format!("reserved name {name} must reject, got {path:?}"));
            }
        }
        match broker
            .tabs()
            .stage_download_with_action("run-a", &tab, 1, "dl-stage", "", None, name)
        {
            Err(_) => {}
            Ok(path) => {
                return Err(format!(
                    "stage_download {name} must reject before the worker, got {path:?}"
                ));
            }
        }
    }
    if *worker.downloads.lock() != 0 {
        return Err("reserved download names must not reach the managed worker".into());
    }
    if let Some(landed) = reserved_name_on_disk(&root)? {
        return Err(format!("reserved download name landed on disk: {landed}"));
    }
    let obs = broker
        .tabs()
        .observe_managed("run-a", "downloads")
        .map_err(|e| e.to_string())?;
    let element = obs
        .nodes
        .iter()
        .find(|n| !n.element_ref.is_empty())
        .map(|n| n.element_ref.clone())
        .ok_or_else(|| "download fixture missing elementRef".to_string())?;
    let dest = broker
        .browser_download(
            "run-a",
            &tab,
            "report.bin",
            None,
            "dl-ok",
            &obs.snapshot_id,
            Some(element.as_str()),
        )
        .map_err(|e| e.to_string())?;
    if dest.file_name().and_then(|s| s.to_str()) != Some("report.bin") {
        return Err(format!("safe download must stage report.bin, got {dest:?}"));
    }
    if *worker.downloads.lock() != 1 {
        return Err("safe download must invoke the worker once".into());
    }
    if reserved_name_on_disk(&root)?.is_some() {
        return Err("safe download must not write a reserved name".into());
    }
    let _ = std::fs::remove_dir_all(root);
    println!("gate: u7_reserved_download_name_rejected");
    Ok(())
}

fn u8_authorize_while_in_flight_refused() -> Result<(), String> {
    let fake = Arc::new(FakeAdapter::new());
    fake.set_include_png(true);
    fake.set_hang(true);
    fake.set_abort_does_not_quiesce(true);
    let broker = ComputerUseBroker::new(fake.clone(), enabled_opts(400));
    broker.open_run("s", "run-1").map_err(|e| e.to_string())?;
    let tid = fake.fixture_id();
    let gen = broker
        .authorize_target("run-1", &tid)
        .map_err(|e| e.to_string())?;
    let obs = broker.observe("run-1").map_err(|e| e.to_string())?;
    let req = click_req(
        "run-1",
        &tid,
        gen,
        &obs.snapshot_id,
        obs.geometry_revision,
        "inflight",
    );
    let broker = Arc::new(broker);
    let pending = broker.clone();
    let worker = std::thread::spawn(move || pending.act(req));
    let until = Instant::now() + Duration::from_secs(2);
    while fake.adapter_in_flight() == 0 && Instant::now() < until {
        std::thread::yield_now();
    }
    if fake.adapter_in_flight() == 0 {
        return Err("expected in-flight adapter".into());
    }
    match broker.authorize_target("run-1", &tid) {
        Err(BrokerError::LeaseHeld { .. }) => {}
        other => {
            return Err(format!(
                "in-flight authorize must be LeaseHeld, got {other:?}"
            ))
        }
    }
    broker.request_stop("run-1").map_err(|e| e.to_string())?;
    fake.finish_in_flight();
    let out = worker
        .join()
        .map_err(|_| "act thread panicked".to_string())?;
    if out.kind == crate::protocol::OutcomeKind::Applied
        || out.kind == crate::protocol::OutcomeKind::Verified
    {
        return Err("late in-flight result must not apply".into());
    }
    println!("gate: u8_authorize_while_in_flight_refused");
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn yolo_never_grants_desktop() {
        super::u1_yolo_never_grants_desktop().expect("yolo_never_grants_desktop");
    }
    #[test]
    fn click_button_reaches_adapter() {
        super::u2_click_button_reaches_adapter().expect("click_button_reaches_adapter");
    }
    #[test]
    fn javascript_url_never_reaches_worker() {
        super::u3_javascript_url_never_reaches_worker()
            .expect("javascript_url_never_reaches_worker");
    }
    #[test]
    fn file_url_rejected() {
        super::u4_file_url_rejected().expect("file_url_rejected");
    }
    #[test]
    fn ipc_non_loopback_host_forbidden() {
        super::u5_ipc_non_loopback_host_forbidden().expect("ipc_non_loopback_host_forbidden");
    }
    #[test]
    fn ipc_rate_limit_429() {
        super::u6_ipc_rate_limit_429().expect("ipc_rate_limit_429");
    }
    #[test]
    fn reserved_download_name_rejected() {
        super::u7_reserved_download_name_rejected().expect("reserved_download_name_rejected");
    }
    #[test]
    fn authorize_while_in_flight_refused() {
        super::u8_authorize_while_in_flight_refused().expect("authorize_while_in_flight_refused");
    }
}
