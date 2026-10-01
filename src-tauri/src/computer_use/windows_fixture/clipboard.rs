use super::{force_foreground, FixtureWindow};
use std::time::Duration;

/// Clipboard paste writes task text then restores the user's clipboard.
/// Postcondition is fixture edit + clipboard contents, not tool ok.
pub fn run_clipboard_restore() -> Result<(), String> {
    use crate::computer_use::adapter::ComputerUseAdapter;
    use crate::computer_use::broker::{BrokerOptions, ComputerUseBroker};
    use crate::computer_use::protocol::{
        ActionKind, ActionRequest, ActionTarget, PROTOCOL_VERSION,
    };
    use crate::computer_use::windows_adapter::WindowsAdapter;
    use crate::computer_use::windows_clipboard;
    use std::sync::Arc;
    use uuid::Uuid;

    // Never seed/overwrite the real clipboard. Full-format fixtures run only
    // through windows-clipboard-isolated in a private window station.
    let user_clip = windows_clipboard::get_text();
    let title = format!("GrokCuFixture-clip-{}", std::process::id());
    let fx = FixtureWindow::spawn(&title)?;
    std::thread::sleep(Duration::from_millis(200));
    let adapter = Arc::new(WindowsAdapter::new());
    let opts = BrokerOptions {
        feature_enabled: true,
        lease_path: std::env::temp_dir().join(format!(
            "grok-cu-clip-{}-{}.lease",
            std::process::id(),
            Uuid::new_v4()
        )),
        ..BrokerOptions::default()
    };
    let broker = ComputerUseBroker::new(adapter, opts);
    broker
        .open_run("clip-sess", "clip-run")
        .map_err(|e| e.to_string())?;
    let listed = broker.list_targets("clip-run").map_err(|e| e.to_string())?;
    let target = listed
        .iter()
        .find(|t| t.title.contains(&title))
        .ok_or_else(|| format!("fixture not listed: {title}"))?;
    let gen = broker
        .authorize_target("clip-run", &target.target_id)
        .map_err(|e| e.to_string())?;
    let obs = broker.observe("clip-run").map_err(|e| e.to_string())?;
    force_foreground(fx.hwnd());
    let edit_ref = obs
        .nodes
        .iter()
        .find(|n| n.role == "edit")
        .map(|n| n.node_ref.clone())
        .ok_or_else(|| format!("clipboard observe missing edit; {:?}", obs.nodes))?;
    let task = "你好剪贴板";
    let out = broker.act(ActionRequest {
        version: PROTOCOL_VERSION,
        action_id: "clip-paste".into(),
        run_id: "clip-run".into(),
        target_id: target.target_id.clone(),
        target_generation: gen,
        snapshot_id: obs.snapshot_id.clone(),
        geometry_revision: obs.geometry_revision,
        action: ActionKind::TypeText,
        target: ActionTarget::Element {
            element_ref: edit_ref,
        },
        parameters: serde_json::json!({ "text": task, "via": "clipboard" }),
    });
    std::thread::sleep(Duration::from_millis(200));
    let edit = fx.edit_text();
    let clip_after = windows_clipboard::get_text();
    fx.close();
    if !out.executed {
        return Err(format!("clipboard paste did not execute: {out:?}"));
    }
    if !edit.contains(task) {
        return Err(format!(
            "clipboard paste postcondition did not match task text; outcome={out:?}"
        ));
    }
    if clip_after != user_clip {
        return Err("clipboard changed during fixture; payloads withheld".into());
    }
    println!("gate: windows_clipboard_restore");
    Ok(())
}
