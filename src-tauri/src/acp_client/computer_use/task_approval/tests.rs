//! Owned localhost ACP wire and explicitly synthetic scope witnesses.
//! No product singleton, settings, native approval or credential reads.
use super::*;
use grok_computer_use_core::{broker::BrokerOptions, fake::FakeAdapter, owned::HostOwnedAdapter};

struct Fixture {
    client: Arc<AcpClient>,
    _peer: tokio::net::TcpStream,
    transport: Arc<RequestTransport>,
    fake: Arc<FakeAdapter>,
    request: String,
    targets: Vec<TaskScopeTarget>,
}

impl Fixture {
    async fn new() -> Self {
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
        fake.pin_synthetic_member("fixture:synthetic-second");
        let broker = Arc::new(crate::computer_use::ComputerUseBroker::new(
            Arc::new(HostOwnedAdapter::new(fake.clone())),
            BrokerOptions {
                feature_enabled: false,
                lease_path: std::env::temp_dir()
                    .join(format!("cu-acp-scope-{}.lease", uuid::Uuid::new_v4())),
                ..BrokerOptions::default()
            },
        ));
        let issued = broker.reserve_transport_connection("scope-app").unwrap();
        broker
            .bind_transport_agent(issued.token(), "synthetic-agent")
            .unwrap();
        let transport = Arc::new(RequestTransport {
            broker: broker.clone(),
            token: issued.token().into(),
            entry: Value::Null,
            deliveries: ParkingMutex::new(delivery::DeliveryLedger::default()),
            history_path: None,
            picker: ParkingMutex::new(None),
        });
        client.computer_use_connections.lock().insert(
            "scope-app".into(),
            RequestConnection {
                transport: Some(transport.clone()),
                error: None,
            },
        );
        let pending = broker.submit_access(issued.token(), json!({"action":"request", "purpose":"synthetic ACP selection", "scopeIntent":"two owned fake windows", "surfaces":["desktop"], "capabilities":["observe"]})).unwrap();
        let targets = vec![
            TaskScopeTarget {
                surface: "desktop".into(),
                target_id: fake.fixture_id(),
            },
            TaskScopeTarget {
                surface: "desktop".into(),
                target_id: "fixture:synthetic-second".into(),
            },
        ];
        Self {
            client,
            _peer: peer,
            transport,
            fake,
            request: pending["requestId"].as_str().unwrap().into(),
            targets,
        }
    }

    fn start(&self) -> Start {
        self.client
            .start_scope_approval(
                "scope-app",
                "synthetic-agent",
                &self.request,
                1,
                &self.targets,
            )
            .unwrap()
    }

    async fn finish(self) {
        self.client.kill().await;
        self.transport.broker.fence_task_consent("scope-app");
        for ticket in self.transport.broker.take_cleanup_tickets() {
            self.transport.broker.finish_stop_cleanup(&ticket).unwrap();
        }
    }
}

#[tokio::test]
async fn exact_acp_selection_rejects_foreign_session_agent_request_and_revision_before_native_work()
{
    let fixture = Fixture::new().await;
    fixture
        .fake
        .arm_liveness_hook(Box::new(|| panic!("invalid Host owner reached adapter")));
    for (app, agent, request, revision) in [
        (
            "foreign-app",
            "synthetic-agent",
            fixture.request.as_str(),
            1,
        ),
        ("scope-app", "foreign-agent", fixture.request.as_str(), 1),
        ("scope-app", "synthetic-agent", "foreign-request", 1),
        ("scope-app", "synthetic-agent", fixture.request.as_str(), 2),
    ] {
        assert!(fixture
            .client
            .start_scope_approval(app, agent, request, revision, &fixture.targets)
            .is_err());
    }
    assert_eq!(fixture.fake.list_calls(), 0);
    assert_eq!(fixture.fake.observe_calls(), 0);
    assert_eq!(fixture.transport.broker.continuation_count("scope-app"), 0);
    fixture.finish().await;
}

