//! Synthetic exact native proof; no App or OS approval is automated.
use super::*;
use grok_computer_use_core::broker::{AccessApprovalReady, HostExecutionAddress};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

impl Fixture {
    pub(crate) fn native_paused_execution(
        &self,
    ) -> (
        Arc<grok_computer_use_core::fake::FakeAdapter>,
        HostExecutionAddress,
    ) {
        let fake = Arc::new(grok_computer_use_core::fake::FakeAdapter::new());
        fake.set_synthetic_scope_instance(Some(1));
        fake.set_include_png(true);
        self.transport
            .broker
            .register_surface_adapter(crate::computer_use::SurfaceKind::Desktop, fake.clone())
            .unwrap();
        let targets = [(crate::computer_use::SurfaceKind::Desktop, fake.fixture_id())];
        let refs = [(targets[0].0, targets[0].1.as_str())];
        let selection = self
            .transport
            .broker
            .capture_task_scope_group(
                &self.transport.token,
                &self.key.request_id,
                self.key.revision,
                &refs,
            )
            .unwrap();
        let AccessApprovalReady::Prepare(mut approval) = self
            .transport
            .broker
            .reserve_selected_access_approval(&self.transport.token, selection)
            .unwrap()
        else {
            panic!("new native proof approval")
        };
        approval.publish_feature().unwrap();
        approval.prepare_execution().unwrap();
        let outcome = approval.commit().unwrap();
        self.transport.broker.pause(&outcome.run_id).unwrap();
        let execution = self
            .transport
            .broker
            .host_task_view(&self.transport.token)
            .unwrap()
            .executions
            .remove(0);
        (
            fake,
            HostExecutionAddress {
                key: self.key.clone(),
                run_id: outcome.run_id,
                generation: execution.generation,
            },
        )
    }

    pub(crate) fn recovery(&self, address: &HostExecutionAddress) -> TaskResumeCall {
        let TaskResumeStarted::Prepare(call) = self
            .client
            .computer_use_execution_call("view-app", "synthetic-view-agent", address)
            .unwrap()
            .reserve_resume()
            .unwrap()
        else {
            panic!("new recovery")
        };
        *call
    }

    fn replace_transport_without_retiring_original(&self) {
        let broker = Arc::new(crate::computer_use::ComputerUseBroker::request_endpoint(
            BrokerOptions::default(),
        ));
        let issued = broker.reserve_transport_connection("view-app").unwrap();
        broker
            .bind_transport_agent(issued.token(), "synthetic-view-agent")
            .unwrap();
        self.client.computer_use_connections.lock().insert(
            "view-app".into(),
            RequestConnection {
                transport: Some(Arc::new(RequestTransport {
                    broker,
                    token: issued.token().into(),
                    entry: Value::Null,
                    deliveries: ParkingMutex::new(delivery::DeliveryLedger::default()),
                    history_path: None,
                    picker: ParkingMutex::new(None),
                })),
                error: None,
            },
        );
    }
}

#[tokio::test]
async fn native_resume_pins_original_acp_and_produces_one_new_decision() {
    let f = Fixture::new().await;
    let (fake, address) = f.native_paused_execution();
    let mut call = f.recovery(&address);
    assert!(matches!(
        f.client
            .computer_use_execution_call("view-app", "synthetic-view-agent", &address,)
            .unwrap()
            .reserve_resume()
            .unwrap(),
        TaskResumeStarted::InProgress
    ));
    let unlocked = Arc::new(AtomicBool::new(false));
    let checked = unlocked.clone();
    let client = f.client.clone();
    fake.arm_liveness_hook(Box::new(move || {
        checked.store(
            client.computer_use_connections.try_lock().is_some(),
            Ordering::SeqCst,
        );
    }));
    call.prepare_native(Duration::from_secs(1)).unwrap();
    assert!(
        unlocked.load(Ordering::SeqCst),
        "native queries must not hold ACP mutex"
    );
    assert!(
        !f.transport
            .broker
            .host_task_view(&f.transport.token)
            .unwrap()
            .grants[0]
            .executable
    );
    let queried = Arc::new(AtomicBool::new(false));
    let checked = queried.clone();
    fake.arm_liveness_hook(Box::new(move || {
        checked.store(true, Ordering::SeqCst);
    }));
    let receipt = call.commit().unwrap();
    assert_ne!(receipt.address.generation, address.generation);
    assert!(
        !queried.load(Ordering::SeqCst),
        "publication must be native-free"
    );
    assert!(call.commit().is_err());
    drop(call);
    let view = f
        .transport
        .broker
        .host_task_view(&f.transport.token)
        .unwrap();
    assert!(view.grants[0].executable);
    assert!(view.executions[0].has_fresh_observation);
    let claim = f.transport.broker.claim_continuation("view-app").unwrap();
    f.transport.broker.consume_continuation(&claim).unwrap();
    assert!(f.transport.broker.claim_continuation("view-app").is_none());
    assert_eq!(fake.observe_calls(), 1);
    f.finish().await;
}

