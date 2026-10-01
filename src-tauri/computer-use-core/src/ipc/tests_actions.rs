use super::*;
use crate::browser::extension_action::*;
use crate::execution::ActionCancellation;
use crate::pairing::{PairingSession, EXTENSION_ID};

const PREFIX: &str = "/cu/extension-actions";

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

fn post(
    server: &IpcServer,
    session: &PairingSession,
    route: &str,
    body: &Value,
) -> reqwest::blocking::Response {
    http(server, route, body)
        .bearer_auth(&session.session_key)
        .send()
        .unwrap()
}

fn connection(session: &PairingSession) -> Value {
    json!({"instanceId":session.instance_id,"connectionNonce":session.connection_nonce,"generation":session.generation})
}

fn negotiation(session: &PairingSession) -> Value {
    json!({"connection":connection(session),"protocol":2,"completionProtocol":2,
        "actions":["click","set_value","type_text","scroll","wait"]})
}

struct Fixture {
    broker: Arc<ComputerUseBroker>,
    server: IpcServer,
    session: PairingSession,
    dispatch: Value,
    task: std::thread::JoinHandle<Result<ActionOutcome, crate::error::BrokerError>>,
}

fn fixture(parameters: Value, action: &str) -> Fixture {
    let (broker, server, session) = super::tests_sharing::fixture();
    assert_eq!(
        post(
            &server,
            &session,
            &format!("{PREFIX}/negotiate"),
            &negotiation(&session)
        )
        .status(),
        200
    );
    assert_eq!(
        post(
            &server,
            &session,
            "/cu/tab-offer",
            &super::tests_sharing::offer(&session)
        )
        .status(),
        200
    );
    let tab = broker
        .tabs()
        .grant_picker_tab("owner", "run", &broker.tabs().list_shared_candidates()[0].0)
        .unwrap();
    let observing = broker.clone();
    let observe = std::thread::spawn(move || {
        observing
            .tabs()
            .observe_existing_document("owner", "run", "101", false)
    });
    let polled: Value = post(
        &server,
        &session,
        "/cu/extension-poll",
        &connection(&session),
    )
    .json()
    .unwrap();
    let request = &polled["request"];
    let snapshot = request["command"]["snapshotId"].as_str().unwrap();
    let element = format!("{snapshot}-1");
    let result = json!({"request":request,"outcome":{"kind":"observation","observation":{
        "snapshotId":snapshot,"documentId":"0123456789abcdef0123456789abcdef",
        "title":"fixture","url":"https://fixture.invalid/","text":"visible fixture",
        "nodes":[{"elementRef":element,"role":"textbox","name":"Field","disabled":false}],
        "viewportWidth":800,"viewportHeight":600,"truncated":false,"screenshot":null}}});
    assert_eq!(
        post(&server, &session, "/cu/extension-result", &result).status(),
        200
    );
    observe.join().unwrap().unwrap();
    let command = serde_json::from_value(
        json!({"kind":"act","snapshotId":snapshot,"elementRef":element,
        "action":action,"parameters":parameters}),
    )
    .unwrap();
    let acting = broker.clone();
    let task = std::thread::spawn(move || {
        acting.tabs().execute_existing_action(ExistingActionInput {
            session: "owner",
            run: "run",
            tab: "101",
            grant_generation: tab.generation,
            document_generation: tab.document_generation,
            command,
            cancellation: ActionCancellation::default(),
        })
    });
    let deadline = Instant::now() + Duration::from_secs(3);
    let dispatch = loop {
        let value: Value = post(
            &server,
            &session,
            &format!("{PREFIX}/poll"),
            &connection(&session),
        )
        .json()
        .unwrap();
        if value["dispatch"].is_object() {
            break value["dispatch"].clone();
        }
        assert!(
            Instant::now() < deadline,
            "queued action should become available"
        );
        std::thread::yield_now();
    };
    Fixture {
        broker,
        server,
        session,
        dispatch,
        task,
    }
}

fn outcome(dispatch: &Value, status: &str) -> Value {
    let mut body = dispatch.clone();
    body["outcome"] = json!({"status":status,"detail":"fixture_result"});
    body
}

#[test]
fn action_http_full_command_claim_settle_and_once_only_result() {
    let f = fixture(json!({"text":"你好🙂"}), "set_value");
    assert_eq!(
        post(
            &f.server,
            &f.session,
            "/cu/extension-completion/claim",
            &f.dispatch["proof"]
        )
        .status(),
        403
    );
    let mut changed = f.dispatch.clone();
    changed["request"]["command"]["parameters"]["text"] = json!("different");
    assert_eq!(
        post(&f.server, &f.session, &format!("{PREFIX}/claim"), &changed).status(),
        403
    );
    assert_eq!(
        http(&f.server, &format!("{PREFIX}/claim"), &f.dispatch)
            .send()
            .unwrap()
            .status(),
        403
    );
    assert_eq!(
        post(
            &f.server,
            &f.session,
            &format!("{PREFIX}/claim"),
            &f.dispatch
        )
        .status(),
        200
    );
    assert_eq!(
        post(
            &f.server,
            &f.session,
            &format!("{PREFIX}/claim"),
            &f.dispatch
        )
        .status(),
        403
    );
    let result = outcome(&f.dispatch, "verified");
    assert_eq!(
        post(&f.server, &f.session, &format!("{PREFIX}/result"), &result).status(),
        403
    );
    assert_eq!(
        http(
            &f.server,
            "/cu/extension-completion/settle",
            &f.dispatch["proof"]
        )
        .send()
        .unwrap()
        .status(),
        200
    );
    assert!(!f.broker.tabs().existing_operations_idle("run"));
    assert_eq!(
        post(&f.server, &f.session, &format!("{PREFIX}/result"), &result).status(),
        200
    );
    assert_eq!(
        post(&f.server, &f.session, &format!("{PREFIX}/result"), &result).status(),
        403
    );
    assert_eq!(
        f.task.join().unwrap().unwrap().status,
        ActionStatus::Verified
    );
    assert!(f.broker.tabs().existing_operations_idle("run"));
}

