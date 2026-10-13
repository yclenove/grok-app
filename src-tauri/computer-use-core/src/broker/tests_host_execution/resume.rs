//! Exact owned synthetic instances; no real user permission is clicked.
use super::*;

pub(super) fn native_fixture() -> Fixture {
    native_fixture_group(false)
}

fn native_fixture_group(group: bool) -> Fixture {
    let adapter = Arc::new(FakeAdapter::new());
    adapter.set_synthetic_scope_instance(Some(1));
    adapter.set_include_png(true);
    let broker = Arc::new(ComputerUseBroker::new(
        adapter.clone(),
        BrokerOptions {
            feature_enabled: true,
            action_timeout: Duration::from_millis(200),
            lease_path: std::env::temp_dir()
                .join(format!("cu-task-resume-{}.lease", Uuid::new_v4())),
            ..BrokerOptions::default()
        },
    ));
    let token = broker
        .reserve_transport_connection("synthetic-execution-app")
        .unwrap();
    broker
        .bind_transport_agent(token.token(), "synthetic-execution-agent")
        .unwrap();
    broker.submit_access(token.token(), json!({"action":"request", "purpose":"owned task recovery", "surfaces":["desktop"], "scopeIntent":"original synthetic instance", "capabilities":["observe","input"]})).unwrap();
    let key = broker
        .host_task_view(token.token())
        .unwrap()
        .request
        .unwrap()
        .key;
    let target = adapter.fixture_id();
    let mut targets = vec![(SurfaceKind::Desktop, target.as_str())];
    if group {
        adapter.pin_synthetic_member("fake:window:inactive");
        targets.push((SurfaceKind::Desktop, "fake:window:inactive"));
    }
    let selection = broker
        .capture_task_scope_group(token.token(), &key.request_id, key.revision, &targets)
        .unwrap();
    let AccessApprovalReady::Prepare(mut approval) = broker
        .reserve_selected_access_approval(token.token(), selection)
        .unwrap()
    else {
        panic!("new approval")
    };
    approval.publish_feature().unwrap();
    approval.prepare_execution().unwrap();
    let outcome = approval.commit().unwrap();
    let execution = broker
        .host_task_view(token.token())
        .unwrap()
        .executions
        .remove(0);
    Fixture {
        broker,
        adapter,
        token,
        address: HostExecutionAddress {
            key,
            run_id: outcome.run_id,
            generation: execution.generation,
        },
    }
}

#[test]
fn one_host_resume_restores_original_scope_with_fresh_observation_and_ticket() {
    let f = native_fixture();
    let old_claim = f
        .broker
        .claim_continuation("synthetic-execution-app")
        .unwrap();
    let old = f.broker.observe(&f.address.run_id).unwrap();
    let first = f.broker.act(crate::broker::gates::click_req(
        &f.address.run_id,
        &f.adapter.fixture_id(),
        old.target_generation,
        &old.snapshot_id,
        old.geometry_revision,
        "before-pause",
    ));
    assert_eq!(first.kind, OutcomeKind::Verified);
    f.broker
        .pause_host_execution(f.token.token(), &f.address)
        .unwrap();
    assert!(f.broker.validate_continuation(&old_claim).is_err());
    let listed = f.adapter.list_calls();
    let captures = f.adapter.observe_calls();
    let execution = f
        .broker
        .host_task_view(f.token.token())
        .unwrap()
        .executions
        .remove(0);
    let address = HostExecutionAddress {
        generation: execution.generation,
        ..f.address.clone()
    };
    let HostTaskResumeReady::Prepare(resume) = f
        .broker
        .reserve_host_task_resume(f.token.token(), &address)
        .unwrap()
    else {
        panic!("new recovery")
    };
    resume.complete(Duration::from_secs(1)).unwrap();
    let view = f.broker.host_task_view(f.token.token()).unwrap();
    assert!(
        view.executions[0].has_fresh_observation,
        "one Host Continue must publish a new model observation"
    );
    assert!(view.grants[0].executable);
    assert_eq!(f.adapter.observe_calls(), captures + 1);
    assert_eq!(f.adapter.list_calls(), listed);
    assert!(f.broker.validate_continuation(&old_claim).is_err());
    let new_claim = f
        .broker
        .claim_continuation("synthetic-execution-app")
        .unwrap();
    assert_ne!(old_claim.id, new_claim.id);
    f.broker.validate_continuation(&new_claim).unwrap();
    let current = {
        let inner = f.broker.inner.lock();
        let run = &inner.runs[&f.address.run_id];
        (
            run.generation,
            run.snapshot_id.clone().unwrap(),
            run.geometry_revision,
        )
    };
    let stale = f.broker.act(crate::broker::gates::click_req(
        &f.address.run_id,
        &f.adapter.fixture_id(),
        old.target_generation,
        &old.snapshot_id,
        old.geometry_revision,
        "old-ref-after-resume",
    ));
    assert_eq!(stale.kind, OutcomeKind::Rejected);
    let second = f.broker.act(crate::broker::gates::click_req(
        &f.address.run_id,
        &f.adapter.fixture_id(),
        current.0,
        &current.1,
        current.2,
        "after-resume",
    ));
    assert_eq!(second.kind, OutcomeKind::Verified);
    assert_eq!(f.adapter.executions().len(), 2);
}

