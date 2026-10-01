//! Probe-only managed-browser gates. Compiled only with computer-use-probe.

use std::path::{Path, PathBuf};
use std::time::Duration;

use uuid::Uuid;

use grok_computer_use_core::browser::{ManagedLocator, ManagedObservation};

use super::browser_supervisor::{
    leftover_browser_pids, live_browser_descendant_pids, live_browser_descendants, process_alive,
    test_node_exe, tracked_process_tree, BrowserSupervisor, SpawnRequest,
};
use super::ComputerUseBroker;

fn loc(element_ref: &str) -> ManagedLocator {
    ManagedLocator {
        element_ref: Some(element_ref.to_string()),
    }
}

pub(super) fn node_named<'a>(
    obs: &'a ManagedObservation,
    name: &str,
    role: Option<&str>,
) -> Result<&'a grok_computer_use_core::browser::ManagedNode, String> {
    let needle = name
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase();
    obs.nodes
        .iter()
        .find(|node| {
            if node.truncated {
                return false;
            }
            if let Some(role) = role {
                if node.role != role {
                    return false;
                }
            }
            let node_name = node
                .name
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .to_ascii_lowercase();
            node_name == needle || node_name.contains(&needle)
        })
        .ok_or_else(|| format!("missing node {name} in {:?}", obs.nodes))
}

