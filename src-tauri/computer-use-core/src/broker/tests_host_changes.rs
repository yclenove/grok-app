//! Synthetic requests; real notification ordering; zero target discovery.
use super::*;
use crate::{
    fake::FakeAdapter,
    task_consent::{HostRequestKey, HostRequestPhase, IssuedTransport},
};
use serde_json::json;

async fn changed(changes: &HostTaskChanges, before: u64) -> u64 {
    tokio::time::timeout(Duration::from_secs(2), changes.changed_after(before))
        .await
        .expect("Host mutation did not publish within the test budget")
}

struct Fixture {
    broker: Arc<ComputerUseBroker>,
    fake: Arc<FakeAdapter>,
    token: IssuedTransport,
    key: HostRequestKey,
}

impl Fixture {
    fn new() -> Self {
        let fake = Arc::new(FakeAdapter::new());
        fake.set_synthetic_scope_instance(Some(1));
        let broker = Arc::new(ComputerUseBroker::new(
            fake.clone(),
            BrokerOptions {
                feature_enabled: false,
                lease_path: std::env::temp_dir()
                    .join(format!("cu-change-{}.lease", Uuid::new_v4())),
                ..BrokerOptions::default()
            },
        ));
        let token = broker.reserve_transport_connection("change-app").unwrap();
        broker
            .bind_transport_agent(token.token(), "synthetic-change-agent")
            .unwrap();
        broker.submit_access(token.token(), json!({"action":"request", "purpose":"synthetic change fixture", "scopeIntent":"only owned fake windows", "surfaces":["desktop"], "capabilities":["observe","input"]})).unwrap();
        let key = broker
            .host_task_view(token.token())
            .unwrap()
            .request
            .unwrap()
            .key;
        Self {
            broker,
            fake,
            token,
            key,
        }
    }

    fn view(&self) -> HostTaskView {
        self.broker.host_task_view(self.token.token()).unwrap()
    }

    fn approval(&self) -> AccessApproval {
        let selected = self
            .broker
            .capture_task_scope_selection(
                self.token.token(),
                &self.key.request_id,
                self.key.revision,
                SurfaceKind::Desktop,
                &self.fake.fixture_id(),
            )
            .unwrap();
        let AccessApprovalReady::Prepare(approval) = self
            .broker
            .reserve_selected_access_approval(self.token.token(), selected)
            .unwrap()
        else {
            panic!("prepare")
        };
        approval
    }

    fn commit(&self) -> crate::task_consent::CommitOutcome {
        let mut approval = self.approval();
        approval.publish_feature().unwrap();
        approval.prepare_execution().unwrap();
        approval.commit().unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.broker.fence_task_consent("change-app");
        for cleanup in self.broker.take_cleanup_tickets() {
            self.broker.finish_stop_cleanup(&cleanup).unwrap();
        }
    }
}

#[tokio::test]
async fn pending_without_run_wakes_host_observer_before_any_execution() {
    let broker = ComputerUseBroker::request_endpoint(BrokerOptions::default());
    let token = broker.reserve_transport_connection("change-app").unwrap();
    broker
        .bind_transport_agent(token.token(), "synthetic-change-agent")
        .unwrap();
    let changes = broker.host_task_changes();
    let before = changes.version();
    broker.submit_access(token.token(), json!({"action":"request", "purpose":"synthetic Host notification", "scopeIntent":"owned fixture", "surfaces":["desktop"], "capabilities":["observe"]})).unwrap();
    let after = tokio::time::timeout(Duration::from_millis(150), changes.changed_after(before))
        .await
        .expect("pending request was not published to the Host");
    assert_ne!(after, before);
    let view = broker.host_task_view(token.token()).unwrap();
    assert_eq!(
        view.request.unwrap().phase,
        crate::task_consent::HostRequestPhase::Pending
    );
    assert!(view.executions.is_empty());
    assert!(view.grants.is_empty());
    assert_eq!(broker.capabilities().backend_id, "request-only");
    assert!(!broker.feature_enabled());
    assert_eq!(broker.continuation_count("change-app"), 0);
}

