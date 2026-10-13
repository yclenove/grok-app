//! Owned localhost ACP wire, synthetic identities; no native/App grants.
use super::*;
use grok_computer_use_core::{
    broker::BrokerOptions,
    fake::FakeAdapter,
    owned::HostOwnedAdapter,
    task_consent::{HostRequestPhase, SYNTHETIC_DECISION_CLASS},
};
use std::time::Duration;

const SESSION: &str = "picker-acp-app";
const AGENT: &str = "synthetic-picker-agent";
const PROCESS: &str = "synthetic-picker-process";
mod existing;
mod webview;

struct Fixture {
    client: Arc<AcpClient>,
    _peer: tokio::net::TcpStream,
    transport: Arc<RequestTransport>,
    fake: Arc<FakeAdapter>,
    key: HostRequestKey,
}

impl Fixture {
    async fn new() -> Self {
        Self::new_for("desktop").await
    }

    async fn new_for(surface: &str) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let (client, _events) = AcpClient::connect_tcp(
            &listener.local_addr().unwrap().to_string(),
            std::env::temp_dir(),
        )
        .await
        .unwrap();
        let (peer, _) = listener.accept().await.unwrap();
        let fake = Arc::new(FakeAdapter::new());
        fake.set_synthetic_scope_instance(Some(1));
        fake.pin_synthetic_member("fixture:picker-second");
        let broker = Arc::new(crate::computer_use::ComputerUseBroker::new(
            Arc::new(HostOwnedAdapter::new(fake.clone())),
            BrokerOptions {
                feature_enabled: false,
                lease_path: std::env::temp_dir()
                    .join(format!("cu-picker-acp-{}.lease", uuid::Uuid::new_v4())),
                ..BrokerOptions::default()
            },
        ));
        let issued = broker.reserve_transport_connection(SESSION).unwrap();
        broker.bind_transport_agent(issued.token(), AGENT).unwrap();
        let transport = Arc::new(RequestTransport {
            broker,
            token: issued.token().into(),
            entry: json!({"env":[{"name":"GROK_APP_CU_IPC","value":"http://127.0.0.1:9"}]}),
            deliveries: ParkingMutex::new(delivery::DeliveryLedger::default()),
            history_path: None,
            picker: ParkingMutex::new(None),
        });
        client.computer_use_connections.lock().insert(
            SESSION.into(),
            RequestConnection {
                transport: Some(transport.clone()),
                error: None,
            },
        );
        transport.broker.submit_access(&transport.token, json!({"action":"request", "purpose":"synthetic ACP picker", "scopeIntent":"explicit synthetic user selection", "surfaces":[surface], "capabilities":["observe"]})).unwrap();
        let key = transport
            .broker
            .host_task_view(&transport.token)
            .unwrap()
            .request
            .unwrap()
            .key;
        Self {
            client,
            _peer: peer,
            transport,
            fake,
            key,
        }
    }

    fn begin(&self) -> TaskPickerStarted {
        self.client
            .begin_computer_use_task_picker(SESSION, AGENT, &self.key, "desktop", PROCESS)
            .unwrap()
    }

    async fn load(&self, id: &str) -> TaskPickerView {
        self.client
            .load_picker_with(SESSION, AGENT, &self.key, id, |_, call| async move {
                call.discover().map_err(|error| error.to_string())
            })
            .await
            .unwrap()
    }

    fn start(&self, id: &str, ids: &[String]) -> Result<PickerApprovalStart, String> {
        self.client
            .start_picker_approval(SESSION, AGENT, &self.key, id, ids)
    }

    fn no_execution(&self) {
        assert!(!self.transport.broker.feature_enabled());
        assert_eq!(self.transport.broker.executable_grant_count(SESSION), 0);
        assert_eq!(self.transport.broker.continuation_count(SESSION), 0);
        assert_eq!(self.fake.observe_calls(), 0);
        assert!(self.fake.executions().is_empty());
    }

    async fn finish(self) {
        self.client.kill().await;
        self.transport.broker.fence_task_consent(SESSION);
        for ticket in self.transport.broker.take_cleanup_tickets() {
            self.transport.broker.finish_stop_cleanup(&ticket).unwrap();
        }
    }
}

