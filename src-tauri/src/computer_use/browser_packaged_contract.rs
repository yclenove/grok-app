//! Packaged Browser runtime contract.
//!
//! Production path only: `repair_from_install` → `product_spawn_request` →
//! `BrowserSupervisor`. Never `GROK_CU_NODE_FILE`, source `server.mjs`, or
//! system Node. Not a substitute for installed-App E4.

use std::ffi::{OsStr, OsString};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use grok_computer_use_core::browser::{ManagedBrowserWorker, ManagedLocator, ManagedWorkerAction};
use grok_computer_use_core::runtime::PLACEHOLDER_PLAYWRIGHT;
use serde_json::{json, Value};
use uuid::Uuid;

use super::browser_supervisor::{
    process_alive, product_spawn_request, BrowserSupervisor, SpawnRequest,
};
use super::playwright_worker::LoopbackPlaywrightWorker;
use super::runtime;

const SENTINEL_OPENAI: &str = "sk-b1-packaged-openai-sentinel-do-not-leak";
const SENTINEL_GROK: &str = "sk-b1-packaged-grok-sentinel-do-not-leak";
const SENTINEL_PROXY: &str = "http://127.0.0.1:9/b1-packaged-proxy-sentinel";

const PAGE_HTML: &str = r#"<!DOCTYPE html>
<html lang="en">
  <head><meta charset="utf-8" /><title>GrokCuPackagedContract</title></head>
  <body>
    <h1 data-gate="packaged-contract">packaged-contract</h1>
    <button id="inc" type="button">+1</button>
    <output id="count">0</output>
    <script>
      let n = 0;
      document.getElementById("inc").onclick = () => {
        n += 1;
        document.getElementById("count").textContent = String(n);
        fetch("/oracle", {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({ count: n }),
        }).catch(() => {});
      };
    </script>
  </body>
</html>
"#;

