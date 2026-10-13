//! Shipped Node → App-embedded MCP → real HTTP/Broker → native App field.
//! Host approval and file choice remain synthetic; no ACP/model/picker claim.
use super::super::probe_download::{DownloadGrant, DownloadWorld};
use super::*;
use grok_computer_use_core::broker::{HostExecutionAddress, WebViewUploadIntent};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread::{self, JoinHandle};

struct NodeCaller {
    child: Child,
    replies: Receiver<Value>,
    reader: Option<JoinHandle<()>>,
}

impl NodeCaller {
    fn spawn(
        grant: &DownloadGrant,
        endpoint: &crate::computer_use::ipc::IpcServer,
    ) -> Result<Self, String> {
        let node = crate::computer_use::runtime::resolve(crate::computer_use::runtime::JS_RUNTIME)?;
        let script =
            crate::computer_use::runtime::resolve(crate::computer_use::runtime::MCP_SERVER)?;
        let mut command = Command::new(node);
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
        let mut child = command
            .arg(script)
            .env_remove("NODE_OPTIONS")
            .env_remove("NODE_PATH")
            .env("GROK_APP_CU_IPC", &endpoint.url)
            .env("GROK_APP_CU_TOKEN", &grant.token)
            .env("GROK_APP_CU_SESSION", &grant.session_id)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| "original embedded MCP child could not start")?;
        let stdout = child.stdout.take().ok_or("MCP stdout missing")?;
        let (send, replies) = mpsc::channel();
        let reader = thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                let Ok(value) = serde_json::from_str(&line) else {
                    break;
                };
                if send.send(value).is_err() {
                    break;
                }
            }
        });
        Ok(Self {
            child,
            replies,
            reader: Some(reader),
        })
    }
    fn send(&mut self, value: Value) -> Result<(), String> {
        writeln!(
            self.child
                .stdin
                .as_mut()
                .ok_or("original MCP stdin retired")?,
            "{value}"
        )
        .map_err(|_| "original MCP send failed".into())
    }
    fn call(&mut self, intent: &WebViewUploadIntent, id: i64) -> Result<(), String> {
        self.send(json!({"jsonrpc":"2.0","id":id,"method":"tools/call",
            "params":{"name":"computer_upload","arguments":{
                "version":1,"actionId":intent.action_id,"runId":intent.run_id,"targetId":intent.target_id,
                "targetGeneration":intent.target_generation,"snapshotId":intent.snapshot_id,
                "geometryRevision":intent.geometry_revision,"elementRef":intent.element_ref}}}))
    }
    fn reply(&self, id: i64) -> Result<Value, String> {
        let reply = self
            .replies
            .recv_timeout(Duration::from_secs(5))
            .map_err(|_| "original MCP response missed deadline")?;
        if reply["id"] != id {
            return Err("MCP response identity changed".into());
        }
        Ok(reply["result"].clone())
    }
    fn finish(&mut self) -> Result<(), String> {
        self.child.stdin.take();
        until(|| self.child.try_wait().ok().flatten().is_some())?;
        if !self
            .child
            .wait()
            .map_err(|_| "original MCP join failed")?
            .success()
        {
            return Err("original MCP child failed".into());
        }
        if let Some(reader) = self.reader.take() {
            reader.join().map_err(|_| "MCP reader panicked")?;
        }
        Ok(())
    }
}
impl Drop for NodeCaller {
    fn drop(&mut self) {
        self.child.stdin.take();
        // Failure unwind owns only this exact fixture child, never other Apps.
        if until(|| self.child.try_wait().ok().flatten().is_some()).is_err() {
            let _ = self.child.kill();
        }
        let _ = self.child.wait();
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}
fn until(mut condition: impl FnMut() -> bool) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !condition() {
        if Instant::now() >= deadline {
            return Err("original MCP/native state missed deadline".into());
        }
        thread::sleep(Duration::from_millis(10));
    }
    Ok(())
}
fn pending(world: &DownloadWorld, grant: &DownloadGrant) -> Result<String, String> {
    let mut request = None;
    until(|| {
        request = world
            .broker
            .host_task_view(&grant.token)
            .ok()
            .and_then(|view| {
                view.executions
                    .into_iter()
                    .find(|row| row.run_id == grant.run_id)
            })
            .and_then(|row| row.upload)
            .map(|upload| upload.request_id);
        request.is_some()
    })?;
    request.ok_or_else(|| "original Host upload missing".into())
}
fn settled(world: &DownloadWorld, grant: &DownloadGrant) -> bool {
    world.broker.host_task_view(&grant.token).is_ok_and(|view| {
        view.executions
            .iter()
            .find(|row| row.run_id == grant.run_id)
            .is_some_and(|row| row.upload.is_none())
    })
}

pub(super) fn check(app: &tauri::AppHandle) -> Result<Value, String> {
    let resource = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources");
    crate::computer_use::runtime::repair_from_install(&resource, &resource)?;
    let mut server = Server::spawn()?;
    let result = scenarios(app, &server);
    let closed = crate::side_browser_host::close(app, TARGET.into());
    let joined = server.finish();
    let result = result?;
    closed?;
    joined?;
    Ok(result)
}

