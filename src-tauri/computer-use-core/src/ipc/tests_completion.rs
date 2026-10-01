use super::*;
use crate::browser::extension_completion::{CompletionProof, COMPLETION_TOMBSTONE_LIMIT};
use crate::execution::ActionCancellation;
use crate::pairing::{PairingSession, EXTENSION_ID};

fn http(server: &IpcServer, route: &str, body: &Value) -> reqwest::blocking::RequestBuilder {
    reqwest::blocking::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap()
        .post(format!("{}{route}", server.url))
        .header("origin", format!("chrome-extension://{EXTENSION_ID}"))
        .json(body)
}

fn fixture() -> (
    Arc<ComputerUseBroker>,
    IpcServer,
    PairingSession,
    CompletionProof,
) {
    let (broker, server, session) = super::tests_sharing::fixture();
    assert_eq!(
        super::tests_sharing::post(
            &server,
            &session,
            "/cu/tab-offer",
            &super::tests_sharing::offer(&session)
        ),
        200
    );
    broker
        .tabs()
        .grant_picker_tab("owner", "run", &broker.tabs().list_shared_candidates()[0].0)
        .unwrap();
    let observing = broker.clone();
    let task = std::thread::spawn(move || {
        observing
            .tabs()
            .observe_existing_document("owner", "run", "101", false)
    });
    let connection = json!({"instanceId":session.instance_id,"connectionNonce":session.connection_nonce,"generation":session.generation});
    let polled: Value = http(&server, "/cu/extension-poll", &connection)
        .bearer_auth(&session.session_key)
        .send()
        .unwrap()
        .json()
        .unwrap();
    let request = &polled["request"];
    assert!(request.is_object());
    let result = json!({"request":request,"outcome":{"kind":"observation","observation":{
        "snapshotId":request["command"]["snapshotId"],"documentId":"0123456789abcdef0123456789abcdef",
        "title":"fixture","url":"https://fixture.invalid/","text":"visible fixture","nodes":[],
        "viewportWidth":800,"viewportHeight":600,"truncated":false,"screenshot":null}}});
    assert_eq!(
        super::tests_sharing::post(&server, &session, "/cu/extension-result", &result),
        200
    );
    let observed = task.join().unwrap().unwrap();
    let proof = broker
        .tabs()
        .offer_existing_completion(
            "owner",
            "run",
            "101",
            &observed.snapshot_id,
            ActionCancellation::default(),
        )
        .unwrap();
    (broker, server, session, proof)
}

const CLAIM: &str = "/cu/extension-completion/claim";
const STATUS: &str = "/cu/extension-completion/status";
const SETTLE: &str = "/cu/extension-completion/settle";
const RETIREMENT: &str = "/cu/extension-completion/retirement";

#[test]
fn host_lifetime_requires_original_proof_and_kernel_query_never_releases_pending() {
    let (broker, server, session, proof) = fixture();
    let original = serde_json::to_value(&proof).unwrap();
    let route = "/cu/extension-completion/host-lifetime";
    let captured: Value = http(&server, route, &original)
        .send()
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(captured["ok"], true);
    let lifetime: crate::process_lifetime::HostLifetime =
        serde_json::from_value(captured["lifetime"].clone()).unwrap();
    assert_eq!(lifetime.pid, std::process::id());
    assert_eq!(lifetime.instance_id, session.instance_id);
    let mut wrong = original.clone();
    wrong["completionKey"] = json!("f".repeat(64));
    assert_eq!(http(&server, route, &wrong).send().unwrap().status(), 403);
    assert_eq!(
        http(&server, CLAIM, &original)
            .bearer_auth(&session.session_key)
            .send()
            .unwrap()
            .status(),
        200
    );
    let body = json!({"connection":proof.binding.connection,"lifetime":lifetime});
    let route = "/cu/extension-completion/host-retirement";
    assert_eq!(http(&server, route, &body).send().unwrap().status(), 403);
    let live: Value = http(&server, route, &body)
        .bearer_auth(&session.session_key)
        .send()
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(live, json!({"ok":true,"state":"live","lifetime":lifetime}));
    assert!(!broker.tabs().existing_operations_idle("run"));
    let mut reused = body.clone();
    reused["lifetime"]["birth"] = json!(format!("{}0", lifetime.birth));
    let retired: Value = http(&server, route, &reused)
        .bearer_auth(&session.session_key)
        .send()
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(retired["state"], "retired");
    assert!(
        !broker.tabs().existing_operations_idle("run"),
        "kernel evidence cannot settle any registry record"
    );
    let mut invalid = body.clone();
    invalid["lifetime"]["pid"] = json!(0);
    assert_eq!(
        http(&server, route, &invalid)
            .bearer_auth(&session.session_key)
            .send()
            .unwrap()
            .status(),
        403
    );
    invalid = body.clone();
    invalid["lifetime"]["extra"] = json!("credential-shaped-sentinel");
    let rejected = http(&server, route, &invalid)
        .bearer_auth(&session.session_key)
        .send()
        .unwrap();
    assert_eq!(rejected.status(), 400);
    assert!(!rejected
        .text()
        .unwrap()
        .contains("credential-shaped-sentinel"));
    broker.set_feature_enabled(false);
    assert_eq!(
        http(&server, route, &body)
            .bearer_auth(&session.session_key)
            .send()
            .unwrap()
            .status(),
        403
    );
    assert!(!broker.tabs().existing_operations_idle("run"));
    assert_eq!(
        http(&server, SETTLE, &original).send().unwrap().status(),
        200
    );
}

