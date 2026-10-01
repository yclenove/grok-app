//! D8 fault matrix (20 classes ×5). Probe-only.

use super::d8::{
    append_jsonl, click_req, ipc_call, leaked, record, sample_pid, FaultRow, ResourceRow, FAULT_N,
    OWNER, ROUNDS,
};
use grok_computer_use_core::adapter::ComputerUseAdapter;
use grok_computer_use_core::broker::{BrokerOptions, ComputerUseBroker};
use grok_computer_use_core::error::BrokerError;
use grok_computer_use_core::fake::FakeAdapter;
use grok_computer_use_core::ipc;
use grok_computer_use_core::privacy::{admit_cleanup_path, write_owner};
use grok_computer_use_core::protocol::OutcomeKind;
use grok_computer_use_core::runtime::{NewComponent, RuntimeStore, JS_RUNTIME};
use serde_json::json;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::browser_supervisor::{
    live_browser_descendant_pids, process_alive, BrowserSupervisor, SpawnRequest,
};

pub(super) fn run_faults(
    out: &Path,
    home: &Path,
    resources: &mut Vec<ResourceRow>,
) -> Result<(), String> {
    let path = out.join("d8-faults.jsonl");
    let _ = fs::remove_file(&path);
    let mut rows = Vec::new();
    for class in [
        "node_spawn",
        "handshake",
        "worker_kill",
        "chromium_kill",
        "tab_close",
        "nav_during_act",
        "stale_snapshot",
        "malformed_mcp",
        "bearer",
        "loopback_reset",
        "session_switch",
        "pause_inflight",
        "extension_disconnect",
        "existing_tab_nav",
        "webview_destroy",
        "runtime_corrupt",
        "repair_interrupt",
        "trace_corrupt",
        "staging_denied",
        "cleanup_junction",
    ] {
        for n in 1..=FAULT_N {
            let row = fault_once(class, n, home, resources)?;
            if leaked(&row.code) || row.leaked {
                return Err(format!("fault {class}#{n} leaked"));
            }
            if !row.ok {
                append_jsonl(&path, &serde_json::to_value(&row).unwrap_or(json!({})))?;
                return Err(format!(
                    "fault {class}#{n} failed code={} executed={}",
                    row.code, row.executed
                ));
            }
            append_jsonl(&path, &serde_json::to_value(&row).unwrap_or(json!({})))?;
            rows.push(row);
        }
        println!("gate: d8_fault {class} n={FAULT_N}");
    }
    if rows.len() != 20 * FAULT_N {
        return Err(format!(
            "expected {} fault rows, got {}",
            20 * FAULT_N,
            rows.len()
        ));
    }
    Ok(())
}