fn scenarios(app: &tauri::AppHandle, server: &Server) -> Result<Value, String> {
    io::create(app, TARGET, &server.url(), 40.0)?;
    ready(app, None)?;
    let live = Arc::new(native_view::SideBrowserLiveView::open(
        app.clone(),
        TARGET.into(),
    )?);
    let mut evidence = Vec::new();
    let mut receiver_requests = 0;
    for mode in ["wait-over-idle", "empty", "cancel-held", "eof-held"] {
        let root = crate::paths::app_data_root().join(format!("upload-mcp-{mode}"));
        std::fs::create_dir(&root).map_err(|_| "MCP fixture root unavailable")?;
        let world = DownloadWorld::new(live.clone(), root)?;
        let grant = world.grant()?;
        let (_permit, intent, address): (_, _, HostExecutionAddress) =
            task::intent(&world, &grant, mode)?;
        let endpoint = crate::computer_use::ipc::spawn(world.broker.clone())
            .map_err(|_| "MCP fixture IPC failed")?;
        let mut caller = NodeCaller::spawn(&grant, &endpoint)?;
        let began = Instant::now();
        caller.call(&intent, 7)?;
        let request_id = pending(&world, &grant)?;
        if mode == "wait-over-idle" {
            thread::sleep(Duration::from_secs(16));
            if caller.replies.try_recv().is_ok() {
                return Err("original upload timed out before Host choice".into());
            }
        }
        let selected = world
            .broker
            .begin_host_upload_selection(&grant.token, &address, &request_id)
            .map_err(|error| error.to_string())?;
        let assigned = matches!(mode, "wait-over-idle" | "empty");
        let mut independent_receipt = None;
        if assigned {
            let bytes = if mode == "empty" {
                vec![]
            } else {
                b"exact MCP native bytes\0\xff".to_vec()
            };
            let file = HostSelectedUploadFile::from_host_reader(
                "中文.bin".into(),
                "application/octet-stream".into(),
                std::io::Cursor::new(bytes),
            )?;
            let completion = selected
                .assign(file.clone())
                .map_err(|error| error.to_string())?;
            if completion.outcome.kind != grok_computer_use_core::protocol::OutcomeKind::Verified {
                return Err("original native field assignment unverified".into());
            }
            let reply = caller.reply(7)?;
            let body: Value = serde_json::from_str(
                reply["content"][0]["text"]
                    .as_str()
                    .ok_or("MCP text missing")?,
            )
            .map_err(|_| "MCP result invalid")?;
            if reply["isError"] != false
                || body["assignmentVerified"] != true
                || body["submissionVerified"] != false
            {
                return Err("MCP assignment/submission semantics wrong".into());
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
            independent_receipt = Some(receipt(app, server, &request, receiver_requests)?);
            caller.call(&intent, 8)?;
            if caller.reply(8)? != reply || server.received().len() != receiver_requests {
                return Err("MCP replay assigned a second file or changed original result".into());
            }
        } else {
            if mode == "cancel-held" {
                caller.send(json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":"7"}}))?;
                caller.send(json!({"jsonrpc":"2.0","id":19,"method":"ping"}))?;
                caller.reply(19)?;
                selected
                    .require_current()
                    .map_err(|_| "wrong typed cancellation retired original upload")?;
                caller.send(json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":7}}))?;
                if caller.reply(7)?["isError"] != true {
                    return Err("cancelled MCP call succeeded".into());
                }
            } else {
                caller.finish()?;
            }
            until(|| settled(&world, &grant) && selected.require_current().is_err())?;
            if world
                .broker
                .request_stop(&grant.run_id)
                .map_err(|error| error.to_string())?
                != crate::computer_use::StopState::StopRequested
            {
                return Err("disconnect released held original file-choice occupancy".into());
            }
            drop(selected);
        }
        caller.finish()?;
        world
            .broker
            .request_stop(&grant.run_id)
            .map_err(|error| error.to_string())?;
        if world
            .broker
            .wait_stopped(&grant.run_id, Duration::from_secs(5))
            .map_err(|error| error.to_string())?
            != crate::computer_use::StopState::Stopped
            || !world.adapter.is_idle(&grant.run_id)
            || server.received().len() != receiver_requests
        {
            return Err("MCP/native owners or zero-replay postcondition failed".into());
        }
        evidence.push(json!({"mode":mode,"assigned":assigned,"originalCallerJoined":true,
            "originalAdapterIdle":true,"stopped":true,"replayDispatched":false,
            "receiverRequests":receiver_requests,"elapsedMs":began.elapsed().as_millis(),"receipt":independent_receipt}));
    }
    Ok(
        json!({"scenariosPassed":evidence.len(),"scenarios":evidence,"totalReceiverRequests":receiver_requests,
        "nativeIdle":true,"shippedNodeAndEmbeddedMcp":true,"fileChoiceSynthetic":true}),
    )
}
