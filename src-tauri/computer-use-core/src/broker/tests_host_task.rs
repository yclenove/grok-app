//! Isolated synthetic Host decisions; no real native or App permissions.
use super::*;
use crate::fake::FakeAdapter;
use crate::task_consent::{HostRequestKey, HostRequestPhase, IssuedTransport};
use serde_json::json;

struct Fixture {
    broker: Arc<ComputerUseBroker>,
    adapter: Arc<FakeAdapter>,
    token: IssuedTransport,
}

impl Fixture {
    fn new() -> Self {
        let adapter = Arc::new(FakeAdapter::new());
        adapter.set_synthetic_scope_instance(Some(1));
        let broker = Arc::new(ComputerUseBroker::new(
            adapter.clone(),
            BrokerOptions {
                feature_enabled: false,
                lease_path: std::env::temp_dir()
                    .join(format!("cu-host-view-{}.lease", Uuid::new_v4())),
                ..BrokerOptions::default()
            },
        ));
        let token = broker.reserve_transport_connection("host-app").unwrap();
        broker
            .bind_transport_agent(token.token(), "synthetic-host-agent")
            .unwrap();
        Self {
            broker,
            adapter,
            token,
        }
    }

    fn request(&self, scope: &str, caps: &[&str]) -> HostRequestKey {
        self.broker
            .submit_access(
                self.token.token(),
                json!({
                    "action": "request", "purpose": "synthetic Host request",
                    "surfaces": ["desktop"], "scopeIntent": scope, "capabilities": caps,
                }),
            )
            .unwrap();
        self.view().request.unwrap().key
    }

    fn view(&self) -> HostTaskView {
        self.broker.host_task_view(self.token.token()).unwrap()
    }

    fn approve(&self, key: &HostRequestKey) -> crate::task_consent::CommitOutcome {
        self.broker
            .bind_synthetic_scope(&key.request_id, &self.adapter.fixture_id())
            .unwrap();
        self.broker
            .commit_access(self.token.token(), &key.request_id, key.revision)
            .unwrap()
    }

    fn decide(&self, key: &HostRequestKey, decision: HostTaskDecision) -> HostTaskView {
        let receipt = self
            .broker
            .decide_host_task(self.token.token(), key, decision)
            .unwrap();
        assert_eq!(receipt.key, *key);
        assert_eq!(receipt.decision, decision);
        self.view()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.broker.fence_task_consent("host-app");
        for ticket in self.broker.take_cleanup_tickets() {
            self.broker.finish_stop_cleanup(&ticket).unwrap();
        }
    }
}

#[test]
fn host_pending_readback_has_no_run_or_execution_side_effects_or_secrets() {
    let fixture = Fixture::new();
    assert!(fixture.view().request.is_none());
    let key = fixture.request("only the owned fixture", &["observe"]);
    let before = fixture.broker.authority_census();
    fixture
        .adapter
        .arm_liveness_hook(Box::new(|| panic!("readback called native liveness")));
    for _ in 0..3 {
        let view = fixture.view();
        let request = view.request.as_ref().unwrap();
        assert_eq!(request.key, key);
        assert_eq!(request.phase, HostRequestPhase::Pending);
        assert!((1..=600_000).contains(&request.remaining_ms));
        assert!(!view.feature_enabled);
        assert!(view.grants.is_empty());
        assert!(view.executions.is_empty());
        let serialized = serde_json::to_string(&view).unwrap();
        assert!(!serialized.contains(fixture.token.token()));
        for forbidden in [
            "native",
            "continuation",
            "bearer",
            "executorGeneration",
            "snapshotId",
        ] {
            assert!(!serialized.contains(forbidden), "{serialized}");
        }
    }
    let after = fixture.broker.authority_census();
    assert_eq!(before.requests, after.requests);
    assert_eq!(before.decisions, after.decisions);
    assert_eq!(before.preparations, after.preparations);
    assert_eq!(fixture.adapter.list_calls(), 0);
    assert_eq!(fixture.adapter.observe_calls(), 0);
    assert!(fixture.adapter.executions().is_empty());
}

#[test]
fn host_readback_uses_request_only_runtime_without_enabling_an_executor() {
    let broker = ComputerUseBroker::request_endpoint(BrokerOptions::default());
    let token = broker
        .reserve_transport_connection("request-only-app")
        .unwrap();
    broker
        .bind_transport_agent(token.token(), "synthetic-agent")
        .unwrap();
    broker.submit_access(token.token(), json!({"action":"request", "purpose":"read", "surfaces":["desktop"], "scopeIntent":"fixture", "capabilities":["observe"]})).unwrap();
    let view = broker.host_task_view(token.token()).unwrap();
    assert_eq!(view.request.unwrap().phase, HostRequestPhase::Pending);
    assert!(view.executions.is_empty());
    assert_eq!(broker.capabilities().backend_id, "request-only");
    assert!(!broker.feature_enabled());
    assert!(broker.host_task_view("unknown").is_err());
}