fn fault_once(
    class: &str,
    n: usize,
    home: &Path,
    resources: &mut Vec<ResourceRow>,
) -> Result<FaultRow, String> {
    match class {
        "node_spawn" => {
            let err = BrowserSupervisor::spawn(SpawnRequest {
                node: home.join("missing-node.exe"),
                script: home.join("missing.mjs"),
                profile_root: home.join("p"),
                browser: None,
            })
            .err()
            .unwrap_or_default();
            Ok(ok_row(
                class,
                n,
                "js-runtime_missing",
                err.contains("js-runtime missing") || err.contains("missing"),
                false,
                leaked(&err),
            ))
        }
        "handshake" => handshake_fault(class, n, home),
        "worker_kill" => worker_kill_fault(class, n, home, resources),
        "chromium_kill" => chromium_kill_fault(class, n, home, resources),
        "tab_close" => broker_fault(class, n, |b, fake| {
            let _ = b.observe("run");
            let (tid, gen, snap, geo) = match b.act_defaults("run") {
                Ok(v) => v,
                Err(e) => return (e.code().into(), false, false),
            };
            fake.set_alive(false);
            let out = b.act(click_req("run", &tid, gen, &snap, geo, "dead"));
            (
                BrokerError::DeadTarget.code().into(),
                !out.executed && out.kind == OutcomeKind::Rejected,
                out.executed,
            )
        }),
        "nav_during_act" => broker_fault(class, n, |b, fake| {
            fake.set_stale_observe_revision(true);
            let obs = b.observe("run");
            (
                "identity_mismatch".into(),
                obs.is_err()
                    && obs
                        .as_ref()
                        .err()
                        .is_some_and(|e| e.code() == "identity_mismatch" || e.code() == "schema"),
                false,
            )
        }),
        "stale_snapshot" => broker_fault(class, n, |b, fake| {
            let obs = b.observe("run").expect("obs");
            let out = b.act(click_req(
                "run",
                &fake.fixture_id(),
                1,
                "stale-snap",
                obs.geometry_revision,
                &format!("stale-{n}"),
            ));
            (
                "identity_mismatch".into(),
                !out.executed && out.kind == OutcomeKind::Rejected,
                out.executed,
            )
        }),
        "malformed_mcp" => malformed_mcp_fault(class, n),
        "bearer" => bearer_fault(class, n),
        "loopback_reset" => loopback_reset_fault(class, n),
        "session_switch" => session_switch_fault(class, n),
        "pause_inflight" => pause_inflight_fault(class, n),
        "extension_disconnect" => extension_fault(class, n),
        "existing_tab_nav" => existing_tab_nav_fault(class, n),
        "webview_destroy" => webview_destroy_fault(class, n),
        "runtime_corrupt" => runtime_corrupt_fault(class, n, home),
        "repair_interrupt" => repair_interrupt_fault(class, n, home),
        "trace_corrupt" => trace_corrupt_fault(class, n, home),
        "staging_denied" => staging_denied_fault(class, n, home),
        "cleanup_junction" => cleanup_junction_fault(class, n, home),
        other => Err(format!("unknown fault class {other}")),
    }
}

fn ok_row(class: &str, n: usize, code: &str, ok: bool, executed: bool, leaked: bool) -> FaultRow {
    FaultRow {
        class: class.into(),
        n,
        code: code.into(),
        ok: ok && !executed && !leaked,
        executed,
        leaked,
    }
}

fn broker_fault(
    class: &str,
    n: usize,
    f: impl FnOnce(&ComputerUseBroker, &FakeAdapter) -> (String, bool, bool),
) -> Result<FaultRow, String> {
    let fake = Arc::new(FakeAdapter::new());
    let broker = ComputerUseBroker::new(
        fake.clone(),
        BrokerOptions {
            feature_enabled: true,
            lease_path: std::env::temp_dir().join(format!("cu-d8-f-{class}-{n}.lease")),
            ..BrokerOptions::default()
        },
    );
    broker.open_run("s", "run").map_err(|e| e.to_string())?;
    broker
        .authorize_target("run", &fake.fixture_id())
        .map_err(|e| e.to_string())?;
    let (code, ok, executed) = f(&broker, &fake);
    Ok(ok_row(class, n, &code, ok, executed, leaked(&code)))
}

fn handshake_fault(class: &str, n: usize, home: &Path) -> Result<FaultRow, String> {
    let script = home.join(format!("bad-handshake-{n}.mjs"));
    // First line is not the worker port JSON. Shipped read_port fails closed.
    fs::write(
        &script,
        "console.log('not-a-port-json');\nsetInterval(() => {}, 30000);\n",
    )
    .map_err(|e| e.to_string())?;
    let node = packaged_node();
    if !node.is_file() {
        return Ok(ok_row(class, n, "js-runtime_missing", false, false, false));
    }
    let err = BrowserSupervisor::spawn(SpawnRequest {
        node,
        script,
        profile_root: home.join(format!("hang-p-{n}")),
        browser: Some(packaged_chromium()),
    })
    .err()
    .unwrap_or_default();
    Ok(ok_row(
        class,
        n,
        "handshake_failed",
        err.contains("port") || err.contains("timeout") || err.contains("json"),
        false,
        leaked(&err),
    ))
}

fn packaged_node() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("resources")
        .join("computer-use")
        .join("seed")
        .join("bin")
        .join("node.exe")
}

