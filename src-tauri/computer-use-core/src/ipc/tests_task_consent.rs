//! U01 request path. The original red run is
//! tools/computer-use-probe/.run/task-consent-u01-20261003/red-request-pending.txt.
//! That dispatch returned run_not_found. This file now drives the IPC route.

use super::*;
use crate::broker::BrokerOptions;
use crate::fake::FakeAdapter;
use crate::task_consent::SYNTHETIC_DECISION_CLASS;

fn broker_off(adapter: Arc<FakeAdapter>) -> Arc<ComputerUseBroker> {
    Arc::new(ComputerUseBroker::new(
        adapter,
        BrokerOptions {
            feature_enabled: false,
            lease_path: std::env::temp_dir().join(format!("cu-u01-{}.lock", uuid::Uuid::new_v4())),
            ..BrokerOptions::default()
        },
    ))
}

fn request_args(purpose: &str, scope: &str, caps: &[&str]) -> Value {
    json!({
        "action": "request",
        "purpose": purpose,
        "surfaces": ["desktop"],
        "capabilities": caps,
        "scopeIntent": scope
    })
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

fn text_of(body: &Value) -> Value {
    serde_json::from_str(body["content"][0]["text"].as_str().unwrap_or("")).unwrap_or(Value::Null)
}

#[test]
fn feature_off_request_without_run_or_target_returns_pending() {
    let adapter = Arc::new(FakeAdapter::new());
    let broker = broker_off(adapter.clone());
    let server = spawn(broker.clone()).unwrap();
    assert!(server
        .credential_for_session("app-local", "missing-run")
        .is_err());
    let token = server.issue_transport_credential("app-local").unwrap();
    assert_eq!(
        server.issue_transport_credential("app-local").unwrap(),
        token
    );
    let (unbound_status, unbound) = call(
        &server,
        &token,
        json!({"name": "computer_request_access", "arguments": request_args("read the fixture window", "GrokCuFixture", &["observe"])}),
    );
    assert_eq!(unbound_status, 200, "{unbound}");
    assert_eq!(unbound["isError"], true, "{unbound}");
    assert_eq!(broker.executable_grant_count("app-local"), 0);
    broker
        .bind_agent_session("app-local", "synthetic-agent")
        .unwrap();
    let (status, body) = call(
        &server,
        &token,
        json!({"name": "computer_request_access", "arguments": request_args("read the fixture window", "GrokCuFixture", &["observe"])}),
    );
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["isError"], false, "{body}");
    let pending = text_of(&body);
    assert_eq!(pending["state"], "pending");
    assert_eq!(pending["authority"], "none");
    assert!(!body.to_string().contains(&token));
    assert_eq!(adapter.list_calls(), 0);
    assert_eq!(adapter.observe_calls(), 0);
    assert!(!broker.feature_enabled());
    assert_eq!(broker.test_run_count(), 0);

    // Consent-only status uses the original authenticated HTTP route, even
    // with the feature off. This must not discover targets or create a run.
    let census = broker.authority_census();
    let (read_status, read_body) = call(
        &server,
        &token,
        json!({"name":"computer_status", "arguments":{}}),
    );
    assert_eq!(read_status, 200);
    assert_eq!(read_body["isError"], false, "{read_body}");
    let read = text_of(&read_body);
    assert_eq!(read["state"], "pending");
    assert_eq!(read["authority"], "none");
    assert_eq!(read["requestId"], pending["requestId"]);
    assert_eq!(read["nextAction"], "wait-for-host");
    assert!(read.get("traces").is_none());
    assert!(!read_body.to_string().contains(&token));
    assert_eq!(broker.authority_census(), census);
    assert_eq!(adapter.list_calls(), 0);
    assert_eq!(adapter.observe_calls(), 0);
    assert_eq!(broker.test_run_count(), 0);

    let request_id = pending["requestId"].as_str().unwrap().to_string();
    let revision = pending["revision"].as_u64().unwrap();
    broker
        .bind_synthetic_scope(&request_id, &adapter.fixture_id())
        .unwrap();
    let outcome = broker.commit_access(&token, &request_id, revision).unwrap();
    assert_eq!(outcome.decision_class, SYNTHETIC_DECISION_CLASS);
    assert!(broker.feature_enabled());
    assert_eq!(adapter.observe_calls(), 0);
    let (_, status_body) = call(
        &server,
        &token,
        json!({"name": "computer_request_access", "arguments": {"action": "status"}}),
    );
    assert_eq!(status_body["isError"], false, "{status_body}");
    assert_eq!(text_of(&status_body)["state"], "approved");
    assert_eq!(broker.continuation_count("app-local"), 1);
    assert!(!status_body.to_string().contains(&token));
    let (_, opened) = call(
        &server,
        &token,
        json!({"name": "computer_open_target", "arguments": {"targetId": adapter.fixture_id()}}),
    );
    assert_eq!(opened["isError"], false, "{opened}");
    let (_, observed) = call(
        &server,
        &token,
        json!({"name": "computer_observe", "arguments": {"screenshot": false}}),
    );
    assert_eq!(observed["isError"], false, "{observed}");
    assert_eq!(adapter.observe_calls(), 1);
    assert!(!observed.to_string().contains(&token));
    adapter.set_include_decoy(true);
    let (_, listed) = call(&server, &token, json!({"name": "computer_list_targets"}));
    assert_eq!(listed["isError"], false, "{listed}");
    let listed_text = listed.to_string();
    assert!(listed_text.contains(&adapter.fixture_id()), "{listed}");
    assert!(!listed_text.contains("OtherApp"), "{listed}");
    assert!(!listed_text.contains("Grok Dev"), "{listed}");
    for invalid_args in [Value::Null, json!([]), json!("all")] {
        let (_, invalid) = call(
            &server,
            &token,
            json!({"name": "computer_list_targets", "arguments": invalid_args}),
        );
        assert_eq!(invalid["isError"], true, "{invalid}");
    }
    let (_, acted) = call(
        &server,
        &token,
        json!({"name": "computer_act", "arguments": {}}),
    );
    assert_eq!(acted["isError"], true, "{acted}");
    assert!(adapter.executions().is_empty());
}

