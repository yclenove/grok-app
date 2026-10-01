use super::{force_foreground, oracle_path, FixtureWindow};
use std::time::Duration;

/// S4.1: elementRef is UIA RuntimeId + pid + window stamp, not HWND/title.
/// Postcondition is fixture click count, not adapter ok.
pub fn run_uia_element_tree() -> Result<(), String> {
    use crate::computer_use::adapter::{ActionScope, ComputerUseAdapter, DispatchRequest};
    use crate::computer_use::protocol::{ActionKind, ActionTarget};
    use crate::computer_use::windows_adapter::WindowsAdapter;
    use crate::computer_use::windows_identity;
    use crate::computer_use::windows_uia;

    let title = format!("GrokCuFixture-uia-{}", std::process::id());
    let fx = FixtureWindow::spawn(&title)?;
    std::thread::sleep(Duration::from_millis(250));
    let adapter = WindowsAdapter::new();
    let listed = adapter.list_targets().map_err(|e| e.to_string())?;
    let target = match listed.iter().find(|t| t.title.contains(&title)) {
        Some(t) => t.clone(),
        None => {
            fx.close();
            return Err(format!("UIA fixture not listed: {title}"));
        }
    };
    let Some((pid, _, stamp)) = windows_identity::parse_target_id(&target.target_id) else {
        fx.close();
        return Err(format!("bad target_id {}", target.target_id));
    };
    let obs = match adapter.observe(&target.target_id) {
        Ok(obs) => obs,
        Err(e) => {
            fx.close();
            return Err(e);
        }
    };
    let operable: Vec<_> = obs
        .nodes
        .iter()
        .filter(|n| !n.truncated && n.role != "meta")
        .cloned()
        .collect();
    if operable.is_empty() {
        fx.close();
        return Err(format!("UIA tree empty; nodes={:?}", obs.nodes));
    }
    for node in &operable {
        if node.node_ref.starts_with("el:") || node.node_ref.eq_ignore_ascii_case("count") {
            fx.close();
            return Err(format!(
                "Win32 HWND/title impersonating UIA: {}",
                node.node_ref
            ));
        }
        let Some((ref_pid, ref_stamp, _)) = windows_uia::parse_element_ref(&node.node_ref) else {
            fx.close();
            return Err(format!("not RuntimeId elementRef: {}", node.node_ref));
        };
        if ref_pid != pid || ref_stamp != stamp {
            fx.close();
            return Err(format!(
                "elementRef stamp mismatch {} vs {pid}:{stamp}",
                node.node_ref
            ));
        }
    }
    let Some(count) = operable
        .iter()
        .find(|n| n.name.eq_ignore_ascii_case("count"))
    else {
        fx.close();
        return Err(format!(
            "Count missing from UIA tree; {:?}",
            operable
                .iter()
                .map(|n| format!("{}:{}", n.role, n.name))
                .collect::<Vec<_>>()
        ));
    };
    if !operable.iter().any(|n| n.role == "edit") {
        fx.close();
        return Err("edit control missing from UIA tree".into());
    }
    let mk = |id: &str, element_ref: &str| DispatchRequest {
        managed_request: None,
        cancellation: Default::default(),
        run_id: "uia-run".into(),
        action_id: id.into(),
        generation: 1,
        target_id: target.target_id.clone(),
        target_generation: 1,
        snapshot_id: obs.snapshot_id.clone(),
        geometry_revision: obs.geometry_revision,
        action: ActionKind::Click,
        target: ActionTarget::Element {
            element_ref: element_ref.into(),
        },
        parameters: serde_json::json!({}),
        scope: ActionScope::Directed,
    };
    let before = fx.clicks();
    let title_click = adapter.act(&mk("title-impersonation", "count"));
    if title_click.is_ok() {
        fx.close();
        return Err(format!(
            "title 'count' must not dispatch, got {title_click:?}"
        ));
    }
    let hwnd_click = adapter.act(&mk("hwnd-impersonation", "el:101"));
    if hwnd_click.is_ok() {
        fx.close();
        return Err(format!("el:101 must not dispatch, got {hwnd_click:?}"));
    }
    if fx.clicks() != before {
        fx.close();
        return Err("HWND/title impersonation moved the fixture".into());
    }
    force_foreground(fx.hwnd());
    let uia_click = adapter.act(&mk("uia-count", &count.node_ref));
    std::thread::sleep(Duration::from_millis(200));
    let after = fx.clicks();
    fx.close();
    if uia_click.is_err() {
        return Err(format!("UIA Count click failed: {uia_click:?}"));
    }
    if after <= before {
        return Err(format!(
            "UIA click postcondition clicks {before}->{after} outcome={uia_click:?}"
        ));
    }
    println!("gate: windows_uia_element_tree count={}", count.node_ref);
    Ok(())
}