pub fn run_browser_packaged_contract_gate() -> Result<(), String> {
    let home = std::env::temp_dir().join(format!(
        "grok-cu-b1-packaged-{}-{}",
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
    println!("gate: browser_packaged_contract start");
    println!("gate: isolated_home={}", home.display());
    println!(
        "gate: GROK_CU_NODE_FILE_set={}",
        u8::from(std::env::var_os("GROK_CU_NODE_FILE").is_some())
    );
    println!(
        "gate: NODE_PATH_set={}",
        u8::from(std::env::var_os("NODE_PATH").is_some())
    );

    let result = run_isolated_contract(&home);
    std::thread::sleep(Duration::from_millis(400));
    let leftover = processes_under(&home);
    for (pid, path) in &leftover {
        terminate_pid(*pid);
        println!("gate: leftover pid={pid} image={path}");
    }
    if !leftover.is_empty() {
        std::thread::sleep(Duration::from_millis(200));
    }
    let leftover_after = processes_under(&home);
    drop(env);
    let cleanup = std::fs::remove_dir_all(&home);
    let home_gone = !home.exists();
    println!("gate: leftover_under_home={}", leftover_after.len());
    println!("gate: isolated_home_removed={}", u8::from(home_gone));

    if !leftover_after.is_empty() {
        return Err(redact(&format!(
            "leftover processes under isolated home: {leftover_after:?}; inner={result:?}"
        )));
    }
    if let Err(error) = cleanup {
        if error.kind() != std::io::ErrorKind::NotFound {
            return Err(redact(&format!(
                "remove isolated home: {error}; inner={result:?}"
            )));
        }
    }
    result.map_err(|e| redact(&e))
}

fn apply_isolated_env(env: &mut EnvGuard, home: &Path) -> Result<(), String> {
    let decoy = home.join("path-decoy");
    std::fs::create_dir_all(&decoy).map_err(|e| e.to_string())?;
    std::fs::write(decoy.join("node.exe"), b"SYSTEM-NODE-DECOY").map_err(|e| e.to_string())?;
    std::fs::write(decoy.join("node"), b"SYSTEM-NODE-DECOY").map_err(|e| e.to_string())?;
    std::fs::write(decoy.join("chrome.exe"), b"SYSTEM-CHROME-DECOY").map_err(|e| e.to_string())?;
    println!("gate: path_decoy={}", decoy.display());

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
        "NPM_CONFIG_PREFIX",
        "npm_config_prefix",
        "NPM_CONFIG_CACHE",
        "npm_config_cache",
        "NPM_CONFIG_USERCONFIG",
        "npm_config_userconfig",
        "NPM_CONFIG_GLOBALCONFIG",
        "npm_config_globalconfig",
    ] {
        env.remove(key);
    }
    Ok(())
}

fn run_isolated_contract(home: &Path) -> Result<(), String> {
    if std::env::var_os("GROK_CU_NODE_FILE").is_some() {
        return Err("GROK_CU_NODE_FILE must stay unset for packaged contract".into());
    }
    if std::env::var_os("NODE_PATH").is_some() {
        return Err("NODE_PATH must stay unset for packaged contract".into());
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

    let node = runtime::resolve(runtime::JS_RUNTIME)?;
    let playwright = runtime::resolve(runtime::PLAYWRIGHT)?;
    if !path_in(home, &node) {
        return Err(format!(
            "repaired js-runtime escaped isolated home: {}",
            node.display()
        ));
    }
    if !path_in(home, &playwright) {
        return Err(format!(
            "repaired playwright escaped isolated home: {}",
            playwright.display()
        ));
    }
    let pw_bytes = std::fs::read(&playwright).map_err(|e| e.to_string())?;
    let pw_tgz = pw_bytes.starts_with(&[0x1f, 0x8b]);
    println!(
        "gate: playwright={} bytes={} tgz={}",
        playwright.display(),
        pw_bytes.len(),
        u8::from(pw_tgz)
    );

    let req = product_spawn_request()?;
    assert_product_paths(home, &req)?;
    let worker_bytes = std::fs::read(&req.script).map_err(|e| e.to_string())?;
    let placeholder = worker_bytes.as_slice() == PLACEHOLDER_PLAYWRIGHT
        || grok_computer_use_core::runtime::is_placeholder_playwright(&worker_bytes);
    println!("gate: spawn node={}", req.node.display());
    println!("gate: spawn script={}", req.script.display());
    println!(
        "gate: worker_bytes={} placeholder={}",
        worker_bytes.len(),
        u8::from(placeholder)
    );
    println!("gate: profile_root={}", req.profile_root.display());
    if !path_in(home, &req.profile_root) {
        return Err(format!(
            "product profile root escaped isolated home: {}",
            req.profile_root.display()
        ));
    }

    let spawned = BrowserSupervisor::spawn(SpawnRequest {
        node: req.node.clone(),
        script: req.script.clone(),
        profile_root: req.profile_root.clone(),
        browser: req.browser.clone(),
    });
    let supervisor = match spawned {
        Ok(supervisor) => supervisor,
        Err(error) => {
            println!("gate: spawn FAIL: {}", redact(&error));
            return Err(error);
        }
    };
    let pid = supervisor.pid();
    println!("gate: worker_pid={pid}");
    let outcome = run_live_contract(&supervisor, home);
    let shutdown = supervisor.shutdown();
    std::thread::sleep(Duration::from_millis(300));
    let still_alive = process_alive(pid);
    println!(
        "gate: worker_pid_alive_after_shutdown={}",
        u8::from(still_alive)
    );
    outcome?;
    shutdown?;
    if still_alive {
        return Err(format!("worker pid {pid} still alive after shutdown"));
    }
    Ok(())
}

fn run_live_contract(supervisor: &BrowserSupervisor, home: &Path) -> Result<(), String> {
    let health = supervisor.health()?;
    deny_leaks(&health.to_string())?;
    assert_handshake(&health)?;
    println!("gate: handshake {health}");

    let worker = LoopbackPlaywrightWorker::new(supervisor.base_url(), supervisor.token());
    let oracle = Arc::new(AtomicU64::new(0));
    let (page_url, stop, page_thread) = serve_contract_page(oracle.clone())?;
    deny_leaks(&page_url)?;

    let result: Result<(), String> = (|| {
        let opened = worker
            .open_profile("packaged-contract", "contract-profile")
            .map_err(|e| e.to_string())?;
        deny_leaks(&format!("{opened:?}"))?;
        if !path_in(home, supervisor.profile_root()) {
            return Err(format!(
                "live profile root escaped isolated home: {}",
                supervisor.profile_root().display()
            ));
        }
        let landed = worker
            .goto(
                "packaged-contract",
                "contract-profile",
                &opened.page.page_id,
                opened.page.page_generation,
                "packaged-goto",
                &page_url,
            )
            .map_err(|e| e.to_string())?;
        let mut generation = landed.page_generation;
        let mut observed = None;
        for _ in 0..6 {
            match worker.observe_page(
                "packaged-contract",
                "contract-profile",
                &landed.page_id,
                generation,
            ) {
                Ok(obs) => {
                    observed = Some(obs);
                    break;
                }
                Err(error)
                    if error.code == "observation_changed_during_capture"
                        || error.code == "stale_observation_generation" =>
                {
                    if let Some(current) = error.current_page_generation {
                        generation = current;
                    }
                }
                Err(error) => return Err(error.to_string()),
            }
        }
        let observed = observed.ok_or_else(|| "observe did not settle".to_string())?;
        deny_leaks(&format!("{observed:?}"))?;
        if observed.snapshot_id.trim().is_empty() {
            return Err("observe missing snapshotId".to_string());
        }
        let png = observed
            .png_base64
            .as_deref()
            .ok_or_else(|| "observe missing PNG".to_string())?;
        if !png.starts_with("iVBOR") {
            return Err("PNG signature missing".to_string());
        }
        let plus = observed
            .nodes
            .iter()
            .find(|node| node.name.contains("+1") && !node.truncated)
            .ok_or_else(|| format!("missing +1 in {:?}", observed.nodes))?;
        let acted = worker
            .act_page(ManagedWorkerAction {
                owner: "packaged-contract",
                profile: "contract-profile",
                page: grok_computer_use_core::browser::ManagedPageRef {
                    page_id: &observed.page_id,
                    page_generation: observed.page_generation,
                },
                snapshot_id: &observed.snapshot_id,
                action_id: "packaged-click",
                kind: "click",
                locator: &ManagedLocator {
                    element_ref: Some(plus.element_ref.clone()),
                },
                params: &json!({}),
            })
            .map_err(|e| e.to_string())?;
        let mut saw_oracle = false;
        for _ in 0..40 {
            if oracle.load(Ordering::SeqCst) == 1 {
                saw_oracle = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        if !saw_oracle {
            return Err(format!(
                "independent oracle count={} after opaque-ref click",
                oracle.load(Ordering::SeqCst)
            ));
        }
        let mut verify_generation = acted.page_generation.max(observed.page_generation);
        let mut verified = None;
        for _ in 0..6 {
            match worker.observe_page(
                "packaged-contract",
                "contract-profile",
                &acted.page_id,
                verify_generation,
            ) {
                Ok(obs) => {
                    verified = Some(obs);
                    break;
                }
                Err(error)
                    if error.code == "observation_changed_during_capture"
                        || error.code == "stale_observation_generation" =>
                {
                    if let Some(current) = error.current_page_generation {
                        verify_generation = current;
                    }
                }
                Err(error) => return Err(error.to_string()),
            }
        }
        let verified = verified.ok_or_else(|| "verify observe did not settle".to_string())?;
        deny_leaks(&format!("{verified:?}"))?;
        if verified.snapshot_id == observed.snapshot_id {
            return Err("verify observe must mint a new snapshot".to_string());
        }
        if verified.page_id != observed.page_id {
            return Err("pageId changed across opaque-ref act".to_string());
        }
        Ok(())
    })();

    stop.store(true, Ordering::SeqCst);
    let _ = page_thread.join();
    result
}

fn serve_contract_page(
    oracle: Arc<AtomicU64>,
) -> Result<(String, Arc<AtomicBool>, std::thread::JoinHandle<()>), String> {
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
    let page_port = listener.local_addr().map_err(|e| e.to_string())?.port();
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let stop = Arc::new(AtomicBool::new(false));
    let stop_t = stop.clone();
    let html = PAGE_HTML.as_bytes();
    let page_thread = std::thread::spawn(move || {
        while !stop_t.load(Ordering::SeqCst) {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let mut buf = [0_u8; 8192];
                    let n = stream.read(&mut buf).unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]);
                    if req.starts_with("POST /oracle") {
                        if let Some(body) = req.split("\r\n\r\n").nth(1) {
                            if let Ok(value) =
                                serde_json::from_str::<Value>(body.trim_end_matches('\0').trim())
                            {
                                if let Some(count) = value.get("count").and_then(Value::as_u64) {
                                    oracle.store(count, Ordering::SeqCst);
                                }
                            }
                        }
                        let _ = stream
                            .write_all(b"HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n");
                    } else {
                        let header = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            html.len()
                        );
                        let _ = stream.write_all(header.as_bytes());
                        let _ = stream.write_all(html);
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
        format!("http://127.0.0.1:{page_port}/?run=packaged-contract"),
        stop,
        page_thread,
    ))
}

fn assert_product_paths(home: &Path, req: &SpawnRequest) -> Result<(), String> {
    if !path_in(home, &req.node) {
        return Err(format!(
            "product node escaped isolated home: {}",
            req.node.display()
        ));
    }
    if !path_in(home, &req.script) {
        return Err(format!(
            "product worker escaped isolated home: {}",
            req.script.display()
        ));
    }
    let script = normalize_path(&req.script);
    if script.contains("/tools/computer-use-browser/") {
        return Err("product spawn used source tools worker".into());
    }
    if script.ends_with("/server.mjs") {
        return Err("product spawn used source server.mjs".into());
    }
    let node = normalize_path(&req.node);
    if node.contains("/program files/nodejs/") || node.contains("/program files (x86)/nodejs/") {
        return Err("product spawn used system node".into());
    }
    if node.contains("/path-decoy/") {
        return Err("product spawn used PATH decoy node".into());
    }
    Ok(())
}

fn assert_handshake(health: &Value) -> Result<(), String> {
    if health.get("ok") != Some(&json!(true)) {
        return Err(format!("handshake ok missing: {health}"));
    }
    if health.get("protocol") != Some(&json!(1)) {
        return Err(format!("handshake protocol missing: {health}"));
    }
    let build = nonempty_str(health, "build")?;
    if build.is_empty() {
        return Err(format!("handshake build missing: {health}"));
    }
    let runtime = nonempty_str(health, "runtime")?;
    if !runtime.starts_with("v20.18.") {
        return Err(format!("handshake node version {runtime}"));
    }
    let source = health
        .get("sourceHash")
        .or_else(|| health.get("workerSourceSha256"))
        .and_then(Value::as_str)
        .unwrap_or("");
    if source.len() != 64 || !source.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(format!("handshake source hash missing: {health}"));
    }
    let pw = health
        .get("playwrightVersion")
        .and_then(Value::as_str)
        .unwrap_or("");
    if pw != "1.48.0" {
        return Err(format!("handshake playwright version {pw}"));
    }
    let browser = health
        .get("browserVersion")
        .or_else(|| health.pointer("/browser/version"))
        .and_then(Value::as_str)
        .unwrap_or("");
    if browser.is_empty() {
        return Err(format!("handshake browser version missing: {health}"));
    }
    if health.get("page").and_then(|v| v.get("observe")) != Some(&json!(true))
        || health.get("page").and_then(|v| v.get("act")) != Some(&json!(true))
    {
        return Err(format!("handshake page caps missing: {health}"));
    }
    Ok(())
}

fn nonempty_str<'a>(health: &'a Value, key: &str) -> Result<&'a str, String> {
    health
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| format!("handshake {key} missing: {health}"))
}