#[tokio::test]
async fn same_agent_acp_pointer_replacement_after_capture_cannot_publish_old_grant() {
    let f = Fixture::new().await;
    let (fake, address) = f.native_paused_execution();
    let mut call = f.recovery(&address);
    call.prepare_native(Duration::from_secs(1)).unwrap();
    f.replace_transport_without_retiring_original();
    let view = f
        .transport
        .broker
        .host_task_view(&f.transport.token)
        .unwrap();
    assert!(
        !view.grants[0].executable,
        "original Core token is still live"
    );
    assert!(call.commit().is_err(), "final ACP pointer must be checked");
    assert_eq!(fake.observe_calls(), 1);
    assert!(f.transport.broker.claim_continuation("view-app").is_none());
    drop(call);
    assert!(
        !f.transport
            .broker
            .host_task_view(&f.transport.token)
            .unwrap()
            .grants[0]
            .executable
    );
    f.transport.retire();
    f.finish().await;
}

#[tokio::test]
async fn replacement_during_capture_and_before_native_admission_rejects_resume() {
    for during_capture in [false, true] {
        let f = Fixture::new().await;
        let (fake, address) = f.native_paused_execution();
        let mut call = f.recovery(&address);
        if during_capture {
            let client = f.client.clone();
            fake.arm_capture_hook(Box::new(move || {
                client.computer_use_connections.lock().remove("view-app");
            }));
        } else {
            f.replace_transport_without_retiring_original();
        }
        assert!(call.prepare_native(Duration::from_secs(1)).is_err());
        assert!(call.commit().is_err());
        assert_eq!(fake.observe_calls(), u32::from(during_capture));
        assert!(f.transport.broker.claim_continuation("view-app").is_none());
        drop(call);
        f.transport.retire();
        f.finish().await;
    }
}

#[tokio::test]
async fn exact_transport_stop_after_preparation_cannot_restore_authority() {
    let f = Fixture::new().await;
    let (_fake, address) = f.native_paused_execution();
    let mut call = f.recovery(&address);
    call.prepare_native(Duration::from_secs(1)).unwrap();
    f.client
        .decide_computer_use_task(
            "view-app",
            "synthetic-view-agent",
            &f.key,
            HostTaskDecision::Stop,
        )
        .unwrap();
    assert!(call.commit().is_err());
    drop(call);
    assert!(f.transport.broker.claim_continuation("view-app").is_none());
    f.finish().await;
}

#[tokio::test]
async fn resumed_decision_writes_once_to_original_agent_session_on_actual_local_wire() {
    use crate::acp_client::computer_use::DeliveryAttempt;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    let mut f = Fixture::new().await;
    let (_fake, address) = f.native_paused_execution();
    let mut call = f.recovery(&address);
    call.prepare_native(Duration::from_secs(1)).unwrap();
    call.commit().unwrap();
    drop(call);
    let delivery = f
        .client
        .computer_use_decision("view-app", "synthetic-view-agent")
        .unwrap();
    let client = f.client.clone();
    let duplicate = delivery.clone();
    let worker = tokio::spawn(async move {
        client
            .prompt_computer_use(&delivery, "synthetic-resume-turn", &|| Ok(()))
            .await
    });
    let original_client = f.client.clone();
    let mut reader = BufReader::new(&mut f._peer);
    let mut line = String::new();
    tokio::time::timeout(Duration::from_secs(2), reader.read_line(&mut line))
        .await
        .unwrap()
        .unwrap();
    let request: Value = serde_json::from_str(&line).unwrap();
    assert_eq!(request["method"], "session/prompt");
    assert_eq!(request["params"]["sessionId"], "synthetic-view-agent");
    let reply = json!({"jsonrpc":"2.0", "id":request["id"], "result":{"stopReason":"end_turn"}})
        .to_string()
        + "\n";
    reader.get_mut().write_all(reply.as_bytes()).await.unwrap();
    assert!(matches!(
        tokio::time::timeout(Duration::from_secs(2), worker)
            .await
            .unwrap()
            .unwrap(),
        DeliveryAttempt::Completed
    ));
    assert!(matches!(
        original_client
            .prompt_computer_use(&duplicate, "duplicate-resume-turn", &|| Ok(()))
            .await,
        DeliveryAttempt::NotStarted
    ));
    line.clear();
    assert!(
        tokio::time::timeout(Duration::from_millis(80), reader.read_line(&mut line))
            .await
            .is_err()
    );
    drop(reader);
    assert!(f
        .client
        .computer_use_decision("view-app", "synthetic-view-agent")
        .is_none());
    f.finish().await;
}
