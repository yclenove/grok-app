//! Loopback gateway: exact Host and Origin, bearer, and per-session images.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use gcu_driver_fake::FakeDriver;
use gcu_host_api::{
    testing::{FakeUserActivity, MemoryPolicyStore, RecordingEventSink, ScriptedApprovals},
    ApprovalDecision, ConfirmationLevel, ForegroundPolicy, PolicyConfig,
};
use gcu_mcp::{http::HttpEndpoint, Connection, Host, ServerConfig};
use serde_json::json;
use std::{net::SocketAddr, sync::Arc, time::Duration};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn init_body() -> String {
    json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2025-11-25",
            "capabilities": {},
            "clientInfo": {"name": "gateway-test", "version": "1"}
        }
    })
    .to_string()
}

fn authority_of(url: &str) -> String {
    url.trim_start_matches("http://")
        .trim_end_matches("/mcp")
        .to_owned()
}

fn addr_of(url: &str) -> SocketAddr {
    authority_of(url).parse().unwrap()
}

async fn bare() -> HttpEndpoint {
    let driver = Arc::new(FakeDriver::from_scene("form").unwrap());
    let connection = Connection::open_host_only(driver, test_host(), ServerConfig::default())
        .await
        .unwrap();
    HttpEndpoint::start(connection, 0).await.unwrap()
}

fn test_host() -> Host {
    let approvals = Arc::new(ScriptedApprovals::new());
    approvals.push_decision(ApprovalDecision::Session);
    approvals.push_decision(ApprovalDecision::Session);
    Host {
        approvals,
        policy: Arc::new(MemoryPolicyStore::new(PolicyConfig {
            sensitive_action_confirmation: ConfirmationLevel::Off,
            allow_foreground: ForegroundPolicy::Always,
            ..PolicyConfig::default()
        })),
        events: Arc::new(RecordingEventSink::new()),
        activity: Arc::new(FakeUserActivity::new()),
    }
}

async fn seeded() -> (HttpEndpoint, String) {
    let driver = Arc::new(
        FakeDriver::from_scene("form")
            .unwrap()
            .with_image_digest([7; 32])
            .unwrap(),
    );
    let connection = Connection::open_host_only(driver, test_host(), ServerConfig::default())
        .await
        .unwrap();
    let result = tokio::time::timeout(
        Duration::from_secs(20),
        connection.call(
            "get_app_state",
            json!({"app": "fake.form", "include_screenshot": true}),
        ),
    )
    .await
    .expect("observe timed out");
    assert_ne!(result.is_error, Some(true), "observe failed");
    let id = result
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/state/screenshot/id"))
        .and_then(|value| value.as_str())
        .expect("screenshot id")
        .to_owned();
    let endpoint = HttpEndpoint::start(connection, 0).await.unwrap();
    (endpoint, id)
}

async fn exchange(addr: SocketAddr, request: &str) -> Result<(u16, String), std::io::Error> {
    let mut stream = tokio::net::TcpStream::connect(addr).await?;
    stream.write_all(request.as_bytes()).await?;
    let mut buf = Vec::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(done) = complete(&buf) {
            return Ok(done);
        }
        if tokio::time::Instant::now() >= deadline {
            break;
        }
        let mut tmp = [0_u8; 4096];
        match tokio::time::timeout(Duration::from_millis(200), stream.read(&mut tmp)).await {
            Ok(Ok(0)) => break,
            Ok(Ok(n)) => buf.extend_from_slice(&tmp[..n]),
            Ok(Err(error)) if buf.is_empty() => return Err(error),
            Ok(Err(_)) => break,
            Err(_) => {}
        }
    }
    complete(&buf).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            "incomplete HTTP response",
        )
    })
}

fn complete(buf: &[u8]) -> Option<(u16, String)> {
    let text = std::str::from_utf8(buf).ok()?;
    let (head, body) = text.split_once("\r\n\r\n")?;
    let status = head
        .lines()
        .next()?
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()?;
    let length = head.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.eq_ignore_ascii_case("content-length")
            .then(|| value.trim().parse::<usize>().ok())?
    });
    match length {
        Some(n) if body.len() >= n => Some((status, body[..n].to_owned())),
        Some(_) => None,
        None if buf.ends_with(b"\r\n\r\n") || !body.is_empty() => Some((status, body.to_owned())),
        None => None,
    }
}

fn request(host: &str, origin: Option<&str>, token: Option<&str>, extra: &str) -> String {
    let body = init_body();
    let mut headers = format!(
        "POST /mcp HTTP/1.1\r\nHost: {host}\r\nContent-Type: application/json\r\nAccept: application/json, text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n",
        body.len()
    );
    if let Some(origin) = origin {
        headers.push_str(&format!("Origin: {origin}\r\n"));
    }
    if let Some(token) = token {
        headers.push_str(&format!("Authorization: Bearer {token}\r\n"));
    }
    headers.push_str(extra);
    if !extra.is_empty() && !extra.ends_with("\r\n") {
        headers.push_str("\r\n");
    }
    headers.push_str("\r\n");
    headers.push_str(&body);
    headers
}

async fn status(
    url: &str,
    host: &str,
    origin: Option<&str>,
    token: Option<&str>,
    extra: &str,
) -> (u16, String) {
    exchange(addr_of(url), &request(host, origin, token, extra))
        .await
        .unwrap()
}