#[test]
fn host_selection_and_approval_have_separate_typed_preparing_state() {
    let fixture = Fixture::new();
    let key = fixture.request("fixture", &["observe"]);
    let target = fixture.adapter.fixture_id();
    let begin = || {
        fixture
            .broker
            .begin_task_scope_group(
                fixture.token.token(),
                &key.request_id,
                key.revision,
                &[(SurfaceKind::Desktop, target.as_str())],
            )
            .unwrap()
    };
    let old = begin();
    assert_eq!(
        fixture.view().request.unwrap().phase,
        HostRequestPhase::Preparing
    );
    let current = begin();
    drop(old);
    assert_eq!(
        fixture.view().request.unwrap().phase,
        HostRequestPhase::Preparing
    );
    drop(current);
    assert_eq!(
        fixture.view().request.unwrap().phase,
        HostRequestPhase::Pending
    );
    fixture
        .broker
        .bind_synthetic_scope(&key.request_id, &target)
        .unwrap();
    let AccessApprovalReady::Prepare(approval) = fixture
        .broker
        .reserve_access_approval(fixture.token.token(), &key.request_id, key.revision)
        .unwrap()
    else {
        panic!("prepare")
    };
    assert_eq!(
        fixture.view().request.unwrap().phase,
        HostRequestPhase::Preparing
    );
    assert!(fixture.view().grants.is_empty());
    drop(approval);
    assert_eq!(
        fixture.view().request.unwrap().phase,
        HostRequestPhase::Pending
    );
    assert_eq!(fixture.adapter.list_calls(), 0);
    assert_eq!(fixture.adapter.observe_calls(), 0);
}

#[test]
fn host_deny_and_cancel_work_without_a_run_and_do_not_approve() {
    let fixture = Fixture::new();
    let denied = fixture.request("deny fixture", &["observe"]);
    let view = fixture.decide(&denied, HostTaskDecision::Deny);
    assert_eq!(view.request.unwrap().phase, HostRequestPhase::Denied);
    let cancelled = fixture.request("cancel fixture", &["observe"]);
    let view = fixture.decide(&cancelled, HostTaskDecision::Cancel);
    assert_eq!(view.request.unwrap().phase, HostRequestPhase::Cancelled);
    assert!(view.executions.is_empty());
    assert_eq!(fixture.broker.executable_grant_count("host-app"), 0);
    assert_eq!(fixture.broker.continuation_count("host-app"), 0);
    assert!(!view.feature_enabled);
    assert_eq!(fixture.adapter.list_calls(), 0);
}

#[test]
fn host_decisions_reject_foreign_keys_old_revisions_and_new_tasks() {
    let fixture = Fixture::new();
    let old = fixture.request("fixture", &["observe"]);
    let current = fixture.request("fixture", &["observe", "input"]);
    assert_eq!(old.request_id, current.request_id);
    assert_ne!(old.revision, current.revision);
    for decision in [
        HostTaskDecision::Deny,
        HostTaskDecision::Cancel,
        HostTaskDecision::Stop,
    ] {
        assert!(fixture
            .broker
            .decide_host_task(fixture.token.token(), &old, decision)
            .is_err());
        for key in [
            HostRequestKey {
                connection_id: "foreign".into(),
                ..current.clone()
            },
            HostRequestKey {
                task_id: "foreign".into(),
                ..current.clone()
            },
            HostRequestKey {
                request_id: "foreign".into(),
                ..current.clone()
            },
        ] {
            assert!(fixture
                .broker
                .decide_host_task(fixture.token.token(), &key, decision)
                .is_err());
        }
    }
    fixture.broker.begin_new_task("host-app").unwrap();
    let fresh = fixture.request("next fixture", &["observe"]);
    assert_ne!(fresh.task_id, current.task_id);
    assert!(fixture
        .broker
        .decide_host_task(fixture.token.token(), &current, HostTaskDecision::Stop)
        .is_err());
    assert_eq!(fixture.view().request.unwrap().key, fresh);
    assert_eq!(fixture.adapter.list_calls(), 0);
}

