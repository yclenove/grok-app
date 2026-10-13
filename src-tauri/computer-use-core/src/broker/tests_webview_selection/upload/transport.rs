//! Real Core HTTP and original Node MCP; file choice/native receipt synthetic.
use super::*;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;

fn arguments(u: &UploadFixture) -> serde_json::Value {
    let mut args = serde_json::to_value(&u.intent).unwrap();
    args["version"] = json!(1);
    args
}
fn body(u: &UploadFixture) -> serde_json::Value {
    json!({"session":"webview-app","name":"computer_upload","arguments":arguments(u)})
}
fn http(
    server: &crate::ipc::IpcServer,
    token: &str,
    path: &str,
    body: &serde_json::Value,
) -> reqwest::blocking::Response {
    reqwest::blocking::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(25))
        .build()
        .unwrap()
        .post(format!("{}{path}", server.url))
        .bearer_auth(token)
        .json(body)
        .send()
        .unwrap()
}
fn wait_for(mut condition: impl FnMut() -> bool) {
    let until = Instant::now() + Duration::from_secs(6);
    while !condition() {
        assert!(
            Instant::now() < until,
            "original upload operation did not settle"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}
fn request_id(u: &UploadFixture) -> String {
    let mut id = None;
    wait_for(|| {
        id = u.f.broker.host_task_view(&u.f.token).unwrap().executions[0]
            .upload
            .as_ref()
            .map(|view| view.request_id.clone());
        id.is_some()
    });
    id.unwrap()
}
fn no_pending(u: &UploadFixture) -> bool {
    u.f.broker.inner.lock().runs[&u.intent.run_id]
        .upload
        .is_none()
}
fn original_socket(u: &UploadFixture, server: &crate::ipc::IpcServer) -> TcpStream {
    let address = server.url.strip_prefix("http://").unwrap();
    let mut socket = TcpStream::connect(address).unwrap();
    socket
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let body = body(u).to_string();
    write!(socket, "POST /cu/upload HTTP/1.1\r\nHost: {address}\r\nAuthorization: Bearer {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}", u.f.token, body.len()).unwrap();
    let mut header = [0; 1024];
    let read = socket.read(&mut header).unwrap();
    assert!(String::from_utf8_lossy(&header[..read]).starts_with("HTTP/1.1 200"));
    socket
}
fn text(result: &serde_json::Value) -> serde_json::Value {
    serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap()
}

#[test]
fn permission_preflight_checks_exact_file_identity_without_reserving_or_spending() {
    let u = UploadFixture::new(false);
    let args = arguments(&u);
    u.f.broker
        .preflight_tool_permission(&u.f.token, "computer_upload", &args)
        .unwrap();
    assert!(no_pending(&u));
    assert!(u.f.broker.inner.lock().runs[&u.intent.run_id]
        .seen
        .is_empty());
    for key in ["path", "bytes", "filename", "selector", "script"] {
        let mut forged = args.clone();
        forged[key] = json!("forged");
        assert!(u
            .f
            .broker
            .preflight_tool_permission(&u.f.token, "computer_upload", &forged)
            .is_err());
    }
    for field in ["runId", "targetId", "snapshotId", "elementRef"] {
        let mut stale = args.clone();
        stale[field] = json!("foreign");
        assert!(
            u.f.broker
                .preflight_tool_permission(&u.f.token, "computer_upload", &stale)
                .is_err(),
            "{field}"
        );
    }
    let mut stale = args.clone();
    stale["geometryRevision"] = json!(u.intent.geometry_revision + 1);
    assert!(u
        .f
        .broker
        .preflight_tool_permission(&u.f.token, "computer_upload", &stale)
        .is_err());
    u.f.broker
        .inner
        .lock()
        .runs
        .get_mut(&u.intent.run_id)
        .unwrap()
        .upload_refs
        .clear();
    assert!(u
        .f
        .broker
        .preflight_tool_permission(&u.f.token, "computer_upload", &args)
        .is_err());
    assert!(no_pending(&u));
}

#[test]
fn an_already_retired_original_caller_never_publishes_a_host_choice() {
    let u = UploadFixture::new(false);
    assert!(u
        .f
        .broker
        .request_webview_upload_for_caller(u.intent.clone(), &u.permit, |_| false)
        .is_err());
    assert!(no_pending(&u));
    assert!(u.f.broker.inner.lock().runs[&u.intent.run_id]
        .seen
        .is_empty());
    assert!(u.f.adapter.uploads.lock().is_empty());
}

#[test]
fn real_upload_http_auth_and_generic_dispatch_never_open_a_file_choice() {
    let u = UploadFixture::new(false);
    let server = crate::ipc::spawn(u.f.broker.clone()).unwrap();
    assert_eq!(
        http(&server, "forged", "/cu/upload", &body(&u))
            .status()
            .as_u16(),
        401
    );
    let mut foreign = body(&u);
    foreign["session"] = json!("another-session");
    assert_eq!(
        http(&server, &u.f.token, "/cu/upload", &foreign)
            .status()
            .as_u16(),
        403
    );
    for (header, value) in [
        ("origin", "http://127.0.0.1"),
        ("cookie", "browser-cookie"),
        ("host", "evil.invalid"),
        ("x-forwarded-host", "127.0.0.1"),
    ] {
        let response = reqwest::blocking::Client::builder()
            .no_proxy()
            .build()
            .unwrap()
            .post(format!("{}/cu/upload", server.url))
            .bearer_auth(&u.f.token)
            .header(header, value)
            .json(&body(&u))
            .send()
            .unwrap();
        assert_eq!(response.status().as_u16(), 403, "{header}");
    }
    let result: serde_json::Value = http(&server, &u.f.token, "/cu/tool", &body(&u))
        .json()
        .unwrap();
    assert_eq!(result["isError"], true);
    let mut wrong = body(&u);
    wrong["name"] = json!("computer_observe");
    assert_eq!(
        http(&server, &u.f.token, "/cu/upload", &wrong)
            .status()
            .as_u16(),
        400
    );
    assert!(no_pending(&u));
    assert!(u.f.adapter.uploads.lock().is_empty());
}

#[test]
fn real_http_disconnect_retires_the_original_choice_without_releasing_a_held_picker() {
    let u = UploadFixture::new(true);
    let server = crate::ipc::spawn(u.f.broker.clone()).unwrap();
    let socket = original_socket(&u, &server);
    let id = request_id(&u);
    let selection =
        u.f.broker
            .begin_host_upload_selection(&u.f.token, &u.address, &id)
            .unwrap();
    drop(socket);
    wait_for(|| no_pending(&u));
    assert!(selection.require_current().is_err());
    assert!(u.f.broker.inner.lock().runs[&u.intent.run_id]
        .in_flight
        .load(Ordering::SeqCst));
    assert_eq!(
        u.f.broker.request_stop(&u.intent.run_id).unwrap(),
        StopState::StopRequested
    );
    drop(selection);
    u.f.drain();
    assert_eq!(
        u.f.broker.stop_state(&u.intent.run_id).unwrap(),
        StopState::Stopped
    );
    assert!(u.f.adapter.uploads.lock().is_empty());
}

#[test]
fn listener_shutdown_fences_an_original_waiter_even_with_a_connected_client() {
    let u = UploadFixture::new(false);
    let server = crate::ipc::spawn(u.f.broker.clone()).unwrap();
    let socket = original_socket(&u, &server);
    request_id(&u);
    drop(server);
    wait_for(|| no_pending(&u));
    assert!(u.f.adapter.uploads.lock().is_empty());
    drop(socket);
}

struct NodeCaller {
    child: Child,
    replies: mpsc::Receiver<serde_json::Value>,
    reader: Option<std::thread::JoinHandle<()>>,
}
impl NodeCaller {
    fn new(u: &UploadFixture, server: &crate::ipc::IpcServer) -> Self {
        let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tools/computer-use-mcp/server.mjs");
        let mut command = Command::new(crate::runtime_test_seed::node());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000);
        }
        let mut child = command
            .arg(script)
            .env("GROK_APP_CU_IPC", &server.url)
            .env("GROK_APP_CU_TOKEN", &u.f.token)
            .env("GROK_APP_CU_SESSION", "webview-app")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        let (send, replies) = mpsc::channel();
        let reader = std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else {
                    break;
                };
                let Ok(value) = serde_json::from_str(&line) else {
                    break;
                };
                if send.send(value).is_err() {
                    break;
                }
            }
        });
        Self {
            child,
            replies,
            reader: Some(reader),
        }
    }
    fn send(&mut self, value: serde_json::Value) {
        writeln!(self.child.stdin.as_mut().unwrap(), "{value}").unwrap();
    }
    fn call(&mut self, u: &UploadFixture, id: i64) {
        self.send(json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":"computer_upload","arguments":arguments(u)}}));
    }
    fn reply(&self) -> serde_json::Value {
        self.replies.recv_timeout(Duration::from_secs(22)).unwrap()
    }
    fn finish(&mut self) {
        self.child.stdin.take();
        wait_for(|| self.child.try_wait().unwrap().is_some());
        assert!(self.child.wait().unwrap().success());
        let mut stderr = String::new();
        self.child
            .stderr
            .take()
            .unwrap()
            .read_to_string(&mut stderr)
            .unwrap();
        assert!(stderr.is_empty(), "{stderr}");
        self.reader.take().unwrap().join().unwrap();
    }
}
impl Drop for NodeCaller {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
        }
        let _ = self.child.wait();
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

