//! Nav/upload/download + S75 gates. Probe-only.

use std::path::PathBuf;
use std::time::Duration;

use uuid::Uuid;

use grok_computer_use_core::browser::ManagedLocator;

use super::browser_supervisor::{
    leftover_browser_pids, live_browser_descendant_pids, live_browser_descendants, process_alive,
    test_node_exe, tracked_process_tree, BrowserSupervisor, SpawnRequest,
};
use super::browser_supervisor_gates::{node_named, read_http_path};
use super::ComputerUseBroker;

fn loc(element_ref: &str) -> ManagedLocator {
    ManagedLocator {
        element_ref: Some(element_ref.to_string()),
    }
}

pub fn run_nav_upload_download_gate() -> Result<(), String> {
    use grok_computer_use_core::fake::FakeAdapter;
    use grok_computer_use_core::ipc::SessionBinding;
    use std::io::Write;
    use std::net::TcpListener;
    use std::sync::Arc;

    let Some(node) = test_node_exe() else {
        println!("managed browser nav/upload/download: not_run (no node file)");
        return Ok(());
    };
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("tools")
        .join("computer-use-browser")
        .join("server.mjs");
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("tools")
        .join("computer-use-browser")
        .join("fixtures");
    let profile_root = std::env::temp_dir().join(format!(
        "grok-cu-s74-{}-{}",
        std::process::id(),
        Uuid::new_v4()
    ));
    let supervisor = BrowserSupervisor::spawn(SpawnRequest {
        node,
        script,
        profile_root: profile_root.clone(),
        browser: None,
    })?;
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
    let page_port = listener.local_addr().map_err(|e| e.to_string())?.port();
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let stop_t = stop.clone();
    let page_thread = std::thread::spawn(move || {
        while !stop_t.load(std::sync::atomic::Ordering::SeqCst) {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let path = read_http_path(&mut stream);
                    if path.starts_with("/redirect-ok") {
                        let loc = "HTTP/1.1 302 Found\r\nLocation: /form.html\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
                        let _ = stream.write_all(loc.as_bytes());
                        continue;
                    }
                    if path.starts_with("/file.bin") {
                        let body = b"staged-by-run";
                        let _ = stream.write_all(
                            b"HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Disposition: attachment; filename=\"report.bin\"\r\nContent-Length: 13\r\nConnection: close\r\n\r\n",
                        );
                        let _ = stream.write_all(body);
                        continue;
                    }
                    let file = fixtures.join("form.html");
                    let body = std::fs::read(&file).unwrap_or_default();
                    let header = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = stream.write_all(header.as_bytes());
                    let _ = stream.write_all(&body);
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(15));
                }
                Err(_) => break,
            }
        }
    });
    let fail = |stop: &Arc<std::sync::atomic::AtomicBool>, msg: String| {
        stop.store(true, std::sync::atomic::Ordering::SeqCst);
        msg
    };
    let fake = Arc::new(FakeAdapter::new());
    let broker = ComputerUseBroker::new(
        fake,
        grok_computer_use_core::broker::BrokerOptions {
            feature_enabled: true,
            lease_path: std::env::temp_dir().join(format!("grok-cu-s74-{}.lease", Uuid::new_v4())),
            ..grok_computer_use_core::broker::BrokerOptions::default()
        },
    );
    supervisor.attach(&broker)?;
    broker
        .open_run("s74", "run-a")
        .map_err(|e| fail(&stop, e.to_string()))?;
    let tab = broker
        .tabs()
        .open_managed_profile("s74", "run-a", "nav")
        .map_err(|e| fail(&stop, e.to_string()))?;
    if broker
        .browser_navigate("run-a", &tab.tab_id, "javascript:alert(1)", "s74-js")
        .is_ok()
    {
        return Err(fail(&stop, "javascript: must not navigate".into()));
    }
    if broker
        .browser_navigate(
            "run-a",
            &tab.tab_id,
            "file:///c:/windows/notepad.exe",
            "s74-file",
        )
        .is_ok()
    {
        return Err(fail(&stop, "file: must not navigate".into()));
    }
    if broker
        .browser_navigate(
            "run-a",
            &tab.tab_id,
            "https://user:pass@example.com/",
            "s74-cred",
        )
        .is_ok()
    {
        return Err(fail(&stop, "credentials must not navigate".into()));
    }
    let bounced = broker
        .browser_navigate(
            "run-a",
            &tab.tab_id,
            &format!("http://127.0.0.1:{page_port}/redirect-ok"),
            "s74-nav",
        )
        .map_err(|e| fail(&stop, e.to_string()))?;
    if !bounced.url.contains("form.html") {
        return Err(fail(
            &stop,
            format!("same-origin redirect not recorded: {}", bounced.url),
        ));
    }
    let dest = broker
        .browser_download(
            "run-a",
            &tab.tab_id,
            "report.bin",
            Some("C:\\\\temp\\\\evil.bin"),
            "s74-dl-evil",
            "",
            None,
        )
        .err();
    if dest.is_none() {
        return Err(fail(&stop, "model path must not download".into()));
    }
    let dl_obs = broker
        .tabs()
        .observe_managed("run-a", "nav")
        .map_err(|e| fail(&stop, e.to_string()))?;
    let dl_link = node_named(&dl_obs, "download", Some("link")).map_err(|e| fail(&stop, e))?;
    let dest = broker
        .browser_download(
            "run-a",
            &tab.tab_id,
            "report.bin",
            None,
            "s74-dl-ok",
            &dl_obs.snapshot_id,
            Some(dl_link.element_ref.as_str()),
        )
        .map_err(|e| fail(&stop, e.to_string()))?;
    let bytes = std::fs::read(&dest).map_err(|e| fail(&stop, e.to_string()))?;
    if bytes.as_slice() != b"staged-by-run" {
        return Err(fail(
            &stop,
            format!("download bytes {:?} at {}", bytes, dest.display()),
        ));
    }
    let upload = grok_computer_use_core::staging::staging_file(
        &profile_root.join(".staging"),
        "run-a",
        "note.txt",
    )
    .map_err(|e| fail(&stop, e.to_string()))?;
    std::fs::write(&upload, "hello-upload").map_err(|e| fail(&stop, e.to_string()))?;
    let file_obs = broker
        .tabs()
        .observe_managed("run-a", "nav")
        .map_err(|e| fail(&stop, e.to_string()))?;
    let file_input = file_obs
        .nodes
        .iter()
        .find(|node| {
            !node.truncated
                && node.role != "link"
                && node.name.to_ascii_lowercase().contains("file")
        })
        .ok_or_else(|| fail(&stop, format!("file input missing in {:?}", file_obs.nodes)))?;
    broker
        .browser_upload(
            "run-a",
            &tab.tab_id,
            "s74-up",
            &file_input.element_ref,
            &upload,
        )
        .map_err(|e| fail(&stop, e.to_string()))?;
    let uploaded_obs = broker
        .tabs()
        .observe_managed("run-a", "nav")
        .map_err(|e| fail(&stop, e.to_string()))?;
    let uploaded = format!(
        "{}{}",
        uploaded_obs.aria,
        uploaded_obs
            .nodes
            .iter()
            .map(|node| node.name.clone())
            .collect::<Vec<_>>()
            .join(" ")
    );
    if !uploaded.contains("note.txt") || !uploaded.contains("12") {
        return Err(fail(&stop, format!("upload postcondition {uploaded:?}")));
    }
    let evil = std::env::temp_dir().join(format!("cu-s74-evil-{}.txt", Uuid::new_v4()));
    std::fs::write(&evil, "nope").map_err(|e| fail(&stop, e.to_string()))?;
    if broker
        .browser_upload("run-a", &tab.tab_id, "s74-up-evil", "#file", &evil)
        .is_ok()
    {
        let _ = std::fs::remove_file(&evil);
        return Err(fail(&stop, "upload outside staging must fail".into()));
    }
    let _ = std::fs::remove_file(&evil);
    let binding = SessionBinding {
        session_id: "s74".into(),
        run_id: "run-a".into(),
    };
    let upload_tool = grok_computer_use_core::tools::dispatch(
        &broker,
        &binding,
        "computer_upload",
        serde_json::json!({"path": "C:\\\\temp\\\\x.bin"}),
    );
    if upload_tool.get("isError") != Some(&serde_json::json!(true)) {
        return Err(fail(&stop, "model upload tool must error".into()));
    }
    stop.store(true, std::sync::atomic::Ordering::SeqCst);
    let _ = supervisor.shutdown();
    let _ = page_thread.join();
    println!(
        "gate: managed_browser_nav_upload_download url={} bytes={} uploaded={uploaded}",
        bounced.url,
        bytes.len()
    );
    Ok(())
}