#[tokio::test]
async fn loopback_gate_rejects_rebinding_browsers_and_foreign_tokens() {
    let mut left = bare().await;
    let mut right = bare().await;
    let url = left.url().to_owned();
    let authority = authority_of(&url);
    let port = authority.rsplit(':').next().unwrap();
    let other_port = if port == "9" { "1" } else { "9" };
    let token = left.token().to_owned();
    let foreign = right.token().to_owned();
    assert_ne!(token, foreign);
    assert!(!url.contains(&token));

    let origin = format!("http://{authority}");
    let localhost_host = format!("localhost:{port}");
    let v6_host = format!("[::1]:{port}");
    let wrong_port = format!("http://127.0.0.1:{other_port}");
    let localhost_origin = format!("http://localhost:{port}");
    let upper = token.to_uppercase();
    let cases = [
        ("evil.test", None, Some(token.as_str()), "", 403_u16),
        ("localhost", None, Some(token.as_str()), "", 403),
        (localhost_host.as_str(), None, Some(token.as_str()), "", 403),
        (v6_host.as_str(), None, Some(token.as_str()), "", 403),
        ("127.0.0.1", None, Some(token.as_str()), "", 403),
        (
            authority.as_str(),
            Some("https://evil.test"),
            Some(token.as_str()),
            "",
            403,
        ),
        (
            authority.as_str(),
            Some("null"),
            Some(token.as_str()),
            "",
            403,
        ),
        (authority.as_str(), Some("*"), Some(token.as_str()), "", 403),
        (
            authority.as_str(),
            Some(wrong_port.as_str()),
            Some(token.as_str()),
            "",
            403,
        ),
        (
            authority.as_str(),
            Some(localhost_origin.as_str()),
            Some(token.as_str()),
            "",
            403,
        ),
        (authority.as_str(), None, None, "", 401),
        (authority.as_str(), Some(origin.as_str()), None, "", 401),
        (authority.as_str(), None, Some(foreign.as_str()), "", 401),
        (authority.as_str(), None, Some(upper.as_str()), "", 401),
        (
            authority.as_str(),
            None,
            Some(token.as_str()),
            "Authorization: Bearer extra\r\n",
            401,
        ),
        (
            authority.as_str(),
            None,
            Some(token.as_str()),
            "Sec-Fetch-Site: cross-site\r\n",
            403,
        ),
    ];
    for (host, origin_header, bearer, extra, expected) in cases {
        let (code, body) = status(&url, host, origin_header, bearer, extra).await;
        assert_eq!(code, expected, "host={host} origin={origin_header:?}");
        assert!(!body.contains(&token), "response echoed the bearer");
        assert!(!body.contains(&foreign), "response echoed the bearer");
    }

    let (ok, body) = status(&url, &authority, None, Some(&token), "").await;
    assert!(ok == 200 || ok == 202, "initialize status {ok}");
    assert!(!body.contains(&token), "response echoed the bearer");
    let (with_origin, body) = status(&url, &authority, Some(&origin), Some(&token), "").await;
    assert!(
        with_origin == 200 || with_origin == 202,
        "origin initialize status {with_origin}"
    );
    assert!(!body.contains(&token), "response echoed the bearer");

    let mut raw = tokio::net::TcpStream::connect(addr_of(&url)).await.unwrap();
    let dup = format!(
        "POST /mcp HTTP/1.1\r\nHost: {authority}\r\nHost: evil.test\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    );
    raw.write_all(dup.as_bytes()).await.unwrap();
    let mut prefix = [0_u8; 24];
    let n = tokio::time::timeout(Duration::from_secs(3), raw.read(&mut prefix))
        .await
        .unwrap()
        .unwrap();
    let line = String::from_utf8_lossy(&prefix[..n]);
    assert!(
        line.contains(" 400") || line.contains(" 403"),
        "duplicate host status {line}"
    );

    right.close().await.unwrap();
    left.close().await.unwrap();
}

#[tokio::test]
async fn cross_session_image_stays_private_and_close_revokes_the_token() {
    let (mut left, id) = seeded().await;
    let mut right = bare().await;
    assert_ne!(left.token(), right.token());
    let shot = left
        .screenshot(&id)
        .expect("owning session keeps the image");
    assert!(!shot.bytes.is_empty());
    assert!(right.screenshot(&id).is_none());

    let url = left.url().to_owned();
    let authority = authority_of(&url);
    let token = left.token().to_owned();
    let foreign = right.token().to_owned();
    let (code, body) = status(&url, &authority, None, Some(&foreign), "").await;
    assert_eq!(code, 401);
    assert!(!body.contains(&token), "response echoed the bearer");
    assert!(!body.contains(&foreign), "response echoed the bearer");

    left.close().await.unwrap();
    assert!(left.screenshot(&id).is_none());
    match exchange(addr_of(&url), &request(&authority, None, Some(&token), "")).await {
        Err(_) => {}
        Ok((code, body)) => {
            assert!(code == 401 || code == 410, "revoked status {code}");
            assert!(!body.contains(&token), "response echoed the bearer");
        }
    }
    right.close().await.unwrap();
}
