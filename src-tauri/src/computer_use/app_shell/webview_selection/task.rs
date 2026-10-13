//! Actual AppHandle picker, product approval worker and native effects.
//! Host approval/publication are synthetic; no ACP/MCP/model claim is made.
use super::*;
use grok_computer_use_core::broker::{
    AccessApprovalReady, BrokerOptions, ComputerUseBroker, HostExecutionAddress,
    HostTaskResumeReady,
};
use grok_computer_use_core::protocol::{ActionRequest, Observation, PROTOCOL_VERSION};
use std::sync::Arc;

const A: &str = "resource-browser-task-fixture-a";
const B: &str = "resource-browser-task-fixture-b";
const SESSION: &str = "native-task-host-fixture";

pub(in crate::computer_use::app_shell) async fn run(app: &tauri::AppHandle) -> Value {
    let broker = Arc::new(ComputerUseBroker::request_endpoint(BrokerOptions {
        feature_enabled: false,
        lease_path: crate::paths::app_data_root().join("native-task.lease"),
        ..Default::default()
    }));
    let result = check(app, broker.clone()).await;
    let cleanup = tauri::async_runtime::spawn_blocking(move || {
        broker.fence_task_consent(SESSION);
        for ticket in broker.take_cleanup_tickets() {
            broker
                .finish_stop_cleanup(&ticket)
                .map_err(|error| error.to_string())?;
        }
        Ok::<_, String>(())
    })
    .await
    .map_err(|_| "task cleanup worker did not join".to_string())
    .and_then(|result| result);
    let close_a = close(app, A).await;
    let close_b = close(app, B).await;
    let adapter = crate::computer_use::product_webview();
    let bindings_empty = adapter
        .list_targets()
        .is_ok_and(|targets| targets.is_empty());
    json!({
        "ok":result.is_ok() && cleanup.is_ok() && close_a.is_ok() && close_b.is_ok() && bindings_empty,
        "mode":"webview-task", "platform":std::env::consts::OS,
        "nativeProductSideBrowser":true, "taskGrantPath":true,
        "productApprovalWorker":true, "hostApprovalSynthetic":true, "publicationPinSynthetic":true,
        "realModel":false, "ACP":false, "MCP":false, "toolDispatch":true,
        "result":result.as_ref().ok(), "error":result.err(), "cleanupError":cleanup.err(),
        "closeAError":close_a.err(), "closeBError":close_b.err(),
        "bindingsEmpty":bindings_empty, "loginDataImported":false,
    })
}

fn body(result: Value) -> Result<Value, String> {
    if result["isError"] != false {
        return Err(format!("native task tool rejected: {result}"));
    }
    let text = result["content"]
        .as_array()
        .and_then(|items| items.iter().find(|item| item["type"] == "text"))
        .and_then(|item| item["text"].as_str())
        .ok_or("native task tool response missing")?;
    serde_json::from_str(text).map_err(|_| "native task tool response invalid".into())
}

async fn call(
    broker: Arc<ComputerUseBroker>,
    token: &str,
    tool: &'static str,
    args: Value,
) -> Result<Value, String> {
    let token = token.to_string();
    tauri::async_runtime::spawn_blocking(move || {
        body(grok_computer_use_core::tools::dispatch_with_transport(
            &broker, &token, tool, args, None,
        ))
    })
    .await
    .map_err(|_| "native task tool worker did not join")?
}

async fn observe(
    broker: Arc<ComputerUseBroker>,
    token: &str,
    run: &str,
) -> Result<Observation, String> {
    serde_json::from_value(
        call(
            broker,
            token,
            "computer_observe",
            json!({"runId":run,"screenshot":false}),
        )
        .await?,
    )
    .map_err(|_| "native task observation invalid".into())
}

fn request(
    observation: &Observation,
    action: ActionKind,
    name: &str,
    parameters: Value,
) -> Result<Value, String> {
    let node = observation
        .nodes
        .iter()
        .find(|node| node.name == name)
        .ok_or("native task control missing")?;
    serde_json::to_value(ActionRequest {
        version: PROTOCOL_VERSION,
        action_id: uuid::Uuid::new_v4().to_string(),
        run_id: observation.run_id.clone(),
        target_id: observation.target_id.clone(),
        target_generation: observation.target_generation,
        snapshot_id: observation.snapshot_id.clone(),
        geometry_revision: observation.geometry_revision,
        action,
        target: ActionTarget::Element {
            element_ref: node.node_ref.clone(),
        },
        parameters,
    })
    .map_err(|_| "native task request invalid".into())
}