#[test]
fn legacy_credential_is_not_replaced_by_transport() {
    let adapter = Arc::new(FakeAdapter::new());
    let broker = Arc::new(ComputerUseBroker::new(
        adapter.clone(),
        BrokerOptions {
            feature_enabled: true,
            lease_path: std::env::temp_dir()
                .join(format!("cu-u01-legacy-{}.lock", uuid::Uuid::new_v4())),
            ..BrokerOptions::default()
        },
    ));
    broker.open_run("session-a", "run-a").unwrap();
    broker
        .authorize_target("run-a", &adapter.fixture_id())
        .unwrap();
    let server = spawn(broker.clone()).unwrap();
    let legacy = server.credential_for_session("session-a", "run-a").unwrap();
    let before = adapter.observe_calls();
    let transport = server.issue_transport_credential("session-a").unwrap();
    assert_ne!(transport, legacy);
    assert_eq!(
        server.credential_for_session("session-a", "run-a").unwrap(),
        legacy
    );
    let (_, denied) = call(
        &server,
        &transport,
        json!({"name": "computer_observe", "arguments": {"screenshot": false}}),
    );
    assert_eq!(denied["isError"], true, "{denied}");
    assert_eq!(adapter.observe_calls(), before);
    assert!(!denied.to_string().contains(&transport));
    assert_eq!(
        call(&server, &legacy, json!({"name": "computer_status"})).0,
        200
    );
    broker.rotate_connection("session-a").unwrap();
    assert_eq!(
        call(&server, &transport, json!({"name": "computer_status"})).0,
        401
    );
    assert_eq!(
        call(&server, &legacy, json!({"name": "computer_status"})).0,
        200
    );
}

