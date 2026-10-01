//! Model-facing tools. Host controls and authorization are deliberately absent.

use serde_json::{json, Value};

use crate::{
    broker::{BrowserOp, BrowserWrite, BrowserWriteResult, ComputerUseBroker, StopState},
    ipc::{error, fail, success, SessionBinding},
    protocol::ActionRequest,
};

pub type ModelStopHandler =
    dyn Fn(&SessionBinding) -> Result<StopState, String> + Send + Sync + 'static;

pub const MODEL_TOOLS: &[&str] = &[
    "computer_status",
    "computer_list_targets",
    "computer_open_target",
    "computer_observe",
    "computer_act",
    "computer_wait",
    "computer_request_handoff",
    "computer_stop",
    "computer_navigate",
    "computer_download",
    "browser_list_tabs",
    "browser_open",
    "browser_observe",
    "browser_act",
];

pub const HOST_ONLY_TOOLS: &[&str] = &[
    "computer_authorize",
    "computer_resume",
    "computer_reconnect",
    "computer_pause",
];

pub fn dispatch(
    broker: &ComputerUseBroker,
    binding: &SessionBinding,
    name: &str,
    args: Value,
) -> Value {
    dispatch_with_stop_handler(broker, binding, name, args, None)
}

pub fn dispatch_with_stop_handler(
    broker: &ComputerUseBroker,
    binding: &SessionBinding,
    name: &str,
    args: Value,
    stop_handler: Option<&ModelStopHandler>,
) -> Value {
    if let Err(e) = broker.require_owner(&binding.session_id, &binding.run_id) {
        return fail(&e);
    }
    if let Some(key) = crate::protocol::forbidden_permission_param(&args) {
        return error(format!("{key} never grants desktop control"));
    }
    if !broker.feature_enabled() {
        return error("computer use is disabled");
    }
    let run = binding.run_id.as_str();
    match name {
        "computer_status" => {
            let capabilities = broker
                .capabilities_for_run(run)
                .unwrap_or_else(|_| broker.capabilities());
            let mut traces = broker.traces(Some(run), true);
            if traces.len() > crate::protocol::MODEL_TRACE_CAP {
                let drop_n = traces.len() - crate::protocol::MODEL_TRACE_CAP;
                traces.drain(0..drop_n);
            }
            let stop = broker
                .stop_state(run)
                .map(|s| s.as_str())
                .unwrap_or("none");
            success(json!({"session":binding.session_id,"runId":run,"stopState":stop,
                "paused":broker.is_paused(run).unwrap_or(false),"backend":capabilities.backend_id,
                "timings":broker.timings(run).ok(),"traces":traces}))
        }
        "computer_list_targets" => match broker.authorized_target(run) {
            Ok((target_id, _)) => match broker.list_targets(run) {
                Ok(rows) => success(json!(rows
                    .into_iter()
                    .filter(|t| t.target_id == target_id)
                    .map(|t| json!({"targetId":t.target_id,"title":t.title,"appName":t.app_name,"kind":t.kind}))
                    .collect::<Vec<_>>())),
                Err(e) => fail(&e),
            },
            Err(_) => success(json!([])),
        },
        "computer_open_target" => match broker.authorized_target(run) {
            Ok((target_id, generation)) if args.get("targetId").and_then(Value::as_str) == Some(target_id.as_str()) => {
                success(json!({"runId":run,"targetId":target_id,"targetGeneration":generation}))
            }
            _ => error("select and authorize this target in the Computer panel first"),
        },
        "computer_observe" => {
            let Some(options) = args.as_object() else { return error("observation options must be an object") };
            if options.keys().any(|key| key != "screenshot") || options.get("screenshot").is_some_and(|value| !value.is_boolean()) {
                return error("only the boolean screenshot option is supported");
            }
            match broker.observe_with_screenshot(run, options.get("screenshot").and_then(Value::as_bool).unwrap_or(true)) {
            Ok(mut obs) => {
                let png = obs.image.png_base64.take();
                let mut content = Vec::new();
                if let Some(data) = png { content.push(json!({"type":"image","mimeType":"image/png","data":data})); }
                content.push(json!({"type":"text","text":serde_json::to_string(&obs).unwrap_or_default()}));
                json!({"isError":false,"content":content})
            }
            Err(e) => fail(&e),
        }},
        "computer_act" => {
            // IDs come from the observation the model actually saw. UI preview must
            // not silently replace them, and no defaults can launder stale requests.
            let req = match serde_json::from_value::<ActionRequest>(args) {
                Ok(req) if req.run_id == run => req,
                Ok(_) => return error("run does not match session credential"),
                Err(e) => return error(format!("invalid action: {e}")),
            };
            let outcome = broker.act(req);
            let is_error = matches!(outcome.kind, crate::protocol::OutcomeKind::Rejected | crate::protocol::OutcomeKind::Unknown);
            json!({"isError":is_error,"content":[{"type":"text","text":serde_json::to_string(&outcome).unwrap_or_default()}]})
        }
        "computer_wait" => crate::tools_wait::wait_for_node(broker, run, args),
        "computer_request_handoff" => match broker.takeover(run) {
            Ok(()) => success(json!({"runId":run,"waitingForUser":true})),
            Err(e) => fail(&e),
        },
        "computer_stop" => match stop_handler
            .map(|handler| handler(binding))
            .unwrap_or_else(|| broker.request_stop(run).map_err(|error| error.to_string()))
        {
            Ok(state) => success(json!({"runId":run,"stopState":state.as_str()})),
            Err(e) => error(e),
        },
        "computer_navigate" => {
            let tab = args.get("tabId").and_then(Value::as_str).unwrap_or("");
            let url = args.get("url").and_then(Value::as_str).unwrap_or("");
            if tab.trim().is_empty() {
                return error("tabId required");
            }
            if tab.len() > 256 {
                return error("invalid identity length");
            }
            if url.trim().is_empty() {
                return error("url required");
            }
            if let Err(e) = crate::browser::is_allowed_navigate_url(url) {
                return fail(&e);
            }
            let action_id = args
                .get("actionId")
                .and_then(Value::as_str)
                .unwrap_or("");
            if action_id.trim().is_empty() {
                return error("actionId required");
            }
            let page_generation = args.get("pageGeneration").and_then(Value::as_u64).unwrap_or(0);
            if page_generation == 0 {
                return error("pageGeneration required");
            }
            match broker.dispatch_browser(BrowserWrite {
                session_id: Some(&binding.session_id),
                run_id: run,
                tab_id: tab,
                action_id,
                page_generation,
                op: BrowserOp::Navigate { url },
            }) {
                Ok(BrowserWriteResult::Tab(info)) => {
                    success(json!({
                        "tabId": info.tab_id,
                        "url": crate::browser::model_visible_url(&info.url),
                        "generation": info.generation,
                        "pageGeneration": info.generation
                    }))
                }
                Ok(_) => error("navigate returned an unexpected result"),
                Err(e) => fail(&e),
            }
        }
        "computer_download" => {
            if args.get("path").is_some() {
                return error("model-supplied filesystem path is not trusted");
            }
            let tab = args.get("tabId").and_then(Value::as_str).unwrap_or("");
            if tab.trim().is_empty() {
                return error("tabId required");
            }
            if tab.len() > 256 {
                return error("invalid identity length");
            }
            let name = args
                .get("filename")
                .and_then(Value::as_str)
                .unwrap_or("download.bin");
            let action_id = args
                .get("actionId")
                .and_then(Value::as_str)
                .unwrap_or("");
            if action_id.trim().is_empty() {
                return error("actionId required");
            }
            let page_generation = args.get("pageGeneration").and_then(Value::as_u64).unwrap_or(0);
            if page_generation == 0 {
                return error("pageGeneration required");
            }
            let snapshot_id = args.get("snapshotId").and_then(Value::as_str).unwrap_or("");
            if snapshot_id.trim().is_empty() {
                return error("snapshotId required");
            }
            let element_ref = args.get("elementRef").and_then(Value::as_str).unwrap_or("");
            if element_ref.trim().is_empty() {
                return error("elementRef required");
            }
            match broker.dispatch_browser(BrowserWrite {
                session_id: Some(&binding.session_id),
                run_id: run,
                tab_id: tab,
                action_id,
                page_generation,
                op: BrowserOp::Download {
                    filename: name,
                    model_path: None,
                    snapshot_id,
                    element_ref,
                },
            }) {
                Ok(BrowserWriteResult::Download(path)) => {
                    success(json!({"path":path.to_string_lossy(),"tabId":tab}))
                }
                Ok(_) => error("download returned an unexpected result"),
                Err(e) => fail(&e),
            }
        }
        "computer_clear_profile"
        | "computer_clear_managed_profile"
        | "computer_open_profile"
        | "computer_delete_profile" => {
            error("profile create/clear belong to the Host; not a model tool")
        }
        "computer_evaluate"
        | "browser_run_code_unsafe"
        | "computer_cdp"
        | "computer_run_js" => error("evaluate/cdp is forbidden"),
        "computer_upload" => error("upload must use a Host-authorized staging file"),
        "browser_list_tabs" => crate::tools_browser::list_tabs(broker, run),
        "browser_open" => crate::tools_browser::open_tab(broker, run, &args),
        "browser_observe" => crate::tools_browser::observe_tab(broker, run, &args),
        "browser_act" => crate::tools_browser::act_tab(broker, &binding.session_id, run, &args),
        _ => error("tool is unavailable; authorization and resume belong to the Host"),
    }
}

