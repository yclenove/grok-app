//! Probe-only MCP scripted inner harness.
use std::ffi::{OsStr, OsString};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::time::Duration;

use grok_computer_use_core::adapter::SurfaceKind;
use grok_computer_use_core::broker::BrokerOptions;
use grok_computer_use_core::fake::FakeAdapter;
use grok_computer_use_core::surface::ComputerUseSurface;
use grok_computer_use_core::tools::{HOST_ONLY_TOOLS, MODEL_TOOLS};
use serde_json::{json, Value};
use uuid::Uuid;

use super::ComputerUseBroker;

use super::browser_supervisor::{
    process_alive, product_spawn_request, BrowserSupervisor, SpawnRequest,
};
use super::inject::{mcp_acp_entry_with, MCP_SERVER_NAME};
use super::runtime;

const SENTINEL_OPENAI: &str = "sk-b1-mcp-openai-sentinel-do-not-leak";
const SENTINEL_GROK: &str = "sk-b1-mcp-grok-sentinel-do-not-leak";
const SENTINEL_PROXY: &str = "http://127.0.0.1:9/b1-mcp-proxy-sentinel";
const QUERY_SENTINEL: &str = "cu-query-sentinel";
const CALL_GAP: Duration = Duration::from_millis(180);

pub(super) fn run_mcp_scripted_agent_gate_inner(product_route: bool) -> Result<(), String> {
    let home = std::env::temp_dir().join(format!(
        "grok-cu-mcp-scripted-{}-{}",
        std::process::id(),
        Uuid::new_v4()
    ));
    std::fs::create_dir_all(&home).map_err(|e| e.to_string())?;
    let home_s = normalize_path(&home);
    if home_s.contains("appdata/local/grok")
        || home_s.ends_with("/.grok")
        || home_s.contains("/.grok/")
    {
        let _ = std::fs::remove_dir_all(&home);
        return Err("isolated GROK_APP_HOME must not be official Grok or shared ~/.grok".into());
    }

    let _lock = crate::paths::APP_HOME_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let mut env = EnvGuard::new();
    apply_isolated_env(&mut env, &home)?;
    println!(
        "gate: {} start",
        if product_route {
            "product_managed_route"
        } else {
            "mcp_scripted_agent"
        }
    );
    println!("gate: isolated_home={}", home.display());
    println!("gate: transport=mcp-stdio ipc=/cu/tool auth=bearer");
    println!(
        "gate: GROK_CU_NODE_FILE_set={}",
        u8::from(std::env::var_os("GROK_CU_NODE_FILE").is_some())
    );

    super::set_feature_enabled(false);
    let result = run_isolated(&home, product_route);
    super::set_feature_enabled(false);
    super::set_session_enabled("mcp-scripted-a", false);
    super::set_session_enabled("mcp-scripted-b", false);
    std::thread::sleep(Duration::from_millis(400));
    let leftover = super::browser_packaged_contract::processes_under(&home);
    for (pid, path) in &leftover {
        super::browser_packaged_contract::terminate_pid(*pid);
        println!("gate: leftover pid={pid} image={path}");
    }
    if !leftover.is_empty() {
        std::thread::sleep(Duration::from_millis(200));
    }
    let leftover_after = super::browser_packaged_contract::processes_under(&home);
    drop(env);
    let cleanup = std::fs::remove_dir_all(&home);
    println!("gate: leftover_under_home={}", leftover_after.len());
    println!(
        "gate: isolated_home_removed={}",
        u8::from(!home.exists() || cleanup.is_ok())
    );
    if !leftover_after.is_empty() {
        return Err(redact(&format!(
            "leftover processes under isolated home: {leftover_after:?}; inner={result:?}"
        )));
    }
    result.map_err(|e| redact(&e))
}

