//! Host publication must stay native-free while the App pins its endpoint.
use super::resume_tests::{native_fixture, paused_address, reserve};
use super::*;

#[test]
fn resume_eligibility_is_native_free_and_only_describes_the_original_suspended_scope() {
    let f = native_fixture();
    assert!(!f.broker.host_task_view(f.token.token()).unwrap().executions[0].resume_available);
    let address = paused_address(&f);
    let queries = f.adapter.scope_metadata_calls();
    let captures = f.adapter.observe_calls();
    assert!(f.broker.host_task_view(f.token.token()).unwrap().executions[0].resume_available);
    assert_eq!(f.adapter.scope_metadata_calls(), queries);
    assert_eq!(f.adapter.observe_calls(), captures);
    let resume = reserve(&f, &address);
    assert!(!f.broker.host_task_view(f.token.token()).unwrap().executions[0].resume_available);
    drop(resume);
    assert!(f.broker.host_task_view(f.token.token()).unwrap().executions[0].resume_available);
    f.broker.set_feature_enabled(false);
    assert!(!f.broker.host_task_view(f.token.token()).unwrap().executions[0].resume_available);
}

#[test]
fn prepared_observation_does_not_publish_input_or_a_continuation() {
    let f = native_fixture();
    let address = paused_address(&f);
    let mut resume = reserve(&f, &address);
    assert!(resume.commit().is_err());
    resume.prepare_native(Duration::from_secs(1)).unwrap();
    let view = f.broker.host_task_view(f.token.token()).unwrap();
    assert!(!view.grants[0].executable);
    assert!(!view.executions[0].has_fresh_observation);
    assert_eq!(view.executions[0].phase, HostExecutionPhase::Resuming);
    assert!(f
        .broker
        .claim_continuation("synthetic-execution-app")
        .is_none());
    assert!(f.broker.observe(&address.run_id).is_err());
    assert!(matches!(
        f.broker
            .reserve_host_task_resume(f.token.token(), &address)
            .unwrap(),
        HostTaskResumeReady::InProgress
    ));
    let captures = f.adapter.observe_calls();
    assert!(resume.prepare_native(Duration::from_secs(1)).is_err());
    assert_eq!(f.adapter.observe_calls(), captures);
}

#[test]
fn commit_has_no_native_callbacks_and_publishes_exactly_once() {
    let f = native_fixture();
    let address = paused_address(&f);
    let mut resume = reserve(&f, &address);
    resume.prepare_native(Duration::from_secs(1)).unwrap();
    let queries = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    for hook in [0, 1, 2] {
        let calls = queries.clone();
        let callback = Box::new(move || {
            calls.fetch_add(1, Ordering::SeqCst);
        });
        match hook {
            0 => f.adapter.arm_liveness_hook(callback),
            1 => f.adapter.arm_scope_metadata_hook(callback),
            _ => f.adapter.arm_capture_hook(callback),
        }
    }
    let receipt = resume.commit().unwrap();
    assert_eq!(receipt.address.run_id, address.run_id);
    assert_ne!(receipt.address.generation, address.generation);
    let claim = f
        .broker
        .claim_continuation("synthetic-execution-app")
        .unwrap();
    assert!(resume.commit().is_err());
    let repeated = f
        .broker
        .claim_continuation("synthetic-execution-app")
        .unwrap();
    assert_eq!(claim.id, repeated.id);
    assert_eq!(queries.load(Ordering::SeqCst), 0);
    assert!(f.broker.host_task_view(f.token.token()).unwrap().grants[0].executable);
    f.broker.consume_continuation(&claim).unwrap();
    assert!(f
        .broker
        .claim_continuation("synthetic-execution-app")
        .is_none());
    drop(resume);
}

#[test]
fn final_stop_cancel_feature_or_transport_fence_rejects_prepared_publication() {
    for cause in ["stop", "cancel", "feature", "transport"] {
        let f = native_fixture();
        let address = paused_address(&f);
        let mut resume = reserve(&f, &address);
        let cancel = resume.cancel_handle();
        resume.prepare_native(Duration::from_secs(1)).unwrap();
        match cause {
            "stop" => f.broker.fence_task_consent("synthetic-execution-app"),
            "cancel" => cancel.fence(),
            "feature" => f.broker.set_feature_enabled(false),
            _ => f.broker.retire_transport_connection(f.token.token()),
        }
        assert!(resume.commit().is_err(), "{cause}");
        assert!(f
            .broker
            .claim_continuation("synthetic-execution-app")
            .is_none());
        drop(resume);
        assert!(f.adapter.executions().is_empty());
        assert!(!f.broker.inner.lock().runs[&address.run_id]
            .in_flight
            .load(Ordering::SeqCst));
    }
}

#[test]
fn abandoning_prepared_recovery_releases_its_lease_without_authority() {
    let f = native_fixture();
    let address = paused_address(&f);
    let mut resume = reserve(&f, &address);
    resume.prepare_native(Duration::from_secs(1)).unwrap();
    assert_eq!(f.broker.lease.holder().unwrap().run_id, address.run_id);
    drop(resume);
    assert!(f.broker.lease.holder().is_none());
    let view = f.broker.host_task_view(f.token.token()).unwrap();
    assert!(!view.grants[0].executable);
    assert_eq!(view.executions[0].phase, HostExecutionPhase::Paused);
    assert!(!view.executions[0].cleanup_pending);
    assert!(f
        .broker
        .claim_continuation("synthetic-execution-app")
        .is_none());
}