#[test]
fn completion_http_cleanup_survives_feature_off_without_resurrecting_pairing() {
    let (broker, server, session, proof) = fixture();
    let body = serde_json::to_value(&proof).unwrap();
    assert_eq!(http(&server, CLAIM, &body).send().unwrap().status(), 403);
    assert_eq!(
        http(&server, CLAIM, &body)
            .bearer_auth(&session.session_key)
            .send()
            .unwrap()
            .status(),
        200
    );
    assert_eq!(
        http(&server, CLAIM, &body)
            .bearer_auth(&session.session_key)
            .send()
            .unwrap()
            .status(),
        403
    );
    broker.set_feature_enabled(false);
    assert!(!broker.tabs().existing_operations_idle("run"));
    assert!(broker.tabs().pairing_session_key().is_none());
    let status: Value = http(&server, STATUS, &body).send().unwrap().json().unwrap();
    assert_eq!(
        status,
        json!({"ok":true,"status":{"phase":"claimed","cancelRequested":true}})
    );
    assert_eq!(
        http(&server, CLAIM, &body)
            .bearer_auth(&session.session_key)
            .send()
            .unwrap()
            .status(),
        403
    );
    for _ in 0..2 {
        let response = http(&server, SETTLE, &body).send().unwrap();
        assert_eq!(response.status(), 200);
        assert_eq!(response.json::<Value>().unwrap(), json!({"ok":true}));
    }
    assert!(broker.tabs().existing_operations_idle("run"));
    assert!(broker.tabs().pairing_session_key().is_none());
    assert!(!broker.tabs().is_closed("101"));
    assert!(broker.tabs().list_shared_candidates().is_empty());
}