#[tokio::test]
async fn change_before_wait_and_multiple_observers_cannot_lose_notifications() {
    let changes = HostTaskChanges::default();
    let before = changes.version();
    changes.publish();
    assert_eq!(changed(&changes, before).await, changes.version());
    let before = changes.version();
    let first = changes.changed_after(before);
    let second = changes.changed_after(before);
    let publish = async {
        tokio::task::yield_now().await;
        changes.publish();
    };
    let (first, second, _) = tokio::time::timeout(Duration::from_secs(2), async {
        tokio::join!(first, second, publish)
    })
    .await
    .unwrap();
    assert_ne!(first, before);
    assert_eq!(first, second);
}

#[tokio::test]
async fn retained_readback_does_not_generate_a_self_triggered_stream_or_discover_targets() {
    let fixture = Fixture::new();
    let changes = fixture.broker.host_task_changes();
    let before = changes.version();
    let census = fixture.broker.authority_census();
    fixture
        .fake
        .arm_liveness_hook(Box::new(|| panic!("Host metadata called liveness")));
    for _ in 0..100 {
        assert_eq!(fixture.view().request.unwrap().key, fixture.key);
    }
    assert_eq!(changes.version(), before);
    assert!(
        tokio::time::timeout(Duration::from_millis(40), changes.changed_after(before))
            .await
            .is_err()
    );
    assert_eq!(fixture.broker.authority_census(), census);
    assert_eq!(fixture.fake.list_calls(), 0);
    assert_eq!(fixture.fake.observe_calls(), 0);
    assert_eq!(fixture.broker.continuation_count("change-app"), 0);
}

#[tokio::test]
async fn no_run_decisions_selection_and_abort_publish_the_actual_phase() {
    for (decision, phase) in [
        (HostTaskDecision::Deny, HostRequestPhase::Denied),
        (HostTaskDecision::Cancel, HostRequestPhase::Cancelled),
        (HostTaskDecision::Stop, HostRequestPhase::Cancelled),
    ] {
        let fixture = Fixture::new();
        let changes = fixture.broker.host_task_changes();
        let before = changes.version();
        fixture
            .broker
            .decide_host_task(fixture.token.token(), &fixture.key, decision)
            .unwrap();
        assert_ne!(changed(&changes, before).await, before);
        assert_eq!(fixture.view().request.unwrap().phase, phase);
        assert!(fixture.view().executions.is_empty());
    }
    let fixture = Fixture::new();
    let changes = fixture.broker.host_task_changes();
    let before = changes.version();
    let picker = fixture
        .broker
        .begin_task_scope_picker(fixture.token.token(), &fixture.key, SurfaceKind::Desktop)
        .unwrap();
    assert_ne!(changed(&changes, before).await, before);
    assert_eq!(
        fixture.view().request.unwrap().phase,
        HostRequestPhase::Preparing
    );
    let before = changes.version();
    drop(picker);
    assert_ne!(changed(&changes, before).await, before);
    assert_eq!(
        fixture.view().request.unwrap().phase,
        HostRequestPhase::Pending
    );
    let mut approval = fixture.approval();
    let before = changes.version();
    approval.abort();
    assert_ne!(changed(&changes, before).await, before);
    assert_eq!(
        fixture.view().request.unwrap().phase,
        HostRequestPhase::Pending
    );
    assert!(!fixture.broker.feature_enabled());
}