#[test]
fn request_flood_does_not_consume_stop_budget() {
    let adapter = Arc::new(FakeAdapter::new());
    let broker = broker_off(adapter);
    let server = spawn(broker).unwrap();
    let token = server.issue_transport_credential("app-local").unwrap();
    server.service.rate.lock().insert(
        format!("request:{token}"),
        vec![Instant::now(); REQUEST_RATE_MAX],
    );
    let (status, body) = call(
        &server,
        &token,
        json!({"name": "computer_request_access", "arguments": request_args("read the fixture window", "GrokCuFixture", &["observe"])}),
    );
    assert_eq!(status, 429, "{body}");
    let (stop_status, stop_body) = call(&server, &token, json!({"name": "computer_stop"}));
    assert_ne!(stop_status, 429, "{stop_body}");
    assert_eq!(stop_body["isError"], true, "{stop_body}");
}

#[test]
fn explicit_completion_uses_its_own_admission_and_fences_only_the_original_task() {
    let adapter = Arc::new(FakeAdapter::new());
    let broker = broker_off(adapter.clone());
    let server = spawn(broker.clone()).unwrap();
    let token = server.issue_transport_credential("finish-owner").unwrap();
    broker
        .bind_agent_session("finish-owner", "synthetic-agent")
        .unwrap();
    let (_, requested) = call(
        &server,
        &token,
        json!({
            "name": "computer_request_access",
            "arguments": request_args("verify fixture", "fixture", &["observe"])
        }),
    );
    let pending = text_of(&requested);
    broker
        .bind_synthetic_scope(
            pending["requestId"].as_str().unwrap(),
            &adapter.fixture_id(),
        )
        .unwrap();
    let approved = broker
        .commit_access(&token, pending["requestId"].as_str().unwrap(), 1)
        .unwrap();
    let other = server.issue_transport_credential("other-owner").unwrap();
    broker
        .bind_agent_session("other-owner", "synthetic-other")
        .unwrap();
    let (_, cross_session) = call(
        &server,
        &other,
        json!({
            "name": "computer_complete_task", "arguments": {"taskId": pending["taskId"]}
        }),
    );
    assert_eq!(cross_session["isError"], true);
    assert_eq!(broker.executable_grant_count("finish-owner"), 1);
    server
        .service
        .rate
        .lock()
        .insert(token.clone(), vec![Instant::now(); RATE_MAX]);
    server.service.rate.lock().insert(
        format!("request:{token}"),
        vec![Instant::now(); REQUEST_RATE_MAX],
    );
    let normal_slots = Arc::clone(&server.service.slots)
        .try_acquire_many_owned(8)
        .unwrap();
    let completion_slots = Arc::clone(&server.service.completion_slots)
        .try_acquire_many_owned(CONTROL_SLOTS as u32)
        .unwrap();
    let args =
        json!({"name": "computer_complete_task", "arguments": {"taskId": pending["taskId"]}});
    assert_eq!(call(&server, &token, args.clone()).0, 429);
    assert_eq!(broker.executable_grant_count("finish-owner"), 1);
    drop(completion_slots);
    let (status, completed) = call(&server, &token, args);
    assert_eq!(status, 200, "{completed}");
    assert_eq!(completed["isError"], false, "{completed}");
    assert_eq!(text_of(&completed)["state"], "completion_requested");
    assert!(!completed.to_string().contains(&token));
    assert_eq!(broker.executable_grant_count("finish-owner"), 0);
    assert_eq!(adapter.observe_calls(), 0);
    assert_ne!(
        broker.stop_state(&approved.run_id).unwrap(),
        crate::broker::StopState::Running
    );
    let notices = broker.take_no_authority("finish-owner", approved.binding.owner_nonce);
    assert!(!notices.is_empty());
    assert!(notices
        .iter()
        .all(|notice| broker.validate_no_authority(notice).is_err()));
    for ticket in broker.take_cleanup_tickets() {
        broker.finish_stop_cleanup(&ticket).unwrap();
    }
    assert_eq!(
        broker.stop_state(&approved.run_id).unwrap(),
        crate::broker::StopState::Stopped
    );
    drop(normal_slots);
}