fn apply_isolated_env(env: &mut EnvGuard, home: &Path) -> Result<(), String> {
    let decoy = home.join("path-decoy");
    std::fs::create_dir_all(&decoy).map_err(|e| e.to_string())?;
    std::fs::write(decoy.join("node.exe"), b"SYSTEM-NODE-DECOY").map_err(|e| e.to_string())?;
    std::fs::write(decoy.join("chrome.exe"), b"SYSTEM-CHROME-DECOY").map_err(|e| e.to_string())?;
    env.set("GROK_APP_HOME", home.as_os_str());
    env.set("PATH", decoy.as_os_str());
    env.set("OPENAI_API_KEY", SENTINEL_OPENAI);
    env.set("GROK_API_KEY", SENTINEL_GROK);
    env.set("HTTPS_PROXY", SENTINEL_PROXY);
    env.set("HTTP_PROXY", SENTINEL_PROXY);
    env.set("ALL_PROXY", SENTINEL_PROXY);
    for key in [
        "GROK_CU_NODE_FILE",
        "GROK_CU_BROWSER_IPC",
        "NODE_PATH",
        "NODE_OPTIONS",
    ] {
        env.remove(key);
    }
    Ok(())
}

fn run_isolated(home: &Path, product_route: bool) -> Result<(), String> {
    if std::env::var_os("GROK_CU_NODE_FILE").is_some() {
        return Err("GROK_CU_NODE_FILE must stay unset for MCP scripted-agent".into());
    }
    let resource = std::env::var("GROK_CU_RESOURCE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources"));
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| resource.clone());
    let pack = runtime::repair_from_install(&resource, &exe_dir)?;
    println!("gate: repair pack={pack}");

    let req = product_spawn_request()?;
    if !path_in(home, &req.node) || !path_in(home, &req.script) || !path_in(home, &req.profile_root)
    {
        return Err("product spawn escaped isolated home".into());
    }
    let script = normalize_path(&req.script);
    if script.contains("/tools/computer-use-browser/") || script.ends_with("/server.mjs") {
        return Err("product spawn used source worker".into());
    }
    println!("gate: spawn node={}", req.node.display());
    println!("gate: spawn script={}", req.script.display());
    run_live(home, req, product_route)
}

fn run_live(home: &Path, req: SpawnRequest, product_route: bool) -> Result<(), String> {
    let fake = Arc::new(FakeAdapter::new());
    let fixture = fake.fixture_id();
    let broker = Arc::new(ComputerUseBroker::new(
        fake,
        BrokerOptions {
            feature_enabled: true,
            lease_path: home.join("desktop.lease"),
            action_timeout: Duration::from_secs(30),
            ..BrokerOptions::default()
        },
    ));
    super::set_feature_enabled(true);
    super::set_session_enabled("mcp-scripted-a", true);
    super::set_session_enabled("mcp-scripted-b", true);
    super::set_session_surface("mcp-scripted-a", ComputerUseSurface::LocalInteractive);
    super::set_session_surface("mcp-scripted-b", ComputerUseSurface::LocalInteractive);
    broker
        .open_run("mcp-scripted-a", "run-a")
        .map_err(|e| e.to_string())?;
    broker
        .open_run("mcp-scripted-b", "run-b")
        .map_err(|e| e.to_string())?;
    broker
        .authorize_target("run-a", &fixture)
        .map_err(|e| e.to_string())?;
    broker
        .authorize_target("run-b", &fixture)
        .map_err(|e| e.to_string())?;

    let managed = broker
        .list_targets_for_surface(SurfaceKind::ManagedBrowser)
        .map_err(|e| e.to_string())?;
    if managed
        .iter()
        .any(|t| t.kind == "window" || t.title == "GrokCuFixture" || t.backend == "fake")
    {
        return Err("managed-browser listing leaked desktop FakeAdapter targets".into());
    }
    println!(
        "gate: product_surface=managed-browser listed={}",
        managed.len()
    );
    let webview = broker
        .list_targets_for_surface(SurfaceKind::WebView)
        .map_err(|e| e.to_string())?;
    if !webview.is_empty() {
        return Err("app-webview must fail closed empty until product bind".into());
    }

    let endpoint = grok_computer_use_core::ipc::spawn(broker.clone())?;
    println!("gate: ipc={}", endpoint.url);
    // Inject MCP before Chromium starts so ensure_embedded_mcp does not recopy a live tree.
    let entry_a = mcp_acp_entry_with(&broker, &endpoint, "mcp-scripted-a", "run-a")?;
    let entry_b = mcp_acp_entry_with(&broker, &endpoint, "mcp-scripted-b", "run-b")?;
    if entry_a.get("name").and_then(Value::as_str) != Some(MCP_SERVER_NAME) {
        return Err("session A MCP name mismatch".into());
    }
    let token_a = env_value(&entry_a, "GROK_APP_CU_TOKEN").ok_or("missing A token")?;
    let token_b = env_value(&entry_b, "GROK_APP_CU_TOKEN").ok_or("missing B token")?;
    if token_a == token_b {
        return Err("sessions must not share a Bearer token".into());
    }
    let args_a = entry_a
        .get("args")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if args_a.iter().any(|v| v.as_str() == Some(token_a.as_str())) {
        return Err("token must not appear in argv".into());
    }
    println!(
        "gate: mcp_command={}",
        entry_a.get("command").and_then(Value::as_str).unwrap_or("")
    );
    println!(
        "gate: mcp_script={}",
        args_a.first().and_then(Value::as_str).unwrap_or("")
    );

    let supervisor = BrowserSupervisor::spawn(SpawnRequest {
        node: req.node.clone(),
        script: req.script.clone(),
        profile_root: req.profile_root.clone(),
        browser: req.browser.clone(),
    })?;
    let worker_pid = supervisor.pid();
    println!("gate: worker_pid={worker_pid}");
    supervisor.attach(&broker)?;
    let live = run_sessions(
        &broker,
        &entry_a,
        &entry_b,
        &token_a,
        &token_b,
        product_route,
    );
    let shutdown = supervisor.shutdown();
    std::thread::sleep(Duration::from_millis(300));
    let still_alive = process_alive(worker_pid);
    println!(
        "gate: worker_pid_alive_after_shutdown={}",
        u8::from(still_alive)
    );
    live?;
    shutdown?;
    if still_alive {
        return Err(format!(
            "worker pid {worker_pid} still alive after shutdown"
        ));
    }
    Ok(())
}

