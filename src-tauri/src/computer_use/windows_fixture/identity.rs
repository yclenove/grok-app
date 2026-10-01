use super::{force_foreground, oracle_path, FixtureWindow};
use std::time::Duration;

/// HWND stamp + host-surface exclusion. Postcondition is fixture click count,
/// not tool ok.
pub fn run_identity_host_protect() -> Result<(), String> {
    use crate::computer_use::adapter::{ActionScope, ComputerUseAdapter, DispatchRequest};
    use crate::computer_use::protocol::{ActionKind, ActionTarget};
    use crate::computer_use::windows_adapter::WindowsAdapter;
    use crate::computer_use::windows_identity;

    let title = format!("GrokCuFixture-id-{}", std::process::id());
    let clicks_path = oracle_path("grok-cu-identity-clicks.txt");
    let _ = std::fs::write(&clicks_path, "0");
    let fx = FixtureWindow::spawn(&title)?;
    std::thread::sleep(Duration::from_millis(200));
    let adapter = WindowsAdapter::new();
    let listed = adapter.list_targets().map_err(|e| e.to_string())?;
    let target = listed
        .iter()
        .find(|t| t.title.contains(&title))
        .ok_or_else(|| format!("fixture not listed: {title}"))?;
    let parts: Vec<&str> = target.target_id.split(':').collect();
    if parts.len() != 4 || parts[0] != "win" {
        fx.close();
        return Err(format!(
            "target_id must be win:pid:hwnd:stamp, got {}",
            target.target_id
        ));
    }

    let prot_title = format!("GrokCuHostChrome-{}", std::process::id());
    let prot = FixtureWindow::spawn(&prot_title)?;
    std::thread::sleep(Duration::from_millis(150));
    windows_identity::mark_protected(prot.hwnd());
    let listed = adapter.list_targets().map_err(|e| e.to_string())?;
    if listed.iter().any(|t| t.title.contains(&prot_title)) {
        prot.close();
        fx.close();
        return Err("protected host surface appeared in list_targets".into());
    }
    let crafted = windows_identity::format_target_id(
        std::process::id(),
        prot.hwnd(),
        windows_identity::read_stamp(prot.hwnd()).unwrap_or(1),
    );
    if adapter.target_alive(&crafted) {
        prot.close();
        fx.close();
        return Err("protected window reported alive".into());
    }
    let clicks_before = prot.clicks();
    let dispatch = DispatchRequest {
        managed_request: None,
        cancellation: Default::default(),
        run_id: "id-run".into(),
        action_id: "prot-click".into(),
        generation: 1,
        target_id: crafted,
        target_generation: 1,
        snapshot_id: "none".into(),
        geometry_revision: 1,
        action: ActionKind::Click,
        target: ActionTarget::Element {
            element_ref: "count".into(),
        },
        parameters: serde_json::json!({}),
        scope: ActionScope::Directed,
    };
    let acted = adapter.act(&dispatch);
    let clicks_after = prot.clicks();
    prot.close();
    if acted.is_ok() {
        fx.close();
        return Err("protected window act must fail".into());
    }
    if clicks_after != clicks_before {
        fx.close();
        return Err(format!(
            "protected window click postcondition clicks {clicks_before}->{clicks_after}"
        ));
    }

    let stale = target.target_id.clone();
    windows_identity::clear_stamp(fx.hwnd());
    if adapter.target_alive(&stale) {
        fx.close();
        return Err("cleared stamp still alive".into());
    }
    let clicks_before = fx.clicks();
    let dispatch = DispatchRequest {
        managed_request: None,
        cancellation: Default::default(),
        run_id: "id-run".into(),
        action_id: "stale-click".into(),
        generation: 1,
        target_id: stale,
        target_generation: 1,
        snapshot_id: "none".into(),
        geometry_revision: 1,
        action: ActionKind::Click,
        target: ActionTarget::Element {
            element_ref: "count".into(),
        },
        parameters: serde_json::json!({}),
        scope: ActionScope::Directed,
    };
    let acted = adapter.act(&dispatch);
    let clicks_after = fx.clicks();
    let file_n = std::fs::read_to_string(&clicks_path)
        .unwrap_or_default()
        .trim()
        .parse::<u32>()
        .unwrap_or(0);
    fx.close();
    if acted.is_ok() {
        return Err("stale stamp act must fail".into());
    }
    if clicks_after != clicks_before || file_n != 0 {
        return Err(format!(
            "stale stamp postcondition clicks={clicks_after} file={file_n} (must stay 0)"
        ));
    }
    println!("gate: windows_identity_host_protect");
    Ok(())
}
