//! Isolated synthetic producers and real retained blocking-worker handles.
use super::*;
use grok_computer_use_core::{
    broker::BrokerOptions,
    fake::FakeAdapter,
    owned::HostOwnedAdapter,
    task_consent::{HostRequestKey, HostRequestPhase},
};
use serde_json::json;
use std::time::Duration;

struct Fixture {
    broker: Arc<ComputerUseBroker>,
    fake: Arc<FakeAdapter>,
    token: String,
    key: HostRequestKey,
}

impl Fixture {
    fn new() -> Self {
        let fake = Arc::new(FakeAdapter::new());
        fake.set_synthetic_scope_instance(Some(1));
        fake.pin_synthetic_member("fixture:picker-second");
        let broker = Arc::new(ComputerUseBroker::new(
            Arc::new(HostOwnedAdapter::new(fake.clone())),
            BrokerOptions {
                feature_enabled: false,
                lease_path: std::env::temp_dir()
                    .join(format!("cu-picker-worker-{}.lease", uuid::Uuid::new_v4())),
                ..BrokerOptions::default()
            },
        ));
        let issued = broker
            .reserve_transport_connection("picker-worker-app")
            .unwrap();
        broker
            .bind_transport_agent(issued.token(), "synthetic-picker-agent")
            .unwrap();
        broker.submit_access(issued.token(), json!({"action":"request", "purpose":"synthetic picker worker", "scopeIntent":"owned fake windows", "surfaces":["desktop"], "capabilities":["observe"]})).unwrap();
        let key = broker
            .host_task_view(issued.token())
            .unwrap()
            .request
            .unwrap()
            .key;
        Self {
            broker,
            fake,
            token: issued.token().into(),
            key,
        }
    }

    fn begin(&self) -> HostScopePickerCall {
        self.broker
            .begin_task_scope_picker(&self.token, &self.key, super::super::SurfaceKind::Desktop)
            .unwrap()
    }

    fn no_execution(&self) {
        assert!(!self.broker.feature_enabled());
        assert_eq!(self.broker.executable_grant_count("picker-worker-app"), 0);
        assert_eq!(self.broker.continuation_count("picker-worker-app"), 0);
        assert_eq!(self.fake.observe_calls(), 0);
        assert!(self.fake.executions().is_empty());
    }
}

#[tokio::test]
async fn discovery_and_reservation_do_not_enable_or_execute_before_approval() {
    let fixture = Fixture::new();
    let workers = Arc::new(DiscoveryWorkers::default());
    let receipt = discover_with(fixture.begin(), workers.clone(), || Ok(()))
        .await
        .unwrap();
    let ids = receipt
        .views()
        .unwrap()
        .into_iter()
        .map(|row| row.candidate_id)
        .collect::<Vec<_>>();
    let reservations = Arc::new(ReservationWorkers::default());
    let AccessApprovalReady::Prepare(approval) = reserve_with(
        fixture.broker.clone(),
        fixture.token.clone(),
        receipt.select(&ids).unwrap(),
        receipt.cancel_handle(),
        reservations.clone(),
    )
    .await
    .unwrap() else {
        panic!("prepare")
    };
    fixture.no_execution();
    assert_eq!(fixture.fake.list_calls(), 1);
    drop(approval);
    workers.join().await.unwrap();
    reservations.join().await.unwrap();
    fixture.no_execution();
}

#[tokio::test]
async fn cancellation_before_discovery_never_registers_or_queries_a_descriptor() {
    let fixture = Fixture::new();
    let call = fixture.begin();
    call.cancel_handle().fence();
    let workers = Arc::new(DiscoveryWorkers::default());
    assert!(discover_with(call, workers.clone(), || panic!(
        "cancelled picker prepared a descriptor"
    ))
    .await
    .is_err());
    assert_eq!(fixture.fake.list_calls(), 0);
    fixture.no_execution();
    workers.join().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancelled_discovery_waiter_fences_immediately_and_joins_original_native_call() {
    let fixture = Fixture::new();
    let workers = Arc::new(DiscoveryWorkers::default());
    let (entered, started) = tokio::sync::oneshot::channel();
    let (release, blocked) = std::sync::mpsc::channel();
    fixture.fake.arm_liveness_hook(Box::new(move || {
        entered.send(()).unwrap();
        blocked.recv_timeout(Duration::from_secs(5)).unwrap();
    }));
    let call = fixture.begin();
    let original = workers.clone();
    let waiter = tokio::spawn(async move { discover_with(call, original, || Ok(())).await });
    tokio::time::timeout(Duration::from_secs(3), started)
        .await
        .unwrap()
        .unwrap();
    waiter.abort();
    assert!(waiter.await.err().unwrap().is_cancelled());
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
    let original = workers.clone();
    let join = tokio::spawn(async move { original.join().await });
    tokio::task::yield_now().await;
    assert!(
        !join.is_finished(),
        "blocked original query must actually return"
    );
    release.send(()).unwrap();
    join.await.unwrap().unwrap();
    fixture.no_execution();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancelled_reservation_waiter_cannot_publish_late_native_recheck() {
    let fixture = Fixture::new();
    let receipt = fixture.begin().discover().unwrap();
    let ids = receipt
        .views()
        .unwrap()
        .into_iter()
        .map(|row| row.candidate_id)
        .collect::<Vec<_>>();
    let selection = receipt.select(&ids).unwrap();
    let cancel = receipt.cancel_handle();
    let workers = Arc::new(ReservationWorkers::default());
    let (entered, started) = tokio::sync::oneshot::channel();
    let (release, blocked) = std::sync::mpsc::channel();
    fixture.fake.arm_liveness_hook(Box::new(move || {
        entered.send(()).unwrap();
        blocked.recv_timeout(Duration::from_secs(5)).unwrap();
    }));
    let original = workers.clone();
    let broker = fixture.broker.clone();
    let token = fixture.token.clone();
    let waiter =
        tokio::spawn(async move { reserve_with(broker, token, selection, cancel, original).await });
    tokio::time::timeout(Duration::from_secs(3), started)
        .await
        .unwrap()
        .unwrap();
    waiter.abort();
    assert!(waiter.await.err().unwrap().is_cancelled());
    assert!(!receipt.is_current());
    let original = workers.clone();
    let join = tokio::spawn(async move { original.join().await });
    tokio::task::yield_now().await;
    assert!(!join.is_finished());
    release.send(()).unwrap();
    join.await.unwrap().unwrap();
    fixture.no_execution();
}
