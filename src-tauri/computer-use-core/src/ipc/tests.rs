use super::*;
use crate::{
    broker::BrokerOptions,
    fake::FakeAdapter,
    protocol::{ActionKind, ActionRequest, ActionTarget, OutcomeKind, PROTOCOL_VERSION},
};

fn fixture() -> (Arc<ComputerUseBroker>, Arc<FakeAdapter>, IpcServer, String) {
    let adapter = Arc::new(FakeAdapter::new());
    let broker = Arc::new(ComputerUseBroker::new(
        adapter.clone(),
        BrokerOptions {
            feature_enabled: true,
            lease_path: std::env::temp_dir()
                .join(format!("cu-ipc-review-{}.lock", uuid::Uuid::new_v4())),
            ..BrokerOptions::default()
        },
    ));
    broker.open_run("session-a", "run-a").unwrap();
    broker
        .authorize_target("run-a", &adapter.fixture_id())
        .unwrap();
    let server = spawn(broker.clone()).unwrap();
    let token = server.credential_for_session("session-a", "run-a").unwrap();
    (broker, adapter, server, token)
}

fn call(server: &IpcServer, token: &str, body: Value) -> (u16, Value) {
    let response = reqwest::blocking::Client::new()
        .post(format!("{}/cu/tool", server.url))
        .bearer_auth(token)
        .json(&body)
        .send()
        .unwrap();
    (response.status().as_u16(), response.json().unwrap())
}

#[test]
fn loopback_host_predicate_rejects_lan_and_public_names() {
    assert!(host_is_loopback("127.0.0.1"));
    assert!(host_is_loopback("127.0.0.1:4312"));
    assert!(host_is_loopback("localhost"));
    assert!(host_is_loopback("[::1]:80"));
    assert!(!host_is_loopback("evil.example"));
    assert!(!host_is_loopback("192.168.0.1:80"));
    assert!(!host_is_loopback(""));
}

#[test]
fn credentials_cannot_impersonate_session_or_run() {
    let (broker, _, server, token) = fixture();
    broker.open_run("session-b", "run-b").unwrap();
    for payload in [
        json!({"session":"session-b","name":"computer_status"}),
        json!({"name":"computer_status","arguments":{"runId":"run-b"}}),
        json!({"name":"computer_status","arguments":{"run_id":"run-b"}}),
        json!({"name":"computer_status","arguments":{"appSessionId":"session-b"}}),
        json!({"name":"computer_status","arguments":{"session":"session-b"}}),
    ] {
        assert_eq!(call(&server, &token, payload).0, 403);
    }
    assert_eq!(
        call(&server, &token, json!({"name":"computer_status"})).0,
        200
    );
    assert_eq!(
        call(
            &server,
            &token.to_uppercase(),
            json!({"name":"computer_status"})
        )
        .0,
        401
    );
}

#[test]
fn models_cannot_authorize_or_resume_themselves() {
    let (broker, fake, server, token) = fixture();
    let binding = broker.authorized_target("run-a").unwrap();
    let (_, result) = call(
        &server,
        &token,
        json!({"name":"computer_open_target","arguments":{"targetId":"other-window"}}),
    );
    assert_eq!(result["isError"], true);
    assert_eq!(broker.authorized_target("run-a").unwrap(), binding);
    broker.pause("run-a").unwrap();
    for tool in ["computer_resume", "computer_reconnect", "computer_observe"] {
        assert_eq!(
            call(&server, &token, json!({"name":tool})).1["isError"],
            true
        );
    }
    assert!(broker.is_paused("run-a").unwrap());
    assert!(fake.executions().is_empty());
}

#[test]
fn token_rotation_and_revocation_disable_previous_client() {
    let (broker, fake, server, token) = fixture();
    let rotated = server
        .rotate_session_credential("session-a", "run-a")
        .unwrap();
    assert_eq!(
        call(&server, &token, json!({"name":"computer_status"})).0,
        401
    );
    assert_eq!(
        call(&server, &rotated, json!({"name":"computer_status"})).0,
        200
    );
    server.revoke_session("session-a");
    assert_eq!(
        call(&server, &rotated, json!({"name":"computer_status"})).0,
        401
    );
    assert_eq!(
        broker.stop_state("run-a").unwrap(),
        crate::broker::StopState::Running,
        "credential revocation must not perform lifecycle cleanup"
    );
    assert_eq!(fake.abort_called(), 0);
}