pub(super) fn paused_address(f: &Fixture) -> HostExecutionAddress {
    f.broker
        .pause_host_execution(f.token.token(), &f.address)
        .unwrap();
    let row = f
        .broker
        .host_task_view(f.token.token())
        .unwrap()
        .executions
        .remove(0);
    HostExecutionAddress {
        generation: row.generation,
        ..f.address.clone()
    }
}

pub(super) fn reserve(f: &Fixture, address: &HostExecutionAddress) -> HostTaskResume {
    let HostTaskResumeReady::Prepare(resume) = f
        .broker
        .reserve_host_task_resume(f.token.token(), address)
        .unwrap()
    else {
        panic!("new recovery")
    };
    *resume
}

#[test]
fn blocked_original_pause_cleanup_must_keep_its_desktop_lease() {
    let f = native_fixture();
    let obs = f.broker.observe(&f.address.run_id).unwrap();
    let action = f.broker.act(crate::broker::gates::click_req(
        &f.address.run_id,
        &f.adapter.fixture_id(),
        obs.target_generation,
        &obs.snapshot_id,
        obs.geometry_revision,
        "acquire-original-lease",
    ));
    assert_eq!(action.kind, OutcomeKind::Verified);
    assert_eq!(f.broker.lease.holder().unwrap().run_id, f.address.run_id);
    f.adapter.set_abort_blocked(true);
    let broker = f.broker.clone();
    let run = f.address.run_id.clone();
    let worker = std::thread::spawn(move || broker.pause(&run));
    let reached = f.adapter.wait_for_abort_call(Duration::from_secs(2));
    let _ = f.broker.stop_state(&f.address.run_id);
    let held = f.broker.lease.holder();
    let pending = f.broker.inner.lock().runs[&f.address.run_id].pause_cleanup_pending;
    f.adapter.set_abort_blocked(false);
    worker.join().unwrap().unwrap();
    assert!(reached);
    assert!(pending);
    assert_eq!(
        held.map(|holder| holder.run_id),
        Some(f.address.run_id.clone()),
        "idle alone cannot release a lease while original pause cleanup is blocked"
    );
}

#[test]
fn recovery_reservation_does_not_admit_model_work_and_double_click_shares_it() {
    let f = native_fixture();
    let address = paused_address(&f);
    let resume = reserve(&f, &address);
    assert!(matches!(
        f.broker
            .reserve_host_task_resume(f.token.token(), &address)
            .unwrap(),
        HostTaskResumeReady::InProgress
    ));
    assert!(f.broker.observe(&address.run_id).is_err());
    assert!(f
        .broker
        .begin_model_call_claiming(
            f.token.token(),
            "computer_act",
            Some(&address.run_id),
            Some(&f.adapter.fixture_id())
        )
        .is_err());
    assert_eq!(f.adapter.observe_calls(), 0);
    assert_eq!(
        f.broker.host_task_view(f.token.token()).unwrap().executions[0].phase,
        HostExecutionPhase::Resuming
    );
    resume.complete(Duration::from_secs(1)).unwrap();
    assert_eq!(f.adapter.observe_calls(), 1);
    assert_eq!(f.broker.continuation_count("synthetic-execution-app"), 2);
    assert!(f
        .broker
        .reserve_host_task_resume(f.token.token(), &address)
        .is_err());
}