#[cfg(any(test, feature = "test-support"))]
pub fn run_model_tool_surface_gates() -> Result<(), String> {
    use crate::fake::FakeAdapter;
    use crate::protocol::PROTOCOL_VERSION;
    use std::sync::Arc;

    for name in HOST_ONLY_TOOLS {
        if MODEL_TOOLS.contains(name) {
            return Err(format!("{name} leaked onto the model tool surface"));
        }
    }
    if !MODEL_TOOLS.contains(&"computer_stop") {
        return Err("computer_stop must be on the model tool surface".into());
    }

    let fake = Arc::new(FakeAdapter::new());
    fake.set_include_png(true);
    let broker = ComputerUseBroker::new(
        fake.clone(),
        crate::broker::BrokerOptions {
            feature_enabled: true,
            lease_path: std::env::temp_dir().join(format!(
                "cu-s102-{}-{}.lease",
                std::process::id(),
                uuid::Uuid::new_v4()
            )),
            ..crate::broker::BrokerOptions::default()
        },
    );
    broker
        .open_run("s102", "run-a")
        .map_err(|e| e.to_string())?;
    let target = fake.fixture_id();
    let gen = broker
        .authorize_target("run-a", &target)
        .map_err(|e| e.to_string())?;
    let binding = SessionBinding {
        session_id: "s102".into(),
        run_id: "run-a".into(),
    };
    let call = |name: &str, args: serde_json::Value| dispatch(&broker, &binding, name, args);

    for name in HOST_ONLY_TOOLS.iter().copied().chain([
        "computer_authorize",
        "computer_resume",
        "computer_reconnect",
        "computer_pause",
        "computer_evaluate",
        "browser_run_code_unsafe",
        "computer_upload",
        "computer_clear_profile",
    ]) {
        let result = call(name, json!({}));
        if result.get("isError") != Some(&json!(true)) {
            return Err(format!("host-only {name} must fail closed, got {result}"));
        }
    }
    println!("gate: model_tools_host_controls_fail_closed");

    let listed = call("computer_list_targets", json!({}));
    if listed.get("isError") == Some(&json!(true)) {
        return Err(format!("list_targets failed: {listed}"));
    }
    let obs = call("computer_observe", json!({}));
    let content = obs
        .get("content")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if !content
        .iter()
        .any(|p| p.get("type") == Some(&json!("image")))
    {
        return Err(format!("observe must return image content, got {obs}"));
    }
    let text = content
        .iter()
        .find(|p| p.get("type") == Some(&json!("text")))
        .and_then(|p| p.get("text"))
        .and_then(Value::as_str)
        .unwrap_or("");
    if text.contains("pngBase64") || text.contains("png_base64") {
        return Err("observe text must not repeat png bytes".into());
    }
    println!("gate: model_tools_observe_image_content");

    let snap = broker.act_defaults("run-a").map_err(|e| e.to_string())?.2;
    let geo = broker.act_defaults("run-a").map_err(|e| e.to_string())?.3;
    let acted = call(
        "computer_act",
        json!({
            "version": PROTOCOL_VERSION,
            "actionId": "s102-click",
            "runId": "run-a",
            "targetId": target,
            "targetGeneration": gen,
            "snapshotId": snap,
            "geometryRevision": geo,
            "action": "click",
            "target": {"elementRef": "n1"},
            "parameters": {}
        }),
    );
    if acted.get("isError") == Some(&json!(true)) {
        return Err(format!("semantic click failed: {acted}"));
    }
    println!("gate: model_tools_act_after_observe");

    fake.set_include_png(false);
    let text_obs = call("computer_observe", json!({}));
    let text_parts = text_obs
        .get("content")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if text_parts
        .iter()
        .any(|p| p.get("type") == Some(&json!("image")))
    {
        return Err("text-only observe must not invent image content".into());
    }
    let text_snap = broker.act_defaults("run-a").map_err(|e| e.to_string())?;
    let coord = call(
        "computer_act",
        json!({
            "version": PROTOCOL_VERSION,
            "actionId": "s102-coord",
            "runId": "run-a",
            "targetId": target,
            "targetGeneration": text_snap.1,
            "snapshotId": text_snap.2,
            "geometryRevision": text_snap.3,
            "action": "click",
            "target": {"x": 4.0, "y": 4.0},
            "parameters": {}
        }),
    );
    let coord_text = coord.to_string();
    if coord.get("isError") != Some(&json!(true)) || !coord_text.contains("visual observation") {
        return Err(format!(
            "text-only coordinate act must be rejected, got {coord}"
        ));
    }
    println!("gate: model_tools_text_only_blocks_coordinates");

    let stopped = call("computer_stop", json!({}));
    if stopped.get("isError") == Some(&json!(true)) {
        return Err(format!("model stop must run, got {stopped}"));
    }
    let resume = call("computer_resume", json!({}));
    if resume.get("isError") != Some(&json!(true)) {
        return Err("model must not resume after stop".into());
    }
    println!("gate: model_tools_stop_without_resume");
    println!("gate: model_tool_surface host_fail=ok observe_image=ok text_coord=rejected stop=ok");
    Ok(())
}