#[test]
fn host_stop_before_run_fences_original_selection_and_duplicate_cannot_stop_next_task() {
    let fixture = Fixture::new();
    let key = fixture.request("fixture", &["observe"]);
    let target = fixture.adapter.fixture_id();
    let selection = fixture
        .broker
        .begin_task_scope_group(
            fixture.token.token(),
            &key.request_id,
            key.revision,
            &[(SurfaceKind::Desktop, &target)],
        )
        .unwrap();
    let view = fixture.decide(&key, HostTaskDecision::Stop);
    assert!(view.task_id.is_none());
    assert!(view.executions.is_empty());
    assert!(!selection.is_current());
    assert!(selection.capture().is_err());
    fixture.decide(&key, HostTaskDecision::Stop);
    let next = fixture.request("next fixture", &["observe"]);
    assert!(fixture
        .broker
        .decide_host_task(fixture.token.token(), &key, HostTaskDecision::Stop)
        .is_err());
    assert_eq!(fixture.view().request.unwrap().key, next);
    assert_eq!(fixture.adapter.observe_calls(), 0);
}

#[test]
fn host_committed_idle_and_stop_cleanup_are_not_reported_as_running() {
    let fixture = Fixture::new();
    let key = fixture.request("fixture", &["observe"]);
    let outcome = fixture.approve(&key);
    let view = fixture.view();
    assert_eq!(view.request.unwrap().phase, HostRequestPhase::Approved);
    assert_eq!(view.grants.len(), 1);
    assert!(view.grants[0].executable);
    assert_eq!(view.executions[0].run_id, outcome.run_id);
    assert_eq!(
        view.executions[0].phase,
        HostExecutionPhase::AwaitingObservation
    );
    assert!(!view.executions[0].has_fresh_observation);
    assert_eq!(fixture.adapter.observe_calls(), 0);
    let before = fixture.adapter.list_calls();
    fixture
        .adapter
        .arm_liveness_hook(Box::new(|| panic!("view/Stop called native liveness")));
    let stopped = fixture.decide(&key, HostTaskDecision::Stop);
    assert!(stopped.grants.is_empty());
    assert_eq!(
        stopped.executions[0].phase,
        HostExecutionPhase::StopRequested
    );
    assert!(stopped.executions[0].cleanup_pending);
    assert_eq!(fixture.adapter.list_calls(), before);
    for ticket in fixture.broker.take_cleanup_tickets() {
        fixture.broker.finish_stop_cleanup(&ticket).unwrap();
    }
    let view = fixture.view();
    assert_eq!(view.executions[0].phase, HostExecutionPhase::Stopped);
    assert!(!view.executions[0].cleanup_pending);
}

#[test]
fn host_incremental_denial_keeps_original_grant_and_stop_uses_same_task() {
    let fixture = Fixture::new();
    let original = fixture.request("original fixture", &["observe"]);
    let outcome = fixture.approve(&original);
    let extra = fixture.request("extra fixture", &["observe", "input"]);
    assert_eq!(original.task_id, extra.task_id);
    let pending = fixture.view();
    assert_eq!(pending.request.unwrap().key, extra);
    assert_eq!(pending.grants[0].key, original);
    assert_eq!(pending.executions[0].key, original);
    fixture.decide(&extra, HostTaskDecision::Deny);
    assert_eq!(fixture.view().grants[0].key, original);
    assert_eq!(
        fixture
            .broker
            .execution_run(fixture.token.token(), "computer_observe")
            .unwrap(),
        outcome.run_id
    );
    fixture.decide(&original, HostTaskDecision::Stop);
    assert!(fixture.view().grants.is_empty());
}

#[test]
fn host_cancel_during_published_prepare_prevents_late_commit_and_rolls_back_first_use() {
    let fixture = Fixture::new();
    let key = fixture.request("fixture", &["observe"]);
    fixture
        .broker
        .bind_synthetic_scope(&key.request_id, &fixture.adapter.fixture_id())
        .unwrap();
    let AccessApprovalReady::Prepare(mut approval) = fixture
        .broker
        .reserve_access_approval(fixture.token.token(), &key.request_id, key.revision)
        .unwrap()
    else {
        panic!("prepare")
    };
    approval.publish_feature().unwrap();
    fixture.decide(&key, HostTaskDecision::Cancel);
    assert!(approval.prepare_execution().is_err());
    drop(approval);
    assert!(!fixture.view().feature_enabled);
    assert!(fixture.view().grants.is_empty());
    assert_eq!(fixture.broker.continuation_count("host-app"), 0);
    assert_eq!(fixture.adapter.observe_calls(), 0);
}