#[test]
fn legacy_resume_cannot_reenable_a_suspended_task() {
    let f = native_fixture();
    let _ = paused_address(&f);
    assert!(matches!(
        f.broker.resume(&f.address.run_id),
        Err(BrokerError::TargetUnauthorized)
    ));
    assert!(f.broker.is_paused(&f.address.run_id).unwrap());
    assert_eq!(f.adapter.observe_calls(), 0);
}

#[test]
fn cancelled_waiter_only_fences_and_keeps_the_native_worker_owned() {
    let f = native_fixture();
    let address = paused_address(&f);
    let resume = reserve(&f, &address);
    let cancel = resume.cancel_handle();
    let aborts = f.adapter.abort_called();
    f.adapter.set_abort_blocked(true);
    // If cancellation invokes abort, this synchronous call would hang.
    cancel.fence();
    assert_eq!(f.adapter.abort_called(), aborts);
    f.adapter.set_abort_blocked(false);
    assert!(resume.complete(Duration::from_secs(1)).is_err());
    assert_eq!(f.adapter.observe_calls(), 0);
    assert!(f.broker.is_paused(&f.address.run_id).unwrap());
    assert!(f
        .broker
        .claim_continuation("synthetic-execution-app")
        .is_none());
    assert_eq!(
        f.broker.host_task_view(f.token.token()).unwrap().executions[0].phase,
        HostExecutionPhase::Paused
    );
}

#[test]
fn replaced_original_instance_empty_capture_and_stale_geometry_do_not_resume() {
    for failure in ["replaced", "empty", "geometry", "locked"] {
        let f = native_fixture();
        let address = paused_address(&f);
        let resume = reserve(&f, &address);
        match failure {
            "replaced" => f.adapter.set_synthetic_scope_instance(Some(2)),
            "empty" => {
                f.adapter.set_blank_image(true);
                f.adapter.set_node_count(0);
            }
            "geometry" => f.adapter.set_stale_observe_revision(true),
            _ => f.adapter.set_session_locked(true),
        }
        assert!(
            resume.complete(Duration::from_secs(1)).is_err(),
            "{failure}"
        );
        assert!(f.broker.is_paused(&f.address.run_id).unwrap(), "{failure}");
        assert!(
            f.broker
                .claim_continuation("synthetic-execution-app")
                .is_none(),
            "{failure}"
        );
        assert_eq!(
            f.broker.executable_grant_count("synthetic-execution-app"),
            0
        );
        assert!(f.adapter.executions().is_empty());
        assert!(f.broker.lease.holder().is_none(), "{failure}");
    }
}

#[test]
fn pause_stop_feature_or_retirement_during_native_revalidation_wins() {
    for decision in [
        "pause",
        "stop",
        "complete",
        "feature_off",
        "retire",
        "cancel",
    ] {
        let f = native_fixture();
        let address = paused_address(&f);
        let resume = reserve(&f, &address);
        let cancel = resume.cancel_handle();
        let broker = f.broker.clone();
        let run = f.address.run_id.clone();
        f.adapter
            .arm_liveness_hook(Box::new(move || match decision {
                "pause" => broker.pause(&run).unwrap(),
                "stop" => {
                    broker.fence_task_consent("synthetic-execution-app");
                }
                "complete" => broker.complete_task("synthetic-execution-app").unwrap(),
                "feature_off" => broker.set_feature_enabled(false),
                "retire" => broker.retire_owner("synthetic-execution-app"),
                _ => cancel.fence(),
            }));
        assert!(
            resume.complete(Duration::from_secs(1)).is_err(),
            "{decision}"
        );
        assert_eq!(f.adapter.observe_calls(), 0, "{decision}");
        assert_eq!(
            f.broker.executable_grant_count("synthetic-execution-app"),
            0,
            "{decision}"
        );
        assert!(f
            .broker
            .claim_continuation("synthetic-execution-app")
            .is_none());
        assert!(f.adapter.executions().is_empty());
    }
}