fn packaged_chromium() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("resources")
        .join("computer-use")
        .join("seed")
        .join("chromium")
        .join("chrome-win")
        .join("chrome.exe")
}

fn write_mini_worker(home: &Path, n: usize) -> Result<PathBuf, String> {
    let script = home.join(format!("mini-worker-{n}.mjs"));
    fs::write(
        &script,
        r#"
import http from "node:http";
const server = http.createServer((req, res) => {
  if ((req.url || "").startsWith("/health") || (req.url || "").startsWith("/pids")) {
    res.writeHead(200, { "content-type": "application/json" });
    res.end(JSON.stringify({
      ok: true,
      protocol: 1,
      build: "d8-mini",
      runtime: process.version,
      workerPid: process.pid,
      browserPids: [],
      page: { observe: true },
    }));
    return;
  }
  res.writeHead(404);
  res.end();
});
server.listen(0, "127.0.0.1", () => {
  const addr = server.address();
  process.stdout.write(JSON.stringify({ port: addr.port }) + "\n");
});
"#,
    )
    .map_err(|e| e.to_string())?;
    Ok(script)
}

fn worker_kill_fault(
    class: &str,
    n: usize,
    home: &Path,
    resources: &mut Vec<ResourceRow>,
) -> Result<FaultRow, String> {
    let node = packaged_node();
    if !node.is_file() {
        return Ok(ok_row(class, n, "js-runtime_missing", false, false, false));
    }
    let script = write_mini_worker(home, n)?;
    let supervisor = match BrowserSupervisor::spawn(SpawnRequest {
        node,
        script,
        profile_root: home.join(format!("wk-{n}")),
        browser: Some(packaged_chromium()),
    }) {
        Ok(s) => s,
        Err(err) => {
            return Ok(ok_row(class, n, "spawn_failed", false, false, leaked(&err)));
        }
    };
    let pid = supervisor.pid();
    if pid == 0 || pid == std::process::id() {
        drop(supervisor);
        return Ok(ok_row(class, n, "refuses_self_kill", false, false, false));
    }
    if let Ok(row) = sample_pid(pid, "worker") {
        resources.push(row);
    }
    terminate_pid(pid);
    std::thread::sleep(Duration::from_millis(150));
    let health = supervisor.health();
    let gone = !process_alive(pid) || health.is_err();
    drop(supervisor);
    Ok(ok_row(
        class,
        n,
        "adapter",
        gone,
        false,
        health.as_ref().ok().is_some_and(|v| leaked(&v.to_string())),
    ))
}