#[cfg(feature = "computer-use-probe")]
pub(super) fn read_http_path(stream: &mut std::net::TcpStream) -> String {
    use std::io::Read;
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(Duration::from_millis(1500)));
    let mut buf = Vec::new();
    let mut tmp = [0u8; 512];
    for _ in 0..32 {
        match stream.read(&mut tmp) {
            Ok(0) => break,
            Ok(n) => {
                buf.extend_from_slice(&tmp[..n]);
                if buf.windows(4).any(|w| w == b"\r\n\r\n") || buf.len() > 8192 {
                    break;
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(_) => break,
        }
    }
    let req = String::from_utf8_lossy(&buf);
    req.lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .unwrap_or("/")
        .to_string()
}

#[cfg(feature = "computer-use-probe")]
pub fn run_profile_tab_gate() -> Result<(), String> {
    use grok_computer_use_core::fake::FakeAdapter;
    use grok_computer_use_core::ipc::SessionBinding;
    use std::io::Write;
    use std::net::TcpListener;
    use std::sync::Arc;

    let Some(node) = test_node_exe() else {
        println!("managed browser profiles: not_run (no node file)");
        return Ok(());
    };
    let chrome_ok = [
        r"C:\Program Files\Google\Chrome\Application\chrome.exe",
        r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe",
        r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
    ]
    .iter()
    .any(|p| Path::new(p).is_file());
    if !chrome_ok {
        println!("managed browser profiles: not_run (no Chrome/Edge)");
        return Ok(());
    }
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
        "grok-cu-s72-{}-{}",
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
                    let file = if path.starts_with("/frame") {
                        fixtures.join("frame.html")
                    } else if path.starts_with("/popup") {
                        fixtures.join("popup.html")
                    } else if path.starts_with("/file.bin") {
                        let body = b"staged-by-run";
                        let _ = stream.write_all(
                            b"HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Length: 13\r\nConnection: close\r\n\r\n",
                        );
                        let _ = stream.write_all(body);
                        continue;
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

    let fake = Arc::new(FakeAdapter::new());
    let broker = ComputerUseBroker::new(
        fake,
        grok_computer_use_core::broker::BrokerOptions {
            feature_enabled: true,
            lease_path: std::env::temp_dir().join(format!("grok-cu-s72-{}.lease", Uuid::new_v4())),
            ..grok_computer_use_core::broker::BrokerOptions::default()
        },
    );
    supervisor.attach(&broker)?;
    broker
        .open_run("s72-a", "run-a")
        .map_err(|e| e.to_string())?;
    broker
        .open_run("s72-b", "run-b")
        .map_err(|e| e.to_string())?;
    let opened = broker
        .tabs()
        .open_managed_profile("s72-a", "run-a", "alice")
        .map_err(|e| e.to_string())?;
    let alice_dir = profile_root.join("alice");
    if !alice_dir.is_dir() {
        stop.store(true, std::sync::atomic::Ordering::SeqCst);
        return Err("alice profile dir missing".into());
    }
    let disk_files = std::fs::read_dir(&alice_dir)
        .map_err(|e| e.to_string())?
        .flatten()
        .count();
    if disk_files == 0 {
        stop.store(true, std::sync::atomic::Ordering::SeqCst);
        return Err("alice profile dir is empty (Chrome did not persist)".into());
    }
    if broker
        .tabs()
        .open_managed_profile("s72-b", "run-b", "alice")
        .is_ok()
    {
        stop.store(true, std::sync::atomic::Ordering::SeqCst);
        return Err("run-b must not steal alice".into());
    }
    let page = format!("http://127.0.0.1:{page_port}/form.html");
    let moved = broker
        .tabs()
        .navigate("run-a", &opened.tab_id, &page, "s72-nav")
        .map_err(|e| e.to_string())?;
    if !moved.url.contains("127.0.0.1") {
        stop.store(true, std::sync::atomic::Ordering::SeqCst);
        return Err(format!("actual url not rechecked: {}", moved.url));
    }
    let extra = broker
        .tabs()
        .new_managed_tab("run-a", "alice", "s72-tab")
        .map_err(|e| e.to_string())?;
    if extra.url.is_empty() {
        stop.store(true, std::sync::atomic::Ordering::SeqCst);
        return Err("new tab returned empty url".into());
    }
    let pages = broker
        .tabs()
        .list_managed_pages("run-a", "alice")
        .map_err(|e| e.to_string())?;
    if pages.len() < 2 {
        stop.store(true, std::sync::atomic::Ordering::SeqCst);
        return Err(format!("expected >=2 pages, got {}", pages.len()));
    }
    let frames = broker
        .tabs()
        .list_managed_frames("run-a", "alice")
        .map_err(|e| e.to_string())?;
    if !frames.iter().any(|u| u.contains("frame")) {
        stop.store(true, std::sync::atomic::Ordering::SeqCst);
        return Err(format!("iframe missing in {frames:?}"));
    }
    let pop_obs = broker
        .tabs()
        .observe_managed("run-a", "alice")
        .map_err(|e| e.to_string())?;
    let pop_btn = node_named(&pop_obs, "popup", Some("button"))?;
    let popup = broker
        .browser_popup("run-a", "alice", "s72-pop", &pop_btn.element_ref)
        .map_err(|e| e.to_string())?;
    if !popup.popup || !popup.url.contains("popup") {
        stop.store(true, std::sync::atomic::Ordering::SeqCst);
        return Err(format!("popup not opened: {popup:?}"));
    }
    let secret = format!("s72-secret-{}", Uuid::new_v4());
    let pages = broker
        .tabs()
        .list_managed_pages("run-a", "alice")
        .map_err(|e| e.to_string())?;
    let live = pages
        .iter()
        .find(|p| !p.popup)
        .or(pages.first())
        .ok_or_else(|| "marker has no live page".to_string())?;
    let marker = reqwest::blocking::Client::new()
        .post(format!("{}/marker", supervisor.base_url()))
        .header("Authorization", format!("Bearer {}", supervisor.token()))
        .json(&serde_json::json!({
            "owner":"run-a",
            "profile":"alice",
            "set":secret,
            "pageId": live.page_id,
            "pageGeneration": live.page_generation
        }))
        .send()
        .map_err(|e| e.to_string())?;
    if !marker.status().is_success() {
        stop.store(true, std::sync::atomic::Ordering::SeqCst);
        return Err(format!("marker {}", marker.status()));
    }
    let binding = SessionBinding {
        session_id: "s72-a".into(),
        run_id: "run-a".into(),
    };
    let status = grok_computer_use_core::tools::dispatch(
        &broker,
        &binding,
        "computer_status",
        serde_json::json!({}),
    );
    let dump = status.to_string();
    if dump.contains(&secret) {
        stop.store(true, std::sync::atomic::Ordering::SeqCst);
        return Err("profile storage leaked into model status".into());
    }
    let clear_tool = grok_computer_use_core::tools::dispatch(
        &broker,
        &binding,
        "computer_clear_profile",
        serde_json::json!({"profile":"alice"}),
    );
    if clear_tool.get("isError") != Some(&serde_json::json!(true)) {
        stop.store(true, std::sync::atomic::Ordering::SeqCst);
        return Err("model must not clear profiles".into());
    }
    let _ = broker.tabs().cancel_run("run-a");
    broker
        .tabs()
        .clear_managed_profile("alice")
        .map_err(|e| e.to_string())?;
    if alice_dir.exists() {
        stop.store(true, std::sync::atomic::Ordering::SeqCst);
        return Err("host clear must remove the profile directory".into());
    }
    stop.store(true, std::sync::atomic::Ordering::SeqCst);
    let _ = supervisor.shutdown();
    let _ = page_thread.join();
    println!("gate: managed_browser_profile_tabs dir_files={disk_files}");
    Ok(())
}

#[cfg(feature = "computer-use-probe")]
pub fn run_observe_act_gate() -> Result<(), String> {
    use grok_computer_use_core::browser::ManagedLocator;
    use grok_computer_use_core::fake::FakeAdapter;
    use grok_computer_use_core::ipc::SessionBinding;
    use std::io::Write;
    use std::net::TcpListener;
    use std::sync::Arc;

    let Some(node) = test_node_exe() else {
        println!("managed browser observe/act: not_run (no node file)");
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
        "grok-cu-s73-{}-{}",
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
            lease_path: std::env::temp_dir().join(format!("grok-cu-s73-{}.lease", Uuid::new_v4())),
            ..grok_computer_use_core::broker::BrokerOptions::default()
        },
    );
    supervisor.attach(&broker)?;
    broker
        .open_run("s73", "run-a")
        .map_err(|e| fail(&stop, e.to_string()))?;
    let opened = broker
        .tabs()
        .open_managed_profile("s73", "run-a", "act")
        .map_err(|e| fail(&stop, e.to_string()))?;
    let page = format!("http://127.0.0.1:{page_port}/form.html");
    broker
        .tabs()
        .navigate("run-a", &opened.tab_id, &page, "s73-nav")
        .map_err(|e| fail(&stop, e.to_string()))?;
    let obs = broker
        .tabs()
        .observe_managed("run-a", "act")
        .map_err(|e| fail(&stop, e.to_string()))?;
    let dump = format!(
        "{}{}",
        obs.aria,
        obs.nodes
            .iter()
            .map(|node| format!("{} {}", node.role, node.name))
            .collect::<Vec<_>>()
            .join(" ")
    );
    if !dump.contains("+1") && !dump.to_ascii_lowercase().contains("button") {
        return Err(fail(&stop, format!("ARIA/roles missing buttons: {dump}")));
    }
    let plus = node_named(&obs, "+1", Some("button")).map_err(|e| fail(&stop, e))?;
    broker
        .browser_act_managed(
            "run-a",
            "act",
            "s73-click-inc",
            "click",
            loc(&plus.element_ref),
            serde_json::json!({}),
        )
        .map_err(|e| fail(&stop, e.to_string()))?;
    let after_click = broker
        .tabs()
        .observe_managed("run-a", "act")
        .map_err(|e| fail(&stop, e.to_string()))?;
    let count_node = node_named(&after_click, "1", None).map_err(|e| fail(&stop, e))?;
    broker
        .browser_act_managed(
            "run-a",
            "act",
            "s73-wait",
            "wait",
            loc(&count_node.element_ref),
            serde_json::json!({"nameEquals": "1", "timeoutMs": 4000}),
        )
        .map_err(|e| fail(&stop, e.to_string()))?;
    let name_obs = broker
        .tabs()
        .observe_managed("run-a", "act")
        .map_err(|e| fail(&stop, e.to_string()))?;
    let name_box = node_named(&name_obs, "Name", Some("textbox")).map_err(|e| fail(&stop, e))?;
    broker
        .browser_act_managed(
            "run-a",
            "act",
            "s73-fill",
            "set_value",
            loc(&name_box.element_ref),
            serde_json::json!({"text": "你好世界"}),
        )
        .map_err(|e| fail(&stop, e.to_string()))?;
    let submit_obs = broker
        .tabs()
        .observe_managed("run-a", "act")
        .map_err(|e| fail(&stop, e.to_string()))?;
    let submit = node_named(&submit_obs, "Submit", Some("button")).map_err(|e| fail(&stop, e))?;
    broker
        .browser_act_managed(
            "run-a",
            "act",
            "s73-submit",
            "click",
            loc(&submit.element_ref),
            serde_json::json!({}),
        )
        .map_err(|e| fail(&stop, e.to_string()))?;
    let result_obs = broker
        .tabs()
        .observe_managed("run-a", "act")
        .map_err(|e| fail(&stop, e.to_string()))?;
    let result = format!(
        "{}{}",
        result_obs.aria,
        result_obs
            .nodes
            .iter()
            .map(|node| node.name.clone())
            .collect::<Vec<_>>()
            .join(" ")
    );
    if !result.contains("你好世界") {
        return Err(fail(&stop, format!("submit postcondition {result:?}")));
    }
    let color = node_named(&result_obs, "color", Some("combobox")).map_err(|e| fail(&stop, e))?;
    broker
        .browser_act_managed(
            "run-a",
            "act",
            "s73-select",
            "select",
            loc(&color.element_ref),
            serde_json::json!({"value": "blue"}),
        )
        .map_err(|e| fail(&stop, e.to_string()))?;
    let space = broker
        .tabs()
        .observe_managed("run-a", "act")
        .map_err(|e| fail(&stop, e.to_string()))?;
    let scroller = node_named(&space, "scroller", None).map_err(|e| fail(&stop, e))?;
    broker
        .browser_act_managed(
            "run-a",
            "act",
            "s73-scroll",
            "scroll",
            loc(&scroller.element_ref),
            serde_json::json!({"delta": 400}),
        )
        .map_err(|e| fail(&stop, e.to_string()))?;
    let drag_obs = broker
        .tabs()
        .observe_managed("run-a", "act")
        .map_err(|e| fail(&stop, e.to_string()))?;
    let drag = node_named(&drag_obs, "Drag", None).map_err(|e| fail(&stop, e))?;
    let drop = node_named(&drag_obs, "Drop", None).map_err(|e| fail(&stop, e))?;
    broker
        .browser_act_managed(
            "run-a",
            "act",
            "s73-drag",
            "drag",
            loc(&drag.element_ref),
            serde_json::json!({"destElementRef": drop.element_ref}),
        )
        .map_err(|e| fail(&stop, e.to_string()))?;
    broker
        .browser_act_managed(
            "run-a",
            "act",
            "s73-key",
            "key",
            ManagedLocator::default(),
            serde_json::json!({"key": "tab"}),
        )
        .map_err(|e| fail(&stop, e.to_string()))?;
    match broker.browser_act_managed(
        "run-a",
        "act",
        "s73-eval",
        "evaluate",
        ManagedLocator::default(),
        serde_json::json!({"script": "1+1"}),
    ) {
        Err(e) if e.to_string().contains("evaluate") => {}
        other => {
            return Err(fail(&stop, format!("evaluate must fail, got {other:?}")));
        }
    }
    let binding = SessionBinding {
        session_id: "s73".into(),
        run_id: "run-a".into(),
    };
    let eval_tool = grok_computer_use_core::tools::dispatch(
        &broker,
        &binding,
        "browser_run_code_unsafe",
        serde_json::json!({"script": "1"}),
    );
    if eval_tool.get("isError") != Some(&serde_json::json!(true)) {
        return Err(fail(&stop, "model evaluate tool must error".into()));
    }
    stop.store(true, std::sync::atomic::Ordering::SeqCst);
    let _ = supervisor.shutdown();
    let _ = page_thread.join();
    println!("gate: managed_browser_observe_act result={result}");
    Ok(())
}