fn run_sessions(
    broker: &Arc<ComputerUseBroker>,
    entry_a: &Value,
    entry_b: &Value,
    token_a: &str,
    token_b: &str,
    product_route: bool,
) -> Result<(), String> {
    let tab_a = broker
        .tabs()
        .open_managed_profile("mcp-scripted-a", "run-a", "alice")
        .map_err(|e| e.to_string())?;
    let tab_b = broker
        .tabs()
        .open_managed_profile("mcp-scripted-b", "run-b", "bob")
        .map_err(|e| e.to_string())?;
    println!("gate: host_tab_a={}", tab_a.tab_id);
    println!("gate: host_tab_b={}", tab_b.tab_id);
    if product_route {
        broker
            .authorize_target("run-a", &tab_a.tab_id)
            .map_err(|e| e.to_string())?;
        broker
            .authorize_target("run-b", &tab_b.tab_id)
            .map_err(|e| e.to_string())?;
        println!("gate: authorized_managed_tabs=1");
    }

    let oracle = Arc::new(AtomicU64::new(0));
    let (page_url, stop, page_thread) = serve_fixture(oracle.clone())?;
    deny_leaks(&page_url, token_a)?;

    let mut mcp_a = McpClient::spawn(entry_a)?;
    let mut mcp_b = McpClient::spawn(entry_b)?;
    let result = (|| {
        mcp_a.initialize()?;
        mcp_b.initialize()?;
        let listed = mcp_a.call("tools/list", json!({}))?;
        let tools = listed
            .get("tools")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let names: Vec<String> = tools
            .iter()
            .filter_map(|t| t.get("name").and_then(Value::as_str).map(str::to_string))
            .collect();
        println!("gate: session_a_tools={}", names.join(","));
        for name in MODEL_TOOLS {
            if !names.iter().any(|n| n == name) {
                return Err(format!("model tool {name} missing from tools/list"));
            }
        }
        for name in HOST_ONLY_TOOLS {
            if names.iter().any(|n| n == name) {
                return Err(format!("host-only {name} leaked onto model tools/list"));
            }
        }
        deny_leaks(&listed.to_string(), token_a)?;

        let targets = mcp_a.tool("computer_list_targets", json!({}))?;
        let target_id = targets
            .as_array()
            .and_then(|rows| rows.first())
            .and_then(|row| row.get("targetId"))
            .and_then(Value::as_str)
            .ok_or_else(|| format!("list_targets empty: {targets}"))?
            .to_string();
        println!("gate: session_a_list_targets={target_id}");
        if product_route && !target_id.starts_with("managed:") {
            return Err(format!(
                "product managed route must authorize a managed tab, got {target_id}"
            ));
        }
        let opened_target = mcp_a.tool("computer_open_target", json!({"targetId": target_id}))?;
        deny_leaks(&opened_target.to_string(), token_a)?;
        println!("gate: session_a_open_target=ok");

        let tabs_a = mcp_a.tool("browser_list_tabs", json!({}))?;
        deny_leaks(&tabs_a.to_string(), token_a)?;
        if tabs_a.to_string().contains(QUERY_SENTINEL) || tabs_a.to_string().contains("secret=") {
            return Err(format!("list_tabs leaked URL query: {tabs_a}"));
        }
        let tab_id = tabs_a["tabs"]
            .as_array()
            .and_then(|rows| {
                rows.iter()
                    .find_map(|row| row.get("tabId").and_then(Value::as_str).map(str::to_string))
            })
            .ok_or_else(|| format!("session A has no tab: {tabs_a}"))?;
        if tab_id == tab_b.tab_id {
            return Err("session A listed session B tab".into());
        }
        let page_generation = tabs_a["tabs"][0]
            .get("pageGeneration")
            .and_then(Value::as_u64)
            .unwrap_or(1)
            .max(1);
        let opened_tab = mcp_a.tool("browser_open", json!({"tabId": tab_id}))?;
        deny_leaks(&opened_tab.to_string(), token_a)?;
        println!("gate: session_a_open_tab={tab_id}");

        let navigated = mcp_a.tool(
            "computer_navigate",
            json!({
                "tabId": tab_id,
                "url": page_url,
                "actionId": "nav-form",
                "pageGeneration": page_generation
            }),
        )?;
        deny_leaks(&navigated.to_string(), token_a)?;
        if navigated.to_string().contains(QUERY_SENTINEL)
            || navigated.to_string().contains("secret=")
        {
            return Err(format!("navigate result leaked URL query: {navigated}"));
        }
        let mut generation = navigated
            .get("pageGeneration")
            .or_else(|| navigated.get("generation"))
            .and_then(Value::as_u64)
            .unwrap_or(page_generation)
            .max(1);
        println!("gate: session_a_navigate generation={generation}");

        let observed = observe_until(&mut mcp_a, &tab_id, &mut generation, token_a)?;
        let snapshot = observed
            .get("snapshotId")
            .and_then(Value::as_str)
            .ok_or("observe missing snapshotId")?
            .to_string();
        if snapshot.is_empty() {
            return Err("observe missing snapshotId".into());
        }
        println!("gate: session_a_observe_snapshot={snapshot}");
        let nodes = observed
            .get("nodes")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        if !nodes.iter().any(|n| {
            n.get("name")
                .and_then(Value::as_str)
                .is_some_and(|name| name.contains("inner"))
        }) {
            println!("gate: iframe_node=missing (continuing if +1 exists)");
        } else {
            println!("gate: iframe_node=ok");
        }
        if nodes.iter().any(|n| {
            n.get("name")
                .and_then(Value::as_str)
                .is_some_and(|name| name.contains("iframe-ok"))
        }) {
            return Err("iframe inner document leaked onto the parent snapshot".into());
        }
        let plus = nodes
            .iter()
            .find(|n| {
                n.get("name")
                    .and_then(Value::as_str)
                    .is_some_and(|name| name.contains("+1"))
                    && n.get("truncated") != Some(&json!(true))
            })
            .ok_or_else(|| format!("missing +1 in {nodes:?}"))?;
        let element_ref = plus
            .get("elementRef")
            .and_then(Value::as_str)
            .ok_or("missing elementRef")?
            .to_string();
        std::thread::sleep(Duration::from_millis(120));

        let acted = mcp_a.tool(
            "browser_act",
            json!({
                "tabId": tab_id,
                "actionId": "click-plus",
                "kind": "click",
                "pageGeneration": generation,
                "snapshotId": snapshot,
                "elementRef": element_ref,
                "parameters": {}
            }),
        )?;
        deny_leaks(&acted.to_string(), token_a)?;
        let act_generation = acted
            .get("pageGeneration")
            .and_then(Value::as_u64)
            .unwrap_or(generation)
            .max(1);
        let mut oracle_count = 0u64;
        for _ in 0..100 {
            oracle_count = oracle.load(Ordering::SeqCst);
            if oracle_count == 1 {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        println!("gate: independent_oracle_count={oracle_count}");
        if oracle_count != 1 {
            return Err(format!(
                "independent oracle expected count=1, got {oracle_count}"
            ));
        }

        let replay = mcp_a.tool(
            "browser_act",
            json!({
                "tabId": tab_id,
                "actionId": "click-plus",
                "kind": "click",
                "pageGeneration": generation,
                "snapshotId": snapshot,
                "elementRef": element_ref,
                "parameters": {}
            }),
        )?;
        deny_leaks(&replay.to_string(), token_a)?;
        std::thread::sleep(Duration::from_millis(200));
        let after_replay = oracle.load(Ordering::SeqCst);
        if after_replay != 1 {
            return Err(format!(
                "duplicate actionId must replay without a second click, oracle={after_replay}"
            ));
        }
        println!("gate: duplicate_action_id=replay");
        generation = act_generation.max(generation);

        let verified = observe_until(&mut mcp_a, &tab_id, &mut generation, token_a)?;
        let verify_snap = verified
            .get("snapshotId")
            .and_then(Value::as_str)
            .unwrap_or("");
        if verify_snap.is_empty() || verify_snap == snapshot {
            return Err(format!(
                "verify observe must mint a new snapshot, got {verify_snap}"
            ));
        }
        println!("gate: session_a_verify_snapshot={verify_snap}");

        let unknown = mcp_a.tool_result("computer_authorize", json!({}))?;
        if unknown.get("isError") != Some(&json!(true)) {
            return Err(format!("host-only tool must fail closed: {unknown}"));
        }
        println!("gate: unknown_or_host_only=rejected");

        let status = mcp_a.tool("computer_status", json!({}))?;
        deny_leaks(&status.to_string(), token_a)?;
        println!("gate: traces_redacted=ok");

        let tabs_b = mcp_b.tool("browser_list_tabs", json!({}))?;
        deny_leaks(&tabs_b.to_string(), token_b)?;
        let b_ids: Vec<String> = tabs_b["tabs"]
            .as_array()
            .map(|rows| {
                rows.iter()
                    .filter_map(|row| row.get("tabId").and_then(Value::as_str).map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        if b_ids.iter().any(|id| id == &tab_id) {
            return Err(format!("session B listed session A tab: {tabs_b}"));
        }
        if !b_ids.iter().any(|id| id == &tab_b.tab_id) {
            return Err(format!("session B missing its own tab: {tabs_b}"));
        }
        let foreign = mcp_b.tool_result("browser_open", json!({"tabId": tab_id}))?;
        if foreign.get("isError") != Some(&json!(true)) {
            return Err(format!("session B opened session A tab: {foreign}"));
        }
        println!("gate: session_b_isolation=ok");

        let stopped = mcp_a.tool("computer_stop", json!({}))?;
        deny_leaks(&stopped.to_string(), token_a)?;
        let after_stop = mcp_a.tool_result(
            "browser_act",
            json!({
                "tabId": tab_id,
                "actionId": "click-after-stop",
                "kind": "click",
                "pageGeneration": generation,
                "snapshotId": verify_snap,
                "elementRef": element_ref,
                "parameters": {}
            }),
        )?;
        if after_stop.get("isError") != Some(&json!(true)) {
            return Err(format!("act after stop must fail: {after_stop}"));
        }
        println!("gate: stop_then_reject=ok");
        Ok(())
    })();

    mcp_a.kill();
    mcp_b.kill();
    stop.store(true, Ordering::SeqCst);
    let _ = page_thread.join();
    result
}

fn observe_until(
    mcp: &mut McpClient,
    tab_id: &str,
    generation: &mut u64,
    token: &str,
) -> Result<Value, String> {
    let mut last = String::new();
    for _ in 0..8 {
        let raw = mcp.tool_result(
            "browser_observe",
            json!({"tabId": tab_id, "pageGeneration": *generation}),
        )?;
        if raw.get("isError") == Some(&json!(true)) {
            last = tool_text(&raw);
            if last.contains("generation") || last.contains("changed") {
                if let Ok(tabs) = mcp.tool("browser_list_tabs", json!({})) {
                    if let Some(gen) = tabs["tabs"]
                        .as_array()
                        .and_then(|rows| rows.iter().find(|row| row["tabId"] == tab_id))
                        .and_then(|row| row.get("pageGeneration"))
                        .and_then(Value::as_u64)
                    {
                        *generation = gen.max(1);
                    }
                }
                continue;
            }
            return Err(format!("observe failed: {last}"));
        }
        deny_leaks(&raw.to_string(), token)?;
        let has_png = raw
            .get("content")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .any(|part| {
                part.get("type").and_then(Value::as_str) == Some("image")
                    && part
                        .get("data")
                        .and_then(Value::as_str)
                        .is_some_and(|d| d.starts_with("iVBOR"))
            });
        if !has_png {
            return Err("observe missing PNG image content".into());
        }
        let payload = tool_json(&raw)?;
        deny_leaks(&payload.to_string(), token)?;
        if payload.to_string().contains(QUERY_SENTINEL) || payload.to_string().contains("secret=") {
            return Err(format!("observe leaked URL query: {payload}"));
        }
        if let Some(gen) = payload.get("pageGeneration").and_then(Value::as_u64) {
            *generation = gen.max(1);
        }
        return Ok(payload);
    }
    Err(format!("observe did not settle: {last}"))
}

fn serve_fixture(
    oracle: Arc<AtomicU64>,
) -> Result<(String, Arc<AtomicBool>, std::thread::JoinHandle<()>), String> {
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("tools")
        .join("computer-use-browser")
        .join("fixtures");
    let form = std::fs::read(fixtures.join("form.html")).map_err(|e| e.to_string())?;
    let frame = std::fs::read(fixtures.join("frame.html")).unwrap_or_else(|_| form.clone());
    let popup =
        std::fs::read(fixtures.join("popup.html")).unwrap_or_else(|_| b"<p>popup</p>".to_vec());
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let stop = Arc::new(AtomicBool::new(false));
    let stop_t = stop.clone();
    let thread = std::thread::spawn(move || {
        while !stop_t.load(Ordering::SeqCst) {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let req = read_http_request(&mut stream);
                    let path = req
                        .lines()
                        .next()
                        .and_then(|l| l.split_whitespace().nth(1))
                        .unwrap_or("/");
                    if req.starts_with("POST /oracle") {
                        let body = req.split("\r\n\r\n").nth(1).unwrap_or("");
                        if let Ok(value) =
                            serde_json::from_str::<Value>(body.trim_end_matches('\0').trim())
                        {
                            if let Some(count) = value.get("count").and_then(Value::as_u64) {
                                oracle.store(count, Ordering::SeqCst);
                            }
                        } else if body.contains("\"count\"") {
                            // Full POST arrived but JSON was noisy; a write still happened.
                            oracle.fetch_max(1, Ordering::SeqCst);
                        }
                        let _ = stream
                            .write_all(b"HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n");
                    } else {
                        let (body, ctype) = if path.starts_with("/frame") {
                            (frame.as_slice(), "text/html; charset=utf-8")
                        } else if path.starts_with("/popup") {
                            (popup.as_slice(), "text/html; charset=utf-8")
                        } else {
                            (form.as_slice(), "text/html; charset=utf-8")
                        };
                        let header = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            body.len()
                        );
                        let _ = stream.write_all(header.as_bytes());
                        let _ = stream.write_all(body);
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(15));
                }
                Err(_) => break,
            }
        }
    });
    Ok((
        format!("http://127.0.0.1:{port}/form.html?run=mcp-scripted&secret={QUERY_SENTINEL}"),
        stop,
        thread,
    ))
}

fn read_http_request(stream: &mut TcpStream) -> String {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(Duration::from_millis(2000)));
    let mut buf = Vec::new();
    let mut tmp = [0u8; 512];
    for _ in 0..64 {
        match stream.read(&mut tmp) {
            Ok(0) => break,
            Ok(n) => {
                buf.extend_from_slice(&tmp[..n]);
                if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&buf[..pos]);
                    let mut need = pos + 4;
                    if let Some(cl) = headers.lines().find_map(|line| {
                        let (k, v) = line.split_once(':')?;
                        if k.eq_ignore_ascii_case("content-length") {
                            v.trim().parse::<usize>().ok()
                        } else {
                            None
                        }
                    }) {
                        need += cl;
                    }
                    if buf.len() >= need {
                        break;
                    }
                }
                if buf.len() > 64 * 1024 {
                    break;
                }
            }
            Err(e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                break;
            }
            Err(_) => break,
        }
    }
    String::from_utf8_lossy(&buf).into_owned()
}