#[test]
fn original_node_mcp_waits_over_fifteen_seconds_assigns_once_and_replays_only_the_result() {
    let u = UploadFixture::new(true);
    let server = crate::ipc::spawn(u.f.broker.clone()).unwrap();
    let mut node = NodeCaller::new(&u, &server);
    let began = Instant::now();
    node.call(&u, 21);
    let id = request_id(&u);
    std::thread::sleep(Duration::from_secs(16));
    assert!(
        node.replies.try_recv().is_err(),
        "no timeout, new model tool or premature result"
    );
    let selection =
        u.f.broker
            .begin_host_upload_selection(&u.f.token, &u.address, &id)
            .unwrap();
    selection.assign(UploadFixture::file()).unwrap();
    let reply = node.reply();
    assert_eq!(reply["id"], 21);
    assert_eq!(reply["result"]["isError"], false, "{reply}");
    assert_eq!(text(&reply["result"])["assignmentVerified"], true);
    assert_eq!(text(&reply["result"])["submissionVerified"], false);
    assert!(began.elapsed() >= Duration::from_secs(16));
    u.f.broker
        .preflight_tool_permission(&u.f.token, "computer_upload", &arguments(&u))
        .unwrap();
    node.call(&u, 22);
    assert_eq!(node.reply()["result"], reply["result"]);
    assert!(no_pending(&u));
    assert_eq!(u.f.adapter.uploads.lock().len(), 1);
    assert_eq!(u.f.broker.inner.lock().runs[&u.intent.run_id].seen.len(), 1);
    node.finish();
}

