//! Synthetic worker witnesses exercise the production task recovery contract.
use super::*;

fn paused(f: &Fixture) -> HostExecutionAddress {
    let mut approval = f.approval();
    approval.publish_feature().unwrap();
    approval.prepare_execution().unwrap();
    let outcome = approval.commit().unwrap();
    let view = f.broker.host_task_view(&f.token).unwrap();
    let address = HostExecutionAddress {
        key: view.executions[0].key.clone(),
        run_id: outcome.run_id,
        generation: view.executions[0].generation.clone(),
    };
    f.broker.pause_host_execution(&f.token, &address).unwrap();
    let view = f.broker.host_task_view(&f.token).unwrap();
    HostExecutionAddress {
        generation: view.executions[0].generation.clone(),
        ..address
    }
}

fn reserve(f: &Fixture, address: &HostExecutionAddress) -> HostTaskResume {
    let HostTaskResumeReady::Prepare(resume) = f
        .broker
        .reserve_host_task_resume(&f.token, address)
        .unwrap()
    else {
        panic!("expected one original recovery")
    };
    *resume
}

#[test]
fn managed_continue_restores_original_scope_and_observation_with_one_ticket() {
    let f = Fixture::new();
    let address = paused(&f);
    let before = f.broker.host_task_view(&f.token).unwrap();
    assert!(before.executions[0].resume_available);
    assert!(!before.grants[0].executable);
    let target = before.executions[0]
        .target
        .as_ref()
        .unwrap()
        .target_id
        .clone();
    let scope = before.grants[0].clone();
    let mut resume = reserve(&f, &address);
    assert!(matches!(
        f.broker
            .reserve_host_task_resume(&f.token, &address)
            .unwrap(),
        HostTaskResumeReady::InProgress
    ));
    resume.prepare_native(Duration::from_secs(1)).unwrap();
    let prepared = f.broker.host_task_view(&f.token).unwrap();
    assert!(!prepared.grants[0].executable);
    assert_eq!(prepared.executions[0].phase, HostExecutionPhase::Resuming);
    assert!(f.broker.claim_continuation("managed-app").is_none());
    assert_eq!(*f.worker.resume_calls.lock(), 1);
    assert_eq!(f.worker.capture_options.lock().as_slice(), &[(true, true)]);
    let receipt = resume.commit().unwrap();
    assert_ne!(receipt.address.generation, address.generation);
    assert!(resume.commit().is_err());
    drop(resume);
    let after = f.broker.host_task_view(&f.token).unwrap();
    assert!(after.grants[0].executable);
    assert!(after.executions[0].has_fresh_observation);
    assert_eq!(
        after.executions[0].target.as_ref().unwrap().target_id,
        target
    );
    assert_eq!(
        serde_json::to_value(&after.grants[0].members).unwrap(),
        serde_json::to_value(&scope.members).unwrap()
    );
    let claim = f.broker.claim_continuation("managed-app").unwrap();
    f.broker.consume_continuation(&claim).unwrap();
    assert!(f.broker.claim_continuation("managed-app").is_none());
    f.broker.observe(&address.run_id).unwrap();
    assert_eq!(f.worker.capture_options.lock().len(), 2);
    assert_eq!(*f.worker.opens.lock(), 1);
    assert_eq!(*f.worker.act_calls.lock(), 0);
}

#[test]
fn uncertain_worker_resume_remains_suspended_and_cannot_be_retried() {
    let f = Fixture::new();
    let address = paused(&f);
    *f.worker.resume_error.lock() = Some(crate::browser::WorkerError::timeout(
        "synthetic acknowledgement lost",
    ));
    let mut resume = reserve(&f, &address);
    assert!(resume.prepare_native(Duration::from_secs(1)).is_err());
    assert!(resume.commit().is_err());
    drop(resume);
    let view = f.broker.host_task_view(&f.token).unwrap();
    assert!(!view.grants[0].executable);
    assert!(!view.executions[0].resume_available);
    assert!(f.worker.capture_options.lock().is_empty());
    assert_eq!(*f.worker.resume_calls.lock(), 1);
    let current = HostExecutionAddress {
        generation: view.executions[0].generation.clone(),
        ..address
    };
    // Even a direct second Host attempt must not replay the unknown exchange.
    if let Ok(HostTaskResumeReady::Prepare(mut attempt)) =
        f.broker.reserve_host_task_resume(&f.token, &current)
    {
        assert!(attempt.prepare_native(Duration::from_millis(30)).is_err());
    }
    assert_eq!(*f.worker.resume_calls.lock(), 1);
    assert!(f.broker.claim_continuation("managed-app").is_none());
}

#[test]
fn stop_after_managed_recovery_capture_prevents_publication_and_continuation() {
    let f = Fixture::new();
    let address = paused(&f);
    let mut resume = reserve(&f, &address);
    resume.prepare_native(Duration::from_secs(1)).unwrap();
    f.broker
        .decide_host_task(&f.token, &address.key, HostTaskDecision::Stop)
        .unwrap();
    assert!(resume.commit().is_err());
    drop(resume);
    assert_eq!(f.broker.executable_grant_count("managed-app"), 0);
    assert!(f.broker.claim_continuation("managed-app").is_none());
    assert_eq!(f.worker.capture_options.lock().len(), 1);
    assert_eq!(*f.worker.act_calls.lock(), 0);
}

#[test]
fn replacement_of_original_managed_context_does_not_inherit_recovered_grant() {
    let f = Fixture::new();
    let address = paused(&f);
    f.worker
        .scope_witness
        .lock()
        .as_mut()
        .unwrap()
        .context_instance_id = Uuid::new_v4().to_string();
    let mut resume = reserve(&f, &address);
    assert!(resume.prepare_native(Duration::from_secs(1)).is_err());
    assert!(resume.commit().is_err());
    drop(resume);
    let view = f.broker.host_task_view(&f.token).unwrap();
    assert!(!view.grants[0].executable);
    assert!(!view.executions[0].has_fresh_observation);
    assert!(f.worker.capture_options.lock().is_empty());
    assert_eq!(*f.worker.resume_calls.lock(), 1);
    assert!(f.broker.claim_continuation("managed-app").is_none());
}