#[test]
fn host_expiry_and_retired_transport_cannot_be_overwritten_by_a_late_decision() {
    let fixture = Fixture::new();
    let key = fixture.request("fixture", &["observe"]);
    fixture
        .broker
        .advance_consent_clock(Duration::from_secs(601));
    assert_eq!(
        fixture.view().request.unwrap().phase,
        HostRequestPhase::Expired
    );
    assert!(fixture
        .broker
        .decide_host_task(fixture.token.token(), &key, HostTaskDecision::Deny)
        .is_err());
    fixture
        .broker
        .retire_transport_connection(fixture.token.token());
    assert!(fixture
        .broker
        .host_task_view(fixture.token.token())
        .is_err());
    assert!(fixture
        .broker
        .decide_host_task(fixture.token.token(), &key, HostTaskDecision::Stop)
        .is_err());
}

#[test]
fn host_view_does_not_adopt_legacy_runs_or_other_session_requests() {
    let fixture = Fixture::new();
    fixture.broker.set_feature_enabled(true);
    fixture
        .broker
        .open_run("host-app", "synthetic-legacy-run")
        .unwrap();
    let foreign = fixture
        .broker
        .reserve_transport_connection("foreign-app")
        .unwrap();
    fixture
        .broker
        .bind_transport_agent(foreign.token(), "synthetic-foreign-agent")
        .unwrap();
    fixture.broker.submit_access(foreign.token(), json!({"action":"request", "purpose":"foreign", "surfaces":["desktop"], "scopeIntent":"fixture", "capabilities":["observe"]})).unwrap();
    let key = fixture
        .broker
        .host_task_view(foreign.token())
        .unwrap()
        .request
        .unwrap()
        .key;
    assert!(fixture.view().request.is_none());
    assert!(fixture.view().executions.is_empty());
    assert!(fixture
        .broker
        .decide_host_task(fixture.token.token(), &key, HostTaskDecision::Stop)
        .is_err());
    assert_eq!(
        fixture.broker.stop_state("synthetic-legacy-run").unwrap(),
        StopState::Running
    );
    fixture.broker.request_stop("synthetic-legacy-run").unwrap();
}

#[test]
fn host_request_key_deserialization_rejects_extra_authority_fields() {
    let value = json!({"connectionId":"1:1:1", "taskId":"task", "requestId":"request", "revision":1, "token":"forged"});
    assert!(serde_json::from_value::<HostRequestKey>(value).is_err());
}

#[test]
fn host_prepared_run_before_commit_keeps_original_stop_and_cleanup_identity() {
    let fixture = Fixture::new();
    let key = fixture.request("fixture", &["observe"]);
    fixture
        .broker
        .bind_synthetic_scope(&key.request_id, &fixture.adapter.fixture_id())
        .unwrap();
    let AccessApprovalReady::Prepare(mut approval) = fixture
        .broker
        .reserve_access_approval(fixture.token.token(), &key.request_id, key.revision)
        .unwrap()
    else {
        panic!("prepare")
    };
    approval.publish_feature().unwrap();
    approval.prepare_execution().unwrap();
    let view = fixture.view();
    assert!(view.grants.is_empty());
    assert_eq!(view.executions.len(), 1);
    assert_eq!(view.executions[0].key, key);
    assert_eq!(view.executions[0].phase, HostExecutionPhase::Preparing);
    let stopped = fixture.decide(&key, HostTaskDecision::Stop);
    assert_eq!(stopped.executions[0].key, key);
    assert_eq!(
        stopped.executions[0].phase,
        HostExecutionPhase::StopRequested
    );
    assert!(stopped.executions[0].cleanup_pending);
    assert!(approval.commit().is_err());
    assert!(fixture.view().grants.is_empty());
    assert_eq!(fixture.broker.continuation_count("host-app"), 0);
}

#[test]
fn host_readback_is_bounded_by_authority_records_rather_than_same_task_run_history() {
    let fixture = Fixture::new();
    let original = fixture.request("fixture-0", &["observe"]);
    fixture.approve(&original);
    for index in 1..40 {
        fixture
            .broker
            .advance_consent_clock(Duration::from_secs(1801));
        for ticket in fixture.broker.take_cleanup_tickets() {
            fixture.broker.finish_stop_cleanup(&ticket).unwrap();
        }
        let key = fixture.request(&format!("fixture-{index}"), &["observe"]);
        assert_eq!(key.task_id, original.task_id);
        fixture.approve(&key);
    }
    let view = fixture.view();
    assert_eq!(view.grants.len(), 1);
    assert!(view.executions.len() <= 8);
    assert!(fixture.broker.test_run_count() >= 40);
    assert_eq!(fixture.adapter.observe_calls(), 0);
}
