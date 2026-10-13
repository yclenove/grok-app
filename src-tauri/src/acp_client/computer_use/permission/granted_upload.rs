//! Fixed permission wire + real Broker/App typed adapter; approval/page synthetic.
//! TCP fixture insertion does not prove production eligibility or live CLI/model.
use super::*;
use grok_computer_use_core::broker::AccessApprovalReady;

fn upload(args: Value) -> Value {
    json!({"sessionId":"fixture-agent","toolCall":{
        "rawInput":{"variant":"UseTool","tool_name":"grok-computer-use__computer_upload","tool_input":args},
        "_meta":{"x.ai/tool":{"name":"use_tool"}}}})
}

#[tokio::test]
async fn granted_upload_preflight_is_once_read_only_and_revoked_on_pause_or_stop() {
    for stop in [false, true] {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let (client, _events) = AcpClient::connect_tcp(
            &listener.local_addr().unwrap().to_string(),
            std::env::temp_dir(),
        )
        .await
        .unwrap();
        let (_peer, _) = listener.accept().await.unwrap();
        let broker = Arc::new(crate::computer_use::ComputerUseBroker::request_endpoint(
            crate::computer_use::BrokerOptions {
                lease_path: std::env::temp_dir().join(format!(
                    "cu-upload-permission-{}.lease",
                    uuid::Uuid::new_v4()
                )),
                ..Default::default()
            },
        ));
        let (adapter, scripts) = crate::computer_use::webview::upload_permission_fixture();
        broker
            .register_surface_adapter(crate::computer_use::SurfaceKind::WebView, adapter)
            .unwrap();
        let issued = broker.reserve_transport_connection("fixture-app").unwrap();
        broker
            .bind_transport_agent(issued.token(), "fixture-agent")
            .unwrap();
        client.computer_use_connections.lock().insert("fixture-app".into(), RequestConnection {
            transport: Some(Arc::new(RequestTransport {
                history_path: None, picker: ParkingMutex::new(None),
                deliveries: ParkingMutex::new(super::super::delivery::DeliveryLedger::default()),
                broker: broker.clone(), token: issued.token().into(),
                entry: json!({"name":crate::computer_use::MCP_SERVER_NAME}),
            })), error: None,
        });
        broker.submit_access(issued.token(), json!({"action":"request","purpose":"synthetic upload preflight",
            "scopeIntent":"one retained page","surfaces":["app-webview"],"capabilities":["observe","input"]})).unwrap();
        let key = broker
            .host_task_view(issued.token())
            .unwrap()
            .request
            .unwrap()
            .key;
        let picker = broker
            .begin_task_scope_picker(
                issued.token(),
                &key,
                crate::computer_use::SurfaceKind::WebView,
            )
            .unwrap()
            .discover()
            .unwrap();
        let id = picker.views().unwrap().remove(0).candidate_id;
        let AccessApprovalReady::Prepare(mut approval) = broker
            .reserve_selected_access_approval(issued.token(), picker.select(&[id]).unwrap())
            .unwrap()
        else {
            panic!("fresh original approval")
        };
        approval.publish_feature().unwrap();
        approval.prepare_execution().unwrap();
        approval.prepare_publication().unwrap();
        let grant = approval.commit_for_transport(issued.token()).unwrap();
        let observing = broker
            .begin_model_call(issued.token(), "computer_observe")
            .unwrap();
        let observation = broker.observe_for_permit(&observing, false).unwrap();
        let args = json!({"version":1,"runId":grant.run_id,"targetId":observation.target_id,
            "targetGeneration":observation.target_generation,"snapshotId":observation.snapshot_id,
            "geometryRevision":observation.geometry_revision,"elementRef":observation.nodes[0].node_ref,
            "actionId":"original-file-choice"});
        let options = json!([{"optionId":"allow-once","kind":"allow_once"},
            {"optionId":"reject-once","kind":"reject_once"},{"optionId":"always-allow","kind":"allow_always"}]);
        let raw = upload(args.clone());
        let count = scripts();
        for _ in 0..2 {
            assert!(
                matches!(client.computer_use_permission("fixture-app","fixture-agent",&raw,&options,true),
                Some(PermissionOutcome::Selected {option_id}) if option_id=="allow-once")
            );
            assert_eq!(
                scripts(),
                count,
                "preflight cannot observe or assign the field"
            );
            assert!(broker.host_task_view(issued.token()).unwrap().executions[0]
                .upload
                .is_none());
        }
        for (agent, active) in [("old-agent", true), ("fixture-agent", false)] {
            assert!(
                matches!(client.computer_use_permission("fixture-app",agent,&raw,&options,active),
                Some(PermissionOutcome::Selected {option_id}) if option_id=="reject-once")
            );
        }
        for (field, value) in [
            ("version", json!(2)),
            ("runId", json!("foreign-run")),
            ("targetGeneration", json!(observation.target_generation + 1)),
            ("snapshotId", json!("stale")),
            ("geometryRevision", json!(observation.geometry_revision + 1)),
            ("elementRef", json!("foreign-ref")),
            ("path", json!("private-file")),
        ] {
            let mut invalid = args.clone();
            invalid[field] = value;
            assert!(
                matches!(client.computer_use_permission("fixture-app","fixture-agent",&upload(invalid),&options,true),
                Some(PermissionOutcome::Selected {option_id}) if option_id=="reject-once"),
                "{field}"
            );
        }
        if stop {
            broker.request_stop(&grant.run_id).unwrap();
        } else {
            broker.pause(&grant.run_id).unwrap();
        }
        assert!(
            matches!(client.computer_use_permission("fixture-app","fixture-agent",&raw,&options,true),
            Some(PermissionOutcome::Selected {option_id}) if option_id=="reject-once")
        );
        assert_eq!(scripts(), count);
        client.kill().await;
        for ticket in broker.take_cleanup_tickets() {
            broker.finish_stop_cleanup(&ticket).unwrap();
        }
    }
}
