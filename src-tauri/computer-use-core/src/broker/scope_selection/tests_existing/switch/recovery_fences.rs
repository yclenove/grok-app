//! Recovery is the original Host transaction, not a fresh raw-tab grant.
use super::*;

fn pause(f: &Fixture, run: &str) {
    let original = f.broker.claim_continuation("existing-app").unwrap();
    f.broker
        .pause_host_execution(&f.token, &io::address(f, run))
        .unwrap();
    assert_eq!(f.broker.executable_grant_count("existing-app"), 0);
    assert!(f.broker.validate_continuation(&original).is_err());
    assert!(f.broker.claim_continuation("existing-app").is_none());
}

fn no_publication(f: &Fixture) {
    assert_eq!(f.broker.executable_grant_count("existing-app"), 0);
    // The historical invalid ticket can remain in the ledger. It is not a
    // new/claimable recovery continuation and must never validate again.
    assert!(f.broker.continuation_count("existing-app") <= 1);
    assert!(f.broker.claim_continuation("existing-app").is_none());
}

pub(super) fn verify_shares_and_no_io(f: &Fixture, mode: &str) {
    if mode == "feature" {
        // Explicit feature-off retires the entire pairing/Share registry by
        // the pre-existing contract. Stop/Pause/replacement do not do this.
        assert!(f.broker.tabs().list_shared_candidates().is_empty());
        assert!(f
            .broker
            .tabs()
            .poll_extension_request(&f.origin, &f.pairing.session_key, &connection(&f.pairing),)
            .is_err());
        assert_eq!(f.desktop.observe_calls(), 0);
        assert!(f.desktop.executions().is_empty());
    } else {
        assert_eq!(f.broker.tabs().list_shared_candidates().len(), 3, "{mode}");
        f.no_io();
    }
}

fn interrupt(f: &Fixture, run: &str, mode: &str, cancel: &crate::broker::HostTaskResumeCancel) {
    match mode {
        "pause" => f
            .broker
            .pause_host_execution(&f.token, &io::address(f, run))
            .unwrap(),
        "cancel" => cancel.fence(),
        "stop" => {
            f.broker.request_stop(run).unwrap();
        }
        "agent" => f
            .broker
            .bind_agent_session("existing-app", "synthetic-successor")
            .unwrap(),
        "transport" => f.broker.rotate_connection("existing-app").unwrap(),
        "task" => {
            f.broker.begin_new_task("existing-app").unwrap();
        }
        "feature" => f.broker.set_feature_enabled(false),
        "drop" => (),
        _ => panic!("unknown interruption"),
    }
}

#[test]
fn existing_task_group_resume_rejects_changed_inactive_proof_or_foreign_active_before_capture() {
    for mode in ["inactive-proof", "foreign-active"] {
        let f = Fixture::new();
        let outcome = approve_group(&f);
        pause(&f, &outcome.run_id);
        let original = f.broker.tabs().existing_target_info("101").unwrap();
        if mode == "inactive-proof" {
            share(&f, 4, "102", "Second");
        } else {
            assert!(f.broker.tabs().release_existing_grant(&original));
            let selector = f
                .broker
                .tabs()
                .list_shared_candidates()
                .into_iter()
                .find(|(id, _, _)| id.starts_with("101@"))
                .unwrap()
                .0;
            f.broker
                .tabs()
                .grant_picker_tab("foreign", "foreign-run", &selector)
                .unwrap();
        }
        let mut resume = recovery::reserve(&f, &outcome.run_id);
        assert!(
            resume.prepare_native(Duration::from_secs(1)).is_err(),
            "{mode}"
        );
        drop(resume);
        assert_eq!(f.broker.executable_grant_count("existing-app"), 0);
        no_publication(&f);
        assert!(f.broker.is_paused(&outcome.run_id).unwrap());
        if mode == "foreign-active" {
            assert_eq!(
                f.broker.tabs().existing_target_info("101").unwrap().run_id,
                "foreign-run"
            );
            f.broker
                .tabs()
                .return_borrowed("foreign-run", "101")
                .unwrap();
        } else {
            assert_eq!(
                f.broker
                    .tabs()
                    .existing_target_info("101")
                    .unwrap()
                    .generation,
                original.generation
            );
        }
        f.no_io();
    }
}

#[test]
fn existing_task_group_prepared_resume_never_publishes_after_cancel_drop_stop_or_replacement() {
    for mode in [
        "pause",
        "cancel",
        "drop",
        "stop",
        "agent",
        "transport",
        "task",
        "feature",
    ] {
        let f = Fixture::new();
        let outcome = approve_group(&f);
        pause(&f, &outcome.run_id);
        let mut resume = recovery::reserve(&f, &outcome.run_id);
        let worker = std::thread::spawn(move || {
            resume.prepare_native(Duration::from_secs(1))?;
            Ok::<_, BrokerError>(resume)
        });
        io::complete(&f, io::poll(&f));
        let mut resume = worker.join().unwrap().unwrap();
        assert_eq!(f.broker.executable_grant_count("existing-app"), 0);
        interrupt(&f, &outcome.run_id, mode, &resume.cancel_handle());
        if mode != "drop" {
            assert!(resume.commit().is_err(), "{mode}");
        }
        drop(resume);
        assert_eq!(f.broker.executable_grant_count("existing-app"), 0, "{mode}");
        no_publication(&f);
        assert!(f.broker.tabs().existing_operations_idle(&outcome.run_id));
        verify_shares_and_no_io(&f, mode);
    }
}

#[test]
fn existing_task_group_pending_resume_rejects_late_original_capture_after_fence() {
    for mode in [
        "pause",
        "cancel",
        "stop",
        "agent",
        "transport",
        "task",
        "feature",
    ] {
        let f = Fixture::new();
        let outcome = approve_group(&f);
        pause(&f, &outcome.run_id);
        let mut resume = recovery::reserve(&f, &outcome.run_id);
        let cancel = resume.cancel_handle();
        let worker = std::thread::spawn(move || {
            resume.prepare_native(Duration::from_secs(1))?;
            resume.commit()
        });
        let original = io::poll(&f);
        interrupt(&f, &outcome.run_id, mode, &cancel);
        assert!(io::try_complete(&f, original).is_err(), "{mode}");
        assert!(worker.join().unwrap().is_err(), "{mode}");
        assert_eq!(f.broker.executable_grant_count("existing-app"), 0, "{mode}");
        no_publication(&f);
        assert!(f.broker.tabs().existing_operations_idle(&outcome.run_id));
        verify_shares_and_no_io(&f, mode);
    }
}