#[tokio::test]
async fn first_use_preparation_commit_pause_and_stop_are_observable_without_new_authority() {
    let fixture = Fixture::new();
    let changes = fixture.broker.host_task_changes();
    let mut approval = fixture.approval();
    let before = changes.version();
    approval.publish_feature().unwrap();
    assert_ne!(changed(&changes, before).await, before);
    assert!(fixture.view().feature_enabled);
    assert!(fixture.view().grants.is_empty());
    let before = changes.version();
    approval.prepare_execution().unwrap();
    assert_ne!(changed(&changes, before).await, before);
    assert_eq!(
        fixture.view().executions[0].phase,
        HostExecutionPhase::Preparing
    );
    let before = changes.version();
    let outcome = approval.commit().unwrap();
    assert_ne!(changed(&changes, before).await, before);
    assert_eq!(
        fixture.view().executions[0].phase,
        HostExecutionPhase::AwaitingObservation
    );
    assert_eq!(fixture.view().grants.len(), 1);
    let before = changes.version();
    fixture.broker.pause(&outcome.run_id).unwrap();
    assert_ne!(changed(&changes, before).await, before);
    assert_eq!(
        fixture.view().executions[0].phase,
        HostExecutionPhase::Paused
    );
    let before = changes.version();
    let ticket = fixture.broker.fence_stop(&outcome.run_id).unwrap().unwrap();
    assert_ne!(changed(&changes, before).await, before);
    assert_eq!(
        fixture.view().executions[0].phase,
        HostExecutionPhase::StopRequested
    );
    assert!(fixture.view().executions[0].cleanup_pending);
    let before = changes.version();
    fixture.broker.finish_stop_cleanup(&ticket).unwrap();
    assert_ne!(changed(&changes, before).await, before);
    assert_eq!(
        fixture.view().executions[0].phase,
        HostExecutionPhase::Stopped
    );
    assert!(!fixture.view().executions[0].cleanup_pending);
    assert_eq!(fixture.fake.observe_calls(), 0);
    assert!(fixture.fake.executions().is_empty());
}

#[tokio::test]
async fn original_physical_admission_drop_notifies_busy_to_idle_without_stopping_the_task() {
    let fixture = Fixture::new();
    let outcome = fixture.commit();
    fixture.broker.observe(&outcome.run_id).unwrap();
    let changes = fixture.broker.host_task_changes();
    let before = changes.version();
    let guard = {
        let state = fixture.broker.inner.lock();
        let run = &state.runs[&outcome.run_id];
        run.in_flight.store(true, Ordering::SeqCst);
        InFlightClear::new(run.in_flight.clone(), &changes)
    };
    assert_ne!(changed(&changes, before).await, before);
    assert_eq!(fixture.view().executions[0].phase, HostExecutionPhase::Busy);
    let before = changes.version();
    drop(guard);
    assert_ne!(changed(&changes, before).await, before);
    assert_eq!(fixture.view().executions[0].phase, HostExecutionPhase::Idle);
    assert_eq!(fixture.broker.executable_grant_count("change-app"), 1);
    assert_eq!(fixture.broker.continuation_count("change-app"), 1);
}

#[tokio::test]
async fn expiry_completion_and_original_owner_retirement_invalidate_readback() {
    let fixture = Fixture::new();
    let changes = fixture.broker.host_task_changes();
    let before = changes.version();
    fixture
        .broker
        .advance_consent_clock(Duration::from_secs(601));
    assert_ne!(changed(&changes, before).await, before);
    assert_eq!(
        fixture.view().request.unwrap().phase,
        HostRequestPhase::Expired
    );
    let before = changes.version();
    fixture
        .broker
        .retire_transport_connection(fixture.token.token());
    assert_ne!(changed(&changes, before).await, before);
    assert!(fixture
        .broker
        .host_task_view(fixture.token.token())
        .is_err());

    let fixture = Fixture::new();
    let changes = fixture.broker.host_task_changes();
    let before = changes.version();
    fixture.broker.complete_task("change-app").unwrap();
    assert_ne!(changed(&changes, before).await, before);
    assert_eq!(
        fixture.view().request.unwrap().phase,
        HostRequestPhase::Completed
    );
}

#[test]
fn direct_commit_publishes_preparing_before_a_blocked_enable_hook() {
    let fixture = Fixture::new();
    fixture
        .broker
        .bind_synthetic_scope(&fixture.key.request_id, &fixture.fake.fixture_id())
        .unwrap();
    let changes = fixture.broker.host_task_changes();
    let before = changes.version();
    let broker = fixture.broker.clone();
    let token = fixture.token.token().to_string();
    fixture
        .broker
        .arm_commit_interrupt_before_enable(Box::new(move || {
            assert_ne!(
                changes.version(),
                before,
                "preparation was not published before native work"
            );
            assert_eq!(
                broker
                    .host_task_view(&token)
                    .unwrap()
                    .request
                    .unwrap()
                    .phase,
                HostRequestPhase::Preparing
            );
        }));
    fixture
        .broker
        .commit_access(
            fixture.token.token(),
            &fixture.key.request_id,
            fixture.key.revision,
        )
        .unwrap();
}
