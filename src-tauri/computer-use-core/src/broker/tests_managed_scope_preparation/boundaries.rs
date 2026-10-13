//! Synthetic failure/owner witnesses use the production resource transaction.
use super::*;
use crate::browser::WorkerError;

fn cleanup(f: &Fixture) {
    for ticket in f.broker.take_cleanup_tickets() {
        f.broker.finish_stop_cleanup(&ticket).unwrap();
    }
}

#[test]
fn missing_backend_fails_without_opening_a_profile_or_falling_back_to_desktop() {
    for (worker, executor) in [(false, true), (true, false)] {
        let f = Fixture::with_backends(worker, executor);
        let mut approval = f.approval();
        approval.publish_feature().unwrap();
        assert!(approval.prepare_execution().is_err());
        assert!(approval.prepare_publication().is_err());
        assert!(approval.commit_authority().is_err());
        f.zero_authority();
        assert_eq!(*f.worker.opens.lock(), 0);
        assert_eq!(*f.worker.scope_calls.lock(), 0);
        drop(approval);
        cleanup(&f);
        assert!(!f.broker.feature_enabled());
        assert!(!f.root.exists());
    }
}

#[test]
fn failed_profile_open_can_only_retry_with_a_new_original_owner_and_profile() {
    let f = Fixture::new();
    let mut approval = f.approval();
    let original = f.worker.scope_witness.lock().clone().unwrap();
    *f.worker.open_error.lock() = Some(WorkerError::not_started(
        503,
        "synthetic_open_unavailable",
        "synthetic owned profile failed before creation",
    ));
    approval.publish_feature().unwrap();
    assert!(approval.prepare_execution().is_err());
    assert!(approval.commit_authority().is_err());
    f.zero_authority();
    drop(approval);
    cleanup(&f);
    assert_eq!(
        f.broker.stop_state(&original.owner).unwrap(),
        StopState::Stopped
    );
    assert!(f.broker.tabs.managed_run_idle(&original.owner));
    assert!(!f.broker.feature_enabled());
    let mut retry = f.approval();
    let fresh = f.worker.scope_witness.lock().clone().unwrap();
    assert_ne!(original.owner, fresh.owner);
    assert_ne!(original.profile, fresh.profile);
    retry.publish_feature().unwrap();
    retry.prepare_execution().unwrap();
    let outcome = retry.commit().unwrap();
    assert_eq!(outcome.run_id, fresh.owner);
    assert_eq!(*f.worker.opens.lock(), 2);
    assert_eq!(f.broker.executable_grant_count("managed-app"), 1);
    assert_eq!(f.broker.continuation_count("managed-app"), 1);
}

#[test]
fn replacing_root_worker_slot_never_retargets_the_original_resource_owner() {
    let f = Fixture::new();
    let mut approval = f.approval();
    approval.publish_feature().unwrap();
    approval.prepare_execution().unwrap();
    let replacement = Arc::new(RecordingBrowserWorker::new(f.root.clone()));
    *replacement.scope_witness.lock() = f.worker.scope_witness.lock().clone();
    f.broker.tabs.set_worker(replacement.clone());
    approval.prepare_publication().unwrap();
    let outcome = approval.commit_for_transport(&f.token).unwrap();
    let observed = crate::tools::dispatch_with_transport(
        &f.broker,
        &f.token,
        "computer_observe",
        json!({"runId":outcome.run_id,"screenshot":false}),
        None,
    );
    assert_eq!(observed["isError"], false);
    assert_eq!(f.worker.capture_options.lock().len(), 1);
    f.broker.fence_task_consent("managed-app");
    cleanup(&f);
    // Stop closes all profiles through cancel_run, not the user-only clear API.
    assert_eq!(*f.worker.cancel_run_calls.lock(), 1);
    assert_eq!(*replacement.opens.lock(), 0);
    assert_eq!(*replacement.scope_calls.lock(), 0);
    assert_eq!(*replacement.cancel_run_calls.lock(), 0);
    assert_eq!(*replacement.clear_calls.lock(), 0);
    assert!(replacement.capture_options.lock().is_empty());
}

#[test]
fn every_precommit_boundary_rejects_fenced_original_authority() {
    for phase in ["reserved", "bound", "publication"] {
        for terminal in [
            "stop",
            "cancel",
            "feature-off",
            "reconnect",
            "expiry",
            "agent",
            "retire",
        ] {
            let f = Fixture::new();
            let mut approval = f.approval();
            let owner = f
                .worker
                .scope_witness
                .lock()
                .as_ref()
                .unwrap()
                .owner
                .clone();
            approval.publish_feature().unwrap();
            if phase != "reserved" {
                approval.prepare_execution().unwrap();
            }
            if phase == "publication" {
                approval.prepare_publication().unwrap();
            }
            match terminal {
                "stop" => f.broker.fence_task_consent("managed-app"),
                "cancel" => approval.cancel_handle().fence(),
                "feature-off" => {
                    f.broker.fence_feature_enabled(false);
                }
                "reconnect" => f.broker.rotate_connection("managed-app").unwrap(),
                "expiry" => f.broker.advance_consent_clock(Duration::from_secs(601)),
                "agent" => f
                    .broker
                    .bind_agent_session("managed-app", "replacement-agent")
                    .unwrap(),
                _ => f.broker.retire_owner("managed-app"),
            }
            assert!(approval.prepare_execution().is_err(), "{phase}/{terminal}");
            assert!(
                approval.prepare_publication().is_err(),
                "{phase}/{terminal}"
            );
            assert!(
                approval.commit_for_transport(&f.token).is_err(),
                "{phase}/{terminal}"
            );
            f.zero_authority();
            assert_eq!(*f.worker.opens.lock(), u32::from(phase != "reserved"));
            drop(approval);
            cleanup(&f);
            if phase != "reserved" {
                assert_eq!(f.broker.stop_state(&owner).unwrap(), StopState::Stopped);
                assert!(f.broker.tabs.managed_run_idle(&owner));
            }
        }
    }
}

#[test]
fn foreign_transport_cannot_commit_or_borrow_the_prepared_managed_tab() {
    let f = Fixture::new();
    let mut approval = f.approval();
    approval.publish_feature().unwrap();
    approval.prepare_execution().unwrap();
    approval.prepare_publication().unwrap();
    let foreign = f
        .broker
        .issue_transport_credential("foreign-managed-app")
        .unwrap();
    f.broker
        .bind_agent_session("foreign-managed-app", "foreign-agent")
        .unwrap();
    assert!(approval.commit_for_transport(foreign.token()).is_err());
    f.zero_authority();
    let outcome = approval.commit_for_transport(&f.token).unwrap();
    let observed = crate::tools::dispatch_with_transport(
        &f.broker,
        foreign.token(),
        "computer_observe",
        json!({"runId":outcome.run_id,"screenshot":false}),
        None,
    );
    assert_eq!(observed["isError"], true);
    assert!(f.worker.capture_options.lock().is_empty());
}
