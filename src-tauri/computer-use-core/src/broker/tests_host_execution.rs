//! Synthetic controls only. No App/OS permission or user desktop is touched.
use super::*;
use crate::fake::FakeAdapter;
use crate::task_consent::IssuedTransport;
use serde_json::json;

#[path = "tests_host_execution/resume_commit.rs"]
mod resume_commit_tests;
#[path = "tests_host_execution/resume.rs"]
mod resume_tests;

struct Fixture {
    broker: Arc<ComputerUseBroker>,
    adapter: Arc<FakeAdapter>,
    token: IssuedTransport,
    address: HostExecutionAddress,
}

impl Fixture {
    fn new(capabilities: &[&str]) -> Self {
        let adapter = Arc::new(FakeAdapter::new());
        adapter.set_synthetic_scope_instance(Some(1));
        let broker = Arc::new(ComputerUseBroker::new(
            adapter.clone(),
            BrokerOptions {
                feature_enabled: false,
                lease_path: std::env::temp_dir()
                    .join(format!("cu-host-execution-{}.lease", Uuid::new_v4())),
                ..BrokerOptions::default()
            },
        ));
        let token = broker
            .reserve_transport_connection("synthetic-execution-app")
            .unwrap();
        broker
            .bind_transport_agent(token.token(), "synthetic-execution-agent")
            .unwrap();
        broker.submit_access(token.token(), json!({"action":"request", "purpose":"synthetic controls", "surfaces":["desktop"], "scopeIntent":"owned fixture", "capabilities":capabilities})).unwrap();
        let key = broker
            .host_task_view(token.token())
            .unwrap()
            .request
            .unwrap()
            .key;
        broker
            .bind_synthetic_scope(&key.request_id, &adapter.fixture_id())
            .unwrap();
        let outcome = broker
            .commit_access(token.token(), &key.request_id, key.revision)
            .unwrap();
        let execution = broker
            .host_task_view(token.token())
            .unwrap()
            .executions
            .remove(0);
        let address = HostExecutionAddress {
            key,
            run_id: outcome.run_id,
            generation: execution.generation,
        };
        Self {
            broker,
            adapter,
            token,
            address,
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.broker.fence_task_consent("synthetic-execution-app");
        for ticket in self.broker.take_cleanup_tickets() {
            self.broker.finish_stop_cleanup(&ticket).unwrap();
        }
    }
}

#[test]
fn synthetic_scope_without_native_proof_does_not_offer_resume() {
    let f = Fixture::new(&["observe", "input"]);
    f.broker.pause(&f.address.run_id).unwrap();
    assert!(!f.broker.host_task_view(f.token.token()).unwrap().executions[0].resume_available);
}

#[test]
fn host_preview_uses_exact_run_without_mutating_model_observation_or_budget() {
    let f = Fixture::new(&["observe", "input"]);
    let model = f.broker.observe(&f.address.run_id).unwrap();
    let before = {
        let inner = f.broker.inner.lock();
        let run = &inner.runs[&f.address.run_id];
        (
            run.snapshot_id.clone(),
            run.observe_count,
            run.geometry_revision,
            run.element_refs.clone(),
        )
    };
    let listed = f.adapter.list_calls();
    let preview = f
        .broker
        .preview_host_execution(f.token.token(), &f.address)
        .unwrap();
    assert_ne!(preview.snapshot_id, model.snapshot_id);
    let inner = f.broker.inner.lock();
    let run = &inner.runs[&f.address.run_id];
    assert_eq!(
        before,
        (
            run.snapshot_id.clone(),
            run.observe_count,
            run.geometry_revision,
            run.element_refs.clone()
        )
    );
    assert!(f.adapter.executions().is_empty());
    assert_eq!(f.adapter.list_calls(), listed);
}

#[test]
fn host_controls_reject_every_foreign_identity_before_native_work() {
    let f = Fixture::new(&["observe"]);
    let original = &f.address;
    let mut addresses = Vec::new();
    for field in [
        "connection",
        "task",
        "request",
        "revision",
        "run",
        "generation",
        "noncanonical",
    ] {
        let mut address = original.clone();
        match field {
            "connection" => address.key.connection_id = "foreign".into(),
            "task" => address.key.task_id = "foreign".into(),
            "request" => address.key.request_id = "foreign".into(),
            "revision" => address.key.revision += 1,
            "run" => address.run_id = "foreign".into(),
            "generation" => address.generation = "18446744073709551615".into(),
            _ => address.generation.insert(0, '0'),
        }
        addresses.push(address);
    }
    f.adapter
        .arm_liveness_hook(Box::new(|| panic!("invalid control queried native target")));
    for address in addresses {
        assert!(f
            .broker
            .preview_host_execution(f.token.token(), &address)
            .is_err());
        assert!(f
            .broker
            .pause_host_execution(f.token.token(), &address)
            .is_err());
    }
    assert_eq!(f.adapter.observe_calls(), 0);
    assert!(!f.broker.is_paused(&original.run_id).unwrap());
}

#[test]
fn host_pause_is_a_generation_fence_and_old_preview_cannot_resume_it() {
    let f = Fixture::new(&["observe"]);
    f.broker
        .pause_host_execution(f.token.token(), &f.address)
        .unwrap();
    let view = f.broker.host_task_view(f.token.token()).unwrap();
    assert_eq!(view.executions[0].phase, HostExecutionPhase::Paused);
    assert_ne!(view.executions[0].generation, f.address.generation);
    assert!(view.grants[0].suspended);
    assert!(!view.grants[0].executable);
    assert!(f
        .broker
        .preview_host_execution(f.token.token(), &f.address)
        .is_err());
    assert!(f
        .broker
        .pause_host_execution(f.token.token(), &f.address)
        .is_err());
    assert_eq!(f.adapter.observe_calls(), 0);
}

#[test]
fn preview_rechecks_original_generation_after_adapter_callback() {
    let f = Fixture::new(&["observe"]);
    let broker = f.broker.clone();
    let run = f.address.run_id.clone();
    f.adapter
        .arm_liveness_hook(Box::new(move || broker.pause(&run).unwrap()));
    assert!(f
        .broker
        .preview_host_execution(f.token.token(), &f.address)
        .is_err());
    let view = f.broker.host_task_view(f.token.token()).unwrap();
    assert_eq!(view.executions[0].phase, HostExecutionPhase::Paused);
    assert!(!view.executions[0].has_fresh_observation);
}

#[test]
fn host_preview_requires_observe_capability_and_live_original_connection() {
    let f = Fixture::new(&["input"]);
    assert!(f
        .broker
        .preview_host_execution(f.token.token(), &f.address)
        .is_err());
    assert_eq!(f.adapter.observe_calls(), 0);
    f.broker.retire_transport_connection(f.token.token());
    assert!(f
        .broker
        .pause_host_execution(f.token.token(), &f.address)
        .is_err());
}

#[test]
fn business_failure_is_typed_retained_after_stop_and_not_guessed_from_trace() {
    let f = Fixture::new(&["observe", "input"]);
    let obs = f.broker.observe(&f.address.run_id).unwrap();
    // Close between admission and dispatch. This is an actual ledger outcome.
    let adapter = f.adapter.clone();
    f.adapter
        .arm_liveness_hook(Box::new(move || adapter.set_alive(false)));
    let outcome = f.broker.act(crate::broker::gates::click_req(
        &f.address.run_id,
        &f.adapter.fixture_id(),
        obs.target_generation,
        &obs.snapshot_id,
        obs.geometry_revision,
        "synthetic-rejected",
    ));
    assert_eq!(outcome.kind, OutcomeKind::Rejected);
    assert_eq!(
        f.broker.host_task_view(f.token.token()).unwrap().executions[0].last_failure,
        Some(HostExecutionFailure::ActionRejected)
    );
    f.broker
        .decide_host_task(f.token.token(), &f.address.key, HostTaskDecision::Stop)
        .unwrap();
    for ticket in f.broker.take_cleanup_tickets() {
        f.broker.finish_stop_cleanup(&ticket).unwrap();
    }
    f.broker.inner.lock().traces.clear();
    let view = f.broker.host_task_view(f.token.token()).unwrap();
    assert_eq!(view.executions[0].phase, HostExecutionPhase::Stopped);
    assert_eq!(
        view.executions[0].last_failure,
        Some(HostExecutionFailure::ActionRejected)
    );
    let json = serde_json::to_string(&view).unwrap();
    assert!(!json.contains("synthetic-rejected"));
    assert!(!json.contains(f.token.token()));
}

#[test]
fn execution_address_rejects_extra_authority_fields() {
    let f = Fixture::new(&["observe"]);
    let mut address = serde_json::to_value(&f.address).unwrap();
    address["token"] = json!("forged");
    assert!(serde_json::from_value::<HostExecutionAddress>(address).is_err());
}

#[test]
fn native_closed_fact_and_backend_are_retained_without_metadata_liveness_queries() {
    let f = Fixture::new(&["observe", "input"]);
    let before = serde_json::to_value(f.broker.host_task_view(f.token.token()).unwrap()).unwrap();
    assert_eq!(before["executions"][0]["backend"], "fake");
    f.adapter.set_alive(false);
    assert!(matches!(
        f.broker.preview_host_execution(f.token.token(), &f.address),
        Err(BrokerError::DeadTarget)
    ));
    // If metadata queries liveness, this hook would be consumed/panic.
    f.adapter
        .arm_liveness_hook(Box::new(|| panic!("metadata queried native liveness")));
    let closed = serde_json::to_value(f.broker.host_task_view(f.token.token()).unwrap()).unwrap();
    assert_eq!(closed["executions"][0]["phase"], "target_closed");
    assert_eq!(closed["executions"][0]["backend"], "fake");
    assert_eq!(closed["executions"][0]["hasFreshObservation"], false);
    // Remove the assertion hook before the fixture's explicit cleanup.
    f.adapter.arm_liveness_hook(Box::new(|| {}));
    f.broker.request_stop(&f.address.run_id).unwrap();
    f.broker.inner.lock().traces.clear();
    let stopped = serde_json::to_value(f.broker.host_task_view(f.token.token()).unwrap()).unwrap();
    assert_eq!(stopped["executions"][0]["phase"], "stopped");
    assert_eq!(stopped["executions"][0]["backend"], "fake");
}

#[test]
fn a_late_closed_witness_cannot_overwrite_a_paused_generation() {
    let f = Fixture::new(&["observe"]);
    let generation = f.address.generation.parse().unwrap();
    f.broker.pause(&f.address.run_id).unwrap();
    f.broker
        .record_closed_target(&f.address.run_id, generation, &f.adapter.fixture_id());
    let view = f.broker.host_task_view(f.token.token()).unwrap();
    assert_eq!(view.executions[0].phase, HostExecutionPhase::Paused);
    assert!(
        !f.broker.inner.lock().runs[&f.address.run_id]
            .target
            .as_ref()
            .unwrap()
            .closed
    );
}

#[test]
fn a_late_rejected_action_cannot_replace_current_generation_business_state() {
    let f = Fixture::new(&["observe", "input"]);
    let obs = f.broker.observe(&f.address.run_id).unwrap();
    let adapter = f.adapter.clone();
    let broker = f.broker.clone();
    let run = f.address.run_id.clone();
    f.adapter.arm_liveness_hook(Box::new(move || {
        adapter.arm_liveness_hook(Box::new(move || broker.pause(&run).unwrap()));
    }));
    let outcome = f.broker.act(crate::broker::gates::click_req(
        &f.address.run_id,
        &f.adapter.fixture_id(),
        obs.target_generation,
        &obs.snapshot_id,
        obs.geometry_revision,
        "synthetic-late-rejected",
    ));
    assert!(matches!(
        outcome.kind,
        OutcomeKind::Rejected | OutcomeKind::Unknown
    ));
    let view = f.broker.host_task_view(f.token.token()).unwrap();
    assert_eq!(view.executions[0].phase, HostExecutionPhase::Paused);
    assert!(view.executions[0].last_failure.is_none());
    assert!(f.broker.inner.lock().dropped_late > 0);
    assert!(f.adapter.executions().is_empty());
}

#[test]
fn pause_retires_queued_continuation_without_losing_original_scope() {
    let f = Fixture::new(&["observe", "input"]);
    let claim = f
        .broker
        .claim_continuation("synthetic-execution-app")
        .unwrap();
    f.broker.validate_continuation(&claim).unwrap();
    f.broker
        .pause_host_execution(f.token.token(), &f.address)
        .unwrap();
    assert!(f.broker.validate_continuation(&claim).is_err());
    assert!(
        f.broker
            .claim_continuation("synthetic-execution-app")
            .is_none(),
        "Pause must permanently retire the queued ticket, not leave it claimable"
    );
    let view = f.broker.host_task_view(f.token.token()).unwrap();
    assert_eq!(view.grants[0].members[0].target_id, f.adapter.fixture_id());
    assert!(view.grants[0].suspended);
    assert!(!view.grants[0].executable);
    assert!(f.broker.observe(&f.address.run_id).is_err());
    assert!(f
        .broker
        .approved_task_targets(f.token.token(), &json!({}))
        .is_err());
    assert_eq!(f.adapter.observe_calls(), 0);
    assert!(f.adapter.executions().is_empty());
}

#[test]
fn task_terminal_decisions_fence_and_clean_the_original_paused_run() {
    for terminal in ["stop", "complete", "retire", "replace", "feature_off"] {
        let f = Fixture::new(&["observe", "input"]);
        let claim = f
            .broker
            .claim_continuation("synthetic-execution-app")
            .unwrap();
        f.broker
            .pause_host_execution(f.token.token(), &f.address)
            .unwrap();
        match terminal {
            "stop" => {
                f.broker
                    .decide_host_task(f.token.token(), &f.address.key, HostTaskDecision::Stop)
                    .unwrap();
            }
            "complete" => f.broker.complete_task("synthetic-execution-app").unwrap(),
            "retire" => f.broker.retire_transport_connection(f.token.token()),
            "replace" => {
                f.broker
                    .bind_agent_session("synthetic-execution-app", "replacement-agent")
                    .unwrap();
            }
            _ => f.broker.set_feature_enabled(false),
        }
        assert_ne!(
            f.broker.stop_state(&f.address.run_id).unwrap(),
            StopState::Running,
            "{terminal} lost the original paused run"
        );
        for ticket in f.broker.take_cleanup_tickets() {
            f.broker.finish_stop_cleanup(&ticket).unwrap();
        }
        assert_eq!(
            f.broker.stop_state(&f.address.run_id).unwrap(),
            StopState::Stopped,
            "{terminal}"
        );
        assert!(
            f.broker.validate_continuation(&claim).is_err(),
            "{terminal}"
        );
        assert_eq!(
            f.broker.executable_grant_count("synthetic-execution-app"),
            0
        );
        assert!(f.broker.observe(&f.address.run_id).is_err());
        assert_eq!(f.adapter.observe_calls(), 0);
        assert!(f.adapter.executions().is_empty());
    }
}