#[test]
fn pause_stop_and_feature_fence_after_capture_or_final_scope_query_cannot_publish() {
    for stage in ["capture", "final_scope"] {
        for decision in [
            "pause",
            "stop",
            "feature_off",
            "replace",
            "geometry",
            "lock",
        ] {
            let f = native_fixture();
            let address = paused_address(&f);
            let resume = reserve(&f, &address);
            let broker = f.broker.clone();
            let adapter = f.adapter.clone();
            let run = f.address.run_id.clone();
            let fence: Box<dyn FnOnce() + Send> = Box::new(move || match decision {
                "pause" => broker.pause(&run).unwrap(),
                "stop" => broker.fence_task_consent("synthetic-execution-app"),
                "feature_off" => broker.set_feature_enabled(false),
                "replace" => adapter.set_synthetic_scope_instance(Some(2)),
                "geometry" => adapter.bump_geometry(),
                _ => adapter.set_session_locked(true),
            });
            if stage == "capture" {
                f.adapter.arm_capture_hook(fence);
            } else {
                let adapter = f.adapter.clone();
                f.adapter
                    .arm_capture_hook(Box::new(move || adapter.arm_liveness_hook(fence)));
            }
            let result = resume.complete(Duration::from_secs(1));
            assert!(result.is_err(), "{stage}/{decision}");
            assert_eq!(f.adapter.observe_calls(), 1, "{stage}/{decision}");
            assert!(
                f.broker
                    .claim_continuation("synthetic-execution-app")
                    .is_none(),
                "{stage}/{decision}"
            );
            assert_eq!(
                f.broker.executable_grant_count("synthetic-execution-app"),
                0
            );
            assert!(f.adapter.executions().is_empty());
        }
    }
}

#[test]
fn inactive_scope_member_is_revalidated_instead_of_rediscovered_or_dropped() {
    let f = native_fixture_group(true);
    let address = paused_address(&f);
    let resume = reserve(&f, &address);
    f.adapter.unpin_synthetic_member("fake:window:inactive");
    let lists = f.adapter.list_calls();
    assert!(resume.complete(Duration::from_secs(1)).is_err());
    assert_eq!(f.adapter.observe_calls(), 0);
    assert_eq!(f.adapter.list_calls(), lists);
    assert_eq!(
        f.broker.host_task_view(f.token.token()).unwrap().grants[0]
            .members
            .len(),
        2
    );
    assert!(f.broker.is_paused(&f.address.run_id).unwrap());
}

#[test]
fn slow_original_cleanup_is_awaited_by_the_same_user_recovery() {
    let f = native_fixture();
    f.adapter.set_abort_blocked(true);
    let broker = f.broker.clone();
    let original = f.address.clone();
    let pause = std::thread::spawn(move || broker.pause(&original.run_id));
    assert!(f.adapter.wait_for_abort_call(Duration::from_secs(2)));
    let row = f
        .broker
        .host_task_view(f.token.token())
        .unwrap()
        .executions
        .remove(0);
    let address = HostExecutionAddress {
        generation: row.generation,
        ..f.address.clone()
    };
    let resume = reserve(&f, &address);
    let recovery = std::thread::spawn(move || resume.complete(Duration::from_secs(2)));
    assert_eq!(f.adapter.observe_calls(), 0);
    assert!(f.broker.observe(&f.address.run_id).is_err());
    f.adapter.set_abort_blocked(false);
    pause.join().unwrap().unwrap();
    recovery.join().unwrap().unwrap();
    assert_eq!(f.adapter.observe_calls(), 1);
    assert!(!f.broker.is_paused(&f.address.run_id).unwrap());
}

#[test]
fn concurrent_duplicate_and_cancel_during_capture_do_not_detach_or_publish() {
    let f = native_fixture();
    let address = paused_address(&f);
    let resume = reserve(&f, &address);
    let cancel = resume.cancel_handle();
    let (entered, started) = std::sync::mpsc::channel();
    let (release, blocked) = std::sync::mpsc::channel();
    f.adapter.arm_capture_hook(Box::new(move || {
        entered.send(()).unwrap();
        blocked.recv_timeout(Duration::from_secs(3)).unwrap();
    }));
    let worker = std::thread::spawn(move || resume.complete(Duration::from_secs(1)));
    started.recv_timeout(Duration::from_secs(2)).unwrap();
    let duplicate = f.broker.reserve_host_task_resume(f.token.token(), &address);
    let aborts = f.adapter.abort_called();
    f.adapter.set_abort_blocked(true);
    let start = Instant::now();
    cancel.fence();
    let fence_elapsed = start.elapsed();
    let aborts_after = f.adapter.abort_called();
    release.send(()).unwrap();
    f.adapter.set_abort_blocked(false);
    let result = worker.join().unwrap();
    assert!(matches!(
        duplicate.unwrap(),
        HostTaskResumeReady::InProgress
    ));
    assert!(fence_elapsed < Duration::from_millis(250));
    assert_eq!(
        aborts, aborts_after,
        "UI cancellation must not run native abort"
    );
    assert!(result.is_err());
    assert_eq!(f.adapter.observe_calls(), 1);
    assert!(f
        .broker
        .claim_continuation("synthetic-execution-app")
        .is_none());
    assert!(f.broker.is_paused(&f.address.run_id).unwrap());
    assert!(!f.broker.inner.lock().runs[&f.address.run_id]
        .in_flight
        .load(Ordering::SeqCst));
}