#[cfg(any(test, feature = "test-support"))]
pub fn run_agent_loop_gates() -> Result<(), String> {
    use crate::fake::FakeAdapter;
    use crate::protocol::PROTOCOL_VERSION;
    use std::sync::Arc;
    use std::time::Duration;

    let fake = Arc::new(FakeAdapter::new());
    fake.set_include_png(true);
    let broker = Arc::new(ComputerUseBroker::new(
        fake.clone(),
        crate::broker::BrokerOptions {
            feature_enabled: true,
            action_budget: 8,
            observe_budget: 3,
            lease_path: std::env::temp_dir().join(format!(
                "cu-s103-{}-{}.lease",
                std::process::id(),
                uuid::Uuid::new_v4()
            )),
            ..crate::broker::BrokerOptions::default()
        },
    ));
    broker
        .open_run("s103", "run-a")
        .map_err(|e| e.to_string())?;
    let target = fake.fixture_id();
    let gen = broker
        .authorize_target("run-a", &target)
        .map_err(|e| e.to_string())?;
    let binding = SessionBinding {
        session_id: "s103".into(),
        run_id: "run-a".into(),
    };
    let call = |name: &str, args: serde_json::Value| dispatch(&broker, &binding, name, args);
    let act_body = |id: &str, gen: u64, snap: &str, geo: u64| {
        json!({
            "version": PROTOCOL_VERSION,
            "actionId": id,
            "runId": "run-a",
            "targetId": target,
            "targetGeneration": gen,
            "snapshotId": snap,
            "geometryRevision": geo,
            "action": "click",
            "target": {"elementRef": "n1"},
            "parameters": {}
        })
    };

    let obs = call("computer_observe", json!({}));
    if obs.get("isError") == Some(&json!(true)) {
        return Err(format!("observe failed: {obs}"));
    }
    let defaults = broker.act_defaults("run-a").map_err(|e| e.to_string())?;
    let first = call(
        "computer_act",
        act_body("s103-a", gen, &defaults.2, defaults.3),
    );
    if first.get("isError") == Some(&json!(true)) {
        return Err(format!("first act failed: {first}"));
    }
    let stale = call(
        "computer_act",
        act_body("s103-stale", gen + 9, &defaults.2, defaults.3),
    );
    if stale.get("isError") != Some(&json!(true)) {
        return Err(format!("stale generation must fail closed, got {stale}"));
    }
    println!("gate: agent_loop_generation_rechecked");

    fake.set_delay(Duration::from_millis(250));
    let obs2 = call("computer_observe", json!({}));
    if obs2.get("isError") == Some(&json!(true)) {
        return Err(format!("observe before parallel failed: {obs2}"));
    }
    let d2 = broker.act_defaults("run-a").map_err(|e| e.to_string())?;
    let broker_a = broker.clone();
    let broker_b = broker.clone();
    let bind = binding.clone();
    let body_p1 = act_body("s103-p1", d2.1, &d2.2, d2.3);
    let body_p2 = act_body("s103-p2", d2.1, &d2.2, d2.3);
    let (one, two) = std::thread::scope(|scope| {
        let t1 = scope.spawn(|| dispatch(&broker_a, &bind, "computer_act", body_p1));
        std::thread::sleep(Duration::from_millis(30));
        let t2 = scope.spawn(|| dispatch(&broker_b, &bind, "computer_act", body_p2));
        (t1.join().expect("p1"), t2.join().expect("p2"))
    });
    fake.set_delay(Duration::from_millis(0));
    let held = one.to_string().contains("held")
        || one.to_string().contains("in flight")
        || two.to_string().contains("held")
        || two.to_string().contains("in flight")
        || one.get("isError") == Some(&json!(true))
        || two.get("isError") == Some(&json!(true));
    if !held {
        return Err(format!(
            "parallel acts must not both dispatch, got {one} / {two}"
        ));
    }
    if fake.executions().len() > 3 {
        return Err(format!(
            "parallel calls bypassed the input mutex: {:?}",
            fake.executions()
        ));
    }
    println!("gate: agent_loop_parallel_mutex");

    broker
        .fork_run("run-a", "run-fork")
        .map_err(|e| e.to_string())?;
    let fork_bind = SessionBinding {
        session_id: "s103".into(),
        run_id: "run-fork".into(),
    };
    let fork_act = dispatch(
        &broker,
        &fork_bind,
        "computer_act",
        json!({
            "version": PROTOCOL_VERSION,
            "actionId": "s103-fork",
            "runId": "run-fork",
            "targetId": target,
            "targetGeneration": gen,
            "snapshotId": d2.2,
            "geometryRevision": d2.3,
            "action": "click",
            "target": {"elementRef": "n1"},
            "parameters": {}
        }),
    );
    if fork_act.get("isError") != Some(&json!(true)) {
        return Err(format!("forked run must not inherit act, got {fork_act}"));
    }
    println!("gate: agent_loop_fork_requires_reauth");

    broker
        .invalidate_for_context_change("run-a")
        .map_err(|e| e.to_string())?;
    let after_compact = call("computer_act", act_body("s103-compact", d2.1, &d2.2, d2.3));
    if after_compact.get("isError") != Some(&json!(true)) {
        return Err(format!(
            "compact/model-switch must drop act, got {after_compact}"
        ));
    }
    println!("gate: agent_loop_context_change_requires_reauth");

    let snap_path = std::env::temp_dir().join(format!(
        "cu-s103-persist-{}-{}.json",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    broker
        .open_run("s103", "run-restore")
        .map_err(|e| e.to_string())?;
    let _ = broker.authorize_target("run-restore", &target);
    let _ = broker.observe("run-restore");
    broker
        .persist_run_to_path("run-restore", &snap_path)
        .map_err(|e| e.to_string())?;
    let restored = ComputerUseBroker::new(
        fake.clone(),
        crate::broker::BrokerOptions {
            feature_enabled: true,
            lease_path: std::env::temp_dir().join(format!(
                "cu-s103-restore-{}-{}.lease",
                std::process::id(),
                uuid::Uuid::new_v4()
            )),
            ..crate::broker::BrokerOptions::default()
        },
    );
    restored
        .restore_run_from_path(&snap_path)
        .map_err(|e| e.to_string())?;
    let restore_bind = SessionBinding {
        session_id: "s103".into(),
        run_id: "run-restore".into(),
    };
    let restore_act = dispatch(
        &restored,
        &restore_bind,
        "computer_act",
        json!({
            "version": PROTOCOL_VERSION,
            "actionId": "s103-restore",
            "runId": "run-restore",
            "targetId": target,
            "targetGeneration": 1,
            "snapshotId": "old",
            "geometryRevision": 1,
            "action": "click",
            "target": {"elementRef": "n1"},
            "parameters": {}
        }),
    );
    let _ = std::fs::remove_file(&snap_path);
    if restore_act.get("isError") != Some(&json!(true)) {
        return Err(format!(
            "restored run must not keep authorization, got {restore_act}"
        ));
    }
    println!("gate: agent_loop_restore_requires_reauth");

    let tight = ComputerUseBroker::new(
        fake.clone(),
        crate::broker::BrokerOptions {
            feature_enabled: true,
            observe_budget: 1,
            lease_path: std::env::temp_dir().join(format!(
                "cu-s103-obs-{}-{}.lease",
                std::process::id(),
                uuid::Uuid::new_v4()
            )),
            ..crate::broker::BrokerOptions::default()
        },
    );
    tight
        .open_run("s103", "run-obs")
        .map_err(|e| e.to_string())?;
    tight
        .authorize_target("run-obs", &target)
        .map_err(|e| e.to_string())?;
    tight.observe("run-obs").map_err(|e| e.to_string())?;
    match tight.observe("run-obs") {
        Err(e) if e.to_string().contains("observe budget") => {}
        other => return Err(format!("observe budget must bound the loop, got {other:?}")),
    }
    println!("gate: agent_loop_observe_budget");

    broker
        .require_owner("s103", "run-a")
        .map_err(|e| e.to_string())?;
    println!("gate: agent_loop_same_session");
    println!(
        "gate: agent_loop generation=ok parallel=ok fork=ok compact=ok restore=ok budget=ok same_session=ok"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn model_tool_surface_gates() {
        super::run_model_tool_surface_gates().expect("model tool surface");
    }

    #[test]
    fn agent_loop_gates() {
        super::run_agent_loop_gates().expect("agent loop");
    }
}
