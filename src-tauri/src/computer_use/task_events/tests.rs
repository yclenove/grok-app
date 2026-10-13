//! Real observer lifecycle; all consent is synthetic, no native/App approval.
use super::*;
use grok_computer_use_core::{broker::ComputerUseBroker, task_consent::HostRequestPhase};
use serde_json::json;

struct Fixture {
    broker: Arc<ComputerUseBroker>,
    token: String,
}

impl Fixture {
    fn new() -> Self {
        let broker = Arc::new(ComputerUseBroker::request_endpoint(Default::default()));
        let issued = broker.reserve_transport_connection("events-app").unwrap();
        broker
            .bind_transport_agent(issued.token(), "synthetic-events-agent")
            .unwrap();
        Self {
            broker,
            token: issued.token().into(),
        }
    }

    fn request(&self) {
        self.broker.submit_access(&self.token, json!({"action":"request", "purpose":"synthetic event lifecycle", "scopeIntent":"owned fake windows", "surfaces":["desktop"], "capabilities":["observe"]})).unwrap();
    }

    fn source(&self) -> TaskWatchSource {
        let broker = self.broker.clone();
        let token = self.token.clone();
        TaskWatchSource {
            changes: broker.host_task_changes(),
            host_instance_id: broker.host_instance_id().into(),
            read: Arc::new(move || {
                broker
                    .host_task_view(&token)
                    .map_err(|_| "original owner retired".into())
            }),
        }
    }
}

fn sink() -> (EventSink, tokio::sync::mpsc::UnboundedReceiver<TaskEvent>) {
    // Test-only queue: production sends coalesced metadata to a Tauri Channel.
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    (
        Arc::new(move |event| {
            tx.send(event)
                .map_err(|_| "synthetic channel closed".into())
        }),
        rx,
    )
}

async fn next(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<TaskEvent>,
    registry: &Subscriptions,
) -> TaskEvent {
    let event = tokio::time::timeout(Duration::from_secs(3), rx.recv())
        .await
        .unwrap()
        .unwrap();
    if !matches!(event.payload, TaskEventPayload::Retired) {
        registry
            .acknowledge(
                &event.owner.session_id,
                &event.owner.subscription_id,
                &event.sequence,
            )
            .unwrap();
    }
    event
}

#[tokio::test]
async fn pending_without_run_streams_and_unsubscribe_does_not_consume_a_decision() {
    let fixture = Fixture::new();
    let registry = Subscriptions::default();
    let (sink, mut rx) = sink();
    let owner = registry
        .start("events-app", fixture.source(), sink)
        .await
        .unwrap();
    assert!(
        matches!(next(&mut rx, &registry).await.payload, TaskEventPayload::View { view } if view.request.is_none())
    );
    fixture.request();
    let event = next(&mut rx, &registry).await;
    assert_eq!(event.owner.subscription_id, owner.subscription_id);
    assert_eq!(event.sequence, "2");
    assert!(
        matches!(event.payload, TaskEventPayload::View { view } if view.request.as_ref().unwrap().phase == HostRequestPhase::Pending && view.executions.is_empty() && view.grants.is_empty())
    );
    assert!(registry
        .remove("events-app", &owner.subscription_id)
        .await
        .unwrap());
    assert!(registry.entries.lock().is_empty());
    assert_eq!(
        fixture
            .broker
            .host_task_view(&fixture.token)
            .unwrap()
            .request
            .unwrap()
            .phase,
        HostRequestPhase::Pending
    );
    assert_eq!(fixture.broker.capabilities().backend_id, "request-only");
    assert_eq!(fixture.broker.continuation_count("events-app"), 0);
    registry.join().await.unwrap();
}

#[tokio::test]
async fn initial_read_start_gap_cannot_lose_a_request() {
    let fixture = Arc::new(Fixture::new());
    let mut source = fixture.source();
    let first = Arc::new(AtomicBool::new(true));
    let fixture_read = fixture.clone();
    source.read = Arc::new(move || {
        let view = fixture_read
            .broker
            .host_task_view(&fixture_read.token)
            .unwrap();
        if first.swap(false, Ordering::SeqCst) {
            fixture_read.request();
        }
        Ok(view)
    });
    let registry = Subscriptions::default();
    let (sink, mut rx) = sink();
    registry.start("events-app", source, sink).await.unwrap();
    assert!(
        matches!(next(&mut rx, &registry).await.payload, TaskEventPayload::View { view } if view.request.is_none())
    );
    assert!(
        matches!(next(&mut rx, &registry).await.payload, TaskEventPayload::View { view } if view.request.as_ref().unwrap().phase == HostRequestPhase::Pending)
    );
    registry.join().await.unwrap();
}