#[test]
fn action_http_lost_result_and_feature_off_keep_only_cleanup_authority() {
    let f = fixture(json!({}), "click");
    assert_eq!(
        post(
            &f.server,
            &f.session,
            &format!("{PREFIX}/claim"),
            &f.dispatch
        )
        .status(),
        200
    );
    f.broker.set_feature_enabled(false);
    assert!(f.task.join().unwrap().is_err());
    assert!(!f.broker.tabs().existing_operations_idle("run"));
    for (route, body) in [
        ("claim", f.dispatch.clone()),
        ("result", outcome(&f.dispatch, "applied")),
        ("poll", connection(&f.session)),
        ("negotiate", negotiation(&f.session)),
    ] {
        assert_eq!(
            post(&f.server, &f.session, &format!("{PREFIX}/{route}"), &body).status(),
            403
        );
    }
    assert_eq!(
        http(
            &f.server,
            "/cu/extension-completion/settle",
            &f.dispatch["proof"]
        )
        .send()
        .unwrap()
        .status(),
        200
    );
    assert!(f.broker.tabs().existing_operations_idle("run"));
    assert!(!f.broker.tabs().is_closed("101"));
}

#[test]
fn action_http_negotiation_rejects_old_versions_unknown_fields_and_wrong_origin() {
    let (_broker, server, session) = super::tests_sharing::fixture();
    assert_eq!(
        post(
            &server,
            &session,
            &format!("{PREFIX}/poll"),
            &connection(&session)
        )
        .status(),
        403
    );
    for field in ["protocol", "completionProtocol"] {
        let mut n = negotiation(&session);
        n[field] = json!(1);
        assert_eq!(
            post(&server, &session, &format!("{PREFIX}/negotiate"), &n).status(),
            403
        );
    }
    let mut n = negotiation(&session);
    n["actions"] = json!(["click", "click"]);
    assert_eq!(
        post(&server, &session, &format!("{PREFIX}/negotiate"), &n).status(),
        403
    );
    n["actions"] = json!(["eval"]);
    let response = post(&server, &session, &format!("{PREFIX}/negotiate"), &n);
    assert_eq!(response.status(), 422);
    assert!(!response.text().unwrap().contains("eval"));
    n = negotiation(&session);
    n["secret-shaped-extra"] = json!("must-never-echo");
    let response = post(&server, &session, &format!("{PREFIX}/negotiate"), &n);
    assert_eq!(response.status(), 422);
    assert!(!response.text().unwrap().contains("must-never-echo"));
    let response = http(
        &server,
        &format!("{PREFIX}/negotiate"),
        &negotiation(&session),
    )
    .header("origin", "https://fixture.invalid")
    .bearer_auth(&session.session_key)
    .send()
    .unwrap();
    assert_eq!(response.status(), 403);
}

#[test]
fn action_http_maximum_unicode_packet_survives_poll_and_body_limit_stays_bounded() {
    let text = "🙂".repeat(4000);
    let f = fixture(json!({"text":text}), "type_text");
    assert!(f.dispatch["request"]["command"]["parameters"]["text"]
        .as_str()
        .is_some_and(|value| value == text));
    assert!(serde_json::to_vec(&f.dispatch).unwrap().len() > 8192);
    assert!(serde_json::to_vec(&f.dispatch).unwrap().len() < ACTION_PACKET_BYTES);
    let response = http(&f.server, &format!("{PREFIX}/claim"), &f.dispatch)
        .bearer_auth(&f.session.session_key)
        .body("x".repeat(ACTION_PACKET_BYTES + 1))
        .send()
        .unwrap();
    assert_eq!(response.status(), 413);
    assert_eq!(
        post(
            &f.server,
            &f.session,
            &format!("{PREFIX}/claim"),
            &f.dispatch
        )
        .status(),
        200
    );
    assert_eq!(
        http(
            &f.server,
            "/cu/extension-completion/settle",
            &f.dispatch["proof"]
        )
        .send()
        .unwrap()
        .status(),
        200
    );
    assert_eq!(
        post(
            &f.server,
            &f.session,
            &format!("{PREFIX}/result"),
            &outcome(&f.dispatch, "verified")
        )
        .status(),
        200
    );
    assert_eq!(
        f.task.join().unwrap().unwrap().status,
        ActionStatus::Verified
    );
}