async fn input(
    broker: Arc<ComputerUseBroker>,
    token: &str,
    run: &str,
    action: ActionKind,
    name: &str,
    parameters: Value,
) -> Result<(), String> {
    let observed = observe(broker.clone(), token, run).await?;
    let result = call(
        broker,
        token,
        "computer_act",
        request(&observed, action, name, parameters)?,
    )
    .await?;
    if !matches!(result["kind"].as_str(), Some("applied" | "verified")) {
        return Err(format!("native task action not applied: {result}"));
    }
    Ok(())
}

async fn business(
    broker: Arc<ComputerUseBroker>,
    token: &str,
    run: &str,
    text: &str,
    count: usize,
) -> Result<(), String> {
    input(
        broker.clone(),
        token,
        run,
        ActionKind::SetValue,
        "Note",
        json!({"text":text}),
    )
    .await?;
    input(
        broker.clone(),
        token,
        run,
        ActionKind::Click,
        "Save",
        json!({}),
    )
    .await?;
    for _ in 0..count {
        input(
            broker.clone(),
            token,
            run,
            ActionKind::Click,
            "Increase",
            json!({}),
        )
        .await?;
    }
    Ok(())
}

async fn check(app: &tauri::AppHandle, broker: Arc<ComputerUseBroker>) -> Result<Value, String> {
    let page = crate::paths::app_data_root().join("task-fixture.html");
    std::fs::write(&page, HTML).map_err(|_| "native task fixture file unavailable")?;
    let url =
        tauri::Url::from_file_path(&page).map_err(|_| "native task fixture URL unavailable")?;
    create(app, A, &url).await?;
    create(app, B, &url).await?;
    let initial_a = ready(app, A).await?;
    let initial_b = ready(app, B).await?;
    crate::computer_use::execution_runtime::prepare_scope_selection(
        &broker,
        crate::computer_use::SurfaceKind::WebView,
    )?;
    let connection = broker
        .reserve_transport_connection(SESSION)
        .map_err(|error| error.to_string())?;
    let token = connection.token().to_string();
    broker
        .bind_transport_agent(&token, "synthetic-native-agent")
        .map_err(|error| error.to_string())?;
    broker.submit_access(&token, json!({"action":"request","purpose":"native App task integration","surfaces":["app-webview"],"scopeIntent":"two selected native pages","capabilities":["observe","input"]})).map_err(|error| error.to_string())?;
    let key = broker
        .host_task_view(&token)
        .map_err(|error| error.to_string())?
        .request
        .ok_or("native request missing")?
        .key;
    let picker_broker = broker.clone();
    let picker_token = token.clone();
    let picker_key = key.clone();
    let picker = tauri::async_runtime::spawn_blocking(move || {
        picker_broker
            .begin_task_scope_picker(
                &picker_token,
                &picker_key,
                crate::computer_use::SurfaceKind::WebView,
            )?
            .discover()
    })
    .await
    .map_err(|_| "native task discovery worker did not join")?
    .map_err(|error| error.to_string())?;
    let views = picker.views().map_err(|error| error.to_string())?;
    if views.len() != 2
        || !crate::computer_use::product_webview()
            .list_targets()?
            .is_empty()
        || state(app, A).await? != initial_a
        || state(app, B).await? != initial_b
    {
        return Err("native task discovery changed bindings or business values".into());
    }
    let ids: Vec<_> = views.into_iter().map(|view| view.candidate_id).collect();
    let selection = picker.select(&ids).map_err(|error| error.to_string())?;
    let AccessApprovalReady::Prepare(approval) = broker
        .reserve_selected_access_approval(&token, selection)
        .map_err(|error| error.to_string())?
    else {
        return Err("native task approval was not original".into());
    };
    if !crate::computer_use::product_webview()
        .list_targets()?
        .is_empty()
    {
        return Err("native task reservation bound before approval".into());
    }
    let publication_token = token.clone();
    let outcome = crate::computer_use::task_access::approve(
        broker.clone(),
        approval,
        Arc::new(move |approval| {
            approval
                .commit_for_transport(&publication_token)
                .map_err(|error| error.to_string())
        }),
    )
    .await?;
    let adapter = crate::computer_use::product_webview();
    let targets = adapter.list_targets_for_run(&outcome.run_id)?;
    if targets.len() != 2
        || broker.executable_grant_count(SESSION) != 1
        || broker.continuation_count(SESSION) != 1
    {
        return Err("native task grant/binding count differs".into());
    }
    let a = targets
        .iter()
        .find(|target| target.display_id == A)
        .ok_or("native task A binding missing")?
        .target_id
        .clone();
    let b = targets
        .iter()
        .find(|target| target.display_id == B)
        .ok_or("native task B binding missing")?
        .target_id
        .clone();
    if picker
        .committed_outcome(&token, &ids)
        .map_err(|error| error.to_string())?
        .is_none()
    {
        return Err("native task idempotent retry missing".into());
    }
    let run = &outcome.run_id;
    call(
        broker.clone(),
        &token,
        "computer_open_target",
        json!({"runId":run,"targetId":a}),
    )
    .await?;
    business(broker.clone(), &token, run, "任务第一页 中文", 2).await?;
    call(
        broker.clone(),
        &token,
        "computer_open_target",
        json!({"runId":run,"targetId":b}),
    )
    .await?;
    business(broker.clone(), &token, run, "任务第二页 你好", 3).await?;
    call(
        broker.clone(),
        &token,
        "computer_open_target",
        json!({"runId":run,"targetId":a}),
    )
    .await?;
    let before_pause = observe(broker.clone(), &token, run).await?;
    let stale_action = request(&before_pause, ActionKind::Click, "Increase", json!({}))?;
    broker.pause(run).map_err(|error| error.to_string())?;
    if call(broker.clone(), &token, "computer_act", stale_action.clone())
        .await
        .is_ok()
    {
        return Err("paused task accepted input".into());
    }
    let execution = broker
        .host_task_view(&token)
        .map_err(|error| error.to_string())?
        .executions
        .remove(0);
    let address = HostExecutionAddress {
        key,
        run_id: run.clone(),
        generation: execution.generation,
    };
    let HostTaskResumeReady::Prepare(resume) = broker
        .reserve_host_task_resume(&token, &address)
        .map_err(|error| error.to_string())?
    else {
        return Err("native recovery not original".into());
    };
    tauri::async_runtime::spawn_blocking(move || {
        resume.complete(std::time::Duration::from_secs(3))
    })
    .await
    .map_err(|_| "native recovery worker did not join")?
    .map_err(|error| error.to_string())?;
    if call(broker.clone(), &token, "computer_act", stale_action)
        .await
        .is_ok()
    {
        return Err("recovery revived old native observation".into());
    }
    input(
        broker.clone(),
        &token,
        run,
        ActionKind::Click,
        "Increase",
        json!({}),
    )
    .await?;
    let a_final = state(app, A).await?;
    let b_final = state(app, B).await?;
    if a_final != json!({"note":"任务第一页 中文","saves":1,"count":3})
        || b_final != json!({"note":"任务第二页 你好","saves":1,"count":3})
    {
        return Err("native TaskGrant business postconditions differ".into());
    }
    let before_stop = observe(broker.clone(), &token, run).await?;
    let old_input = request(&before_stop, ActionKind::Click, "Increase", json!({}))?;
    broker
        .request_stop(run)
        .map_err(|error| error.to_string())?;
    if !adapter.list_targets_for_run(run)?.is_empty()
        || call(broker.clone(), &token, "computer_act", old_input)
            .await
            .is_ok()
        || state(app, A).await? != a_final
        || state(app, B).await? != b_final
    {
        return Err("native Stop retained permission or replayed input".into());
    }
    Ok(
        json!({"pages":2,"metadataOnlyPicker":true,"pendingReservationZeroBind":true,
        "candidateToOriginalNewRun":true,"singleGrant":true,"singleInitialContinuation":true,
        "idempotentRetry":true,"switchBackPreservesBinding":true,"pauseRejectsInput":true,
        "resumeFreshNativeObservation":true,"oldSnapshotRejected":true,"stopCleansAllMembers":true,
        "stopZeroNewEffects":true,"aFinal":a_final,"bFinal":b_final,"typedBusinessActions":10}),
    )
}
