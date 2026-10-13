use super::*;
use crate::broker::HostTaskResumeReady;

pub(super) fn reserve(f: &Fixture, run: &str) -> crate::broker::HostTaskResume {
    let HostTaskResumeReady::Prepare(resume) = f
        .broker
        .reserve_host_task_resume(&f.token, &io::address(f, run))
        .unwrap()
    else {
        panic!("prepare")
    };
    *resume
}

fn complete_resume(f: &Fixture, run: &str) {
    let mut resume = reserve(f, run);
    let worker = std::thread::spawn(move || {
        resume.prepare_native(Duration::from_secs(1))?;
        resume.commit()
    });
    let request = io::poll(f);
    assert_eq!(f.broker.executable_grant_count("existing-app"), 0);
    io::complete(f, request);
    worker.join().unwrap().unwrap();
}

#[test]
fn existing_task_group_current_host_pause_fences_pending_resume_without_restoring_authority() {
    let f = Fixture::new();
    let outcome = approve_group(&f);
    let old = io::observe(&f, &outcome.run_id);
    let claim = f.broker.claim_continuation("existing-app").unwrap();
    f.broker
        .pause_host_execution(&f.token, &io::address(&f, &outcome.run_id))
        .unwrap();
    let paused = io::address(&f, &outcome.run_id);
    let mut resume = reserve(&f, &outcome.run_id);
    let worker = std::thread::spawn(move || {
        resume.prepare_native(Duration::from_secs(1))?;
        resume.commit()
    });
    let original_capture = io::poll(&f);
    let current = io::address(&f, &outcome.run_id);
    assert_ne!(current.generation, paused.generation);
    assert!(f.broker.pause_host_execution(&f.token, &paused).is_err());
    f.broker.pause_host_execution(&f.token, &current).unwrap();
    assert!(io::try_complete(&f, original_capture).is_err());
    assert!(worker.join().unwrap().is_err());
    assert_eq!(f.broker.executable_grant_count("existing-app"), 0);
    assert!(f.broker.claim_continuation("existing-app").is_none());
    assert!(f.broker.validate_continuation(&claim).is_err());
    assert!(f.broker.is_paused(&outcome.run_id).unwrap());
    assert!(!f.broker.host_task_view(&f.token).unwrap().executions[0].has_fresh_observation);
    assert!(f.broker.tabs().existing_operations_idle(&outcome.run_id));
    io::stale_action(&f, &old, "old-after-interrupted-host-resume");
    f.no_io();
    // A separate explicit user Continue may recover only after the old owner joined.
    complete_resume(&f, &outcome.run_id);
    assert_eq!(f.broker.executable_grant_count("existing-app"), 1);
    assert!(f.broker.validate_continuation(&claim).is_err());
    io::stale_action(&f, &old, "old-after-second-host-resume");
    assert_eq!(f.broker.tabs().list_shared_candidates().len(), 3);
    f.no_io();
}

#[test]
fn existing_task_group_pending_action_cannot_be_cleared_by_host_resume_timeout() {
    let f = Fixture::new();
    let outcome = approve_group(&f);
    let old = io::observe(&f, &outcome.run_id);
    let (worker, dispatch) = action::start(&f, &old, "pending-pause-resume");
    assert!(matches!(
        open(&f, &outcome.run_id, "102"),
        Err(BrokerError::LeaseHeld { .. })
    ));
    f.broker
        .pause_host_execution(&f.token, &io::address(&f, &outcome.run_id))
        .unwrap();
    let mut resume = reserve(&f, &outcome.run_id);
    assert!(matches!(
        resume.prepare_native(Duration::from_millis(20)),
        Err(BrokerError::LeaseHeld { .. })
    ));
    drop(resume);
    assert!(f.broker.inner.lock().runs[&outcome.run_id].switch_cleanup_pending);
    assert!(!f.broker.tabs().existing_operations_idle(&outcome.run_id));
    assert_eq!(f.broker.executable_grant_count("existing-app"), 0);
    f.no_io();
    f.broker
        .tabs()
        .settle_existing_completion(&f.origin, &dispatch.proof)
        .unwrap();
    assert_eq!(worker.join().unwrap()["isError"], true);
    complete_resume(&f, &outcome.run_id);
    assert_eq!(f.broker.executable_grant_count("existing-app"), 1);
    io::stale_action(&f, &old, "pending-before-pause");
    f.no_io();
}

#[test]
fn existing_task_group_switch_resume_and_stop_leave_foreign_desktop_lease_unchanged() {
    let f = Fixture::new();
    f.broker
        .lease
        .try_acquire("desktop-owner", "other-instance")
        .unwrap();
    let holder = f.broker.lease.holder().unwrap();
    let outcome = approve_group(&f);
    open(&f, &outcome.run_id, "102").unwrap();
    f.broker
        .pause_host_execution(&f.token, &io::address(&f, &outcome.run_id))
        .unwrap();
    complete_resume(&f, &outcome.run_id);
    open(&f, &outcome.run_id, "101").unwrap();
    f.broker.request_stop(&outcome.run_id).unwrap();
    let after = f.broker.lease.holder().unwrap();
    assert_eq!(after.run_id, holder.run_id);
    assert_eq!(after.instance_id, holder.instance_id);
    assert_eq!(after.pid, holder.pid);
    f.broker.lease.release("desktop-owner").unwrap();
    f.no_io();
}