#[tokio::test]
async fn two_early_clicks_retire_only_the_older_selection_and_retries_keep_one_committed_scope() {
    let fixture = Fixture::new().await;
    let Start::Select(first_transport, old) = fixture.start() else {
        panic!("select")
    };
    let Start::Select(transport, fresh) = fixture.start() else {
        panic!("select")
    };
    assert!(Arc::ptr_eq(&first_transport, &transport));
    assert!(!old.is_current());
    drop(old);
    let AccessApprovalReady::Prepare(mut approval) = transport
        .broker
        .reserve_selected_access_approval(&transport.token, fresh.capture().unwrap())
        .unwrap()
    else {
        panic!("prepare")
    };
    approval.publish_feature().unwrap();
    approval.prepare_execution().unwrap();
    let outcome = approval.commit().unwrap();
    let Start::Already(retry) = fixture.start() else {
        panic!("retry must not recapture")
    };
    assert_eq!(outcome.run_id, retry.run_id);
    assert_eq!(outcome.continuation_id, retry.continuation_id);
    let mut changed = fixture.targets.clone();
    changed.pop();
    assert!(fixture
        .client
        .start_scope_approval(
            "scope-app",
            "synthetic-agent",
            &fixture.request,
            1,
            &changed
        )
        .is_err());
    assert_eq!(
        fixture.transport.broker.executable_grant_count("scope-app"),
        1
    );
    assert_eq!(fixture.transport.broker.continuation_count("scope-app"), 1);
    assert_eq!(fixture.fake.list_calls(), 0);
    assert_eq!(fixture.fake.observe_calls(), 0);
    fixture.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn native_query_releases_acp_lock_and_late_result_cannot_use_reconnected_transport() {
    let fixture = Fixture::new().await;
    let Start::Select(original, selection) = fixture.start() else {
        panic!("select")
    };
    let (entered, started) = tokio::sync::oneshot::channel();
    let (release, blocked) = std::sync::mpsc::channel();
    fixture.fake.arm_liveness_hook(Box::new(move || {
        entered.send(()).unwrap();
        blocked.recv_timeout(Duration::from_secs(3)).unwrap();
    }));
    let native = tauri::async_runtime::spawn_blocking(move || selection.capture());
    tokio::time::timeout(Duration::from_secs(3), started)
        .await
        .unwrap()
        .unwrap();
    let issued = original
        .broker
        .reserve_transport_connection("scope-app")
        .unwrap();
    original
        .broker
        .bind_transport_agent(issued.token(), "replacement-agent")
        .unwrap();
    let replacement = Arc::new(RequestTransport {
        broker: original.broker.clone(),
        token: issued.token().into(),
        entry: Value::Null,
        deliveries: ParkingMutex::new(delivery::DeliveryLedger::default()),
        history_path: None,
        picker: ParkingMutex::new(None),
    });
    {
        let mut slots = fixture
            .client
            .computer_use_connections
            .try_lock()
            .expect("native query must not hold ACP mutex");
        slots.insert(
            "scope-app".into(),
            RequestConnection {
                transport: Some(replacement.clone()),
                error: None,
            },
        );
    }
    assert!(!fixture
        .client
        .scope_owner_is_current("scope-app", "synthetic-agent", &original));
    assert!(fixture
        .client
        .scope_owner_is_current("scope-app", "replacement-agent", &replacement));
    release.send(()).unwrap();
    assert!(native.await.unwrap().is_err());
    assert_eq!(original.broker.executable_grant_count("scope-app"), 0);
    assert_eq!(fixture.fake.observe_calls(), 0);
    fixture.finish().await;
}

#[tokio::test]
async fn retained_acp_async_approval_commits_once_and_keeps_the_same_transport() {
    let fixture = Fixture::new().await;
    let start = fixture.start();
    let outcome = fixture
        .client
        .finish_scope_approval(
            "scope-app",
            "synthetic-agent",
            start,
            |broker, token, selection| async move {
                broker
                    .reserve_selected_access_approval(&token, selection.capture().unwrap())
                    .map_err(|error| error.to_string())
            },
            |_, mut approval| async move {
                approval.publish_feature().unwrap();
                approval.prepare_execution().unwrap();
                approval.commit().map_err(|error| error.to_string())
            },
        )
        .await
        .unwrap();
    assert!(fixture.client.scope_owner_is_current(
        "scope-app",
        "synthetic-agent",
        &fixture.transport
    ));
    let retry = fixture
        .client
        .finish_scope_approval(
            "scope-app",
            "synthetic-agent",
            fixture.start(),
            |_, _, _| async { panic!("committed retry captured again") },
            |_, _| async { panic!("committed retry approved again") },
        )
        .await
        .unwrap();
    assert_eq!(retry.run_id, outcome.run_id);
    assert_eq!(fixture.transport.broker.continuation_count("scope-app"), 1);
    assert_eq!(fixture.fake.list_calls(), 0);
    assert_eq!(fixture.fake.observe_calls(), 0);
    fixture.finish().await;
}

#[tokio::test]
async fn cancelling_the_original_acp_waiter_fences_a_reserved_selection_before_final_approval() {
    let fixture = Fixture::new().await;
    let start = fixture.start();
    let (entered, started) = tokio::sync::oneshot::channel();
    let client = fixture.client.clone();
    let waiter = tokio::spawn(async move {
        client
            .finish_scope_approval(
                "scope-app",
                "synthetic-agent",
                start,
                |broker, token, selection| async move {
                    broker
                        .reserve_selected_access_approval(&token, selection.capture().unwrap())
                        .map_err(|error| error.to_string())
                },
                |_, approval| async move {
                    entered
                        .send(())
                        .unwrap_or_else(|_| panic!("fixture waiter absent"));
                    std::future::pending::<()>().await;
                    drop(approval);
                    Err("synthetic cancellation".into())
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
    assert!(
        fixture
            .transport
            .broker
            .reserve_access_approval(&fixture.transport.token, &fixture.request, 1)
            .is_err(),
        "original ACP call must clear its selected scope when cancelled"
    );
    assert_eq!(
        fixture.transport.broker.executable_grant_count("scope-app"),
        0
    );
    assert_eq!(fixture.transport.broker.continuation_count("scope-app"), 0);
    assert!(!fixture.transport.broker.feature_enabled());
    let Start::Select(_, retry) = fixture.start() else {
        panic!("fresh retry")
    };
    assert!(retry.is_current());
    drop(retry);
    fixture.finish().await;
}
