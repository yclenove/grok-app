use super::{force_foreground, oracle_path, FixtureWindow};
use std::time::Duration;
use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

/// R1 native: distractor takes focus; click must not increment the fixture file.
pub fn run_focus_drift_pauses() -> Result<(), String> {
    use crate::computer_use::adapter::ComputerUseAdapter;
    use crate::computer_use::broker::{BrokerOptions, ComputerUseBroker};
    use crate::computer_use::protocol::{
        ActionKind, ActionRequest, ActionTarget, OutcomeKind, PROTOCOL_VERSION,
    };
    use crate::computer_use::windows_adapter::WindowsAdapter;
    use std::sync::Arc;
    use uuid::Uuid;

    let title = format!("GrokCuFixture-focus-{}", std::process::id());
    let clicks_path = oracle_path("grok-cu-fixture-focus-clicks.txt");
    let _ = std::fs::write(&clicks_path, "0");
    let fx = FixtureWindow::spawn(&title)?;
    std::thread::sleep(Duration::from_millis(200));
    let adapter = Arc::new(WindowsAdapter::new());
    let broker = ComputerUseBroker::new(
        adapter,
        BrokerOptions {
            feature_enabled: true,
            lease_path: std::env::temp_dir().join(format!(
                "grok-cu-focus-{}-{}.lease",
                std::process::id(),
                Uuid::new_v4()
            )),
            ..BrokerOptions::default()
        },
    );
    broker
        .open_run("focus-sess", "focus-run")
        .map_err(|e| e.to_string())?;
    let listed = broker
        .list_targets("focus-run")
        .map_err(|e| e.to_string())?;
    let target = listed
        .iter()
        .find(|t| t.title.contains(&title))
        .ok_or("focus fixture not listed")?
        .clone();
    let gen = broker
        .authorize_target("focus-run", &target.target_id)
        .map_err(|e| e.to_string())?;
    let obs = broker.observe("focus-run").map_err(|e| e.to_string())?;
    let count_ref = obs
        .nodes
        .iter()
        .find(|n| n.name.eq_ignore_ascii_case("count"))
        .map(|n| n.node_ref.clone())
        .unwrap_or_else(|| "count".into());
    force_foreground(fx.hwnd());
    let other = format!("GrokCuFixture-distract-{}", std::process::id());
    let distractor = FixtureWindow::spawn(&other)?;
    std::thread::sleep(Duration::from_millis(150));
    force_foreground(distractor.hwnd());
    std::thread::sleep(Duration::from_millis(80));
    let fg = unsafe { GetForegroundWindow() };
    if fg == fx.hwnd() {
        distractor.close();
        fx.close();
        println!("gate: windows_focus_drift native not_run (could not steal foreground)");
        return Ok(());
    }
    let before = fx.clicks();
    let out = broker.act(ActionRequest {
        version: PROTOCOL_VERSION,
        action_id: "drift-click".into(),
        run_id: "focus-run".into(),
        target_id: target.target_id.clone(),
        target_generation: gen,
        snapshot_id: obs.snapshot_id.clone(),
        geometry_revision: obs.geometry_revision,
        action: ActionKind::Click,
        target: ActionTarget::Element {
            element_ref: count_ref,
        },
        parameters: serde_json::json!({}),
    });
    std::thread::sleep(Duration::from_millis(120));
    let after = fx.clicks();
    let file_n = std::fs::read_to_string(&clicks_path)
        .unwrap_or_default()
        .trim()
        .parse::<u32>()
        .unwrap_or(0);
    distractor.close();
    fx.close();
    if out.executed || out.kind != OutcomeKind::Rejected {
        return Err(format!("focus drift native must reject, got {out:?}"));
    }
    if after != before || file_n != 0 {
        return Err(format!(
            "focus drift postcondition clicks={after} file={file_n} (must stay 0)"
        ));
    }
    println!("gate: windows_focus_drift_pauses");
    Ok(())
}

/// R7 native: takeover pauses; click postcondition stays unchanged.
pub fn run_user_takeover_pauses() -> Result<(), String> {
    use crate::computer_use::adapter::ComputerUseAdapter;
    use crate::computer_use::broker::{BrokerOptions, ComputerUseBroker};
    use crate::computer_use::protocol::{
        ActionKind, ActionRequest, ActionTarget, OutcomeKind, PROTOCOL_VERSION,
    };
    use crate::computer_use::windows_adapter::WindowsAdapter;
    use std::sync::Arc;
    use uuid::Uuid;

    let title = format!("GrokCuFixture-takeover-{}", std::process::id());
    let clicks_path = oracle_path("grok-cu-fixture-takeover-clicks.txt");
    let _ = std::fs::write(&clicks_path, "0");
    let fx = FixtureWindow::spawn(&title)?;
    std::thread::sleep(Duration::from_millis(200));
    let adapter = Arc::new(WindowsAdapter::new());
    let broker = ComputerUseBroker::new(
        adapter.clone(),
        BrokerOptions {
            feature_enabled: true,
            lease_path: std::env::temp_dir().join(format!(
                "grok-cu-takeover-{}-{}.lease",
                std::process::id(),
                Uuid::new_v4()
            )),
            ..BrokerOptions::default()
        },
    );
    broker
        .open_run("to-sess", "to-run")
        .map_err(|e| e.to_string())?;
    let listed = broker.list_targets("to-run").map_err(|e| e.to_string())?;
    let target = listed
        .iter()
        .find(|t| t.title.contains(&title))
        .ok_or("takeover fixture not listed")?;
    let gen = broker
        .authorize_target("to-run", &target.target_id)
        .map_err(|e| e.to_string())?;
    let obs = broker.observe("to-run").map_err(|e| e.to_string())?;
    broker
        .set_preview_visible("to-run", true)
        .map_err(|e| e.to_string())?;
    broker.takeover("to-run").map_err(|e| e.to_string())?;
    let before = fx.clicks();
    let out = broker.act(ActionRequest {
        version: PROTOCOL_VERSION,
        action_id: "takeover-click".into(),
        run_id: "to-run".into(),
        target_id: target.target_id.clone(),
        target_generation: gen,
        snapshot_id: obs.snapshot_id.clone(),
        geometry_revision: obs.geometry_revision,
        action: ActionKind::Click,
        target: ActionTarget::Element {
            element_ref: "count".into(),
        },
        parameters: serde_json::json!({}),
    });
    std::thread::sleep(Duration::from_millis(80));
    let after = fx.clicks();
    fx.close();
    if out.executed || out.kind != OutcomeKind::Rejected {
        return Err(format!("takeover must reject, got {out:?}"));
    }
    if after != before {
        return Err(format!("takeover postcondition clicks {before}->{after}"));
    }
    if adapter.periodic_preview_active() {
        return Err("takeover must stop preview capture".into());
    }
    let _ = clicks_path;
    println!("gate: windows_user_takeover_pauses");
    Ok(())
}
