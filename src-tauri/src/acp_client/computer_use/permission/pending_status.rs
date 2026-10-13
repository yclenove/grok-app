//! Recorded ACP wire; synthetic insertion only, not live model acceptance.
use super::*;

fn status(args: Value) -> Value {
    json!({"sessionId":"fixture-agent","toolCall":{
        "rawInput":{"variant":"UseTool","tool_name":"grok-computer-use__computer_status","tool_input":args},
        "_meta":{"x.ai/tool":{"name":"use_tool"}}}})
}

#[tokio::test]
async fn pending_status_is_allow_once_without_authorizing_any_execution_or_generic_cache() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let (client, _events) = AcpClient::connect_tcp(
        &listener.local_addr().unwrap().to_string(),
        std::env::temp_dir(),
    )
    .await
    .unwrap();
    let (_peer, _) = listener.accept().await.unwrap();
    let broker = Arc::new(crate::computer_use::ComputerUseBroker::request_endpoint(
        crate::computer_use::BrokerOptions::default(),
    ));
    let issued = broker.reserve_transport_connection("fixture-app").unwrap();
    broker
        .bind_transport_agent(issued.token(), "fixture-agent")
        .unwrap();
    client.computer_use_connections.lock().insert(
        "fixture-app".into(),
        RequestConnection {
            transport: Some(Arc::new(RequestTransport {
                history_path: None,
                picker: ParkingMutex::new(None),
                deliveries: ParkingMutex::new(super::super::delivery::DeliveryLedger::default()),
                broker: broker.clone(),
                token: issued.token().into(),
                entry: json!({"name":crate::computer_use::MCP_SERVER_NAME}),
            })),
            error: None,
        },
    );
    let request = broker.submit_access(issued.token(), json!({
        "action":"request", "purpose":"pending status regression", "scopeIntent":"owned fixture only",
        "surfaces":["app-webview"], "capabilities":["observe","input"]
    })).unwrap();
    let options = json!([{"optionId":"allow-once","kind":"allow_once"},
        {"optionId":"reject-once","kind":"reject_once"}, {"optionId":"always-allow","kind":"allow_always"}]);
    let raw = status(json!({}));
    assert!(client.is_registered_computer_use_call("fixture-app", &raw));
    for _ in 0..2 {
        assert!(matches!(
            client.computer_use_permission("fixture-app", "fixture-agent", &raw, &options, true),
            Some(PermissionOutcome::Selected { option_id }) if option_id == "allow-once"
        ));
        assert_eq!(broker.executable_grant_count("fixture-app"), 0);
        assert_eq!(broker.continuation_count("fixture-app"), 0);
        assert_eq!(broker.live_request_count("fixture-app"), 1);
        assert!(!broker.feature_enabled());
    }
    for (agent, active) in [("old-agent", true), ("fixture-agent", false)] {
        assert!(matches!(
            client.computer_use_permission("fixture-app", agent, &raw, &options, active),
            Some(PermissionOutcome::Selected { option_id }) if option_id == "reject-once"
        ));
    }
    for args in [
        json!({"runId":"foreign"}),
        json!({"runId":null}),
        json!({"appSessionId":"foreign"}),
        json!({"alwaysApprove":true}),
        json!({"screenshot":true}),
    ] {
        assert!(matches!(
            client.computer_use_permission("fixture-app", "fixture-agent", &status(args), &options, true),
            Some(PermissionOutcome::Selected { option_id }) if option_id == "reject-once"
        ));
    }
    assert!(matches!(
        client.computer_use_permission(
            "fixture-app",
            "fixture-agent",
            &raw,
            &json!([{"optionId":"always-allow","kind":"allow_always"}]),
            true
        ),
        Some(PermissionOutcome::Cancelled)
    ));
    for tool in [
        "computer_observe",
        "computer_list_targets",
        "computer_upload",
    ] {
        let mut execution = raw.clone();
        execution["toolCall"]["rawInput"]["tool_name"] =
            json!(format!("grok-computer-use__{tool}"));
        assert!(matches!(
            client.computer_use_permission("fixture-app", "fixture-agent", &execution, &options, true),
            Some(PermissionOutcome::Selected { option_id }) if option_id == "reject-once"
        ));
    }
    broker
        .deny_access(issued.token(), request["requestId"].as_str().unwrap(), 1)
        .unwrap();
    assert!(matches!(
        client.computer_use_permission("fixture-app", "fixture-agent", &raw, &options, true),
        Some(PermissionOutcome::Selected { option_id }) if option_id == "allow-once"
    ));
    assert_eq!(broker.executable_grant_count("fixture-app"), 0);
    broker.retire_transport_connection(issued.token());
    assert!(matches!(
        client.computer_use_permission("fixture-app", "fixture-agent", &raw, &options, true),
        Some(PermissionOutcome::Selected { option_id }) if option_id == "reject-once"
    ));
    client.kill().await;
}