#[test]
fn another_chat_desktop_lease_is_not_stolen_by_recovery() {
    let f = native_fixture();
    let address = paused_address(&f);
    f.broker
        .open_run("foreign-synthetic-chat", "foreign-synthetic-run")
        .unwrap();
    f.broker
        .authorize_target("foreign-synthetic-run", &f.adapter.fixture_id())
        .unwrap();
    let obs = f.broker.observe("foreign-synthetic-run").unwrap();
    let effect = f.broker.act(crate::broker::gates::click_req(
        "foreign-synthetic-run",
        &f.adapter.fixture_id(),
        obs.target_generation,
        &obs.snapshot_id,
        obs.geometry_revision,
        "foreign-once",
    ));
    assert_eq!(effect.kind, OutcomeKind::Verified);
    let resume = reserve(&f, &address);
    let result = resume.complete(Duration::from_secs(1));
    let holder = f.broker.lease.holder().unwrap().run_id;
    let continued = f.broker.act(crate::broker::gates::click_req(
        "foreign-synthetic-run",
        &f.adapter.fixture_id(),
        obs.target_generation,
        &obs.snapshot_id,
        obs.geometry_revision,
        "foreign-still-live",
    ));
    f.broker.request_stop("foreign-synthetic-run").unwrap();
    assert!(matches!(result, Err(BrokerError::LeaseHeld { .. })));
    assert_eq!(holder, "foreign-synthetic-run");
    assert_eq!(continued.kind, OutcomeKind::Verified);
    assert_eq!(
        f.adapter.executions(),
        ["foreign-once", "foreign-still-live"]
    );
    assert!(f
        .broker
        .claim_continuation("synthetic-execution-app")
        .is_none());
    assert!(f.broker.is_paused(&f.address.run_id).unwrap());
}

#[test]
fn unknown_action_is_never_replayed_by_recovery_or_a_new_snapshot() {
    let f = native_fixture();
    let old = f.broker.observe(&f.address.run_id).unwrap();
    f.adapter.set_hang(true);
    let uncertain = f.broker.act(crate::broker::gates::click_req(
        &f.address.run_id,
        &f.adapter.fixture_id(),
        old.target_generation,
        &old.snapshot_id,
        old.geometry_revision,
        "unknown-original",
    ));
    assert_eq!(uncertain.kind, OutcomeKind::Unknown);
    assert!(f
        .adapter
        .wait_for_execution("unknown-original", Duration::from_secs(2)));
    let address = paused_address(&f);
    f.adapter.set_hang(false);
    reserve(&f, &address)
        .complete(Duration::from_secs(2))
        .unwrap();
    let current = {
        let inner = f.broker.inner.lock();
        let run = &inner.runs[&f.address.run_id];
        (
            run.generation,
            run.snapshot_id.clone().unwrap(),
            run.geometry_revision,
        )
    };
    let repeated = f.broker.act(crate::broker::gates::click_req(
        &f.address.run_id,
        &f.adapter.fixture_id(),
        current.0,
        &current.1,
        current.2,
        "unknown-original",
    ));
    assert_eq!(repeated.kind, OutcomeKind::Unknown);
    assert_eq!(f.adapter.executions(), ["unknown-original"]);
    let fresh = f.broker.act(crate::broker::gates::click_req(
        &f.address.run_id,
        &f.adapter.fixture_id(),
        current.0,
        &current.1,
        current.2,
        "new-deliberate-action",
    ));
    assert_eq!(fresh.kind, OutcomeKind::Verified);
    assert_eq!(
        f.adapter.executions(),
        ["unknown-original", "new-deliberate-action"]
    );
    let claim = f
        .broker
        .claim_continuation("synthetic-execution-app")
        .unwrap();
    f.broker.consume_continuation(&claim).unwrap();
    assert!(f.broker.consume_continuation(&claim).is_err());
    assert!(f
        .broker
        .claim_continuation("synthetic-execution-app")
        .is_none());
}