#[tokio::test]
async fn existing_first_use_app_picker_prepares_without_desktop_discovery() {
    let fixture = Fixture::new_for("existing-tabs").await;
    let result = fixture.client.begin_computer_use_task_picker(
        SESSION,
        AGENT,
        &fixture.key,
        "existing-tabs",
        PROCESS,
    );
    assert!(
        result.is_ok(),
        "original App picker must admit an explicit ExistingTab request"
    );
    let started = result.unwrap();
    assert!(fixture
        .transport
        .broker
        .tabs()
        .pending_pairing_challenge()
        .is_some());
    assert_eq!(fixture.fake.list_calls(), 0);
    fixture.no_execution();
    assert!(fixture
        .client
        .load_computer_use_task_picker(SESSION, AGENT, &fixture.key, &started.picker_id,)
        .await
        .is_err());
    assert!(
        fixture
            .transport
            .picker
            .lock()
            .as_ref()
            .unwrap()
            .call
            .is_some(),
        "preparation must not freeze an empty receipt before Pair/Share"
    );
    fixture
        .client
        .cancel_computer_use_task_picker(SESSION, AGENT, &fixture.key, &started.picker_id)
        .unwrap();
    assert!(fixture
        .transport
        .broker
        .tabs()
        .pending_pairing_challenge()
        .is_none());
    fixture.no_execution();
    fixture.finish().await;
}

fn ids(view: &TaskPickerView) -> Vec<String> {
    view.candidates
        .iter()
        .map(|row| row.candidate_id.clone())
        .collect()
}

async fn reserve_synthetic(
    broker: Arc<crate::computer_use::ComputerUseBroker>,
    token: String,
    selection: HostScopeSelection,
    _cancel: HostScopePickerCancel,
) -> Result<AccessApprovalReady, String> {
    broker
        .reserve_selected_access_approval(&token, selection)
        .map_err(|error| error.to_string())
}

async fn approve_synthetic(
    _broker: Arc<crate::computer_use::ComputerUseBroker>,
    mut approval: AccessApproval,
) -> Result<CommitOutcome, String> {
    approval
        .publish_feature()
        .map_err(|error| error.to_string())?;
    approval
        .prepare_execution()
        .map_err(|error| error.to_string())?;
    approval.commit().map_err(|error| error.to_string())
}

#[tokio::test]
async fn explicit_picker_reads_once_redacts_native_identity_and_cancel_restores_pending() {
    let fixture = Fixture::new().await;
    let started = fixture.begin();
    assert_eq!(fixture.fake.list_calls(), 0);
    let view = fixture.load(&started.picker_id).await;
    let cached = fixture.load(&started.picker_id).await;
    assert_eq!(ids(&view), ids(&cached));
    assert_eq!(fixture.fake.list_calls(), 1);
    assert_eq!(view.candidates.len(), 2);
    let json = serde_json::to_string(&view).unwrap();
    assert!(!json.contains(&fixture.transport.token));
    assert!(!json.contains(&fixture.fake.fixture_id()));
    assert!(!json.contains("executorGeneration"));
    fixture.no_execution();
    assert!(fixture
        .client
        .cancel_computer_use_task_picker(SESSION, AGENT, &fixture.key, &started.picker_id)
        .unwrap());
    assert_eq!(
        fixture
            .transport
            .broker
            .host_task_view(&fixture.transport.token)
            .unwrap()
            .request
            .unwrap()
            .phase,
        HostRequestPhase::Pending
    );
    assert!(!fixture
        .client
        .cancel_computer_use_task_picker(SESSION, AGENT, &fixture.key, &started.picker_id)
        .unwrap());
    assert!(fixture.start(&started.picker_id, &ids(&view)).is_err());
    fixture.finish().await;
}