#[test]
fn catalog_rebuild_reuses_the_live_session_credential() {
    let (_, _, server, token) = fixture();
    let rebuilt = server.credential_for_session("session-a", "run-a").unwrap();
    assert_eq!(rebuilt, token);
    assert_eq!(
        call(&server, &token, json!({"name":"computer_status"})).0,
        200
    );
}

#[test]
fn complete_http_body_is_read_across_multiple_tcp_packets() {
    use std::io::{Read, Write};
    let (_, _, server, token) = fixture();
    let mut socket =
        std::net::TcpStream::connect(server.url.trim_start_matches("http://")).unwrap();
    socket
        .set_read_timeout(Some(std::time::Duration::from_secs(2)))
        .unwrap();
    let body = r#"{"name":"computer_status"}"#;
    write!(socket, "POST /cu/tool HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).unwrap();
    for chunk in body.as_bytes().chunks(3) {
        socket.write_all(chunk).unwrap();
    }
    let mut response = String::new();
    socket.read_to_string(&mut response).unwrap();
    assert!(response.starts_with("HTTP/1.1 200"));
    assert!(response.contains("run-a"));
}

#[test]
fn status_is_responsive_while_an_action_is_waiting() {
    let (broker, fake, server, token) = fixture();
    fake.set_hang(true);
    fake.set_abort_does_not_quiesce(true);
    let obs = broker.observe("run-a").unwrap();
    let request = ActionRequest {
        version: PROTOCOL_VERSION,
        action_id: "pending".into(),
        run_id: "run-a".into(),
        target_id: fake.fixture_id(),
        target_generation: obs.target_generation,
        snapshot_id: obs.snapshot_id,
        geometry_revision: obs.geometry_revision,
        action: ActionKind::Click,
        target: ActionTarget::Element {
            element_ref: "n1".into(),
        },
        parameters: json!({}),
    };
    let caller_url = format!("{}/cu/tool", server.url);
    let caller_token = token.clone();
    let caller = std::thread::spawn(move || {
        reqwest::blocking::Client::new()
            .post(caller_url)
            .bearer_auth(caller_token)
            .json(&json!({"name":"computer_act","arguments":request}))
            .send()
            .unwrap()
            .json::<Value>()
            .unwrap()
    });
    let until = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while fake.executions().is_empty() && std::time::Instant::now() < until {
        std::thread::yield_now();
    }
    assert_eq!(fake.executions().len(), 1);
    let start = std::time::Instant::now();
    assert_eq!(
        call(&server, &token, json!({"name":"computer_status"})).0,
        200
    );
    assert!(start.elapsed() < std::time::Duration::from_secs(1));
    broker.request_stop("run-a").unwrap();
    fake.finish_in_flight();
    let result = caller.join().unwrap();
    let outcome: crate::protocol::ActionOutcome =
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(outcome.kind, OutcomeKind::Unknown);
}

#[test]
fn listener_is_loopback_and_token_is_not_session_identity() {
    let (_, _, server, token) = fixture();
    assert!(
        server.url.starts_with("http://127.0.0.1:"),
        "{}",
        server.url
    );
    assert_ne!(token, "session-a");
    assert_ne!(token, "run-a");
    assert!(token.len() >= 32);
    assert!(token.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn drop_revokes_tokens_and_fences_the_run_without_blocking_cleanup() {
    let (broker, _, server, token) = fixture();
    let url = server.url.clone();
    drop(server);
    let response = reqwest::blocking::Client::new()
        .post(format!("{url}/cu/tool"))
        .bearer_auth(&token)
        .json(&json!({"name":"computer_status"}))
        .timeout(std::time::Duration::from_secs(2))
        .send();
    match response {
        Err(_) => {}
        Ok(res) => assert_ne!(res.status().as_u16(), 200),
    }
    assert_eq!(
        broker.stop_state("run-a").unwrap(),
        crate::broker::StopState::StopRequested
    );
}

#[test]
fn model_stop_can_delegate_to_a_non_blocking_host_lifecycle_handler() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    let adapter = Arc::new(FakeAdapter::new());
    adapter.set_abort_blocked(true);
    let broker = Arc::new(ComputerUseBroker::new(
        adapter.clone(),
        BrokerOptions {
            feature_enabled: true,
            lease_path: std::env::temp_dir()
                .join(format!("cu-ipc-stop-handler-{}.lock", uuid::Uuid::new_v4())),
            ..BrokerOptions::default()
        },
    ));
    broker.open_run("session-a", "run-a").unwrap();
    broker
        .authorize_target("run-a", &adapter.fixture_id())
        .unwrap();

    let calls = Arc::new(AtomicUsize::new(0));
    let cleanup = Arc::new(Mutex::new(None));
    let handler_broker = Arc::clone(&broker);
    let handler_calls = Arc::clone(&calls);
    let handler_cleanup = Arc::clone(&cleanup);
    let handler: Arc<crate::tools::ModelStopHandler> = Arc::new(move |binding| {
        assert_eq!(binding.session_id, "session-a");
        assert_eq!(binding.run_id, "run-a");
        handler_calls.fetch_add(1, Ordering::SeqCst);
        let ticket = handler_broker
            .fence_stop(&binding.run_id)
            .map_err(|error| error.to_string())?;
        *handler_cleanup.lock() = ticket;
        handler_broker
            .stop_state(&binding.run_id)
            .map_err(|error| error.to_string())
    });
    let server = spawn_with_stop_handler(Arc::clone(&broker), Some(handler)).unwrap();
    let token = server.credential_for_session("session-a", "run-a").unwrap();

    let started = std::time::Instant::now();
    let (status, result) = call(&server, &token, json!({"name":"computer_stop"}));
    assert_eq!(status, 200);
    assert_eq!(result["isError"], false);
    let stop: Value = serde_json::from_str(
        result["content"][0]["text"]
            .as_str()
            .expect("stop result text"),
    )
    .expect("stop result JSON");
    assert_eq!(stop["stopState"], "stop_requested");
    assert!(started.elapsed() < Duration::from_secs(1));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        adapter.abort_called(),
        0,
        "Host acknowledgement must not clean"
    );

    adapter.set_abort_blocked(false);
    let ticket = cleanup.lock().take().expect("Host cleanup ticket");
    broker.finish_stop_cleanup(&ticket).unwrap();
    assert_eq!(
        broker.stop_state("run-a").unwrap(),
        crate::broker::StopState::Stopped
    );
    assert_eq!(adapter.abort_called(), 1);
}

const EXT_ORIGIN: &str = "chrome-extension://bgegbabkegkanjbmjbeaockdijnbkjgi";

fn proof_body(ch: &crate::pairing::PairingChallenge) -> Value {
    let proof =
        crate::pairing::ExtensionPairing::proof_for(ch, "00000000-0000-4000-8000-000000000001");
    json!({
        "protocol": proof.protocol, "nonce": proof.nonce, "instance": proof.instance,
        "ext": proof.ext, "expiresAt": proof.expires_at,
        "connectionNonce": proof.connection_nonce, "response": proof.response
    })
}

fn pair_post(server: &IpcServer, body: &Value) -> reqwest::blocking::Response {
    reqwest::blocking::Client::builder()
        .no_proxy()
        .build()
        .unwrap()
        .post(format!("{}/cu/pairing-confirm", server.url))
        .header("origin", EXT_ORIGIN)
        .json(body)
        .send()
        .unwrap()
}

#[test]
fn pairing_challenge_is_public_and_key_requires_both_confirmations() {
    let (broker, _, server, _) = fixture();
    let ch = broker.begin_pairing().unwrap();
    let client = reqwest::blocking::Client::builder()
        .no_proxy()
        .build()
        .unwrap();
    let response = client
        .get(format!("{}/cu/pairing-challenge", server.url))
        .header("origin", EXT_ORIGIN)
        .send()
        .unwrap();
    assert_eq!(response.status().as_u16(), 200);
    assert_eq!(response.headers()["cache-control"], "no-store");
    let got: Value = response.json().unwrap();
    assert_eq!(got["nonce"], ch.nonce);
    assert_eq!(got["instance"], ch.instance_id);
    assert_eq!(got["protocol"], 1);
    assert_eq!(got.as_object().unwrap().len(), 5);
    assert!(!got.to_string().contains(&ch.verification_code));
    let proof = proof_body(&ch);
    assert_eq!(pair_post(&server, &proof).status().as_u16(), 403);
    broker.confirm_pairing(&ch.nonce).unwrap();
    let mut wrong = proof.clone();
    wrong["response"] = json!(ch.nonce);
    assert_eq!(pair_post(&server, &wrong).status().as_u16(), 403);
    assert!(broker.tabs().pairing_session_key().is_none());
    let response = pair_post(&server, &proof);
    assert_eq!(response.status().as_u16(), 200);
    assert_eq!(response.headers()["cache-control"], "no-store");
    let body: Value = response.json().unwrap();
    assert_eq!(body["sessionKey"].as_str().unwrap().len(), 64);
    assert_eq!(body["instanceId"], ch.instance_id);
    assert_eq!(body["connectionNonce"], proof["connectionNonce"]);
    assert!(body["generation"].as_u64().unwrap() > 0);
    assert_eq!(pair_post(&server, &proof).status().as_u16(), 403);
    assert_eq!(
        client
            .post(format!("{}/cu/pairing-session", server.url))
            .json(&proof)
            .send()
            .unwrap()
            .status()
            .as_u16(),
        404
    );
}

#[test]
fn pairing_requires_exact_extension_origin_and_loopback_host() {
    let mut headers = HeaderMap::new();
    headers.insert("host", "127.0.0.1:9".parse().unwrap());
    let id = Some(crate::pairing::EXTENSION_ID);
    for bad in [
        "",
        "null",
        "http://127.0.0.1:9",
        "https://evil.example",
        "chrome-extension://another",
        "chrome-extension://bgegbabkegkanjbmjbeaockdijnbkjgi/extra",
    ] {
        headers.insert("origin", bad.parse().unwrap());
        assert!(!pairing_headers_ok_for(&headers, id), "{bad}");
    }
    headers.insert("origin", EXT_ORIGIN.parse().unwrap());
    assert!(pairing_headers_ok_for(&headers, id));
    assert!(!pairing_headers_ok(&headers));
    for key in ["forwarded", "x-forwarded-for", "x-forwarded-host"] {
        headers.insert(key, "127.0.0.1".parse().unwrap());
        assert!(!pairing_headers_ok_for(&headers, id));
        headers.remove(key);
    }
    headers.remove("origin");
    assert!(!pairing_headers_ok_for(&headers, id));
}

#[test]
fn pairing_http_preflight_and_get_reject_webpages_and_absent_origin() {
    let (broker, _, server, _) = fixture();
    broker.begin_pairing().unwrap();
    let client = reqwest::blocking::Client::builder()
        .no_proxy()
        .build()
        .unwrap();
    for origin in [
        None,
        Some("http://127.0.0.1:9"),
        Some("https://evil.example"),
        Some(EXT_ORIGIN),
    ] {
        for method in [reqwest::Method::GET, reqwest::Method::OPTIONS] {
            let expected = if origin == Some(EXT_ORIGIN) {
                if method == reqwest::Method::GET {
                    200
                } else {
                    204
                }
            } else {
                403
            };
            let mut request =
                client.request(method, format!("{}/cu/pairing-challenge", server.url));
            if let Some(origin) = origin {
                request = request.header("origin", origin);
            }
            let response = request.send().unwrap();
            assert_eq!(response.status().as_u16(), expected);
            assert_eq!(
                response
                    .headers()
                    .get("access-control-allow-origin")
                    .map(|v| v.to_str().unwrap()),
                if expected == 403 {
                    None
                } else {
                    Some(EXT_ORIGIN)
                }
            );
        }
    }
}

#[test]
fn pairing_proof_binds_every_identity_field() {
    let (broker, _, server, _) = fixture();
    let ch = broker.begin_pairing().unwrap();
    broker.confirm_pairing(&ch.nonce).unwrap();
    let good = proof_body(&ch);
    for (field, wrong) in [
        ("nonce", json!("other")),
        ("instance", json!("other")),
        ("ext", json!("other")),
        ("protocol", json!(2)),
        ("expiresAt", json!(ch.expires_at_ms + 1)),
        (
            "connectionNonce",
            json!("00000000-0000-4000-8000-000000000002"),
        ),
        ("response", json!("00".repeat(32))),
    ] {
        let mut body = good.clone();
        body[field] = wrong;
        assert_eq!(pair_post(&server, &body).status().as_u16(), 403, "{field}");
        assert!(broker.tabs().pairing_session_key().is_none());
    }
    assert_eq!(pair_post(&server, &good).status().as_u16(), 200);
}

#[test]
fn pairing_expiry_feature_off_and_stale_app_confirmation_fail_closed() {
    let (broker, _, server, _) = fixture();
    let old = broker.begin_pairing().unwrap();
    let current = broker.begin_pairing().unwrap();
    assert!(broker.confirm_pairing(&old.nonce).is_err());
    assert_eq!(
        pair_post(&server, &proof_body(&current)).status().as_u16(),
        403
    );
    broker.confirm_pairing(&current.nonce).unwrap();
    assert_eq!(pair_post(&server, &proof_body(&old)).status().as_u16(), 403);
    broker.tabs().expire_pending_pairing();
    assert_eq!(
        pair_post(&server, &proof_body(&current)).status().as_u16(),
        403
    );
    let current = broker.begin_pairing().unwrap();
    broker.confirm_pairing(&current.nonce).unwrap();
    broker.set_feature_enabled(false);
    assert!(broker.begin_pairing().is_err());
    assert_eq!(
        pair_post(&server, &proof_body(&current)).status().as_u16(),
        403
    );
    assert!(broker.tabs().pairing_session_key().is_none());
}

#[test]
fn concurrent_pairing_proof_has_exactly_one_winner() {
    let (broker, _, server, _) = fixture();
    let ch = broker.begin_pairing().unwrap();
    broker.confirm_pairing(&ch.nonce).unwrap();
    let body = proof_body(&ch);
    let barrier = Arc::new(std::sync::Barrier::new(2));
    let status = std::thread::scope(|scope| {
        let request = || {
            barrier.wait();
            pair_post(&server, &body).status().as_u16()
        };
        let a = scope.spawn(request);
        let b = scope.spawn(request);
        [a.join().unwrap(), b.join().unwrap()]
    });
    assert_eq!(status.iter().filter(|&&s| s == 200).count(), 1);
    assert_eq!(status.iter().filter(|&&s| s == 403).count(), 1);
}

#[test]
fn pairing_rejects_incomplete_oversized_and_rate_limited_requests() {
    let (broker, _, server, _) = fixture();
    let ch = broker.begin_pairing().unwrap();
    broker.confirm_pairing(&ch.nonce).unwrap();
    let good = proof_body(&ch);
    let mut missing = good.clone();
    missing.as_object_mut().unwrap().remove("response");
    assert_eq!(pair_post(&server, &missing).status().as_u16(), 422);
    let mut huge = good.clone();
    huge["response"] = json!("x".repeat(70_000));
    assert_eq!(pair_post(&server, &huge).status().as_u16(), 413);
    let mut wrong = good;
    wrong["response"] = json!("bad");
    for _ in 0..RATE_MAX {
        assert_eq!(pair_post(&server, &wrong).status().as_u16(), 403);
    }
    assert_eq!(pair_post(&server, &wrong).status().as_u16(), 429);
    assert!(broker.tabs().pairing_session_key().is_none());
}

#[test]
fn extension_connection_requires_bearer_and_full_binding_and_disconnect_invalidates_it() {
    let (broker, _, server, _) = fixture();
    let ch = broker.begin_pairing().unwrap();
    broker.confirm_pairing(&ch.nonce).unwrap();
    let session = broker
        .complete_pairing(&crate::pairing::ExtensionPairing::proof_for(
            &ch,
            "00000000-0000-4000-8000-000000000001",
        ))
        .unwrap();
    let body = json!({"instanceId": session.instance_id, "connectionNonce": session.connection_nonce,
        "generation": session.generation});
    let client = reqwest::blocking::Client::builder()
        .no_proxy()
        .build()
        .unwrap();
    let request = |path: &str, token: &str, value: &Value| {
        client
            .post(format!("{}{path}", server.url))
            .header("origin", EXT_ORIGIN)
            .bearer_auth(token)
            .json(value)
            .send()
            .unwrap()
            .status()
            .as_u16()
    };
    assert_eq!(request("/cu/extension-status", "wrong", &body), 403);
    let mut wrong = body.clone();
    wrong["instanceId"] = json!("old-instance");
    assert_eq!(
        request("/cu/extension-status", &session.session_key, &wrong),
        403
    );
    wrong = body.clone();
    wrong["generation"] = json!(session.generation + 1);
    assert_eq!(
        request("/cu/extension-status", &session.session_key, &wrong),
        403
    );
    wrong = body.clone();
    wrong["connectionNonce"] = json!("other-connection");
    assert_eq!(
        request("/cu/extension-disconnect", &session.session_key, &wrong),
        403
    );
    assert!(broker.tabs().pairing_session_key().is_some());
    assert_eq!(
        request("/cu/extension-status", &session.session_key, &body),
        200
    );
    assert_eq!(
        request("/cu/extension-disconnect", &session.session_key, &body),
        200
    );
    assert!(broker.tabs().pairing_session_key().is_none());
    assert_eq!(
        request("/cu/extension-status", &session.session_key, &body),
        403
    );
}

#[test]
fn old_connection_cannot_revoke_a_replacement_pairing() {
    let (broker, _, _, _) = fixture();
    let pair = || {
        let ch = broker.begin_pairing().unwrap();
        broker.confirm_pairing(&ch.nonce).unwrap();
        broker
            .complete_pairing(&crate::pairing::ExtensionPairing::proof_for(
                &ch,
                "00000000-0000-4000-8000-000000000001",
            ))
            .unwrap()
    };
    let old = pair();
    let fresh = pair();
    let binding = crate::pairing::PairingConnection {
        instance_id: old.instance_id,
        connection_nonce: old.connection_nonce,
        generation: old.generation,
    };
    assert!(broker
        .tabs()
        .check_pairing_connection(EXT_ORIGIN, &old.session_key, &binding, true)
        .is_err());
    assert!(broker.tabs().pairing_session_key().as_deref() == Some(fresh.session_key.as_str()));
}

#[test]
fn listener_maintenance_expires_pairing_without_extension_traffic() {
    let (broker, _, server, _) = fixture();
    let ch = broker.begin_pairing().unwrap();
    broker.confirm_pairing(&ch.nonce).unwrap();
    let session = broker
        .complete_pairing(&crate::pairing::ExtensionPairing::proof_for(
            &ch,
            "00000000-0000-4000-8000-000000000001",
        ))
        .unwrap();
    broker.tabs().expire_pairing_connection_for_test();
    // Inspect the actual invalidation result, not session_key()'s read-time check.
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if !broker.tabs().has_stored_pairing_connection_for_test() {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "listener must clear the abandoned session"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    let client = reqwest::blocking::Client::builder()
        .no_proxy()
        .build()
        .unwrap();
    let response = client
        .post(format!("{}/cu/extension-status", server.url))
        .header("origin", EXT_ORIGIN)
        .bearer_auth(&session.session_key)
        .json(
            &json!({"instanceId": session.instance_id, "connectionNonce": session.connection_nonce,
            "generation": session.generation}),
        )
        .send()
        .unwrap();
    assert_eq!(response.status().as_u16(), 403);
}

#[test]
fn dropping_listener_revokes_its_extension_connection() {
    let (broker, _, server, _) = fixture();
    let challenge = broker.begin_pairing().unwrap();
    broker.confirm_pairing(&challenge.nonce).unwrap();
    broker
        .complete_pairing(&crate::pairing::ExtensionPairing::proof_for(
            &challenge,
            "00000000-0000-4000-8000-000000000001",
        ))
        .unwrap();
    assert!(broker.tabs().pairing_session_key().is_some());
    drop(server);
    assert!(broker.tabs().pairing_session_key().is_none());
}
