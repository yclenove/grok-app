//! S4.5: Win32 dialog/virtual-screen, WPF, and WebView2 fixtures.
//! Postconditions are files or control values, not API ok.

#![cfg(all(target_os = "windows", feature = "computer-use-probe"))]

use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::Arc;
use std::time::{Duration, Instant};

use uuid::Uuid;

use super::adapter::{ComputerUseAdapter, TargetInfo};
use super::broker::{BrokerOptions, ComputerUseBroker};
use super::protocol::{
    ActionKind, ActionOutcome, ActionRequest, ActionTarget, Observation, OutcomeKind,
    PROTOCOL_VERSION,
};
use super::windows_adapter::WindowsAdapter;
use super::windows_fixture::{force_foreground, FixtureWindow};

pub fn run_windows_s45_fixtures() -> Result<(), String> {
    run_win32_dialog_and_virtual()?;
    run_wpf_fixture()?;
    run_webview2_fixture()?;
    println!("electron native fixture: not_run (no electron.exe; WebView2 covered hosted web UI)");
    println!("gate: windows_s45_fixtures");
    Ok(())
}

fn enabled_opts(tag: &str) -> BrokerOptions {
    BrokerOptions {
        feature_enabled: true,
        lease_path: std::env::temp_dir().join(format!(
            "grok-cu-s45-{tag}-{}-{}.lease",
            std::process::id(),
            Uuid::new_v4()
        )),
        ..BrokerOptions::default()
    }
}