#[test]
fn exact_task_stop_is_bounded_while_original_recovery_capture_is_blocked() {
    let f = native_fixture();
    let address = paused_address(&f);
    let resume = reserve(&f, &address);
    let (entered, started) = std::sync::mpsc::channel();
    let (release, blocked) = std::sync::mpsc::channel();
    f.adapter.arm_capture_hook(Box::new(move || {
        entered.send(()).unwrap();
        blocked.recv_timeout(Duration::from_secs(3)).unwrap();
    }));
    let worker = std::thread::spawn(move || resume.complete(Duration::from_secs(1)));
    started.recv_timeout(Duration::from_secs(2)).unwrap();
    f.adapter.set_abort_blocked(true);
    let start = Instant::now();
    let receipt = f
        .broker
        .decide_host_task(f.token.token(), &address.key, HostTaskDecision::Stop);
    let elapsed = start.elapsed();
    let state = f.broker.stop_state(&address.run_id).unwrap();
    let held = f.broker.lease.holder().unwrap().run_id;
    release.send(()).unwrap();
    f.adapter.set_abort_blocked(false);
    let result = worker.join().unwrap();
    for ticket in f.broker.take_cleanup_tickets() {
        f.broker.finish_stop_cleanup(&ticket).unwrap();
    }
    assert!(receipt.is_ok());
    assert!(elapsed < Duration::from_millis(250));
    assert_eq!(state, StopState::StopRequested);
    assert_eq!(held, address.run_id);
    assert!(result.is_err());
    assert_eq!(
        f.broker.stop_state(&address.run_id).unwrap(),
        StopState::Stopped
    );
    assert!(f.broker.lease.holder().is_none());
    assert!(f
        .broker
        .claim_continuation("synthetic-execution-app")
        .is_none());
    assert!(f.adapter.executions().is_empty());
}

#[test]
fn forged_or_stale_recovery_address_is_rejected_before_any_native_query() {
    let f = native_fixture();
    let address = paused_address(&f);
    f.adapter.arm_liveness_hook(Box::new(|| {
        panic!("invalid recovery queried native identity")
    }));
    for field in [
        "connection",
        "task",
        "request",
        "revision",
        "run",
        "generation",
        "noncanonical",
    ] {
        let mut foreign = address.clone();
        match field {
            "connection" => foreign.key.connection_id = "foreign".into(),
            "task" => foreign.key.task_id = "foreign".into(),
            "request" => foreign.key.request_id = "foreign".into(),
            "revision" => foreign.key.revision += 1,
            "run" => foreign.run_id = "foreign".into(),
            "generation" => foreign.generation = f.address.generation.clone(),
            _ => foreign.generation.insert(0, '0'),
        }
        assert!(
            f.broker
                .reserve_host_task_resume(f.token.token(), &foreign)
                .is_err(),
            "{field}"
        );
    }
    assert!(f.broker.observe(&f.address.run_id).is_err());
    assert_eq!(f.adapter.observe_calls(), 0);
    f.adapter.arm_liveness_hook(Box::new(|| {}));
}

#[test]
fn semantic_recovery_is_fresh_without_claiming_visual_coordinate_authority() {
    let f = native_fixture();
    let address = paused_address(&f);
    f.adapter.set_include_png(false);
    reserve(&f, &address)
        .complete(Duration::from_secs(1))
        .unwrap();
    let view = f.broker.host_task_view(f.token.token()).unwrap();
    assert!(
        view.executions[0].has_fresh_observation,
        "semantic observations are model observations too"
    );
    let inner = f.broker.inner.lock();
    let run = &inner.runs[&address.run_id];
    assert!(!run.last_image_ok);
    assert!(!run.element_refs.is_empty());
    assert!(run.snapshot_id.is_some());
}