#[tokio::test]
async fn foreign_keys_and_unavailable_surfaces_do_not_enumerate_or_replace_the_picker() {
    let fixture = Fixture::new().await;
    let started = fixture.begin();
    for surface in ["managed-browser", "existing-tab", "webview", "Desktop"] {
        assert!(fixture
            .client
            .begin_computer_use_task_picker(SESSION, AGENT, &fixture.key, surface, PROCESS)
            .is_err());
    }
    for key in [
        HostRequestKey {
            connection_id: "foreign".into(),
            ..fixture.key.clone()
        },
        HostRequestKey {
            task_id: "foreign".into(),
            ..fixture.key.clone()
        },
        HostRequestKey {
            request_id: "foreign".into(),
            ..fixture.key.clone()
        },
        HostRequestKey {
            revision: 2,
            ..fixture.key.clone()
        },
    ] {
        assert!(fixture
            .client
            .begin_computer_use_task_picker(SESSION, AGENT, &key, "desktop", PROCESS)
            .is_err());
        assert!(!fixture
            .client
            .cancel_computer_use_task_picker(SESSION, AGENT, &key, &started.picker_id)
            .unwrap());
    }
    for (session, agent) in [("foreign", AGENT), (SESSION, "foreign")] {
        assert!(fixture
            .client
            .begin_computer_use_task_picker(session, agent, &fixture.key, "desktop", PROCESS)
            .is_err());
    }
    assert_eq!(fixture.fake.list_calls(), 0);
    assert_eq!(fixture.load(&started.picker_id).await.candidates.len(), 2);
    fixture.no_execution();
    fixture.finish().await;
}

#[tokio::test]
async fn old_close_and_old_candidates_cannot_affect_the_replacement_picker() {
    let fixture = Fixture::new().await;
    let old = fixture.begin();
    let old_view = fixture.load(&old.picker_id).await;
    let fresh = fixture.begin();
    assert!(!fixture
        .client
        .cancel_computer_use_task_picker(SESSION, AGENT, &fixture.key, &old.picker_id)
        .unwrap());
    let fresh_view = fixture.load(&fresh.picker_id).await;
    assert!(fixture.start(&old.picker_id, &ids(&old_view)).is_err());
    assert!(fixture.start(&fresh.picker_id, &ids(&old_view)).is_err());
    let valid = fixture.start(&fresh.picker_id, &ids(&fresh_view)).unwrap();
    drop(valid);
    fixture.no_execution();
    fixture.finish().await;
}

#[tokio::test]
async fn double_approval_is_rejected_without_fencing_original_and_commit_retry_is_idempotent() {
    let fixture = Fixture::new().await;
    let picker = fixture.begin();
    let view = fixture.load(&picker.picker_id).await;
    let chosen = vec![ids(&view)[0].clone()];
    let start = fixture.start(&picker.picker_id, &chosen).unwrap();
    assert!(fixture
        .start(&picker.picker_id, &chosen)
        .err()
        .unwrap()
        .contains("in progress"));
    let outcome = fixture
        .client
        .finish_picker_approval(SESSION, AGENT, start, reserve_synthetic, approve_synthetic)
        .await
        .unwrap();
    assert_eq!(outcome.decision_class, SYNTHETIC_DECISION_CLASS);
    let retry = fixture
        .client
        .finish_picker_approval(
            SESSION,
            AGENT,
            fixture.start(&picker.picker_id, &chosen).unwrap(),
            |_, _, _, _| async { panic!("committed retry reserved again") },
            |_, _| async { panic!("committed retry approved again") },
        )
        .await
        .unwrap();
    assert_eq!(retry.run_id, outcome.run_id);
    assert_eq!(retry.continuation_id, outcome.continuation_id);
    assert!(fixture.start(&picker.picker_id, &ids(&view)).is_err());
    assert!(fixture
        .client
        .cancel_computer_use_task_picker(SESSION, AGENT, &fixture.key, &picker.picker_id)
        .unwrap());
    assert_eq!(fixture.transport.broker.executable_grant_count(SESSION), 1);
    assert_eq!(fixture.transport.broker.continuation_count(SESSION), 1);
    assert_eq!(fixture.fake.list_calls(), 1);
    assert_eq!(fixture.fake.observe_calls(), 0);
    fixture.finish().await;
}