#[test]
fn original_mcp_cancel_is_exact_and_eof_retirement_does_not_pause_a_successor() {
    for eof in [false, true] {
        let u = UploadFixture::new(false);
        let server = crate::ipc::spawn(u.f.broker.clone()).unwrap();
        let mut node = NodeCaller::new(&u, &server);
        node.call(&u, 41);
        let id = request_id(&u);
        let selection =
            u.f.broker
                .begin_host_upload_selection(&u.f.token, &u.address, &id)
                .unwrap();
        node.send(
            json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":"41"}}),
        );
        node.send(json!({"jsonrpc":"2.0","id":42,"method":"ping"}));
        assert_eq!(node.reply()["id"], 42);
        assert!(!no_pending(&u));
        assert!(!u.f.broker.is_paused(&u.intent.run_id).unwrap());
        if eof {
            node.finish();
        } else {
            node.send(json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":41}}));
            let cancelled = node.reply();
            assert_eq!(cancelled["id"], 41);
            assert_eq!(cancelled["result"]["isError"], true);
            node.finish();
        }
        wait_for(|| no_pending(&u));
        assert!(selection.require_current().is_err());
        assert!(!u.f.broker.is_paused(&u.intent.run_id).unwrap());
        assert_eq!(
            u.f.broker.request_stop(&u.intent.run_id).unwrap(),
            StopState::StopRequested
        );
        drop(selection);
        u.f.drain();
        assert_eq!(
            u.f.broker.stop_state(&u.intent.run_id).unwrap(),
            StopState::Stopped
        );
        assert!(u.f.adapter.uploads.lock().is_empty());
    }
}

#[test]
fn original_mcp_cancel_during_preparation_cannot_publish_a_late_file_choice() {
    let u = UploadFixture::new(false);
    let server = crate::ipc::spawn(u.f.broker.clone()).unwrap();
    let (entered, entered_rx) = mpsc::channel();
    let (release, release_rx) = mpsc::channel();
    u.f.broker.arm_execution_barrier(Box::new(move || {
        entered.send(()).unwrap();
        release_rx.recv_timeout(Duration::from_secs(6)).unwrap();
    }));
    let mut node = NodeCaller::new(&u, &server);
    node.call(&u, 61);
    entered_rx.recv_timeout(Duration::from_secs(6)).unwrap();
    assert_eq!(server.test_upload_waiter_counts(), (1, 0));
    node.send(
        json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":61}}),
    );
    assert_eq!(node.reply()["result"]["isError"], true);
    wait_for(|| server.test_upload_waiter_counts() == (1, 1));
    assert!(no_pending(&u));
    release.send(()).unwrap();
    wait_for(|| server.test_upload_waiter_counts() == (0, 0));
    assert!(no_pending(&u));
    assert!(u.f.broker.inner.lock().runs[&u.intent.run_id]
        .seen
        .is_empty());
    assert!(u.f.adapter.uploads.lock().is_empty());
    node.finish();
}