/// S4.2: window move invalidates coordinates; minimized capture is refused.
/// Postcondition is fixture click count, not API ok.
pub fn run_geometry_capture() -> Result<(), String> {
    use crate::computer_use::adapter::ComputerUseAdapter;
    use crate::computer_use::broker::{BrokerOptions, ComputerUseBroker};
    use crate::computer_use::protocol::{
        ActionKind, ActionRequest, ActionTarget, OutcomeKind, PROTOCOL_VERSION,
    };
    use crate::computer_use::windows_adapter::{self, WindowsAdapter};
    use std::sync::Arc;
    use uuid::Uuid;
    use windows::Win32::Foundation::RECT;
    use windows::Win32::UI::WindowsAndMessaging::{
        GetSystemMetrics, GetWindowRect, SetWindowPos, ShowWindow, SM_XVIRTUALSCREEN,
        SM_YVIRTUALSCREEN, SWP_NOSIZE, SWP_NOZORDER, SW_MINIMIZE, SW_RESTORE,
    };

    let title = format!("GrokCuFixture-geo-{}", std::process::id());
    let fx = FixtureWindow::spawn(&title)?;
    std::thread::sleep(Duration::from_millis(250));
    force_foreground(fx.hwnd());
    let adapter = Arc::new(WindowsAdapter::new());
    let opts = BrokerOptions {
        feature_enabled: true,
        lease_path: std::env::temp_dir().join(format!(
            "grok-cu-geo-{}-{}.lease",
            std::process::id(),
            Uuid::new_v4()
        )),
        ..BrokerOptions::default()
    };
    let broker = ComputerUseBroker::new(adapter.clone(), opts);
    broker
        .open_run("geo-sess", "geo-run")
        .map_err(|e| e.to_string())?;
    let listed = broker.list_targets("geo-run").map_err(|e| e.to_string())?;
    let target = listed
        .iter()
        .find(|t| t.title.contains(&title))
        .ok_or_else(|| format!("geometry fixture not listed: {title}"))?;
    let gen = broker
        .authorize_target("geo-run", &target.target_id)
        .map_err(|e| e.to_string())?;
    let obs = broker.observe("geo-run").map_err(|e| e.to_string())?;
    if obs.image.width == 0 || obs.image.height == 0 || obs.image.png_base64.is_none() {
        fx.close();
        return Err(format!(
            "capture missing visual {}x{}",
            obs.image.width, obs.image.height
        ));
    }
    let dpi = fx.dpi() as f64;
    if (obs.dpi - dpi).abs() > 0.5 {
        fx.close();
        return Err(format!("observation dpi {} != window {dpi}", obs.dpi));
    }
    if (obs.scale - dpi / 96.0).abs() > 0.01 {
        fx.close();
        return Err(format!("scale {} != dpi/96", obs.scale));
    }
    let geo_before = windows_adapter::window_geometry_revision(fx.hwnd());
    if obs.geometry_revision != geo_before {
        fx.close();
        return Err(format!(
            "observe geometry {} != window {}",
            obs.geometry_revision, geo_before
        ));
    }
    let vx = unsafe { GetSystemMetrics(SM_XVIRTUALSCREEN) };
    let vy = unsafe { GetSystemMetrics(SM_YVIRTUALSCREEN) };
    println!(
        "geo capture: {}x{} dpi={} origin=({},{}) virtual=({},{})",
        obs.image.width, obs.image.height, obs.dpi, obs.origin_x, obs.origin_y, vx, vy
    );

    let mut rc = RECT::default();
    unsafe {
        GetWindowRect(fx.hwnd(), &mut rc).map_err(|e| e.to_string())?;
    }
    let nx = rc.left + 72;
    let ny = rc.top + 48;
    unsafe {
        SetWindowPos(fx.hwnd(), None, nx, ny, 0, 0, SWP_NOSIZE | SWP_NOZORDER)
            .map_err(|e| e.to_string())?;
    }
    std::thread::sleep(Duration::from_millis(80));
    let geo_after = windows_adapter::window_geometry_revision(fx.hwnd());
    if geo_after == geo_before {
        fx.close();
        return Err("window move did not change geometry revision".into());
    }
    let before_clicks = fx.clicks();
    let stale = broker.act(ActionRequest {
        version: PROTOCOL_VERSION,
        action_id: "stale-geo-click".into(),
        run_id: "geo-run".into(),
        target_id: target.target_id.clone(),
        target_generation: gen,
        snapshot_id: obs.snapshot_id.clone(),
        geometry_revision: obs.geometry_revision,
        action: ActionKind::Click,
        target: ActionTarget::Coord { x: 24.0, y: 24.0 },
        parameters: serde_json::json!({}),
    });
    if stale.kind != OutcomeKind::Rejected || stale.executed {
        fx.close();
        return Err(format!(
            "moved window must reject old coords, got {stale:?}"
        ));
    }
    if fx.clicks() != before_clicks {
        fx.close();
        return Err("stale geometry click moved the fixture".into());
    }

    unsafe {
        let _ = ShowWindow(fx.hwnd(), SW_MINIMIZE);
    }
    std::thread::sleep(Duration::from_millis(80));
    match broker.observe("geo-run") {
        Ok(_) => {
            unsafe {
                let _ = ShowWindow(fx.hwnd(), SW_RESTORE);
            }
            fx.close();
            return Err("minimized window must not capture".into());
        }
        Err(e) => {
            let text = e.to_string();
            if !text.to_ascii_lowercase().contains("minimized") {
                unsafe {
                    let _ = ShowWindow(fx.hwnd(), SW_RESTORE);
                }
                fx.close();
                return Err(format!(
                    "minimized observe error should say minimized, got {text}"
                ));
            }
        }
    }
    unsafe {
        let _ = ShowWindow(fx.hwnd(), SW_RESTORE);
    }
    fx.close();
    println!("gate: windows_geometry_capture");
    Ok(())
}