struct McpClient {
    child: Child,
    stdin: ChildStdin,
    rx: Receiver<String>,
    next_id: i64,
}

impl McpClient {
    fn spawn(entry: &Value) -> Result<Self, String> {
        let command = entry
            .get("command")
            .and_then(Value::as_str)
            .ok_or("MCP command missing")?;
        let args = entry
            .get("args")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let mut cmd = Command::new(command);
        for arg in &args {
            if let Some(s) = arg.as_str() {
                cmd.arg(s);
            }
        }
        if let Some(rows) = entry.get("env").and_then(Value::as_array) {
            for row in rows {
                if let (Some(name), Some(value)) = (
                    row.get("name").and_then(Value::as_str),
                    row.get("value").and_then(Value::as_str),
                ) {
                    cmd.env(name, value);
                }
            }
        }
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000);
        }
        let mut child = cmd.spawn().map_err(|e| format!("spawn MCP: {e}"))?;
        let stdin = child.stdin.take().ok_or("MCP stdin missing")?;
        let stdout = child.stdout.take().ok_or("MCP stdout missing")?;
        let stderr = child.stderr.take();
        if let Some(stderr) = stderr {
            std::thread::spawn(move || {
                let mut buf = String::new();
                let _ = BufReader::new(stderr).read_to_string(&mut buf);
            });
        }
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                let mut line = String::new();
                match reader.read_line(&mut line) {
                    Ok(0) => break,
                    Ok(_) => {
                        if tx.send(line).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        });
        Ok(Self {
            child,
            stdin,
            rx,
            next_id: 1,
        })
    }

    fn initialize(&mut self) -> Result<(), String> {
        let _ = self.call(
            "initialize",
            json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {"name": "cu-probe", "version": "1"}
            }),
        )?;
        self.notify("notifications/initialized", json!({}))
    }

    fn notify(&mut self, method: &str, params: Value) -> Result<(), String> {
        let msg = json!({"jsonrpc":"2.0","method":method,"params":params});
        self.write(&msg)
    }

    fn call(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let id = self.next_id;
        self.next_id += 1;
        let msg = json!({"jsonrpc":"2.0","id":id,"method":method,"params":params});
        self.write(&msg)?;
        let line = self
            .rx
            .recv_timeout(Duration::from_secs(45))
            .map_err(|_| format!("MCP {method} timed out"))?;
        let value: Value = serde_json::from_str(line.trim())
            .map_err(|e| format!("MCP {method} json: {e} line={line}"))?;
        if let Some(err) = value.get("error") {
            return Err(format!("MCP {method} error: {err}"));
        }
        value
            .get("result")
            .cloned()
            .ok_or_else(|| format!("MCP {method} missing result: {value}"))
    }

    fn tool_result(&mut self, name: &str, args: Value) -> Result<Value, String> {
        std::thread::sleep(CALL_GAP);
        self.call("tools/call", json!({"name": name, "arguments": args}))
    }

    fn tool(&mut self, name: &str, args: Value) -> Result<Value, String> {
        let raw = self.tool_result(name, args)?;
        if raw.get("isError") == Some(&json!(true)) {
            return Err(format!("{name} failed: {}", tool_text(&raw)));
        }
        if name == "computer_list_targets"
            || name == "browser_list_tabs"
            || name == "browser_open"
            || name == "computer_open_target"
            || name == "computer_navigate"
            || name == "browser_act"
            || name == "computer_status"
            || name == "computer_stop"
        {
            tool_json(&raw)
        } else {
            Ok(raw)
        }
    }

    fn write(&mut self, msg: &Value) -> Result<(), String> {
        let line = format!("{msg}\n");
        self.stdin
            .write_all(line.as_bytes())
            .map_err(|e| format!("MCP stdin: {e}"))?;
        self.stdin.flush().map_err(|e| format!("MCP flush: {e}"))
    }

    fn kill(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn tool_text(result: &Value) -> String {
    result
        .get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find(|part| part.get("type").and_then(Value::as_str) == Some("text"))
        .and_then(|part| part.get("text").and_then(Value::as_str))
        .unwrap_or("")
        .to_string()
}

fn tool_json(result: &Value) -> Result<Value, String> {
    let text = tool_text(result);
    serde_json::from_str(&text).map_err(|e| format!("tool payload json: {e} text={text}"))
}

fn env_value(entry: &Value, name: &str) -> Option<String> {
    entry.get("env")?.as_array()?.iter().find_map(|row| {
        if row.get("name").and_then(Value::as_str) == Some(name) {
            row.get("value").and_then(Value::as_str).map(str::to_string)
        } else {
            None
        }
    })
}

fn deny_leaks(text: &str, token: &str) -> Result<(), String> {
    if !token.is_empty() && text.contains(token) {
        return Err("Bearer token leaked into MCP/tool/trace output".into());
    }
    for needle in [SENTINEL_OPENAI, SENTINEL_GROK, SENTINEL_PROXY] {
        if text.contains(needle) {
            return Err("sentinel leaked into MCP/tool/trace output".into());
        }
    }
    let lower = text.replace('\\', "/").to_ascii_lowercase();
    if lower.contains("cookie:") || lower.contains("set-cookie") {
        return Err("cookie leaked into MCP/tool/trace output".into());
    }
    if lower.contains("/tools/computer-use-browser/") || lower.contains("/tools/computer-use-mcp/")
    {
        return Err("source path leaked into MCP/tool/trace output".into());
    }
    Ok(())
}

fn redact(text: &str) -> String {
    text.replace(SENTINEL_OPENAI, "[redacted-openai]")
        .replace(SENTINEL_GROK, "[redacted-grok]")
        .replace(SENTINEL_PROXY, "[redacted-proxy]")
        .replace(QUERY_SENTINEL, "[redacted-query]")
}

fn path_in(root: &Path, child: &Path) -> bool {
    let root = normalize_path(root);
    let child = normalize_path(child);
    child == root || child.starts_with(&(root + "/"))
}

fn normalize_path(path: &Path) -> String {
    let raw = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let mut s = raw
        .to_string_lossy()
        .replace('\\', "/")
        .to_ascii_lowercase();
    if let Some(stripped) = s.strip_prefix("//?/") {
        s = stripped.to_string();
    }
    s
}

struct EnvGuard {
    saved: Vec<(OsString, Option<OsString>)>,
}

impl EnvGuard {
    fn new() -> Self {
        Self { saved: Vec::new() }
    }

    fn remember(&mut self, key: &str) {
        let key = OsString::from(key);
        if self.saved.iter().any(|(existing, _)| existing == &key) {
            return;
        }
        self.saved.push((key.clone(), std::env::var_os(&key)));
    }

    fn set(&mut self, key: &str, value: impl AsRef<OsStr>) {
        self.remember(key);
        std::env::set_var(key, value);
    }

    fn remove(&mut self, key: &str) {
        self.remember(key);
        std::env::remove_var(key);
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        for (key, value) in self.saved.drain(..).rev() {
            match value {
                Some(value) => std::env::set_var(&key, value),
                None => std::env::remove_var(&key),
            }
        }
    }
}