#[tokio::test]
async fn same_agent_reconnect_retires_only_the_original_stream() {
    let fixture = Fixture::new();
    fixture.request();
    let registry = Subscriptions::default();
    let (sink, mut rx) = sink();
    registry
        .start("events-app", fixture.source(), sink)
        .await
        .unwrap();
    next(&mut rx, &registry).await;
    let replacement = fixture
        .broker
        .reserve_transport_connection("events-app")
        .unwrap();
    fixture
        .broker
        .bind_transport_agent(replacement.token(), "synthetic-events-agent")
        .unwrap();
    assert!(matches!(
        next(&mut rx, &registry).await.payload,
        TaskEventPayload::Retired
    ));
    registry.join().await.unwrap();
    assert!(fixture.broker.host_task_view(replacement.token()).is_ok());
}

#[tokio::test]
async fn metadata_health_is_explicit_heartbeat_and_failed_channel_is_joined() {
    let fixture = Fixture::new();
    let registry = Subscriptions::default();
    let (sink, mut rx) = sink();
    registry
        .start("events-app", fixture.source(), sink)
        .await
        .unwrap();
    next(&mut rx, &registry).await;
    assert!(
        matches!(next(&mut rx, &registry).await.payload, TaskEventPayload::Heartbeat { view } if view.executions.is_empty())
    );
    drop(rx);
    fixture.request();
    tokio::time::timeout(Duration::from_secs(3), async {
        while !registry.entries.lock().is_empty() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    registry.join().await.unwrap();
    assert!(registry.entries.lock().is_empty());
    assert_eq!(
        fixture
            .broker
            .host_task_view(&fixture.token)
            .unwrap()
            .request
            .unwrap()
            .phase,
        HostRequestPhase::Pending
    );
}

#[tokio::test]
async fn foreign_cancel_and_old_cancel_cannot_remove_another_observer() {
    let fixture = Fixture::new();
    let registry = Subscriptions::default();
    let (first_sink, _rx) = sink();
    let first = registry
        .start("events-app", fixture.source(), first_sink)
        .await
        .unwrap();
    let (second_sink, _rx2) = sink();
    let second = registry
        .start("events-app", fixture.source(), second_sink)
        .await
        .unwrap();
    assert!(registry
        .remove("foreign", &first.subscription_id)
        .await
        .is_err());
    assert_eq!(registry.entries.lock().len(), 2);
    assert!(registry
        .remove("events-app", &first.subscription_id)
        .await
        .unwrap());
    assert!(!registry
        .remove("events-app", &first.subscription_id)
        .await
        .unwrap());
    assert!(registry
        .entries
        .lock()
        .contains_key(&second.subscription_id));
    registry.join().await.unwrap();
}

#[tokio::test]
async fn overflow_shutdown_and_wrong_initial_owner_cannot_start_an_unowned_worker() {
    let fixture = Fixture::new();
    let registry = Subscriptions::default();
    let (wrong_sink, _wrong_rx) = sink();
    assert!(registry
        .start("foreign", fixture.source(), wrong_sink)
        .await
        .is_err());
    let mut receivers = Vec::new();
    for _ in 0..MAX_SUBSCRIPTIONS {
        let (sink, rx) = sink();
        receivers.push(rx);
        registry
            .start("events-app", fixture.source(), sink)
            .await
            .unwrap();
    }
    let (overflow_sink, _rx) = sink();
    assert!(registry
        .start("events-app", fixture.source(), overflow_sink)
        .await
        .is_err());
    assert_eq!(registry.entries.lock().len(), MAX_SUBSCRIPTIONS);
    registry.join().await.unwrap();
    let (late_sink, _late_rx) = sink();
    assert!(registry
        .start("events-app", fixture.source(), late_sink)
        .await
        .is_err());
    assert!(registry.entries.lock().is_empty());
    drop(receivers);
}

#[tokio::test]
async fn cancelling_unsubscribe_waiter_keeps_original_blocked_handle_for_shutdown_join() {
    let registry = Arc::new(Subscriptions::default());
    let (entered, started) = tokio::sync::oneshot::channel();
    let (release, blocked) = tokio::sync::oneshot::channel();
    let owner = TaskSubscriptionStarted {
        subscription_id: "synthetic-blocked-worker".into(),
        session_id: "events-app".into(),
        connection_id: "synthetic-connection".into(),
        host_instance_id: "synthetic-host".into(),
    };
    let entry = Arc::new(Entry {
        owner: owner.clone(),
        cancel: Arc::new(Cancel::default()),
        ack: Arc::new(Acknowledgement::default()),
        task: Mutex::new(Some(tauri::async_runtime::spawn(async move {
            entered.send(()).unwrap();
            blocked.await.unwrap();
        }))),
    });
    registry
        .entries
        .lock()
        .insert(owner.subscription_id.clone(), entry.clone());
    started.await.unwrap();
    let removing = registry.clone();
    let waiter =
        tokio::spawn(async move { removing.remove("events-app", &owner.subscription_id).await });
    tokio::time::timeout(Duration::from_secs(3), async {
        while !entry.cancel.set.load(Ordering::SeqCst) {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    waiter.abort();
    assert!(waiter.await.unwrap_err().is_cancelled());
    assert!(entry.cancel.set.load(Ordering::SeqCst));
    let joining = registry.clone();
    let join = tokio::spawn(async move { joining.join().await });
    tokio::task::yield_now().await;
    assert!(!join.is_finished(), "original worker has not completed");
    release.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(3), join)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(registry.entries.lock().is_empty());
}

#[tokio::test]
async fn final_observer_exit_preserves_committed_synthetic_grant_and_continuation() {
    use grok_computer_use_core::{broker::BrokerOptions, fake::FakeAdapter};
    let fake = Arc::new(FakeAdapter::new());
    let broker = Arc::new(ComputerUseBroker::new(
        fake.clone(),
        BrokerOptions {
            feature_enabled: true,
            lease_path: std::env::temp_dir()
                .join(format!("cu-observer-{}.lease", uuid::Uuid::new_v4())),
            ..BrokerOptions::default()
        },
    ));
    let issued = broker.reserve_transport_connection("events-app").unwrap();
    broker
        .bind_transport_agent(issued.token(), "synthetic-events-agent")
        .unwrap();
    let fixture = Fixture {
        broker,
        token: issued.token().into(),
    };
    fixture.request();
    let key = fixture
        .broker
        .host_task_view(&fixture.token)
        .unwrap()
        .request
        .unwrap()
        .key;
    fixture
        .broker
        .bind_synthetic_scope(&key.request_id, &fake.fixture_id())
        .unwrap();
    fixture
        .broker
        .commit_access(&fixture.token, &key.request_id, key.revision)
        .unwrap();
    let registry = Subscriptions::default();
    let (sink, mut rx) = sink();
    let owner = registry
        .start("events-app", fixture.source(), sink)
        .await
        .unwrap();
    next(&mut rx, &registry).await;
    registry
        .remove("events-app", &owner.subscription_id)
        .await
        .unwrap();
    assert_eq!(fixture.broker.executable_grant_count("events-app"), 1);
    assert_eq!(fixture.broker.continuation_count("events-app"), 1);
    assert_eq!(fake.observe_calls(), 0);
    assert!(fake.executions().is_empty());
    // Fixture teardown is explicit and separate from observer cancellation.
    fixture.broker.fence_task_consent("events-app");
    for ticket in fixture.broker.take_cleanup_tickets() {
        fixture.broker.finish_stop_cleanup(&ticket).unwrap();
    }
    registry.join().await.unwrap();
}

#[tokio::test]
async fn unacknowledged_view_bounds_channel_backlog_and_rejects_foreign_or_future_ack() {
    let fixture = Fixture::new();
    let registry = Subscriptions::default();
    let (sink, mut rx) = sink();
    let owner = registry
        .start("events-app", fixture.source(), sink)
        .await
        .unwrap();
    let first = tokio::time::timeout(Duration::from_secs(3), rx.recv())
        .await
        .unwrap()
        .unwrap();
    fixture.request();
    assert!(registry
        .acknowledge("foreign", &owner.subscription_id, "1")
        .is_err());
    for sequence in ["0", "01", "+1", "2", "18446744073709551616"] {
        assert!(registry
            .acknowledge("events-app", &owner.subscription_id, sequence)
            .is_err());
    }
    tokio::time::sleep(Duration::from_millis(220)).await;
    assert!(
        rx.try_recv().is_err(),
        "a second view queued before the first was acknowledged"
    );
    registry
        .acknowledge("events-app", &owner.subscription_id, &first.sequence)
        .unwrap();
    assert!(
        matches!(next(&mut rx, &registry).await.payload, TaskEventPayload::View { view } if view.request.is_some())
    );
    registry.join().await.unwrap();
}

#[tokio::test]
async fn a_renderer_that_never_acknowledges_retires_without_touching_the_request() {
    let fixture = Fixture::new();
    fixture.request();
    let registry = Subscriptions::default();
    let (sink, mut rx) = sink();
    registry
        .start("events-app", fixture.source(), sink)
        .await
        .unwrap();
    let first = tokio::time::timeout(Duration::from_secs(3), rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(first.sequence, "1");
    let final_event = tokio::time::timeout(DELIVERY_BUDGET + Duration::from_secs(2), rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(final_event.sequence, "2");
    assert!(matches!(final_event.payload, TaskEventPayload::Retired));
    assert!(rx.recv().await.is_none());
    tokio::time::timeout(Duration::from_secs(3), async {
        while !registry.entries.lock().is_empty() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(!registry.reaper_failed.load(Ordering::SeqCst));
    registry.join().await.unwrap();
    assert_eq!(
        fixture
            .broker
            .host_task_view(&fixture.token)
            .unwrap()
            .request
            .unwrap()
            .phase,
        HostRequestPhase::Pending
    );
}