/// S4.3: UIA Invoke/Value first; SendInput only after foreground recheck.
/// Postcondition is fixture clicks/edit text, not API ok.
pub fn run_uia_full_actions() -> Result<(), String> {
    use crate::computer_use::adapter::{ActionScope, ComputerUseAdapter, DispatchRequest};
    use crate::computer_use::protocol::{ActionKind, ActionTarget};
    use crate::computer_use::windows_adapter::WindowsAdapter;

    let title = format!("GrokCuFixture-act-{}", std::process::id());
    let fx = FixtureWindow::spawn(&title)?;
    std::thread::sleep(Duration::from_millis(250));
    force_foreground(fx.hwnd());
    let adapter = WindowsAdapter::new();
    let listed = adapter.list_targets().map_err(|e| e.to_string())?;
    let target = listed
        .iter()
        .find(|t| t.title.contains(&title))
        .ok_or("action fixture not listed")?
        .clone();
    let obs = adapter
        .observe(&target.target_id)
        .map_err(|e| e.to_string())?;
    let count_ref = obs
        .nodes
        .iter()
        .find(|n| n.name.eq_ignore_ascii_case("count"))
        .map(|n| n.node_ref.clone())
        .ok_or("Count missing")?;
    let edit_ref = obs
        .nodes
        .iter()
        .find(|n| n.role == "edit")
        .map(|n| n.node_ref.clone())
        .ok_or("edit missing")?;
    let mk = |id: &str, action: ActionKind, target_el: &str, parameters: serde_json::Value| {
        DispatchRequest {
            managed_request: None,
            cancellation: Default::default(),
            run_id: "act-run".into(),
            action_id: id.into(),
            generation: 1,
            target_id: target.target_id.clone(),
            target_generation: 1,
            snapshot_id: obs.snapshot_id.clone(),
            geometry_revision: obs.geometry_revision,
            action,
            target: ActionTarget::Element {
                element_ref: target_el.into(),
            },
            parameters,
            scope: ActionScope::Directed,
        }
    };
    let before = fx.clicks();
    let click = match adapter.act(&mk(
        "uia-click",
        ActionKind::Click,
        &count_ref,
        serde_json::json!({}),
    )) {
        Ok(v) => v,
        Err(e) => {
            fx.close();
            return Err(format!("UIA click: {e}"));
        }
    };
    std::thread::sleep(Duration::from_millis(120));
    if !click.detail.contains("uia") {
        let after = fx.clicks();
        fx.close();
        return Err(format!(
            "Count click must use UIA first, got detail={} clicks {before}->{after}",
            click.detail
        ));
    }
    let after_click = fx.clicks();
    if after_click <= before {
        fx.close();
        return Err(format!(
            "UIA Invoke postcondition clicks {before}->{after_click}"
        ));
    }
    let set = match adapter.act(&mk(
        "uia-set",
        ActionKind::SetValue,
        &edit_ref,
        serde_json::json!({ "text": "你好世界" }),
    )) {
        Ok(v) => v,
        Err(e) => {
            fx.close();
            return Err(format!("UIA set_value: {e}"));
        }
    };
    std::thread::sleep(Duration::from_millis(80));
    if !set.detail.contains("uia") {
        fx.close();
        return Err(format!("set_value must use UIA Value, got {}", set.detail));
    }
    let edit = fx.edit_text();
    if !edit.contains("你好世界") {
        fx.close();
        return Err(format!("UIA Value postcondition edit={edit:?}"));
    }

    let distractor = FixtureWindow::spawn(&format!("GrokCuDistract-{}", std::process::id()))?;
    std::thread::sleep(Duration::from_millis(150));
    force_foreground(distractor.hwnd());
    std::thread::sleep(Duration::from_millis(80));
    let before_fallback = fx.clicks();
    let drifted = adapter.act(&DispatchRequest {
        managed_request: None,
        cancellation: Default::default(),
        run_id: "act-run".into(),
        action_id: "fallback-coord".into(),
        generation: 1,
        target_id: target.target_id.clone(),
        target_generation: 1,
        snapshot_id: obs.snapshot_id.clone(),
        geometry_revision: obs.geometry_revision,
        action: ActionKind::Click,
        target: ActionTarget::Coord { x: 24.0, y: 24.0 },
        parameters: serde_json::json!({}),
        scope: ActionScope::Directed,
    });
    let after_fallback = fx.clicks();
    distractor.close();
    fx.close();
    match drifted {
        Err(e) if e.to_ascii_lowercase().contains("focus") => {}
        other => {
            return Err(format!(
                "coord fallback without foreground must fail, got {other:?}"
            ))
        }
    }
    if after_fallback != before_fallback {
        return Err(format!(
            "fallback without recheck clicked {before_fallback}->{after_fallback}"
        ));
    }
    println!("gate: windows_uia_full_actions");
    Ok(())
}