fn chromium_kill_fault(
    class: &str,
    n: usize,
    home: &Path,
    resources: &mut Vec<ResourceRow>,
) -> Result<FaultRow, String> {
    let chrome = packaged_chromium();
    if !chrome.is_file() {
        return Ok(ok_row(class, n, "chromium_missing", false, false, false));
    }
    let profile = home.join(format!("chrome-kill-{n}"));
    fs::create_dir_all(&profile).map_err(|e| e.to_string())?;
    let mut child = spawn_isolated_chromium(&chrome, &profile)?;
    let pid = child.id();
    if pid == 0 || pid == std::process::id() {
        let _ = child.kill();
        let _ = child.wait();
        return Ok(ok_row(class, n, "refuses_self_kill", false, false, false));
    }
    let mut ready = false;
    for _ in 0..40 {
        if process_alive(pid) {
            ready = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let kids = live_browser_descendant_pids(pid);
    if !ready && kids.is_empty() {
        let _ = child.kill();
        let _ = child.wait();
        return Ok(ok_row(
            class,
            n,
            "chromium_did_not_start",
            false,
            false,
            false,
        ));
    }
    if let Ok(row) = sample_pid(pid, "chromium") {
        resources.push(row);
    }
    terminate_pid(pid);
    for kid in &kids {
        terminate_pid(*kid);
    }
    let _ = child.kill();
    let _ = child.wait();
    std::thread::sleep(Duration::from_millis(120));
    let gone = !process_alive(pid) && kids.iter().all(|k| !process_alive(*k));
    Ok(ok_row(class, n, "chromium_gone", gone, false, false))
}

fn spawn_isolated_chromium(chrome: &Path, profile: &Path) -> Result<std::process::Child, String> {
    let mut cmd = std::process::Command::new(chrome);
    cmd.args([
        "--headless=new",
        "--disable-gpu",
        "--disable-extensions",
        "--no-first-run",
        "--no-default-browser-check",
        "--disable-background-networking",
        "--disable-sync",
        "--mute-audio",
        "about:blank",
    ])
    .arg(format!("--user-data-dir={}", profile.display()))
    .stdin(std::process::Stdio::null())
    .stdout(std::process::Stdio::null())
    .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    cmd.spawn().map_err(|e| format!("spawn chromium: {e}"))
}

pub(super) fn time_browser_cold(
    out: &Path,
    map: &mut BTreeMap<String, Vec<u64>>,
    fails: &mut BTreeMap<String, usize>,
    path: &Path,
) -> Result<(), String> {
    let chrome = packaged_chromium();
    if !chrome.is_file() {
        return Err("packaged chromium missing for browser_cold".into());
    }
    for i in 0..ROUNDS {
        let profile = out.join(format!("chrome-cold-{i}"));
        let _ = fs::remove_dir_all(&profile);
        fs::create_dir_all(&profile).map_err(|e| e.to_string())?;
        let t0 = Instant::now();
        let mut child = spawn_isolated_chromium(&chrome, &profile)?;
        let pid = child.id();
        let mut alive = false;
        for _ in 0..50 {
            if process_alive(pid) {
                alive = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(40));
        }
        record(map, fails, path, "cold", "browser_start", t0, alive)?;
        terminate_pid(pid);
        for kid in live_browser_descendant_pids(pid) {
            terminate_pid(kid);
        }
        let _ = child.kill();
        let _ = child.wait();
        let _ = fs::remove_dir_all(&profile);
    }
    Ok(())
}

fn terminate_pid(pid: u32) {
    if pid == 0 || pid == std::process::id() {
        return;
    }
    #[cfg(windows)]
    {
        use windows::Win32::Foundation::CloseHandle;
        use windows::Win32::System::Threading::{OpenProcess, TerminateProcess, PROCESS_TERMINATE};
        unsafe {
            if let Ok(h) = OpenProcess(PROCESS_TERMINATE, false, pid) {
                let _ = TerminateProcess(h, 1);
                let _ = CloseHandle(h);
            }
        }
    }
    let _ = pid;
}

fn malformed_mcp_fault(class: &str, n: usize) -> Result<FaultRow, String> {
    let fake = Arc::new(FakeAdapter::new());
    let broker = Arc::new(ComputerUseBroker::new(
        fake.clone(),
        BrokerOptions {
            feature_enabled: true,
            lease_path: std::env::temp_dir().join(format!("cu-d8-mcp-{n}.lease")),
            ..BrokerOptions::default()
        },
    ));
    broker.open_run("s", "run").map_err(|e| e.to_string())?;
    broker
        .authorize_target("run", &fake.fixture_id())
        .map_err(|e| e.to_string())?;
    let server = ipc::spawn(broker).map_err(|e| e.to_string())?;
    let token = server
        .issue_session("s", "run")
        .map_err(|e| e.to_string())?;
    let url = format!("{}/cu/tool", server.url);
    let bad = reqwest::blocking::Client::new()
        .post(&url)
        .bearer_auth(&token)
        .header("content-type", "application/json")
        .body("{")
        .timeout(Duration::from_secs(3))
        .send();
    let oversize = reqwest::blocking::Client::new()
        .post(&url)
        .bearer_auth(&token)
        .header("content-type", "application/json")
        .body("x".repeat(80 * 1024))
        .timeout(Duration::from_secs(3))
        .send();
    drop(server);
    let bad_ok = bad
        .as_ref()
        .ok()
        .is_some_and(|r| r.status().as_u16() >= 400)
        || bad.as_ref().err().is_some();
    let over_ok = oversize
        .as_ref()
        .ok()
        .is_some_and(|r| r.status().as_u16() >= 400)
        || oversize.as_ref().err().is_some();
    Ok(ok_row(class, n, "schema", bad_ok && over_ok, false, false))
}

fn bearer_fault(class: &str, n: usize) -> Result<FaultRow, String> {
    let fake = Arc::new(FakeAdapter::new());
    let broker = Arc::new(ComputerUseBroker::new(
        fake.clone(),
        BrokerOptions {
            feature_enabled: true,
            lease_path: std::env::temp_dir().join(format!("cu-d8-b-{n}.lease")),
            ..BrokerOptions::default()
        },
    ));
    broker.open_run("s", "run").map_err(|e| e.to_string())?;
    broker
        .authorize_target("run", &fake.fixture_id())
        .map_err(|e| e.to_string())?;
    let server = ipc::spawn(broker).map_err(|e| e.to_string())?;
    let token = server
        .issue_session("s", "run")
        .map_err(|e| e.to_string())?;
    let rotated = server
        .issue_session("s", "run")
        .map_err(|e| e.to_string())?;
    let missing = ipc_call(&server.url, "", json!({"name":"computer_status"}));
    let wrong = ipc_call(&server.url, "deadbeef", json!({"name":"computer_status"}));
    let replay = ipc_call(&server.url, &token, json!({"name":"computer_status"}));
    let live = ipc_call(&server.url, &rotated, json!({"name":"computer_status"}));
    drop(server);
    let ok = missing.0 == 401 && wrong.0 == 401 && replay.0 == 401 && live.0 == 200;
    Ok(ok_row(
        class,
        n,
        "invalid_session_credential",
        ok,
        false,
        leaked(&missing.1) || leaked(&wrong.1),
    ))
}

fn loopback_reset_fault(class: &str, n: usize) -> Result<FaultRow, String> {
    let fake = Arc::new(FakeAdapter::new());
    let broker = Arc::new(ComputerUseBroker::new(
        fake.clone(),
        BrokerOptions {
            feature_enabled: true,
            lease_path: std::env::temp_dir().join(format!("cu-d8-r-{n}.lease")),
            ..BrokerOptions::default()
        },
    ));
    broker.open_run("s", "run").map_err(|e| e.to_string())?;
    broker
        .authorize_target("run", &fake.fixture_id())
        .map_err(|e| e.to_string())?;
    let server = ipc::spawn(broker).map_err(|e| e.to_string())?;
    let token = server
        .issue_session("s", "run")
        .map_err(|e| e.to_string())?;
    let url = server.url.clone();
    drop(server);
    std::thread::sleep(Duration::from_millis(30));
    let (status, body) = ipc_call(&url, &token, json!({"name":"computer_status"}));
    Ok(ok_row(
        class,
        n,
        "connection_reset",
        status == 0 || status >= 400,
        false,
        leaked(&body),
    ))
}

fn session_switch_fault(class: &str, n: usize) -> Result<FaultRow, String> {
    let fake = Arc::new(FakeAdapter::new());
    let broker = ComputerUseBroker::new(
        fake.clone(),
        BrokerOptions {
            feature_enabled: true,
            lease_path: std::env::temp_dir().join(format!("cu-d8-sw-{n}.lease")),
            ..BrokerOptions::default()
        },
    );
    broker.open_run("s-a", "run-a").map_err(|e| e.to_string())?;
    broker.open_run("s-b", "run-b").map_err(|e| e.to_string())?;
    broker
        .authorize_target("run-a", &fake.fixture_id())
        .map_err(|e| e.to_string())?;
    let err = broker.require_owner("s-b", "run-a");
    Ok(ok_row(
        class,
        n,
        "identity_mismatch",
        err.as_ref().is_err_and(|e| e.code() == "identity_mismatch"),
        false,
        err.as_ref().err().is_some_and(|e| leaked(&e.to_string())),
    ))
}

fn pause_inflight_fault(class: &str, n: usize) -> Result<FaultRow, String> {
    let fake = Arc::new(FakeAdapter::new());
    fake.set_hang(true);
    let broker = Arc::new(ComputerUseBroker::new(
        fake.clone(),
        BrokerOptions {
            feature_enabled: true,
            lease_path: std::env::temp_dir().join(format!("cu-d8-p-{n}.lease")),
            ..BrokerOptions::default()
        },
    ));
    broker.open_run("s", "run").map_err(|e| e.to_string())?;
    broker
        .authorize_target("run", &fake.fixture_id())
        .map_err(|e| e.to_string())?;
    let obs = broker.observe("run").map_err(|e| e.to_string())?;
    let b2 = broker.clone();
    let fake2 = fake.clone();
    let snap = obs.snapshot_id.clone();
    let snap_for_thread = snap.clone();
    let geo = obs.geometry_revision;
    let gen = obs.target_generation;
    let handle = std::thread::spawn(move || {
        b2.act(click_req(
            "run",
            &fake2.fixture_id(),
            gen,
            &snap_for_thread,
            geo,
            &format!("hang-{n}"),
        ))
    });
    let wait_start = Instant::now();
    while fake.adapter_in_flight() == 0 {
        if wait_start.elapsed() > Duration::from_secs(2) {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let paused = broker.pause("run");
    fake.finish_in_flight();
    let out = handle.join().unwrap();
    // Shipped Broker records in-flight cancel as Unknown/executed=true (do not
    // replay). The oracle is: pause succeeds, the hung action is not Verified,
    // and a follow-up dispatch with the old snapshot is rejected.
    let second = broker.act(click_req(
        "run",
        &fake.fixture_id(),
        gen,
        &snap,
        geo,
        &format!("after-pause-{n}"),
    ));
    Ok(FaultRow {
        class: class.into(),
        n,
        code: "stop_requested".into(),
        ok: paused.is_ok()
            && out.kind != OutcomeKind::Verified
            && !second.executed
            && second.kind == OutcomeKind::Rejected,
        executed: second.executed,
        leaked: leaked(&out.reason.clone().unwrap_or_default())
            || leaked(&second.reason.clone().unwrap_or_default()),
    })
}

fn extension_fault(class: &str, n: usize) -> Result<FaultRow, String> {
    let fake = Arc::new(FakeAdapter::new());
    let broker = ComputerUseBroker::new(
        fake,
        BrokerOptions {
            feature_enabled: true,
            lease_path: std::env::temp_dir().join(format!("cu-d8-e-{n}.lease")),
            ..BrokerOptions::default()
        },
    );
    let _ = broker.tabs().begin_pairing_challenge();
    broker.tabs().revoke_pairing();
    let listed = broker.tabs().list_shared_candidates();
    Ok(ok_row(
        class,
        n,
        "pairing_revoked",
        listed.is_empty(),
        false,
        false,
    ))
}

fn existing_tab_nav_fault(class: &str, n: usize) -> Result<FaultRow, String> {
    let fake = Arc::new(FakeAdapter::new());
    let broker = ComputerUseBroker::new(
        fake,
        BrokerOptions {
            feature_enabled: true,
            lease_path: std::env::temp_dir().join(format!("cu-d8-t-{n}.lease")),
            ..BrokerOptions::default()
        },
    );
    broker.open_run("s", "run").map_err(|e| e.to_string())?;
    let err = broker
        .tabs()
        .act_with_document("run", "missing-tab", 1)
        .err();
    Ok(ok_row(
        class,
        n,
        "target_unauthorized",
        err.as_ref()
            .is_some_and(|e| e.code() == "target_unauthorized" || e.code() == "identity_mismatch"),
        false,
        err.as_ref().is_some_and(|e| leaked(&e.to_string())),
    ))
}

fn webview_destroy_fault(class: &str, n: usize) -> Result<FaultRow, String> {
    let wv = crate::computer_use::webview::WebViewAdapter::new();
    let err = wv.observe("wv|gone").err().unwrap_or_default();
    Ok(ok_row(
        class,
        n,
        "not_bound",
        err.contains("not bound") || err.contains("stale"),
        false,
        leaked(&err),
    ))
}

fn runtime_corrupt_fault(class: &str, n: usize, home: &Path) -> Result<FaultRow, String> {
    let dir = home.join(format!("rt-{n}"));
    let store = RuntimeStore::new(dir.clone());
    let pack = format!("pack-a-{n}");
    store
        .install_pack(
            &pack,
            vec![NewComponent {
                id: JS_RUNTIME.into(),
                version: "1".into(),
                relpath: "bin/js-runtime".into(),
                bytes: b"ok-runtime".to_vec(),
            }],
        )
        .map_err(|e| e.to_string())?;
    store.activate_pack(&pack).map_err(|e| e.to_string())?;
    let path = store.resolve(JS_RUNTIME).map_err(|e| e.to_string())?;
    fs::write(&path, b"corrupt").map_err(|e| e.to_string())?;
    let issues = store.diagnose(&[JS_RUNTIME]);
    Ok(ok_row(
        class,
        n,
        "hash_mismatch",
        issues
            .iter()
            .any(|i| i.code == "hash_mismatch" || i.code == "placeholder"),
        false,
        false,
    ))
}

fn repair_interrupt_fault(class: &str, n: usize, home: &Path) -> Result<FaultRow, String> {
    let dir = home.join(format!("ri-{n}"));
    let store = RuntimeStore::new(dir.clone());
    let pack = format!("pack-ri-{n}");
    store
        .install_pack(
            &pack,
            vec![NewComponent {
                id: JS_RUNTIME.into(),
                version: "1".into(),
                relpath: "bin/js-runtime".into(),
                bytes: b"stable-bytes".to_vec(),
            }],
        )
        .map_err(|e| e.to_string())?;
    store.activate_pack(&pack).map_err(|e| e.to_string())?;
    let staging = dir.join(".staging").join("partial");
    fs::create_dir_all(&staging).map_err(|e| e.to_string())?;
    fs::write(staging.join("broken.bin"), b"partial").map_err(|e| e.to_string())?;
    let issues = store.diagnose(&[JS_RUNTIME]);
    store
        .discard_interrupted_upgrade()
        .map_err(|e| e.to_string())?;
    Ok(ok_row(
        class,
        n,
        "interrupted_upgrade",
        issues.iter().any(|i| i.code == "interrupted_upgrade") && !store.staging_present(),
        false,
        false,
    ))
}

fn trace_corrupt_fault(class: &str, n: usize, home: &Path) -> Result<FaultRow, String> {
    let dir = home.join(format!("tr-{n}"));
    let store = grok_computer_use_core::privacy::TraceStore::open(dir.clone(), OWNER)
        .map_err(|e| e.to_string())?;
    fs::write(dir.join("run-a.jsonl"), "{not-json\n").map_err(|e| e.to_string())?;
    let rows = store.load_run("run-a").map_err(|e| e.to_string())?;
    Ok(ok_row(
        class,
        n,
        "trace_corrupt",
        rows.is_empty(),
        false,
        false,
    ))
}

fn staging_denied_fault(class: &str, n: usize, home: &Path) -> Result<FaultRow, String> {
    let p = home.join(format!("stg-file-{n}"));
    fs::write(&p, b"not-a-dir").map_err(|e| e.to_string())?;
    let err = admit_cleanup_path(&p, OWNER).err().unwrap_or_default();
    Ok(ok_row(
        class,
        n,
        "schema",
        err.contains("not a directory") || err.contains("missing") || err.contains("refuses"),
        false,
        leaked(&err),
    ))
}

fn cleanup_junction_fault(class: &str, n: usize, home: &Path) -> Result<FaultRow, String> {
    let real = home.join(format!("real-{n}"));
    write_owner(&real, OWNER)?;
    let link = home.join(format!("junc-{n}"));
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("cmd")
            .args([
                "/C",
                "mklink",
                "/J",
                &link.to_string_lossy(),
                &real.to_string_lossy(),
            ])
            .status();
    }
    let err = admit_cleanup_path(&link, OWNER);
    let _ = fs::remove_dir(&link);
    Ok(ok_row(
        class,
        n,
        "cleanup_refuses_junction",
        err.is_err(),
        false,
        err.as_ref().err().is_some_and(|e| leaked(e)),
    ))
}
