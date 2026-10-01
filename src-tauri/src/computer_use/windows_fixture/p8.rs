use super::{force_foreground, oracle_path, write_list_oracle, FixtureWindow};
use std::time::Duration;

pub fn hold_p8_ipc() -> Result<(), String> {
    use crate::computer_use::broker::{BrokerOptions, ComputerUseBroker};
    use crate::computer_use::ipc;
    use crate::computer_use::windows_adapter::WindowsAdapter;
    use std::io::Write;
    use std::sync::Arc;
    use std::time::{Duration, Instant};
    use uuid::Uuid;

    let title = format!("GrokCuFixture-p8-{}", std::process::id());
    let clicks_path = std::env::temp_dir().join("grok-cu-fixture-clicks.txt");
    let _ = std::fs::write(&clicks_path, "0");
    let fx = FixtureWindow::spawn(&title)?;
    std::thread::sleep(Duration::from_millis(250));

    let adapter = Arc::new(WindowsAdapter::new());
    let opts = BrokerOptions {
        feature_enabled: true,
        lease_path: std::env::temp_dir().join(format!(
            "grok-cu-hold-{}-{}.lease",
            std::process::id(),
            Uuid::new_v4()
        )),
        ..BrokerOptions::default()
    };
    let broker = Arc::new(ComputerUseBroker::new(adapter, opts));
    broker
        .open_run("p8-probe", "run:p8-probe")
        .map_err(|e| e.to_string())?;
    let target = broker
        .adapter()
        .list_targets()?
        .into_iter()
        .find(|t| t.title == title)
        .ok_or("fixture target missing")?;
    broker
        .authorize_target("run:p8-probe", &target.target_id)
        .map_err(|e| e.to_string())?;
    let server = ipc::spawn(broker)?;
    let token = server.issue_session("p8-probe", "run:p8-probe")?;
    let payload = serde_json::json!({
        "url": server.url,
        "token": token,
        "title": title,
        "clicksPath": clicks_path.to_string_lossy(),
    });
    println!("{payload}");
    let _ = std::io::stdout().flush();
    if let Ok(path) = std::env::var("GROK_CU_HOLD_FILE") {
        let _ = std::fs::write(path, payload.to_string());
    }
    let secs: u64 = std::env::var("GROK_CU_HOLD_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(150);
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(secs) {
        if fx.clicks() >= 1 {
            break;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    let n = fx.clicks();
    let file = std::fs::read_to_string(&clicks_path).unwrap_or_default();
    println!("hold: done clicks={n} file={}", file.trim());
    fx.close();
    Ok(())
}

/// Bounded native observe→act→verify through the shipped Broker.
/// Postconditions are fixture counters / files, not tool ok.
pub fn run_p8_native_loop() -> Result<(), String> {
    use crate::computer_use::adapter::ComputerUseAdapter;
    use crate::computer_use::broker::{BrokerOptions, ComputerUseBroker};
    use crate::computer_use::protocol::{
        ActionKind, ActionRequest, ActionTarget, OutcomeKind, PROTOCOL_VERSION,
    };
    use crate::computer_use::windows_adapter::WindowsAdapter;
    use std::sync::Arc;
    use uuid::Uuid;

    let title = format!("GrokCuFixture-p8-{}", std::process::id());
    let clicks_path = oracle_path("grok-cu-fixture-clicks.txt");
    let _ = std::fs::write(&clicks_path, "0");
    let fx = FixtureWindow::spawn(&title)?;
    std::thread::sleep(Duration::from_millis(250));
    force_foreground(fx.hwnd());

    let adapter = Arc::new(WindowsAdapter::new());
    let opts = BrokerOptions {
        feature_enabled: true,
        lease_path: std::env::temp_dir().join(format!(
            "grok-cu-p8-{}-{}.lease",
            std::process::id(),
            Uuid::new_v4()
        )),
        ..BrokerOptions::default()
    };
    let broker = ComputerUseBroker::new(adapter, opts);
    broker
        .open_run("p8-sess", "p8-run")
        .map_err(|e| e.to_string())?;
    let listed = broker.list_targets("p8-run").map_err(|e| e.to_string())?;
    let target = listed
        .iter()
        .find(|t| t.title.contains(&title))
        .ok_or_else(|| format!("self-built fixture not listed: {title}"))?;
    let gen = broker
        .authorize_target("p8-run", &target.target_id)
        .map_err(|e| e.to_string())?;
    let obs = broker.observe("p8-run").map_err(|e| e.to_string())?;
    let has_png = obs
        .image
        .png_base64
        .as_ref()
        .map(|s| !s.is_empty())
        .unwrap_or(false);
    if !has_png || obs.image.width == 0 || obs.image.height == 0 {
        fx.close();
        return Err(format!(
            "observe missing visual {}x{} png={}",
            obs.image.width, obs.image.height, has_png
        ));
    }
    let refs: Vec<&str> = obs.nodes.iter().map(|n| n.node_ref.as_str()).collect();
    let missing = |what: &str| -> Result<(), String> {
        Err(format!("observe missing {what}; have {refs:?}"))
    };
    let Some(count_ref) = obs
        .nodes
        .iter()
        .find(|n| n.name.eq_ignore_ascii_case("count"))
        .map(|n| n.node_ref.clone())
    else {
        fx.close();
        missing("Count")?;
        unreachable!();
    };
    if !count_ref.starts_with("uia:") || count_ref.eq_ignore_ascii_case("count") {
        fx.close();
        return Err(format!("Count identity is not UIA RuntimeId: {count_ref}"));
    }
    let Some(edit_ref) = obs
        .nodes
        .iter()
        .find(|n| n.role == "edit")
        .map(|n| n.node_ref.clone())
    else {
        fx.close();
        missing("edit")?;
        unreachable!();
    };
    let Some(scroll_ref) = obs
        .nodes
        .iter()
        .find(|n| n.role == "list" || n.role == "listbox")
        .map(|n| n.node_ref.clone())
    else {
        fx.close();
        missing("listbox")?;
        unreachable!();
    };
    let Some(drag_ref) = obs
        .nodes
        .iter()
        .find(|n| n.name.to_ascii_lowercase().contains("drag"))
        .map(|n| n.node_ref.clone())
    else {
        fx.close();
        missing("drag")?;
        unreachable!();
    };
    if !obs.nodes.iter().any(|n| n.node_ref == "meta:dpi") {
        fx.close();
        missing("meta:dpi")?;
        unreachable!();
    }
    println!("gate: windows_semantic_element_ref count={count_ref}");
    let dpi = fx.dpi();
    let dpi_file =
        std::fs::read_to_string(oracle_path("grok-cu-fixture-dpi.txt")).unwrap_or_default();
    println!(
        "p8 native: image={}x{} dpi={} file={} nodes={refs:?}",
        obs.image.width,
        obs.image.height,
        dpi,
        dpi_file.trim()
    );
    if dpi < 96 {
        fx.close();
        return Err(format!("dpi below Windows baseline: {dpi}"));
    }

    let mk = |id: &str, action: ActionKind, target_el: &str, parameters: serde_json::Value| {
        ActionRequest {
            version: PROTOCOL_VERSION,
            action_id: id.into(),
            run_id: "p8-run".into(),
            target_id: target.target_id.clone(),
            target_generation: gen,
            snapshot_id: obs.snapshot_id.clone(),
            geometry_revision: obs.geometry_revision,
            action,
            target: ActionTarget::Element {
                element_ref: target_el.into(),
            },
            parameters,
        }
    };
    let node_center = |node_ref: &str| -> (f64, f64) {
        obs.nodes
            .iter()
            .find(|n| n.node_ref == node_ref)
            .and_then(|n| Some((n.x? + n.width? / 2.0, n.y? + n.height? / 2.0)))
            .unwrap_or((24.0, 24.0))
    };
    let mk_coord = |id: &str, action: ActionKind, x: f64, y: f64, parameters: serde_json::Value| {
        ActionRequest {
            version: PROTOCOL_VERSION,
            action_id: id.into(),
            run_id: "p8-run".into(),
            target_id: target.target_id.clone(),
            target_generation: gen,
            snapshot_id: obs.snapshot_id.clone(),
            geometry_revision: obs.geometry_revision,
            action,
            target: ActionTarget::Coord { x, y },
            parameters,
        }
    };
    let fail = |fx: FixtureWindow, msg: String| -> Result<(), String> {
        fx.close();
        Err(msg)
    };
    let hwnd = fx.hwnd();
    let step = |broker: &ComputerUseBroker, req: ActionRequest| {
        let mut last = "observe_act_verify".to_string();
        for _ in 0..3 {
            force_foreground(hwnd);
            if broker.is_paused("p8-run").unwrap_or(false) {
                let _ = broker.resume("p8-run");
            }
            match broker.observe_act_verify(req.clone()) {
                Ok(v) => return Ok(v),
                Err(e) => {
                    last = e.to_string();
                    std::thread::sleep(Duration::from_millis(80));
                }
            }
        }
        Err(last)
    };

    let (before, out, after) = step(
        &broker,
        mk(
            "p8-click",
            ActionKind::Click,
            &count_ref,
            serde_json::json!({}),
        ),
    )?;
    std::thread::sleep(Duration::from_millis(250));
    let clicks = fx.clicks();
    let file_n = std::fs::read_to_string(&clicks_path)
        .unwrap_or_default()
        .trim()
        .parse::<u32>()
        .unwrap_or(0);
    println!(
        "p8 click: outcome={:?} executed={} clicks={} file={}",
        out.kind, out.executed, clicks, file_n
    );
    if clicks < 1 && file_n < 1 {
        return fail(
            fx,
            format!(
                "click postcondition failed clicks={clicks} file={file_n} kind={:?}",
                out.kind
            ),
        );
    }
    if before.snapshot_id == after.snapshot_id {
        return fail(fx, "post-act observe reused snapshot".into());
    }
    if out.kind == OutcomeKind::Rejected || !out.executed {
        return fail(fx, format!("click did not execute: {:?}", out));
    }

    let (_, set_out, _) = step(
        &broker,
        mk(
            "p8-set",
            ActionKind::SetValue,
            &edit_ref,
            serde_json::json!({ "text": "你好世界" }),
        ),
    )?;
    std::thread::sleep(Duration::from_millis(150));
    let edit_after_set = fx.edit_text();
    let edit_file =
        std::fs::read_to_string(oracle_path("grok-cu-fixture-edit.txt")).unwrap_or_default();
    println!(
        "p8 set_value: outcome={:?} edit={edit_after_set:?} file={:?}",
        set_out.kind,
        edit_file.trim()
    );
    if !edit_after_set.contains("你好世界") && !edit_file.contains("你好世界") {
        return fail(
            fx,
            format!(
                "CJK set_value postcondition failed edit={edit_after_set:?} file={edit_file:?}"
            ),
        );
    }

    let (_, type_out, _) = step(
        &broker,
        mk(
            "p8-type",
            ActionKind::TypeText,
            &edit_ref,
            serde_json::json!({ "text": "测试" }),
        ),
    )?;
    std::thread::sleep(Duration::from_millis(250));
    let edit_after_type = fx.edit_text();
    let edit_file =
        std::fs::read_to_string(oracle_path("grok-cu-fixture-edit.txt")).unwrap_or_default();
    println!(
        "p8 type_text: outcome={:?} edit={edit_after_type:?} file={:?}",
        type_out.kind,
        edit_file.trim()
    );
    if !edit_after_type.contains("测试") && !edit_file.contains("测试") {
        return fail(
            fx,
            format!(
                "CJK type_text postcondition failed edit={edit_after_type:?} file={edit_file:?}"
            ),
        );
    }

    for i in 0..3 {
        let (_, key_out, _) = step(
            &broker,
            mk(
                &format!("p8-key-{i}"),
                ActionKind::Key,
                &scroll_ref,
                serde_json::json!({ "key": "down" }),
            ),
        )?;
        if key_out.kind == OutcomeKind::Rejected || !key_out.executed {
            return fail(fx, format!("key down did not execute: {:?}", key_out));
        }
        std::thread::sleep(Duration::from_millis(80));
    }
    write_list_oracle(fx.hwnd());
    let sel = fx.list_sel();
    let list_file =
        std::fs::read_to_string(oracle_path("grok-cu-fixture-list.txt")).unwrap_or_default();
    println!("p8 key: sel={sel} file={}", list_file.trim());
    if sel < 1 {
        return fail(
            fx,
            format!("key down postcondition failed sel={sel} file={list_file:?}"),
        );
    }

    let top_before = fx.list_top_index();
    let (_, scroll_out, _) = step(
        &broker,
        mk(
            "p8-scroll",
            ActionKind::Scroll,
            &scroll_ref,
            serde_json::json!({ "delta": 480 }),
        ),
    )?;
    std::thread::sleep(Duration::from_millis(150));
    write_list_oracle(fx.hwnd());
    let top_after = fx.list_top_index();
    println!(
        "p8 scroll: outcome={:?} top {top_before}->{top_after}",
        scroll_out.kind
    );
    if scroll_out.kind == OutcomeKind::Rejected || !scroll_out.executed {
        return fail(fx, format!("scroll did not execute: {:?}", scroll_out));
    }
    if top_after <= top_before {
        return fail(
            fx,
            format!("scroll postcondition failed top {top_before}->{top_after}"),
        );
    }

    let (dx0, dy0) = node_center(&drag_ref);
    let drop_ref = obs
        .nodes
        .iter()
        .find(|n| n.name.to_ascii_lowercase().contains("drop"))
        .map(|n| n.node_ref.clone())
        .unwrap_or_else(|| drag_ref.clone());
    let (dx1, dy1) = node_center(&drop_ref);
    let (_, drag_out, _) = step(
        &broker,
        mk_coord(
            "p8-drag",
            ActionKind::Drag,
            dx0,
            dy0,
            serde_json::json!({ "toX": dx1, "toY": dy1 }),
        ),
    )?;
    std::thread::sleep(Duration::from_millis(150));
    let drags = fx.drags();
    let drag_file =
        std::fs::read_to_string(oracle_path("grok-cu-fixture-drag.txt")).unwrap_or_default();
    let drag_n = drag_file.trim().parse::<u32>().unwrap_or(0);
    println!(
        "p8 drag: outcome={:?} drags={drags} file={drag_n}",
        drag_out.kind
    );
    if drags < 1 && drag_n < 1 {
        return fail(
            fx,
            format!(
                "drag postcondition failed drags={drags} file={drag_file:?} kind={:?}",
                drag_out.kind
            ),
        );
    }

    broker.request_stop("p8-run").map_err(|e| e.to_string())?;
    let late = broker.act(mk(
        "p8-late",
        ActionKind::Click,
        &count_ref,
        serde_json::json!({}),
    ));
    fx.close();
    if late.kind != OutcomeKind::Rejected || late.executed {
        return Err(format!("post-stop dispatch must reject, got {:?}", late));
    }
    println!("gate: windows_p8_observe_act_verify");
    Ok(())
}