#[tokio::test]
async fn stopping_preparation_drops_late_ready_result_without_publishing_candidates() {
    let fixture = Fixture::new().await;
    let picker = fixture.begin();
    let broker = fixture.transport.broker.clone();
    let token = fixture.transport.token.clone();
    let key = fixture.key.clone();
    let result = fixture
        .client
        .load_picker_with(
            SESSION,
            AGENT,
            &fixture.key,
            &picker.picker_id,
            move |_, call| async move {
                broker
                    .decide_host_task(
                        &token,
                        &key,
                        grok_computer_use_core::broker::HostTaskDecision::Stop,
                    )
                    .unwrap();
                call.discover().map_err(|error| error.to_string())
            },
        )
        .await;
    assert!(result.is_err());
    assert_eq!(fixture.fake.list_calls(), 0);
    fixture.no_execution();
    fixture.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reconnect_during_native_discovery_rejects_late_original_receipt_even_with_same_agent() {
    let fixture = Fixture::new().await;
    let picker = fixture.begin();
    let (entered, started) = tokio::sync::oneshot::channel();
    let (release, blocked) = std::sync::mpsc::channel();
    fixture.fake.arm_liveness_hook(Box::new(move || {
        entered.send(()).unwrap();
        blocked.recv_timeout(Duration::from_secs(5)).unwrap();
    }));
    let client = fixture.client.clone();
    let key = fixture.key.clone();
    let waiter = tokio::spawn(async move {
        client
            .load_picker_with(
                SESSION,
                AGENT,
                &key,
                &picker.picker_id,
                |_, call| async move {
                    tauri::async_runtime::spawn_blocking(move || call.discover())
                        .await
                        .unwrap()
                        .map_err(|error| error.to_string())
                },
            )
            .await
    });
    tokio::time::timeout(Duration::from_secs(3), started)
        .await
        .unwrap()
        .unwrap();
    let broker = fixture.transport.broker.clone();
    let issued = broker.reserve_transport_connection(SESSION).unwrap();
    broker.bind_transport_agent(issued.token(), AGENT).unwrap();
    let replacement = Arc::new(RequestTransport {
        broker,
        token: issued.token().into(),
        entry: Value::Null,
        deliveries: ParkingMutex::new(delivery::DeliveryLedger::default()),
        history_path: None,
        picker: ParkingMutex::new(None),
    });
    fixture
        .client
        .computer_use_connections
        .try_lock()
        .expect("discovery held ACP lock")
        .insert(
            SESSION.into(),
            RequestConnection {
                transport: Some(replacement),
                error: None,
            },
        );
    release.send(()).unwrap();
    assert!(waiter.await.unwrap().is_err());
    assert!(fixture
        .transport
        .picker
        .lock()
        .as_ref()
        .unwrap()
        .ready
        .is_none());
    fixture.no_execution();
    fixture.finish().await;
}

#[tokio::test]
async fn cancelled_approval_waiter_fences_reserved_scope_and_cannot_enable_feature() {
    let fixture = Fixture::new().await;
    let picker = fixture.begin();
    let view = fixture.load(&picker.picker_id).await;
    let start = fixture.start(&picker.picker_id, &ids(&view)).unwrap();
    let (entered, started) = tokio::sync::oneshot::channel();
    let client = fixture.client.clone();
    let waiter = tokio::spawn(async move {
        client
            .finish_picker_approval(
                SESSION,
                AGENT,
                start,
                reserve_synthetic,
                move |_, approval| async move {
                    entered.send(()).unwrap();
                    std::future::pending::<()>().await;
                    drop(approval);
                    Err("synthetic cancelled approval".into())
                },
            )
            .await
    });
    tokio::time::timeout(Duration::from_secs(3), started)
        .await
        .unwrap()
        .unwrap();
    waiter.abort();
    assert!(waiter.await.err().unwrap().is_cancelled());
    assert!(fixture.start(&picker.picker_id, &ids(&view)).is_err());
    assert_eq!(
        fixture
            .transport
            .broker
            .host_task_view(&fixture.transport.token)
            .unwrap()
            .request
            .unwrap()
            .phase,
        HostRequestPhase::Pending
    );
    fixture.no_execution();
    fixture.finish().await;
}