#[cfg(feature = "computer-use-probe")]
pub fn run_s75_integration_gate() -> Result<(), String> {
    use grok_computer_use_core::browser::ManagedLocator;
    use grok_computer_use_core::fake::FakeAdapter;
    use std::io::Write;
    use std::net::TcpListener;
    use std::sync::Arc;

    let Some(node) = test_node_exe() else {
        println!("managed browser s75: not_run (no node file)");
        return Ok(());
    };
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("tools")
        .join("computer-use-browser")
        .join("server.mjs");
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("tools")
        .join("computer-use-browser")
        .join("fixtures");
    let profile_root = std::env::temp_dir().join(format!(
        "grok-cu-s75-{}-{}",
        std::process::id(),
        Uuid::new_v4()
    ));
    let supervisor = BrowserSupervisor::spawn(SpawnRequest {
        node,
        script,
        profile_root: profile_root.clone(),
        browser: None,
    })?;
    let worker_pid = supervisor.pid();
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
    let page_port = listener.local_addr().map_err(|e| e.to_string())?.port();
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let ping_hits = Arc::new(std::sync::atomic::AtomicU32::new(0));
    let stop_t = stop.clone();
    let ping_t = ping_hits.clone();
    let page_thread = std::thread::spawn(move || {
        while !stop_t.load(std::sync::atomic::Ordering::SeqCst) {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let path = read_http_path(&mut stream);
                    if path.starts_with("/ping-hit") {
                        ping_t.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        let _ = stream.write_all(
                            b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                        );
                        continue;
                    }
                    if path.starts_with("/file.bin") {
                        let body = b"staged-by-run";
                        let _ = stream.write_all(
                            b"HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Disposition: attachment; filename=\"report.bin\"\r\nContent-Length: 13\r\nConnection: close\r\n\r\n",
                        );
                        let _ = stream.write_all(body);
                        continue;
                    }
                    let file = if path.starts_with("/frame") {
                        fixtures.join("frame.html")
                    } else if path.starts_with("/popup") {
                        fixtures.join("popup.html")
                    } else {
                        fixtures.join("form.html")
                    };
                    let body = std::fs::read(&file).unwrap_or_default();
                    let header = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = stream.write_all(header.as_bytes());
                    let _ = stream.write_all(&body);
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(15));
                }
                Err(_) => break,
            }
        }
    });
    let fail = |stop: &Arc<std::sync::atomic::AtomicBool>, msg: String| {
        stop.store(true, std::sync::atomic::Ordering::SeqCst);
        msg
    };
    let fake = Arc::new(FakeAdapter::new());
    let broker = ComputerUseBroker::new(
        fake,
        grok_computer_use_core::broker::BrokerOptions {
            feature_enabled: true,
            lease_path: std::env::temp_dir().join(format!("grok-cu-s75-{}.lease", Uuid::new_v4())),
            ..grok_computer_use_core::broker::BrokerOptions::default()
        },
    );
    supervisor.attach(&broker)?;
    broker
        .open_run("s75-a", "run-a")
        .map_err(|e| fail(&stop, e.to_string()))?;
    broker
        .open_run("s75-b", "run-b")
        .map_err(|e| fail(&stop, e.to_string()))?;
    let tab_a = broker
        .tabs()
        .open_managed_profile("s75-a", "run-a", "alice")
        .map_err(|e| fail(&stop, e.to_string()))?;
    let tab_b = broker
        .tabs()
        .open_managed_profile("s75-b", "run-b", "bob")
        .map_err(|e| fail(&stop, e.to_string()))?;
    let page = format!("http://127.0.0.1:{page_port}/form.html");
    broker
        .tabs()
        .navigate("run-a", &tab_a.tab_id, &page, "s75-nav-a")
        .map_err(|e| fail(&stop, e.to_string()))?;
    broker
        .tabs()
        .navigate("run-b", &tab_b.tab_id, &page, "s75-nav-b")
        .map_err(|e| fail(&stop, e.to_string()))?;
    let fill = |run: &str, profile: &str, action_id: &str, text: &str| -> Result<(), String> {
        let obs = broker
            .tabs()
            .observe_managed(run, profile)
            .map_err(|e| e.to_string())?;
        let name_box = node_named(&obs, "Name", Some("textbox"))?;
        broker
            .browser_act_managed(
                run,
                profile,
                action_id,
                "set_value",
                loc(&name_box.element_ref),
                serde_json::json!({ "text": text }),
            )
            .map_err(|e| e.to_string())
    };
    let submit = |run: &str, profile: &str, action_id: &str| -> Result<(), String> {
        let obs = broker
            .tabs()
            .observe_managed(run, profile)
            .map_err(|e| e.to_string())?;
        let button = node_named(&obs, "Submit", Some("button"))?;
        broker
            .browser_act_managed(
                run,
                profile,
                action_id,
                "click",
                loc(&button.element_ref),
                serde_json::json!({}),
            )
            .map_err(|e| e.to_string())
    };
    fill("run-a", "alice", "s75-fill-a", "AAA").map_err(|e| fail(&stop, e))?;
    submit("run-a", "alice", "s75-sub-a").map_err(|e| fail(&stop, e))?;
    fill("run-b", "bob", "s75-fill-b", "BBB").map_err(|e| fail(&stop, e))?;
    submit("run-b", "bob", "s75-sub-b").map_err(|e| fail(&stop, e))?;
    let a_obs = broker
        .tabs()
        .observe_managed("run-a", "alice")
        .map_err(|e| fail(&stop, e.to_string()))?;
    let b_obs = broker
        .tabs()
        .observe_managed("run-b", "bob")
        .map_err(|e| fail(&stop, e.to_string()))?;
    let a_result = format!(
        "{}{}",
        a_obs.aria,
        a_obs
            .nodes
            .iter()
            .map(|node| node.name.clone())
            .collect::<Vec<_>>()
            .join(" ")
    );
    let b_result = format!(
        "{}{}",
        b_obs.aria,
        b_obs
            .nodes
            .iter()
            .map(|node| node.name.clone())
            .collect::<Vec<_>>()
            .join(" ")
    );
    if !a_result.contains("AAA") || a_result.contains("BBB") {
        return Err(fail(&stop, format!("alice isolation {a_result}")));
    }
    if !b_result.contains("BBB") || b_result.contains("AAA") {
        return Err(fail(&stop, format!("bob isolation {b_result}")));
    }
    submit("run-a", "alice", "s75-sub-a2").map_err(|e| fail(&stop, e))?;
    let a_again_obs = broker
        .tabs()
        .observe_managed("run-a", "alice")
        .map_err(|e| fail(&stop, e.to_string()))?;
    let a_again = format!(
        "{}{}",
        a_again_obs.aria,
        a_again_obs
            .nodes
            .iter()
            .map(|node| node.name.clone())
            .collect::<Vec<_>>()
            .join(" ")
    );
    if a_again.contains("BBB") {
        return Err(fail(&stop, format!("duplicate submit {a_again}")));
    }
    let frames = broker
        .tabs()
        .list_managed_frames("run-a", "alice")
        .map_err(|e| fail(&stop, e.to_string()))?;
    if !frames.iter().any(|u| u.contains("frame")) {
        return Err(fail(&stop, format!("iframe missing {frames:?}")));
    }
    let pop_obs = broker
        .tabs()
        .observe_managed("run-a", "alice")
        .map_err(|e| fail(&stop, e.to_string()))?;
    let pop_btn = node_named(&pop_obs, "popup", Some("button")).map_err(|e| fail(&stop, e))?;
    let popup = broker
        .browser_popup("run-a", "alice", "s75-pop", &pop_btn.element_ref)
        .map_err(|e| fail(&stop, e.to_string()))?;
    if !popup.popup || !popup.url.contains("popup") || popup.url.contains("about:blank") {
        return Err(fail(&stop, format!("popup missing {popup:?}")));
    }
    let dl_obs = broker
        .tabs()
        .observe_managed("run-a", "alice")
        .map_err(|e| fail(&stop, e.to_string()))?;
    let dl_link = node_named(&dl_obs, "download", Some("link")).map_err(|e| fail(&stop, e))?;
    let dl = broker
        .browser_download(
            "run-a",
            &tab_a.tab_id,
            "report.bin",
            None,
            "s75-dl",
            &dl_obs.snapshot_id,
            Some(dl_link.element_ref.as_str()),
        )
        .map_err(|e| fail(&stop, e.to_string()))?;
    let dl_bytes = std::fs::read(&dl).map_err(|e| fail(&stop, e.to_string()))?;
    if dl_bytes.as_slice() != b"staged-by-run" {
        return Err(fail(&stop, format!("download bytes {dl_bytes:?}")));
    }
    let reported = supervisor.browser_pids().unwrap_or_default();
    let browsers = live_browser_descendants(worker_pid);
    if browsers.is_empty() {
        return Err(fail(
            &stop,
            format!("expected chrome descendants of worker {worker_pid}, pids={reported:?}"),
        ));
    }
    let mut chrome: Vec<u32> = reported;
    chrome.extend(browsers.iter().map(|(pid, _)| *pid));
    chrome.push(worker_pid);
    chrome = tracked_process_tree(&chrome);
    let slow_out = std::thread::scope(|scope| {
        let slow = scope.spawn(|| {
            let obs = broker.tabs().observe_managed("run-b", "bob")?;
            let ping = node_named(&obs, "ping", Some("button"))
                .map_err(grok_computer_use_core::error::BrokerError::Schema)?;
            broker.browser_act_managed(
                "run-b",
                "bob",
                "s75-hang",
                "click",
                loc(&ping.element_ref),
                serde_json::json!({}),
            )
        });
        std::thread::sleep(Duration::from_millis(200));
        let _ = broker.request_stop("run-b");
        let _ = broker.tabs().cancel_run("run-b");
        slow.join()
    });
    let slow_out = match slow_out {
        Ok(result) => result,
        Err(_) => return Err(fail(&stop, "slow thread panic".into())),
    };
    if slow_out.is_ok() {
        return Err(fail(
            &stop,
            "slow click must not finish after cancel".into(),
        ));
    }
    std::thread::sleep(Duration::from_millis(400));
    let hits = ping_hits.load(std::sync::atomic::Ordering::SeqCst);
    if hits != 0 {
        return Err(fail(
            &stop,
            format!("cancel still pinged server hits={hits}"),
        ));
    }
    let pings_closed = broker.tabs().is_closed(&tab_b.tab_id);
    if !pings_closed {
        return Err(fail(&stop, "cancel must close managed bob tab".into()));
    }
    let a_after_obs = broker
        .tabs()
        .observe_managed("run-a", "alice")
        .map_err(|e| fail(&stop, e.to_string()))?;
    let a_after = format!(
        "{}{}",
        a_after_obs.aria,
        a_after_obs
            .nodes
            .iter()
            .map(|node| node.name.clone())
            .collect::<Vec<_>>()
            .join(" ")
    );
    if !a_after.contains("AAA") || a_after.contains("BBB") {
        return Err(fail(
            &stop,
            format!("alice must survive bob cancel {a_after}"),
        ));
    }
    chrome.extend(live_browser_descendant_pids(worker_pid));
    if let Ok(live) = supervisor.browser_pids() {
        chrome.extend(live);
    }
    chrome = tracked_process_tree(&chrome);
    let _ = reqwest::blocking::Client::new()
        .post(format!("{}/crash", supervisor.base_url()))
        .header("Authorization", format!("Bearer {}", supervisor.token()))
        .timeout(Duration::from_secs(2))
        .json(&serde_json::json!({}))
        .send();
    std::thread::sleep(Duration::from_millis(150));
    supervisor
        .shutdown_with(&chrome)
        .map_err(|e| fail(&stop, e.to_string()))?;
    std::thread::sleep(Duration::from_millis(200));
    if process_alive(worker_pid) {
        return Err(fail(&stop, format!("worker {worker_pid} still alive")));
    }
    let leftover = leftover_browser_pids(&chrome);
    if !leftover.is_empty() {
        return Err(fail(&stop, format!("leftover browsers {leftover:?}")));
    }
    for pid in &chrome {
        if process_alive(*pid) {
            return Err(fail(&stop, format!("chrome/worker pid {pid} still alive")));
        }
    }
    stop.store(true, std::sync::atomic::Ordering::SeqCst);
    let _ = page_thread.join();
    println!(
        "gate: managed_browser_s75_integration alice={a_result} bob={b_result} dl={} popup={} browsers={} ping_hits={hits} leftover=0",
        dl_bytes.len(),
        popup.url,
        browsers.len()
    );
    Ok(())
}
