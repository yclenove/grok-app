//! Process-level route B contracts with a Rust Host and the offline fake driver.
#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used)]
use gcu_host_protocol::{messages::*, server::RPC_TIMEOUT, testing::TestHost};
use rmcp::{
    model::{CallToolRequestParams, ClientConfig},
    transport::{
        streamable_http_client::StreamableHttpClientTransportConfig, StreamableHttpClientTransport,
    },
    ServiceExt,
};
use serde_json::json;
use std::{path::Path, time::Duration};
fn binary() -> &'static Path {
    Path::new(assert_cmd::cargo::cargo_bin!("gcu"))
}
async fn open(host: &TestHost, id: &str) -> SessionOpened {
    host.peer
        .call(
            "engine.session.open",
            &SessionOpen {
                session_id: id.into(),
                kind: SessionKind::Local,
            },
            RPC_TIMEOUT,
        )
        .await
        .unwrap()
}
async fn client(
    endpoint: &SessionOpened,
) -> rmcp::service::RunningService<rmcp::RoleClient, ClientConfig> {
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let config = StreamableHttpClientTransportConfig::with_uri(endpoint.mcp_url.clone())
        .auth_header(endpoint.token.clone())
        .reinit_on_expired_session(false);
    ClientConfig::default()
        .serve(StreamableHttpClientTransport::with_client(client, config))
        .await
        .unwrap()
}
fn tool(name: &str, args: serde_json::Value) -> CallToolRequestParams {
    CallToolRequestParams::new(name.to_owned()).with_arguments(args.as_object().unwrap().clone())
}
#[tokio::test]
async fn engine_session_approval_action_turn_and_disconnect_follow_host_lifecycle() {
    let mut host = TestHost::spawn(binary(), "form").await.unwrap();
    let endpoint = open(&host, "chat-1").await;
    assert_eq!(endpoint.token.len(), 64);
    assert!(endpoint.mcp_url.starts_with("http://127.0.0.1:"));
    let client = client(&endpoint).await;
    let call = client.call_tool(tool(
        "get_app_state",
        json!({"app":"fake.form","include_screenshot":false}),
    ));
    tokio::pin!(call);
    let approval = tokio::select! {biased;r=&mut call=>panic!("approval was bypassed: {r:?}"),r=host.next_method("host.approval.requestApp")=>r.unwrap()};
    let params: RequestApp = serde_json::from_value(approval.request.params.clone()).unwrap();
    assert_eq!(params.session_id, "chat-1");
    approval.reply(AppDecision {
        decision: Decision::Turn,
    });
    let result = call.await.unwrap();
    assert_ne!(result.is_error, Some(true));
    let result = client
        .call_tool(tool(
            "set_value",
            json!({"app":"fake.form","element_index":1,"value":"Ada"}),
        ))
        .await
        .unwrap();
    assert_ne!(
        result.is_error,
        Some(true),
        "{:?}",
        result.structured_content
    );
    let event = host.next_method("host.event.emit").await.unwrap();
    let event: Event = serde_json::from_value(event.request.params).unwrap();
    assert_eq!(event.session_id, "chat-1");
    let _: Empty = host
        .peer
        .call(
            "engine.session.turnEnded",
            &SessionId {
                session_id: "chat-1".into(),
            },
            RPC_TIMEOUT,
        )
        .await
        .unwrap();
    let call = client.call_tool(tool(
        "get_app_state",
        json!({"app":"fake.form","include_screenshot":false}),
    ));
    tokio::pin!(call);
    let approval = tokio::select! {r=&mut call=>panic!("turn grant survived: {r:?}"),r=host.next_method("host.approval.requestApp")=>r.unwrap()};
    approval.reply(AppDecision {
        decision: Decision::Session,
    });
    assert_ne!(call.await.unwrap().is_error, Some(true));
    let report: DoctorReport = host
        .peer
        .call("engine.system.doctor", &Empty {}, RPC_TIMEOUT)
        .await
        .unwrap();
    assert!(!report.capabilities.user_activity_guard);
    assert!(report
        .issues
        .iter()
        .any(|s| s == "user_activity_guard: unavailable"));
    host.disconnect();
    tokio::time::timeout(Duration::from_secs(8), host.child.wait())
        .await
        .unwrap()
        .unwrap();
    assert!(client
        .call_tool(tool("list_apps", json!({})))
        .await
        .is_err());
    assert!(reqwest::Client::builder()
        .no_proxy()
        .build()
        .unwrap()
        .post(&endpoint.mcp_url)
        .bearer_auth(&endpoint.token)
        .send()
        .await
        .is_err());
}
#[tokio::test]
async fn engine_stop_bypasses_pending_approval_and_resume_requires_control_and_fresh_event() {
    let mut host = TestHost::spawn(binary(), "form").await.unwrap();
    let endpoint = open(&host, "blocked").await;
    let client = client(&endpoint).await;
    let call = client.call_tool(tool(
        "get_app_state",
        json!({"app":"fake.form","include_screenshot":false}),
    ));
    tokio::pin!(call);
    let pending = tokio::select! {r=&mut call=>panic!("unexpected result {r:?}"),r=host.next_method("host.approval.requestApp")=>r.unwrap()};
    let start = tokio::time::Instant::now();
    let _: Empty = host
        .stop
        .call(
            "engine.session.stop",
            &Stop { session_id: None },
            gcu_core::session::STOP_LATCH_BUDGET,
        )
        .await
        .unwrap();
    assert!(start.elapsed() <= gcu_core::session::STOP_LATCH_BUDGET);
    let resume = Resume {
        session_id: None,
        sequence: 1,
        user_event: [7; 32],
    };
    assert!(host
        .peer
        .call::<_, Empty>("engine.session.resume", &resume, RPC_TIMEOUT)
        .await
        .is_err());
    pending.reply(AppDecision {
        decision: Decision::Session,
    });
    let result = tokio::time::timeout(gcu_core::session::RELEASE_INPUT_BUDGET, &mut call)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(result.is_error, Some(true));
    let _: Empty = host
        .stop
        .call("engine.session.resume", &resume, RPC_TIMEOUT)
        .await
        .unwrap();
    let _: Empty = host
        .stop
        .call(
            "engine.session.stop",
            &Stop { session_id: None },
            RPC_TIMEOUT,
        )
        .await
        .unwrap();
    assert!(host
        .stop
        .call::<_, Empty>("engine.session.resume", &resume, RPC_TIMEOUT)
        .await
        .is_err());
    let result = client
        .call_tool(tool("click", json!({"app":"fake.form","element_index":1})))
        .await
        .unwrap();
    assert_eq!(result.is_error, Some(true));
    let fresh = Resume {
        sequence: 2,
        user_event: [8; 32],
        ..resume
    };
    let _: Empty = host
        .stop
        .call("engine.session.resume", &fresh, RPC_TIMEOUT)
        .await
        .unwrap();
}
#[tokio::test]
async fn engine_http_rejects_rebinding_origins_missing_tokens_and_cross_session_credentials() {
    let host = TestHost::spawn(binary(), "form").await.unwrap();
    let a = open(&host, "a").await;
    let b = open(&host, "b").await;
    assert_ne!(a.token, b.token);
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let init = json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"test","version":"1"}}});
    for (host_header, origin, token, expected) in [
        (Some("evil.test"), None, Some(a.token.as_str()), 403),
        (None, Some("https://evil.test"), Some(a.token.as_str()), 403),
        (None, None, None, 401),
        (None, None, Some(b.token.as_str()), 401),
    ] {
        let mut request = client
            .post(&a.mcp_url)
            .header("Content-Type", "application/json")
            .header("Accept", "application/json, text/event-stream")
            .json(&init);
        if let Some(h) = host_header {
            request = request.header("Host", h);
        }
        if let Some(o) = origin {
            request = request.header("Origin", o);
        }
        if let Some(t) = token {
            request = request.bearer_auth(t);
        }
        assert_eq!(request.send().await.unwrap().status().as_u16(), expected);
    }
    // Hyper must reject duplicate Host before routing; exercise the actual socket.
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let authority = a
        .mcp_url
        .strip_prefix("http://")
        .unwrap()
        .strip_suffix("/mcp")
        .unwrap();
    let mut raw = tokio::net::TcpStream::connect(authority).await.unwrap();
    raw.write_all(format!("POST /mcp HTTP/1.1\r\nHost: {authority}\r\nHost: evil.test\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").as_bytes()).await.unwrap();
    let mut response = String::new();
    raw.read_to_string(&mut response).await.unwrap();
    assert!(response.starts_with("HTTP/1.1 400") || response.starts_with("HTTP/1.1 403"));
    let result: Result<SessionOpened, _> = host
        .peer
        .call(
            "engine.session.open",
            &SessionOpen {
                session_id: "remote".into(),
                kind: SessionKind::Ssh,
            },
            RPC_TIMEOUT,
        )
        .await;
    assert!(result.is_err());
    let _: Empty = host
        .peer
        .call(
            "engine.session.close",
            &SessionId {
                session_id: "a".into(),
            },
            RPC_TIMEOUT,
        )
        .await
        .unwrap();
    assert!(client
        .post(&a.mcp_url)
        .bearer_auth(&a.token)
        .json(&init)
        .send()
        .await
        .is_err());
}
#[tokio::test]
async fn engine_major_mismatch_and_untrusted_persistent_grant_methods_are_rejected() {
    assert!(TestHost::spawn_version(
        binary(),
        "form",
        Version {
            major: 99,
            minor: 0
        },
        false
    )
    .await
    .is_err());
    let host = TestHost::spawn(binary(), "form").await.unwrap();
    assert!(host
        .peer
        .call::<_, Empty>("host.policy.grantAlways", &Empty {}, RPC_TIMEOUT)
        .await
        .is_err());
    assert!(!METHODS.iter().any(|m| m.name == "host.policy.grantAlways"));
}
#[tokio::test]
async fn engine_disconnect_denies_pending_approval_and_persistent_stop_survives_restart() {
    let mut host = TestHost::spawn_version(binary(), "form", VERSION, true)
        .await
        .unwrap();
    let endpoint = open(&host, "stopped").await;
    let client = client(&endpoint).await;
    let result = client
        .call_tool(tool(
            "get_app_state",
            json!({"app":"fake.form","include_screenshot":false}),
        ))
        .await
        .unwrap();
    assert_eq!(result.is_error, Some(true));
    let _: Empty = host
        .stop
        .call(
            "engine.session.resume",
            &Resume {
                session_id: None,
                sequence: 1,
                user_event: [1; 32],
            },
            RPC_TIMEOUT,
        )
        .await
        .unwrap();
    let call = client.call_tool(tool(
        "get_app_state",
        json!({"app":"fake.form","include_screenshot":false}),
    ));
    tokio::pin!(call);
    let _pending = tokio::select! {r=&mut call=>panic!("unexpected result {r:?}"),r=host.next_method("host.approval.requestApp")=>r.unwrap()};
    host.disconnect();
    let result = tokio::time::timeout(Duration::from_secs(8), call)
        .await
        .unwrap();
    assert!(result.is_err() || result.unwrap().is_error == Some(true));
    tokio::time::timeout(Duration::from_secs(8), host.child.wait())
        .await
        .unwrap()
        .unwrap();
}
#[tokio::test]
async fn engine_stdio_proxy_uses_secret_fd_and_the_same_host_approval_path() {
    use std::{io::Write, os::fd::AsRawFd, process::Stdio};
    let mut host = TestHost::spawn(binary(), "form").await.unwrap();
    let endpoint = open(&host, "proxy").await;
    let (mut token_writer, token_reader) = std::os::unix::net::UnixStream::pair().unwrap();
    let mut token = [0; 32];
    for (i, byte) in token.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&endpoint.token[i * 2..i * 2 + 2], 16).unwrap();
    }
    token_writer.write_all(&token).unwrap();
    drop(token_writer);
    let mut command = std::process::Command::new(binary());
    command
        .args([
            "mcp",
            "--connect",
            &endpoint.mcp_url,
            "--token-fd",
            &token_reader.as_raw_fd().to_string(),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    gcu_host_protocol::testing::inherit_fds(&mut command, vec![token_reader.as_raw_fd()]);
    let mut command = tokio::process::Command::from(command);
    command.kill_on_drop(true);
    let mut child = command.spawn().unwrap();
    drop(token_reader);
    let client = tokio::time::timeout(
        RPC_TIMEOUT,
        ClientConfig::default().serve((child.stdout.take().unwrap(), child.stdin.take().unwrap())),
    )
    .await
    .unwrap()
    .unwrap();
    {
        let call = client.call_tool(tool(
            "get_app_state",
            json!({"app":"fake.form","include_screenshot":false}),
        ));
        tokio::pin!(call);
        let request = tokio::select! {r=&mut call=>panic!("approval bypass {r:?}"),r=host.next_method("host.approval.requestApp")=>r.unwrap()};
        request.reply(AppDecision {
            decision: Decision::Session,
        });
        assert_ne!(call.await.unwrap().is_error, Some(true));
    }
    client.cancel().await.unwrap();
    tokio::time::timeout(RPC_TIMEOUT, child.wait())
        .await
        .unwrap()
        .unwrap();
}
#[tokio::test]
async fn engine_screenshot_ownership_policy_revisions_and_always_without_host_persistence_fail_closed(
) {
    let mut host = TestHost::spawn(binary(), "form").await.unwrap();
    let a = open(&host, "images").await;
    let _b = open(&host, "other").await;
    let client = client(&a).await;
    let call = client.call_tool(tool("get_app_state", json!({"app":"fake.form"})));
    tokio::pin!(call);
    let request = tokio::select! {r=&mut call=>panic!("approval bypass {r:?}"),r=host.next_method("host.approval.requestApp")=>r.unwrap()};
    request.reply(AppDecision {
        decision: Decision::Always,
    });
    let result = call.await.unwrap();
    // A bare Always response does not persist an engine-owned grant.
    let call = client.call_tool(tool("get_app_state", json!({"app":"fake.form"})));
    tokio::pin!(call);
    let request = tokio::select! {r=&mut call=>panic!("Always persisted without Host {r:?}"),r=host.next_method("host.approval.requestApp")=>r.unwrap()};
    request.reply(AppDecision {
        decision: Decision::Session,
    });
    let approved = call.await.unwrap();
    assert_ne!(approved.is_error, Some(true));
    assert_eq!(result.is_error, Some(true));
    let id = approved.structured_content.unwrap()["state"]["screenshot"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let image: Image = host
        .peer
        .call(
            "engine.screenshot.get",
            &ScreenshotGet {
                session_id: "images".into(),
                id: id.clone(),
            },
            RPC_TIMEOUT,
        )
        .await
        .unwrap();
    assert!(!image.bytes.is_empty());
    assert_eq!(image.mime, "image/png");
    assert!(host
        .peer
        .call::<_, Image>(
            "engine.screenshot.get",
            &ScreenshotGet {
                session_id: "other".into(),
                id
            },
            RPC_TIMEOUT
        )
        .await
        .is_err());
    host.policy.lock().unwrap().version = 2;
    let _: Empty = host
        .peer
        .call(
            "engine.policy.changed",
            &PolicyChanged { version: 2 },
            RPC_TIMEOUT,
        )
        .await
        .unwrap();
    assert!(host
        .peer
        .call::<_, Empty>(
            "engine.policy.changed",
            &PolicyChanged { version: 2 },
            RPC_TIMEOUT
        )
        .await
        .is_err());
    let _: Empty = host
        .peer
        .call("engine.system.shutdown", &Empty {}, RPC_TIMEOUT)
        .await
        .unwrap();
    tokio::time::timeout(RPC_TIMEOUT, host.child.wait())
        .await
        .unwrap()
        .unwrap();
}
#[tokio::test]
async fn engine_pinned_fallback_authenticates_and_exposes_only_local_sessions() {
    let host = TestHost::spawn_fallback(binary(), "form").await.unwrap();
    let endpoint = open(&host, "fallback").await;
    let client = client(&endpoint).await;
    let result = client
        .call_tool(tool("list_apps", json!({})))
        .await
        .unwrap();
    assert_ne!(result.is_error, Some(true));
}
#[tokio::test]
async fn engine_lost_stop_channel_denies_approval_and_exits_with_rpc_still_connected() {
    let mut host = TestHost::spawn(binary(), "form").await.unwrap();
    let endpoint = open(&host, "lost-stop").await;
    let client = client(&endpoint).await;
    let call = client.call_tool(tool("get_app_state", json!({"app":"fake.form"})));
    tokio::pin!(call);
    let pending = tokio::select! {r=&mut call=>panic!("approval bypass {r:?}"),r=host.next_method("host.approval.requestApp")=>r.unwrap()};
    host.stop.close();
    tokio::time::timeout(RPC_TIMEOUT, host.peer.disconnected())
        .await
        .unwrap();
    pending.reply(AppDecision {
        decision: Decision::Session,
    });
    let result = tokio::time::timeout(RPC_TIMEOUT, call).await.unwrap();
    assert!(result.is_err() || result.unwrap().is_error == Some(true));
    tokio::time::timeout(RPC_TIMEOUT, host.child.wait())
        .await
        .unwrap()
        .unwrap();
    assert!(client
        .call_tool(tool("list_apps", json!({})))
        .await
        .is_err());
}
#[tokio::test]
async fn standalone_http_reads_launcher_token_fd_and_serves_real_mcp() {
    use std::{io::Write, os::fd::AsRawFd, process::Stdio};
    use tokio::io::AsyncBufReadExt;
    let fixture = tempfile::TempDir::new().unwrap();
    let secret = gcu_host_protocol::auth::Secret::from_bytes([9; 32]);
    let (mut writer, reader) = std::os::unix::net::UnixStream::pair().unwrap();
    writer.write_all(&[9; 32]).unwrap();
    drop(writer);
    let mut command = std::process::Command::new(binary());
    command
        .args([
            "mcp",
            "--transport",
            "http",
            "--driver",
            "fake",
            "--token-fd",
            &reader.as_raw_fd().to_string(),
        ])
        .env("XDG_CONFIG_HOME", fixture.path())
        .env("XDG_STATE_HOME", fixture.path())
        .env("XDG_RUNTIME_DIR", fixture.path())
        .env("HOME", fixture.path())
        .env_remove("GCU_APPROVAL")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    gcu_host_protocol::testing::inherit_fds(&mut command, vec![reader.as_raw_fd()]);
    let mut command = tokio::process::Command::from(command);
    command.kill_on_drop(true);
    let mut child = command.spawn().unwrap();
    drop(reader);
    let mut line = String::new();
    let mut stdout = tokio::io::BufReader::new(child.stdout.take().unwrap());
    tokio::time::timeout(RPC_TIMEOUT, stdout.read_line(&mut line))
        .await
        .unwrap()
        .unwrap();
    assert!(!line.contains(&secret.hex()));
    let value: serde_json::Value = serde_json::from_str(&line).unwrap();
    let endpoint = SessionOpened {
        mcp_url: value["mcp_url"].as_str().unwrap().into(),
        token: secret.hex(),
    };
    let client = client(&endpoint).await;
    assert_ne!(
        client
            .call_tool(tool("list_apps", json!({})))
            .await
            .unwrap()
            .is_error,
        Some(true)
    );
    child.kill().await.unwrap();
}