fn deny_leaks(text: &str) -> Result<(), String> {
    for needle in [SENTINEL_OPENAI, SENTINEL_GROK, SENTINEL_PROXY] {
        if text.contains(needle) {
            return Err("sentinel leaked into packaged-contract output".into());
        }
    }
    let lower = text.replace('\\', "/").to_ascii_lowercase();
    if lower.contains("/tools/computer-use-browser/") {
        return Err("source worker path leaked into packaged-contract output".into());
    }
    Ok(())
}

fn redact(text: &str) -> String {
    text.replace(SENTINEL_OPENAI, "[redacted-openai]")
        .replace(SENTINEL_GROK, "[redacted-grok]")
        .replace(SENTINEL_PROXY, "[redacted-proxy]")
}

fn path_in(root: &Path, child: &Path) -> bool {
    let root = normalize_path(root);
    let child = normalize_path(child);
    child == root || child.starts_with(&(root.clone() + "/"))
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

pub(crate) fn processes_under(root: &Path) -> Vec<(u32, String)> {
    #[cfg(windows)]
    {
        live_images()
            .into_iter()
            .filter(|(_, image)| path_in(root, Path::new(image)))
            .collect()
    }
    #[cfg(not(windows))]
    {
        let _ = root;
        Vec::new()
    }
}

pub(crate) fn terminate_pid(pid: u32) {
    #[cfg(windows)]
    {
        use windows::Win32::Foundation::CloseHandle;
        use windows::Win32::System::Threading::{OpenProcess, TerminateProcess, PROCESS_TERMINATE};
        unsafe {
            if let Ok(handle) = OpenProcess(PROCESS_TERMINATE, false, pid) {
                let _ = TerminateProcess(handle, 1);
                let _ = CloseHandle(handle);
            }
        }
    }
    let _ = pid;
}

#[cfg(windows)]
fn live_images() -> Vec<(u32, String)> {
    use windows::core::PWSTR;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
            return Vec::new();
        };
        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        let mut out = Vec::new();
        if Process32FirstW(snap, &mut entry).is_ok() {
            loop {
                let pid = entry.th32ProcessID;
                if pid != 0 && pid != std::process::id() {
                    if let Ok(handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
                        let mut buf = [0u16; 1024];
                        let mut len = buf.len() as u32;
                        if QueryFullProcessImageNameW(
                            handle,
                            PROCESS_NAME_WIN32,
                            PWSTR(buf.as_mut_ptr()),
                            &mut len,
                        )
                        .is_ok()
                        {
                            let image = String::from_utf16_lossy(&buf[..len as usize]);
                            out.push((pid, image));
                        }
                        let _ = CloseHandle(handle);
                    }
                }
                if Process32NextW(snap, &mut entry).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snap);
        out
    }
}
