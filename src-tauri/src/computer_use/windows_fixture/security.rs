use super::{force_foreground, oracle_path, FixtureWindow};

/// S4.4: lock/UIPI/dead target/Stop fail closed. Postcondition is fixture clicks.
pub fn run_security_and_cancel() -> Result<(), String> {
    use crate::computer_use::adapter::{ActionScope, ComputerUseAdapter, DispatchRequest};
    use crate::computer_use::broker::{BrokerOptions, ComputerUseBroker, StopState};
    use crate::computer_use::protocol::{ActionKind, ActionTarget, OutcomeKind, PROTOCOL_VERSION};
    use crate::computer_use::windows_adapter::WindowsAdapter;
    use crate::computer_use::windows_identity;
    use std::sync::Arc;
    use std::time::Duration;
    use uuid::Uuid;

    if !windows_identity::session_input_available() {
        return Err("session_input_available false while unlocked (or session is locked)".into());
    }
    if !windows_identity::can_control_process(std::process::id()) {
        return Err("same-process UIPI check must allow self".into());
    }
    if windows_identity::can_control_process(0) {
        return Err("pid 0 must be fail-closed".into());
    }
    if windows_identity::can_control_process(4) {
        return Err("must not control System process (UIPI fail-closed)".into());
    }

    let adapter = Arc::new(WindowsAdapter::new());
    let decoy = FixtureWindow::spawn("User Account Control")?;
    std::thread::sleep(Duration::from_millis(200));
    let listed_decoy = adapter.list_targets()?;
    if listed_decoy.iter().any(|t| {
        t.title
            .to_ascii_lowercase()
            .contains("user account control")
            || t.title.contains("用户账户控制")
    }) {
        return Err("UAC-titled decoy was listed".into());
    }
    drop(decoy);

    let title = format!("GrokCuFixture-sec-{}", std::process::id());
    let fx = FixtureWindow::spawn(&title)?;
    std::thread::sleep(Duration::from_millis(250));
    force_foreground(fx.hwnd());
    let opts = BrokerOptions {
        feature_enabled: true,
        lease_path: std::env::temp_dir().join(format!(
            "grok-cu-sec-{}-{}.lease",
            std::process::id(),
            Uuid::new_v4()
        )),
        ..BrokerOptions::default()
    };
    let broker = ComputerUseBroker::new(adapter.clone(), opts);
    broker
        .open_run("sec-sess", "sec-run")
        .map_err(|e| e.to_string())?;
    let listed = broker.list_targets("sec-run").map_err(|e| e.to_string())?;
    if listed.iter().any(|t| {
        t.title
            .to_ascii_lowercase()
            .contains("user account control")
            || t.title.contains("用户账户控制")
    }) {
        return Err("UAC consent window was listed".into());
    }
    let target = listed
        .iter()
        .find(|t| t.title.contains(&title))
        .cloned()
        .ok_or("security fixture not listed")?;
    let Some((pid, _, _)) = windows_identity::parse_target_id(&target.target_id) else {
        return Err("bad target_id".into());
    };
    if !windows_identity::can_control_process(pid) {
        return Err("fixture pid must be controllable".into());
    }
    let gen = broker
        .authorize_target("sec-run", &target.target_id)
        .map_err(|e| e.to_string())?;
    let obs = broker.observe("sec-run").map_err(|e| e.to_string())?;
    let count_ref = obs
        .nodes
        .iter()
        .find(|n| n.name.eq_ignore_ascii_case("count"))
        .map(|n| n.node_ref.clone())
        .ok_or("Count missing")?;

    let before = fx.clicks();
    let st = broker.request_stop("sec-run").map_err(|e| e.to_string())?;
    if st != StopState::StopRequested && st != StopState::Stopped {
        return Err(format!("stop state {st:?}"));
    }
    let stopped = broker
        .wait_stopped("sec-run", Duration::from_secs(2))
        .map_err(|e| e.to_string())?;
    if stopped != StopState::Stopped {
        return Err(format!("expected Stopped, got {stopped:?}"));
    }
    if !adapter.is_idle("sec-run") {
        return Err("adapter not idle after stop".into());
    }
    if adapter.periodic_preview_active() {
        return Err("preview still running after stop".into());
    }
    let late = broker.act(crate::computer_use::protocol::ActionRequest {
        version: PROTOCOL_VERSION,
        action_id: "after-stop".into(),
        run_id: "sec-run".into(),
        target_id: target.target_id.clone(),
        target_generation: gen,
        snapshot_id: obs.snapshot_id.clone(),
        geometry_revision: obs.geometry_revision,
        action: ActionKind::Click,
        target: ActionTarget::Element {
            element_ref: count_ref.clone(),
        },
        parameters: serde_json::json!({}),
    });
    if late.kind != OutcomeKind::Rejected || late.executed {
        return Err(format!("post-stop dispatch must reject, got {late:?}"));
    }
    if fx.clicks() != before {
        return Err(format!(
            "stop postcondition clicks {before}->{}",
            fx.clicks()
        ));
    }

    let target_id = target.target_id.clone();
    let snapshot_id = obs.snapshot_id.clone();
    let geometry_revision = obs.geometry_revision;
    drop(fx);
    std::thread::sleep(Duration::from_millis(80));
    if adapter.target_alive(&target_id) {
        return Err("destroyed window still target_alive".into());
    }
    let dead = adapter.act(&DispatchRequest {
        managed_request: None,
        cancellation: Default::default(),
        run_id: "dead-run".into(),
        action_id: "dead-click".into(),
        generation: 1,
        target_id: target_id.clone(),
        target_generation: 1,
        snapshot_id,
        geometry_revision,
        action: ActionKind::Click,
        target: ActionTarget::Element {
            element_ref: count_ref,
        },
        parameters: serde_json::json!({}),
        scope: ActionScope::Directed,
    });
    if dead.is_ok() {
        return Err(format!("act after close must fail, got {dead:?}"));
    }
    if adapter.observe(&target_id).is_ok() {
        return Err("observe after close must fail".into());
    }
    println!("gate: windows_security_and_cancel");
    Ok(())
}