#[test]
fn existing_task_group_failed_switch_can_pause_and_continue_after_original_action_exits() {
    let f = Fixture::new();
    let outcome = approve_group(&f);
    let old = io::observe(&f, &outcome.run_id);
    let (worker, dispatch) = action::start(&f, &old, "failed-switch-pause");
    assert!(matches!(
        open(&f, &outcome.run_id, "102"),
        Err(BrokerError::LeaseHeld { .. })
    ));
    f.broker
        .tabs()
        .settle_existing_completion(&f.origin, &dispatch.proof)
        .unwrap();
    assert_eq!(worker.join().unwrap()["isError"], true);
    f.broker
        .pause_host_execution(&f.token, &io::address(&f, &outcome.run_id))
        .unwrap();
    assert!(f.broker.host_task_view(&f.token).unwrap().executions[0].resume_available);
    let mut resume = reserve(&f, &outcome.run_id);
    let worker = std::thread::spawn(move || {
        resume.prepare_native(Duration::from_secs(1))?;
        resume.commit()
    });
    let request = io::poll(&f);
    assert_eq!(request.tab_id, "101");
    assert_eq!(f.broker.executable_grant_count("existing-app"), 0);
    io::complete(&f, request);
    worker.join().unwrap().unwrap();
    assert_eq!(f.broker.executable_grant_count("existing-app"), 1);
    io::stale_action(&f, &old, "before-failed-switch-pause");
    open(&f, &outcome.run_id, "102").unwrap();
    f.no_io();
}

#[test]
fn existing_task_group_one_host_resume_restores_original_share_after_fresh_capture() {
    let f = Fixture::new();
    let outcome = approve_group(&f);
    let old = io::observe(&f, &outcome.run_id);
    let old_claim = f.broker.claim_continuation("existing-app").unwrap();
    f.broker
        .pause_host_execution(&f.token, &io::address(&f, &outcome.run_id))
        .unwrap();
    assert!(f.broker.validate_continuation(&old_claim).is_err());
    assert_eq!(f.broker.executable_grant_count("existing-app"), 0);
    let address = io::address(&f, &outcome.run_id);
    let HostTaskResumeReady::Prepare(mut resume) = f
        .broker
        .reserve_host_task_resume(&f.token, &address)
        .unwrap()
    else {
        panic!("prepare")
    };
    let worker = std::thread::spawn(move || {
        resume.prepare_native(Duration::from_secs(1))?;
        resume.commit()
    });
    let request = io::poll(&f);
    assert_eq!(request.tab_id, "101");
    assert_eq!(f.broker.executable_grant_count("existing-app"), 0);
    io::complete(&f, request);
    worker.join().unwrap().unwrap();
    assert!(f.broker.host_task_view(&f.token).unwrap().executions[0].has_fresh_observation);
    assert_eq!(f.broker.executable_grant_count("existing-app"), 1);
    assert!(f.broker.lease.holder().is_none());
    assert!(f.broker.validate_continuation(&old_claim).is_err());
    let new_claim = f.broker.claim_continuation("existing-app").unwrap();
    assert_ne!(new_claim.id, old_claim.id);
    f.broker.validate_continuation(&new_claim).unwrap();
    io::stale_action(&f, &old, "old-after-resume");
    open(&f, &outcome.run_id, "102").unwrap();
    assert!(f.broker.tabs().existing_target_info("101").is_none());
    f.no_io();
}

#[test]
fn existing_task_group_resume_without_capture_permission_never_grants_coordinates() {
    let f = Fixture::new();
    let outcome = approve_group(&f);
    let old = io::observe(&f, &outcome.run_id);
    f.broker
        .pause_host_execution(&f.token, &io::address(&f, &outcome.run_id))
        .unwrap();
    let mut resume = reserve(&f, &outcome.run_id);
    let worker = std::thread::spawn(move || {
        resume.prepare_native(Duration::from_secs(1))?;
        resume.commit()
    });
    let request = io::poll(&f);
    let crate::browser::extension_protocol::ExtensionCommand::Observe { screenshot, .. } =
        &request.command;
    assert!(
        !screenshot,
        "semantic recovery must not require activeTab capture"
    );
    assert_eq!(f.broker.executable_grant_count("existing-app"), 0);
    io::complete(&f, request);
    worker.join().unwrap().unwrap();
    let view = f.broker.host_task_view(&f.token).unwrap();
    assert!(view.executions[0].has_fresh_observation);
    assert!(view.grants[0].executable);
    let mut coordinate = {
        let inner = f.broker.inner.lock();
        let run = &inner.runs[&outcome.run_id];
        assert!(!run.last_image_ok);
        assert!(!run.element_refs.is_empty());
        crate::broker::gates::click_req(
            &outcome.run_id,
            "tab:101",
            run.generation,
            run.snapshot_id.as_ref().unwrap(),
            run.geometry_revision,
            "semantic-resume-coordinate-rejected",
        )
    };
    coordinate.target = crate::protocol::ActionTarget::Coord { x: 10.0, y: 10.0 };
    let rejected = crate::tools::dispatch_with_transport(
        &f.broker,
        &f.token,
        "computer_act",
        serde_json::to_value(coordinate).unwrap(),
        None,
    );
    assert_eq!(rejected["isError"], true);
    assert!(rejected["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("unsupported action Click for backend chromium-extension; not dispatched"));
    f.no_io();
    io::stale_action(&f, &old, "before-semantic-resume");
    f.no_io();
}
