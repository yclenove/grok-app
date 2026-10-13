//! Original Broker TaskGrant + native field assignment. File choice is synthetic.
use super::super::probe_download::{DownloadGrant, DownloadWorld};
use super::*;
use grok_computer_use_core::broker::{
    HostExecutionAddress, WebViewUploadCall, WebViewUploadCompletion, WebViewUploadIntent,
    WebViewUploadReady,
};
use grok_computer_use_core::protocol::OutcomeKind;
use grok_computer_use_core::task_consent::ExecutionPermit;
use std::sync::mpsc::{self, Receiver};
use std::thread::{self, JoinHandle};

struct Caller {
    cancellation: grok_computer_use_core::broker::HostUploadCancel,
    result: Receiver<Result<WebViewUploadCompletion, String>>,
    join: Option<JoinHandle<()>>,
}
impl Caller {
    fn spawn(call: WebViewUploadCall) -> Self {
        let cancellation = call.cancel_handle();
        let (sender, result) = mpsc::channel();
        let join = thread::spawn(move || {
            let _ = sender.send(call.wait().map_err(|error| error.to_string()));
        });
        Self {
            cancellation,
            result,
            join: Some(join),
        }
    }
    fn terminal(&mut self) -> Result<Result<WebViewUploadCompletion, String>, String> {
        let result = self
            .result
            .recv_timeout(Duration::from_secs(5))
            .map_err(|_| "original upload caller missed deadline")?;
        if let Some(join) = self.join.take() {
            join.join().map_err(|_| "original upload caller panicked")?;
        }
        Ok(result)
    }
}
impl Drop for Caller {
    fn drop(&mut self) {
        self.cancellation.fence();
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

pub(super) fn intent(
    world: &DownloadWorld,
    grant: &DownloadGrant,
    action: &str,
) -> Result<(ExecutionPermit, WebViewUploadIntent, HostExecutionAddress), String> {
    let observing = world
        .broker
        .begin_model_call(&grant.token, "computer_observe")
        .map_err(|error| error.to_string())?;
    let observation = world
        .broker
        .observe_for_permit(&observing, false)
        .map_err(|error| error.to_string())?;
    let field = observation
        .nodes
        .iter()
        .find(|node| node.actions.iter().any(|action| action == "upload"))
        .ok_or("original native file reference missing")?;
    let permit = world
        .broker
        .begin_model_call(&grant.token, "computer_upload")
        .map_err(|error| error.to_string())?;
    let intent = WebViewUploadIntent {
        run_id: observation.run_id,
        target_id: observation.target_id,
        target_generation: observation.target_generation,
        action_id: action.into(),
        snapshot_id: observation.snapshot_id,
        geometry_revision: observation.geometry_revision,
        element_ref: field.node_ref.clone(),
    };
    let view = world
        .broker
        .host_task_view(&grant.token)
        .map_err(|error| error.to_string())?;
    let execution = view
        .executions
        .iter()
        .find(|execution| execution.run_id == grant.run_id)
        .ok_or("original execution readback missing")?;
    let address = HostExecutionAddress {
        key: execution.key.clone(),
        run_id: execution.run_id.clone(),
        generation: execution.generation.clone(),
    };
    Ok((permit, intent, address))
}

pub(super) fn check(app: &tauri::AppHandle) -> Result<serde_json::Value, String> {
    let mut server = Server::spawn()?;
    let result = scenarios(app, &server);
    let closed = crate::side_browser_host::close(app, TARGET.into());
    let joined = server.finish();
    let result = result?;
    closed?;
    joined?;
    Ok(result)
}

fn scenarios(app: &tauri::AppHandle, server: &Server) -> Result<serde_json::Value, String> {
    io::create(app, TARGET, &server.url(), 40.0)?;
    ready(app, None)?;
    let live = Arc::new(native_view::SideBrowserLiveView::open(
        app.clone(),
        TARGET.into(),
    )?);
    let mut evidence = Vec::new();
    let mut receiver_requests = 0;
    for mode in [
        "normal",
        "cancel",
        "empty",
        "stale-field",
        "pause",
        "stop",
        "full",
        "navigation",
    ] {
        let root = crate::paths::app_data_root().join(format!("upload-task-{mode}"));
        std::fs::create_dir(&root).map_err(|_| "upload fixture root unavailable")?;
        let world = DownloadWorld::new(live.clone(), root)?;
        let grant = world.grant()?;
        let (permit, intent, address) = intent(&world, &grant, mode)?;
        let WebViewUploadReady::Pending(call) = world
            .broker
            .request_webview_upload(intent.clone(), &permit)
            .map_err(|error| error.to_string())?
        else {
            return Err("fresh upload request replayed".into());
        };
        let request_id = call.request_id().to_string();
        let mut caller = Caller::spawn(call);
        let selected = world
            .broker
            .begin_host_upload_selection(&grant.token, &address, &request_id)
            .map_err(|error| error.to_string())?;
        let started = Instant::now();
        let assigned = matches!(mode, "normal" | "empty" | "full");
        let mut receipt_result = None;
        if assigned {
            let bytes = match mode {
                "empty" => vec![],
                "full" => vec![0xa5; grok_computer_use_core::adapter::MAX_UPLOAD_BYTES],
                _ => b"native original task bytes\0\xff".to_vec(),
            };
            let file = HostSelectedUploadFile::from_host_reader(
                "中文内容.bin".into(),
                "application/octet-stream".into(),
                std::io::Cursor::new(bytes),
            )?;
            let completion = selected
                .assign(file.clone())
                .map_err(|error| error.to_string())?;
            if completion.outcome.kind != OutcomeKind::Verified
                || caller.terminal()??.outcome.kind != OutcomeKind::Verified
            {
                return Err(format!("{mode} original assignment was not verified"));
            }
            receiver_requests += 1;
            let request = UploadRequest {
                run_id: intent.run_id.clone(),
                target_id: intent.target_id.clone(),
                target_generation: intent.target_generation,
                action_id: intent.action_id.clone(),
                snapshot_id: intent.snapshot_id.clone(),
                element_ref: intent.element_ref.clone(),
                file,
                cancellation: Default::default(),
            };
            receipt_result = Some(receipt(app, server, &request, receiver_requests)?);
            if !matches!(
                world.broker.request_webview_upload(intent.clone(), &permit),
                Ok(WebViewUploadReady::Replay(_))
            ) {
                return Err("original upload completion did not replay from ledger".into());
            }
        } else {
            match mode {
                "cancel" => {
                    if world
                        .broker
                        .cancel_host_upload(&grant.token, &address, &request_id)
                        .map_err(|error| error.to_string())?
                        .outcome
                        .kind
                        != OutcomeKind::Rejected
                        || caller.terminal()??.outcome.kind != OutcomeKind::Rejected
                        || selected.require_current().is_ok()
                    {
                        return Err("cancelled native choice was not rejected".into());
                    }
                    drop(selected);
                }
                "stale-field" => {
                    io::eval(
                        app,
                        TARGET,
                        "document.querySelector('input').setAttribute('accept','image/*'); true",
                    )?;
                    let file = HostSelectedUploadFile::from_host_selection(
                        "stale.bin".into(),
                        "application/octet-stream".into(),
                        vec![1],
                    )?;
                    if selected
                        .assign(file)
                        .map_err(|error| error.to_string())?
                        .outcome
                        .kind
                        != OutcomeKind::Unknown
                        || caller.terminal()??.outcome.kind != OutcomeKind::Unknown
                    {
                        return Err("stale native field incorrectly published success".into());
                    }
                }
                "pause" | "stop" | "navigation" => {
                    match mode {
                        "pause" => world
                            .broker
                            .pause(&grant.run_id)
                            .map_err(|error| error.to_string())?,
                        "stop" => {
                            if world
                                .broker
                                .request_stop(&grant.run_id)
                                .map_err(|error| error.to_string())?
                                != super::super::super::StopState::StopRequested
                            {
                                return Err("Stop released original file-choice owner early".into());
                            }
                        }
                        _ => {
                            let page = io::eval(app, TARGET, "uploadPage")?;
                            live.navigate(&server.url())?;
                            ready(app, page.as_str())?;
                        }
                    }
                    if selected.require_current().is_ok() {
                        return Err("retired choice retained authority".into());
                    }
                    let file = HostSelectedUploadFile::from_host_selection(
                        "retired.bin".into(),
                        "application/octet-stream".into(),
                        vec![2],
                    )?;
                    if selected.assign(file).is_ok() || caller.terminal()?.is_ok() {
                        return Err("retired original caller incorrectly succeeded".into());
                    }
                }
                _ => unreachable!(),
            }
        }
        world
            .broker
            .request_stop(&grant.run_id)
            .map_err(|error| error.to_string())?;
        if world
            .broker
            .wait_stopped(&grant.run_id, Duration::from_secs(5))
            .map_err(|error| error.to_string())?
            != super::super::super::StopState::Stopped
            || !world.adapter.is_idle(&grant.run_id)
            || server.received().len() != receiver_requests
        {
            return Err(format!(
                "{mode} physical completion or no-replay postcondition failed"
            ));
        }
        evidence.push(serde_json::json!({"mode":mode,"assigned":assigned,"originalCallerJoined":true,"originalAdapterIdle":true,
            "stopped":true,"replayDispatched":false,"receiverRequests":receiver_requests,"elapsedMs":started.elapsed().as_millis(),"receipt":receipt_result}));
    }
    Ok(
        serde_json::json!({"scenariosPassed":evidence.len(),"scenarios":evidence,"totalReceiverRequests":receiver_requests,
        "nativeIdle":true,"fileChoiceSynthetic":true,"emptyConnectionsWithoutHttp":server.empty_connections()}),
    )
}