fn wait_listed(
    adapter: &WindowsAdapter,
    needle: &str,
    owner_pid: u32,
    timeout: Duration,
) -> Result<TargetInfo, String> {
    let start = Instant::now();
    loop {
        if let Ok(list) = adapter.list_targets() {
            if let Some(t) = list.iter().find(|t| {
                t.title == needle
                    && super::windows_identity::parse_target_id(&t.target_id)
                        .is_some_and(|(pid, _, _)| pid == owner_pid)
            }) {
                return Ok(t.clone());
            }
        }
        if start.elapsed() >= timeout {
            return Err(format!("window {needle:?} not listed within {timeout:?}"));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn node_ref(
    obs: &Observation,
    pred: impl Fn(&super::protocol::ObservationNode) -> bool,
) -> Result<String, String> {
    obs.nodes
        .iter()
        .find(|n| pred(n))
        .map(|n| n.node_ref.clone())
        .ok_or_else(|| {
            format!(
                "node missing; names={:?}",
                obs.nodes.iter().map(|n| &n.name).collect::<Vec<_>>()
            )
        })
}

fn wait_foreground(
    adapter: &WindowsAdapter,
    hwnd: windows::Win32::Foundation::HWND,
    target_id: &str,
    timeout: Duration,
) -> Result<(), String> {
    let start = Instant::now();
    loop {
        let attempt = force_foreground(hwnd);
        if adapter.foreground_input_available(target_id) {
            return Ok(());
        }
        if start.elapsed() >= timeout {
            use windows::Win32::UI::WindowsAndMessaging::{
                GetForegroundWindow, GetWindowThreadProcessId, IsIconic, IsWindow, IsWindowVisible,
            };
            let (exists, visible, minimized, foreground_matches, foreground_is_owned) = unsafe {
                let foreground = GetForegroundWindow();
                let mut pid = 0;
                GetWindowThreadProcessId(foreground, Some(&mut pid));
                (
                    IsWindow(Some(hwnd)).as_bool(),
                    IsWindowVisible(hwnd).as_bool(),
                    IsIconic(hwnd).as_bool(),
                    foreground == hwnd,
                    pid == std::process::id(),
                )
            };
            return Err(format!(
                "isolated fixture {target_id} not foreground within {timeout:?}; \
                 alive={} exists={exists} visible={visible} minimized={minimized} \
                 foreground_matches={foreground_matches} foreground_is_owned={foreground_is_owned}; \
                 attach_foreground={:?} attach_target={:?} requested={} before_detach={} after_detach={}",
                adapter.target_alive(target_id), attempt.foreground_attach_error,
                attempt.target_attach_error, attempt.requested,
                attempt.active_before_detach, attempt.active_after_detach
            ));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn click_named(
    broker: &ComputerUseBroker,
    run_id: &str,
    target_id: &str,
    gen: u64,
    obs: &Observation,
    action_id: &str,
    name: &str,
) -> Result<ActionOutcome, String> {
    let element_ref = node_ref(obs, |n| n.name.eq_ignore_ascii_case(name))?;
    Ok(broker.act(ActionRequest {
        version: PROTOCOL_VERSION,
        action_id: action_id.into(),
        run_id: run_id.into(),
        target_id: target_id.into(),
        target_generation: gen,
        snapshot_id: obs.snapshot_id.clone(),
        geometry_revision: obs.geometry_revision,
        action: ActionKind::Click,
        target: ActionTarget::Element { element_ref },
        parameters: serde_json::json!({}),
    }))
}

struct NativeActionContext<'a> {
    broker: &'a ComputerUseBroker,
    run_id: &'a str,
    target_id: &'a str,
    generation: u64,
}

fn set_named(
    context: NativeActionContext<'_>,
    obs: &Observation,
    action_id: &str,
    role: &str,
    text: &str,
) -> Result<ActionOutcome, String> {
    let element_ref = node_ref(obs, |n| n.role == role)?;
    Ok(context.broker.act(ActionRequest {
        version: PROTOCOL_VERSION,
        action_id: action_id.into(),
        run_id: context.run_id.into(),
        target_id: context.target_id.into(),
        target_generation: context.generation,
        snapshot_id: obs.snapshot_id.clone(),
        geometry_revision: obs.geometry_revision,
        action: ActionKind::SetValue,
        target: ActionTarget::Element { element_ref },
        parameters: serde_json::json!({ "text": text }),
    }))
}

fn scroll_any(
    broker: &ComputerUseBroker,
    run_id: &str,
    target_id: &str,
    gen: u64,
    obs: &Observation,
    action_id: &str,
) -> Result<ActionOutcome, String> {
    let element_ref = node_ref(obs, |n| {
        !n.truncated && n.actions.iter().any(|a| a == "scroll")
    })?;
    Ok(broker.act(ActionRequest {
        version: PROTOCOL_VERSION,
        action_id: action_id.into(),
        run_id: run_id.into(),
        target_id: target_id.into(),
        target_generation: gen,
        snapshot_id: obs.snapshot_id.clone(),
        geometry_revision: obs.geometry_revision,
        action: ActionKind::Scroll,
        target: ActionTarget::Element { element_ref },
        parameters: serde_json::json!({ "delta": 400 }),
    }))
}

fn read_trimmed(path: &Path) -> String {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .trim()
        .to_string()
}

fn run_win32_dialog_and_virtual() -> Result<(), String> {
    let title = format!("GrokCuFixture-s45-{}", std::process::id());
    let dialog_file = std::env::temp_dir().join("grok-cu-fixture-dialog.txt");
    let clicks_file = std::env::temp_dir().join("grok-cu-fixture-clicks.txt");
    let _ = std::fs::write(&dialog_file, "");
    let _ = std::fs::write(&clicks_file, "0");
    let fx = FixtureWindow::spawn(&title)?;
    std::thread::sleep(Duration::from_millis(250));
    let (vx, vy) = fx.move_to_virtual_origin();
    force_foreground(fx.hwnd());
    let adapter = Arc::new(WindowsAdapter::new());
    let broker = ComputerUseBroker::new(adapter.clone(), enabled_opts("win32"));
    broker
        .open_run("s45-win", "s45-win-run")
        .map_err(|e| e.to_string())?;
    let target = wait_listed(&adapter, &title, std::process::id(), Duration::from_secs(5))?;
    println!("s45 stage: win32 foreground");
    wait_foreground(
        &adapter,
        fx.hwnd(),
        &target.target_id,
        Duration::from_secs(2),
    )?;
    let gen = broker
        .authorize_target("s45-win-run", &target.target_id)
        .map_err(|e| e.to_string())?;
    let obs = broker.observe("s45-win-run").map_err(|e| e.to_string())?;
    println!(
        "s45 win32 virtual origin want=({vx},{vy}) observed=({},{})",
        obs.origin_x, obs.origin_y
    );
    if (obs.origin_x - vx).abs() > 80 || (obs.origin_y - vy).abs() > 80 {
        return Err(format!(
            "observe origin ({},{}) not near virtual ({vx},{vy})",
            obs.origin_x, obs.origin_y
        ));
    }
    let clicked = click_named(
        &broker,
        "s45-win-run",
        &target.target_id,
        gen,
        &obs,
        "s45-win-click",
        "count",
    )?;
    if !clicked.executed {
        return Err(format!("win32 click not executed: {clicked:?}"));
    }
    std::thread::sleep(Duration::from_millis(120));
    let clicks = read_trimmed(&clicks_file);
    if clicks != "1" && fx.clicks() == 0 {
        return Err(format!("win32 click file={clicks} clicks={}", fx.clicks()));
    }
    let ask = click_named(
        &broker,
        "s45-win-run",
        &target.target_id,
        gen,
        &obs,
        "s45-win-ask",
        "ask",
    )?;
    if !ask.executed {
        return Err(format!("Ask not executed: {ask:?}"));
    }
    std::thread::sleep(Duration::from_millis(400));
    let dlg = wait_listed(
        &adapter,
        "GrokCuDialog",
        std::process::id(),
        Duration::from_secs(4),
    )?;
    let dlg_hwnd = super::windows_identity::parse_target_id(&dlg.target_id)
        .map(|(_, hwnd, _)| hwnd)
        .ok_or("dialog hwnd")?;
    println!("s45 stage: win32 dialog foreground");
    wait_foreground(&adapter, dlg_hwnd, &dlg.target_id, Duration::from_secs(2))?;
    let dlg_obs = adapter.observe(&dlg.target_id)?;
    let ok_ref = node_ref(&dlg_obs, |n| {
        n.role == "button"
            && (n.name == "OK" || n.name == "确定" || n.name.eq_ignore_ascii_case("ok"))
    })
    .or_else(|_| node_ref(&dlg_obs, |n| n.role == "button"))?;
    let dlg_click = adapter.act(&super::adapter::DispatchRequest {
        managed_request: None,
        cancellation: Default::default(),
        run_id: "s45-dlg".into(),
        action_id: "s45-dlg-ok".into(),
        generation: 1,
        target_id: dlg.target_id.clone(),
        target_generation: 1,
        snapshot_id: dlg_obs.snapshot_id,
        geometry_revision: dlg_obs.geometry_revision,
        action: ActionKind::Click,
        target: ActionTarget::Element {
            element_ref: ok_ref,
        },
        parameters: serde_json::json!({}),
        scope: super::adapter::ActionScope::Directed,
    });
    if let Err(e) = &dlg_click {
        return Err(format!("dialog OK click: {e}"));
    }
    let start = Instant::now();
    while read_trimmed(&dialog_file) != "ok" {
        if start.elapsed() > Duration::from_secs(4) {
            return Err(format!(
                "dialog file still {:?} after single click act={dlg_click:?}",
                read_trimmed(&dialog_file)
            ));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    drop(fx);
    println!("gate: windows_s45_win32_dialog_virtual");
    Ok(())
}

struct ChildGuard(Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn wpf_script() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("tools")
        .join("computer-use-fixtures")
        .join("windows")
        .join("wpf.ps1")
}

pub fn run_wpf_fixture() -> Result<(), String> {
    let title = format!("GrokCuWpf-{}", std::process::id());
    let oracle = std::env::temp_dir().join(format!(
        "grok-cu-wpf-{}-{}",
        std::process::id(),
        Uuid::new_v4()
    ));
    std::fs::create_dir_all(&oracle).map_err(|e| e.to_string())?;
    let script = wpf_script();
    if !script.is_file() {
        return Err(format!("missing WPF fixture script {}", script.display()));
    }
    let child = Command::new("powershell.exe")
        .args([
            "-STA",
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
            script.to_str().ok_or("wpf path")?,
        ])
        .env("GROK_CU_WPF_TITLE", &title)
        .env("GROK_CU_WPF_ORACLE", oracle.to_string_lossy().as_ref())
        .spawn()
        .map_err(|e| format!("spawn wpf: {e}"))?;
    let owner_pid = child.id();
    let _guard = ChildGuard(child);
    let adapter = Arc::new(WindowsAdapter::new());
    let target = wait_listed(&adapter, &title, owner_pid, Duration::from_secs(12))?;
    force_foreground(
        super::windows_identity::parse_target_id(&target.target_id)
            .map(|(_, hwnd, _)| hwnd)
            .ok_or("wpf hwnd")?,
    );
    let broker = ComputerUseBroker::new(adapter.clone(), enabled_opts("wpf"));
    broker
        .open_run("s45-wpf", "s45-wpf-run")
        .map_err(|e| e.to_string())?;
    let hwnd = super::windows_identity::parse_target_id(&target.target_id)
        .map(|(_, hwnd, _)| hwnd)
        .ok_or("wpf hwnd")?;
    wait_foreground(&adapter, hwnd, &target.target_id, Duration::from_secs(3))?;
    let gen = broker
        .authorize_target("s45-wpf-run", &target.target_id)
        .map_err(|e| e.to_string())?;
    let obs = broker.observe("s45-wpf-run").map_err(|e| e.to_string())?;
    let click = click_named(
        &broker,
        "s45-wpf-run",
        &target.target_id,
        gen,
        &obs,
        "wpf-click",
        "count",
    )?;
    if !click.executed {
        return Err(format!("wpf click not executed: {click:?}"));
    }
    std::thread::sleep(Duration::from_millis(200));
    if read_trimmed(&oracle.join("clicks.txt")) != "1" {
        return Err(format!(
            "wpf click file={}",
            read_trimmed(&oracle.join("clicks.txt"))
        ));
    }
    wait_foreground(&adapter, hwnd, &target.target_id, Duration::from_secs(2))?;
    let obs = broker.observe("s45-wpf-run").map_err(|e| e.to_string())?;
    let typed = set_named(
        NativeActionContext {
            broker: &broker,
            run_id: "s45-wpf-run",
            target_id: &target.target_id,
            generation: gen,
        },
        &obs,
        "wpf-cjk",
        "edit",
        "你好世界",
    )?;
    if typed.kind == OutcomeKind::Rejected && !typed.executed {
        return Err(format!("wpf set_value rejected: {typed:?}"));
    }
    std::thread::sleep(Duration::from_millis(200));
    if !read_trimmed(&oracle.join("edit.txt")).contains("你好") {
        return Err(format!(
            "wpf edit file={}",
            read_trimmed(&oracle.join("edit.txt"))
        ));
    }
    let scroll_obs = broker.observe("s45-wpf-run").map_err(|e| e.to_string())?;
    if !scroll_obs.nodes.iter().any(|node| {
        node.name.trim().is_empty() && !node.truncated && node.actions.iter().any(|a| a == "scroll")
    }) {
        return Err("unnamed WPF ScrollViewer was omitted from semantic observation".into());
    }
    let scroll_before = read_trimmed(&oracle.join("scroll.txt"))
        .parse::<i32>()
        .map_err(|_| "WPF scroll oracle missing")?;
    let scrolled = scroll_any(
        &broker,
        "s45-wpf-run",
        &target.target_id,
        gen,
        &scroll_obs,
        "wpf-scroll",
    )?;
    std::thread::sleep(Duration::from_millis(200));
    let scroll_after = read_trimmed(&oracle.join("scroll.txt"))
        .parse::<i32>()
        .map_err(|_| "WPF scroll oracle missing")?;
    if !scrolled.executed
        || scrolled.kind == OutcomeKind::Rejected
        || scrolled.reason.as_deref() != Some("windows uia scroll")
        || scroll_after <= scroll_before
    {
        return Err(format!(
            "WPF scroll down not proved: {scroll_before}->{scroll_after}, outcome={scrolled:?}"
        ));
    }
    println!("PASS native WPF semantic scroll down oracle={scroll_before}->{scroll_after}, no coordinate fallback");
    wait_foreground(&adapter, hwnd, &target.target_id, Duration::from_secs(2))?;
    let obs = broker.observe("s45-wpf-run").map_err(|e| e.to_string())?;
    let ask = click_named(
        &broker,
        "s45-wpf-run",
        &target.target_id,
        gen,
        &obs,
        "wpf-ask",
        "ask",
    )?;
    if !ask.executed {
        return Err(format!("wpf Ask not executed: {ask:?}"));
    }
    std::thread::sleep(Duration::from_millis(400));
    let dlg = wait_listed(
        &adapter,
        "GrokCuWpfDialog",
        owner_pid,
        Duration::from_secs(5),
    )?;
    let dlg_hwnd = super::windows_identity::parse_target_id(&dlg.target_id)
        .map(|(_, hwnd, _)| hwnd)
        .ok_or("wpf dialog hwnd")?;
    wait_foreground(&adapter, dlg_hwnd, &dlg.target_id, Duration::from_secs(2))?;
    let dlg_obs = adapter.observe(&dlg.target_id)?;
    let ok_ref = node_ref(&dlg_obs, |n| {
        n.role == "button" && (n.name == "WpfOK" || n.name == "OK" || n.name == "确定")
    })?;
    let dlg_click = adapter.act(&super::adapter::DispatchRequest {
        managed_request: None,
        cancellation: Default::default(),
        run_id: "s45-wpf-dlg".into(),
        action_id: "wpf-ok".into(),
        generation: 1,
        target_id: dlg.target_id.clone(),
        target_generation: 1,
        snapshot_id: dlg_obs.snapshot_id,
        geometry_revision: dlg_obs.geometry_revision,
        action: ActionKind::Click,
        target: ActionTarget::Element {
            element_ref: ok_ref,
        },
        parameters: serde_json::json!({}),
        scope: super::adapter::ActionScope::Directed,
    })?;
    let start = Instant::now();
    while read_trimmed(&oracle.join("dialog.txt")) != "ok" {
        if start.elapsed() > Duration::from_secs(4) {
            return Err(format!(
                "wpf dialog file={} after single click act={dlg_click:?}",
                read_trimmed(&oracle.join("dialog.txt"))
            ));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    println!("gate: windows_s45_wpf");
    Ok(())
}

fn run_webview2_fixture() -> Result<(), String> {
    super::windows_webview2_fixture::run()
}