#[test]
fn completion_http_rejects_wrong_origin_forwarding_host_and_forged_proof() {
    let (broker, server, session, proof) = fixture();
    let body = serde_json::to_value(&proof).unwrap();
    for route in [CLAIM, STATUS, SETTLE, RETIREMENT] {
        for (name, value) in [
            ("origin", "https://fixture.invalid"),
            (
                "origin",
                "chrome-extension://aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            ),
            ("host", "external.invalid:1234"),
            ("forwarded", "for=127.0.0.1"),
            ("x-forwarded-for", "127.0.0.1"),
        ] {
            assert_eq!(
                http(&server, route, &body)
                    .bearer_auth(&session.session_key)
                    .header(name, value)
                    .send()
                    .unwrap()
                    .status(),
                403,
                "{route} {name}"
            );
        }
        let mut forged = body.clone();
        forged["completionKey"] = json!("f".repeat(64));
        assert_eq!(
            http(&server, route, &forged)
                .bearer_auth(&session.session_key)
                .send()
                .unwrap()
                .status(),
            403
        );
        assert!(!broker.tabs().existing_operations_idle("run"));
    }
    // The trusted Origin alone never supplies action authority.
    assert_eq!(
        http(&server, SETTLE, &json!({})).send().unwrap().status(),
        422
    );
    assert_eq!(http(&server, SETTLE, &body).send().unwrap().status(), 200);
}

#[test]
fn completion_http_bounds_and_parse_errors_do_not_echo_credentials() {
    let (broker, server, session, proof) = fixture();
    let body = serde_json::to_value(&proof).unwrap();
    for route in [CLAIM, STATUS, SETTLE, RETIREMENT] {
        for (value, expected) in [
            (json!({"credential":session.session_key}), 422),
            (
                {
                    let mut v = body.clone();
                    v["binding"]["protocol"] = json!(session.session_key);
                    v
                },
                422,
            ),
            (
                {
                    let mut v = body.clone();
                    v["binding"]["extra"] = json!(true);
                    v
                },
                422,
            ),
            (
                {
                    let mut v = body.clone();
                    v["completionKey"] = json!("x".repeat(5000));
                    v
                },
                413,
            ),
        ] {
            let response = http(&server, route, &value).send().unwrap();
            assert_eq!(response.status().as_u16(), expected);
            let text = response.text().unwrap();
            assert!(!text.contains(&session.session_key));
            assert!(!text.contains(body["completionKey"].as_str().unwrap()));
            assert_eq!(
                serde_json::from_str::<Value>(&text).unwrap(),
                json!({"error":"invalid completion request"})
            );
        }
    }
    assert!(!broker.tabs().existing_operations_idle("run"));
}

#[test]
fn completion_http_poll_rate_bucket_cannot_starve_settlement() {
    let (_, server, session, proof) = fixture();
    let body = serde_json::to_value(&proof).unwrap();
    assert_eq!(
        http(&server, CLAIM, &body)
            .bearer_auth(&session.session_key)
            .send()
            .unwrap()
            .status(),
        200
    );
    server.service.rate.lock().insert(
        "extension-transport".into(),
        (0..40).map(|_| Instant::now()).collect(),
    );
    assert_eq!(http(&server, "/cu/extension-poll", &json!({"instanceId":session.instance_id,"connectionNonce":session.connection_nonce,"generation":session.generation})).bearer_auth(&session.session_key).send().unwrap().status(), 429);
    assert_eq!(http(&server, SETTLE, &body).send().unwrap().status(), 200);
}

#[test]
fn retirement_http_reports_pending_settled_and_authenticated_absence_without_mutation() {
    let (broker, server, session, old) = fixture();
    let old_body = serde_json::to_value(&old).unwrap();
    let pending: Value = http(&server, RETIREMENT, &old_body)
        .send()
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(pending, json!({"ok":true,"state":"pending"}));
    assert_eq!(
        http(&server, CLAIM, &old_body)
            .bearer_auth(&session.session_key)
            .send()
            .unwrap()
            .status(),
        200
    );
    assert!(!broker.tabs().existing_operations_idle("run"));
    assert_eq!(
        http(&server, SETTLE, &old_body).send().unwrap().status(),
        200
    );
    let settled: Value = http(&server, RETIREMENT, &old_body)
        .send()
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(settled, json!({"ok":true,"state":"settled"}));

    for _ in 0..COMPLETION_TOMBSTONE_LIMIT {
        let proof = broker
            .tabs()
            .offer_existing_completion(
                "owner",
                "run",
                "101",
                &old.binding.snapshot_id,
                ActionCancellation::default(),
            )
            .unwrap();
        broker
            .tabs()
            .settle_existing_completion(&format!("chrome-extension://{EXTENSION_ID}"), &proof)
            .unwrap();
    }
    assert_eq!(
        http(&server, SETTLE, &old_body).send().unwrap().status(),
        403
    );
    let absent: Value = http(&server, RETIREMENT, &old_body)
        .send()
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(absent, json!({"ok":true,"state":"absent"}));

    let current = broker
        .tabs()
        .offer_existing_completion(
            "owner",
            "run",
            "101",
            &old.binding.snapshot_id,
            ActionCancellation::default(),
        )
        .unwrap();
    assert_eq!(
        http(&server, RETIREMENT, &old_body)
            .send()
            .unwrap()
            .json::<Value>()
            .unwrap(),
        json!({"ok":true,"state":"absent"})
    );
    assert!(!broker.tabs().existing_operations_idle("run"));
    assert_eq!(
        http(
            &server,
            RETIREMENT,
            &serde_json::to_value(&current).unwrap()
        )
        .send()
        .unwrap()
        .json::<Value>()
        .unwrap(),
        json!({"ok":true,"state":"pending"})
    );
}

#[test]
fn browser_restart_deserializes_versioned_cleanup_and_releases_owner() {
    let (broker, server, session) = super::tests_sharing::fixture();
    let previous = "00000000-0000-4000-8000-000000000031";
    let current = "00000000-0000-4000-8000-000000000032";
    let document_id = "0123456789abcdef0123456789abcdef";
    let connection = json!({
        "instanceId": session.instance_id,
        "connectionNonce": session.connection_nonce,
        "generation": session.generation
    });
    let restart = |body: Value| {
        reqwest::blocking::Client::builder()
            .no_proxy()
            .build()
            .unwrap()
            .post(format!("{}/cu/browser-restart", server.url))
            .header("origin", format!("chrome-extension://{EXTENSION_ID}"))
            .bearer_auth(&session.session_key)
            .json(&body)
            .send()
            .unwrap()
    };
    let started = restart(json!({
        "connection": connection,
        "currentId": previous,
        "previousId": null,
        "cleanups": []
    }));
    let started_status = started.status();
    let started_body = started.text().unwrap_or_default();
    assert_eq!(started_status, 200, "{started_body}");
    assert_eq!(
        super::tests_sharing::post(
            &server,
            &session,
            "/cu/tab-offer",
            &super::tests_sharing::offer(&session)
        ),
        200
    );
    broker
        .tabs()
        .grant_picker_tab("owner", "run", &broker.tabs().list_shared_candidates()[0].0)
        .unwrap();
    let observing = broker.clone();
    let task = std::thread::spawn(move || {
        observing
            .tabs()
            .observe_existing_document("owner", "run", "101", false)
    });
    let polled: Value = http(&server, "/cu/extension-poll", &connection)
        .bearer_auth(&session.session_key)
        .send()
        .unwrap()
        .json()
        .unwrap();
    let request = &polled["request"];
    let result = json!({"request": request, "outcome": {"kind": "observation", "observation": {
        "snapshotId": request["command"]["snapshotId"], "documentId": document_id,
        "title": "fixture", "url": "https://fixture.invalid/", "text": "visible fixture",
        "nodes": [], "viewportWidth": 800, "viewportHeight": 600, "truncated": false,
        "screenshot": null
    }}});
    assert_eq!(
        super::tests_sharing::post(&server, &session, "/cu/extension-result", &result),
        200
    );
    let observed = task.join().unwrap().unwrap();
    let proof = broker
        .tabs()
        .offer_existing_completion(
            "owner",
            "run",
            "101",
            &observed.snapshot_id,
            ActionCancellation::default(),
        )
        .unwrap();
    assert!(!broker.tabs().existing_operations_idle("run"));
    // Same object durableCleanupRecord emits, plus a secret field the host must reject.
    let mut poisoned = json!({
        "version": 1,
        "browserId": previous,
        "requestId": proof.binding.request_id,
        "documentId": document_id,
        "phase": "prepared"
    });
    poisoned["completionKey"] = json!("a".repeat(64));
    let rejected = restart(json!({
        "connection": connection,
        "currentId": current,
        "previousId": previous,
        "cleanups": [poisoned]
    }));
    assert_ne!(rejected.status(), 200);
    assert!(!broker.tabs().existing_operations_idle("run"));
    let released = restart(json!({
        "connection": connection,
        "currentId": current,
        "previousId": previous,
        "cleanups": [{
            "version": 1,
            "browserId": previous,
            "requestId": proof.binding.request_id,
            "documentId": document_id,
            "phase": "prepared"
        }]
    }));
    assert_eq!(released.status(), 200);
    let body: Value = released.json().unwrap();
    assert_eq!(body["abandoned"], 1);
    assert_eq!(body["results"][0]["requestId"], proof.binding.request_id);
    assert_eq!(body["results"][0]["result"], "unknown");
    assert!(broker.tabs().existing_operations_idle("run"));
}
