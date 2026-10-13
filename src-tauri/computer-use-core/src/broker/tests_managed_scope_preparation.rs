//! Explicit synthetic Host decisions/worker witnesses, real authority path.
use super::access_approval::ApprovalTicket;
use super::*;
use crate::browser::{ManagedScopeIdentity, RecordingBrowserWorker, RecordingGotoBarrier};
use crate::fake::FakeAdapter;
use serde_json::json;

struct Fixture {
    broker: Arc<ComputerUseBroker>,
    worker: Arc<RecordingBrowserWorker>,
    root: PathBuf,
    token: String,
    request: String,
}

impl Fixture {
    fn new() -> Self {
        Self::with_backends(true, true)
    }

    fn with_backends(worker_available: bool, executor_available: bool) -> Self {
        let root = std::env::temp_dir().join(format!("cu-managed-prep-{}", Uuid::new_v4()));
        let broker = Arc::new(ComputerUseBroker::new(
            Arc::new(FakeAdapter::new()),
            BrokerOptions {
                feature_enabled: false,
                lease_path: root.join("unused.lease"),
                ..Default::default()
            },
        ));
        let worker = Arc::new(RecordingBrowserWorker::new(root.clone()));
        if worker_available {
            broker.tabs.set_worker(worker.clone());
        }
        broker.tabs.set_profile_root(root.join("profiles"));
        if executor_available {
            broker.register_managed_browser_adapter().unwrap();
        }
        let token = broker
            .issue_transport_credential("managed-app")
            .unwrap()
            .token()
            .to_owned();
        broker
            .bind_agent_session("managed-app", "synthetic-managed-agent")
            .unwrap();
        let requested = broker.submit_access(&token, json!({
            "action":"request", "purpose":"synthetic isolated task", "scopeIntent":"one blank tab",
            "surfaces":["managed-browser"], "capabilities":["observe","input"]
        })).unwrap();
        Self {
            broker,
            worker,
            root,
            token,
            request: requested["requestId"].as_str().unwrap().into(),
        }
    }

    fn approval(&self) -> AccessApproval {
        let AccessApprovalReady::Prepare(approval) = self
            .broker
            .reserve_managed_access_approval(&self.token, &self.request, 1)
            .unwrap()
        else {
            panic!("expected new resource reservation");
        };
        let ApprovalTicket::Managed(ticket) = &approval.ticket else {
            panic!("resource phase")
        };
        *self.worker.scope_witness.lock() = Some(ManagedScopeIdentity {
            owner: ticket.run_id.clone(),
            profile: ticket.profile.clone(),
            page_id: "rec-page".into(),
            worker_instance_id: Uuid::new_v4().to_string(),
            context_instance_id: Uuid::new_v4().to_string(),
        });
        approval
    }

    fn zero_authority(&self) {
        assert_eq!(self.broker.executable_grant_count("managed-app"), 0);
        assert_eq!(self.broker.continuation_count("managed-app"), 0);
        assert!(self.worker.capture_options.lock().is_empty());
        assert_eq!(*self.worker.act_calls.lock(), 0);
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.broker.fence_task_consent("managed-app");
        for ticket in self.broker.take_cleanup_tickets() {
            self.broker.finish_stop_cleanup(&ticket).unwrap();
        }
        if self.root.exists() {
            assert!(!std::fs::symlink_metadata(&self.root)
                .unwrap()
                .file_type()
                .is_symlink());
            std::fs::remove_dir_all(&self.root).unwrap();
        }
    }
}

#[test]
fn full_host_managed_key_rejects_foreign_address_before_reservation_or_native_work() {
    for field in ["connection", "task", "request", "revision"] {
        let f = Fixture::new();
        let original = f
            .broker
            .host_task_view(&f.token)
            .unwrap()
            .request
            .unwrap()
            .key;
        let mut changed = original.clone();
        match field {
            "connection" => changed.connection_id = "foreign-connection".into(),
            "task" => changed.task_id = "foreign-task".into(),
            "request" => changed.request_id = "foreign-request".into(),
            _ => changed.revision += 1,
        }
        assert!(
            f.broker
                .reserve_host_managed_access_approval(&f.token, &changed)
                .is_err(),
            "{field}"
        );
        assert_eq!(f.broker.authority_census().preparations, 0);
        assert_eq!(f.broker.test_run_count(), 0);
        assert_eq!(*f.worker.opens.lock(), 0);
        assert_eq!(*f.worker.scope_calls.lock(), 0);
        assert!(!f.broker.feature_enabled());
        f.zero_authority();
        let AccessApprovalReady::Prepare(approval) = f
            .broker
            .reserve_host_managed_access_approval(&f.token, &original)
            .unwrap()
        else {
            panic!("wrong address consumed the original decision")
        };
        assert!(approval.is_current());
        drop(approval);
    }
}

#[test]
fn managed_abort_returns_pending_and_retired_handles_cannot_clear_a_newer_retry() {
    let f = Fixture::new();
    let mut old = f.approval();
    let retired_cancel = old.cancel_handle();
    old.abort();
    assert_eq!(
        f.broker
            .host_task_view(&f.token)
            .unwrap()
            .request
            .unwrap()
            .phase,
        crate::task_consent::HostRequestPhase::Pending,
        "aborted managed selection is still shown as preparing"
    );
    let mut fresh = f.approval();
    retired_cancel.fence();
    drop(old);
    assert!(
        fresh.is_current(),
        "old abort changed the newer request state"
    );
    fresh.publish_feature().unwrap();
    fresh.prepare_execution().unwrap();
    let outcome = fresh.commit().unwrap();
    assert_eq!(f.broker.executable_grant_count("managed-app"), 1);
    assert_eq!(f.broker.continuation_count("managed-app"), 1);
    assert_eq!(*f.worker.opens.lock(), 1);
    assert_eq!(
        outcome.run_id,
        f.worker.scope_witness.lock().as_ref().unwrap().owner
    );
}

#[test]
fn model_request_readback_and_resource_reservation_have_no_native_effects() {
    let f = Fixture::new();
    for _ in 0..3 {
        f.broker.host_task_view(&f.token).unwrap();
    }
    assert!(f.broker.submit_access(&f.token, json!({
        "action":"request", "purpose":"spoof", "scopeIntent":"spoof", "surfaces":["managed-browser"],
        "capabilities":["observe"], "profile":"daily-profile"
    })).is_err());
    assert!(f
        .broker
        .reserve_access_approval(&f.token, &f.request, 1)
        .is_err());
    let mut approval = f.approval();
    assert!(approval.commit_authority().is_err());
    assert!(approval.prepare_publication().is_err());
    assert!(approval.prepare_execution().is_err());
    assert!(f
        .broker
        .reserve_managed_access_approval(&f.token, &f.request, 1)
        .is_err());
    assert!(!f.broker.feature_enabled());
    assert_eq!(f.broker.test_run_count(), 0);
    assert_eq!(*f.worker.opens.lock(), 0);
    assert_eq!(*f.worker.scope_calls.lock(), 0);
    f.zero_authority();
    drop(approval);
    assert_eq!(f.broker.authority_census().preparations, 0);
}

#[test]
fn resource_proof_binds_same_pending_run_then_publishes_once() {
    let f = Fixture::new();
    let mut approval = f.approval();
    let original = f.worker.scope_witness.lock().clone().unwrap();
    approval.publish_feature().unwrap();
    approval.prepare_execution().unwrap();
    assert!(f
        .broker
        .require_owner("managed-app", &original.owner)
        .is_err());
    assert!(f
        .broker
        .authorize_on_surface(
            "managed-app",
            &original.owner,
            SurfaceKind::ManagedBrowser,
            &original.target_id().unwrap()
        )
        .is_err());
    f.zero_authority();
    assert_eq!(*f.worker.opens.lock(), 1);
    approval.prepare_publication().unwrap();
    let outcome = approval.commit_for_transport(&f.token).unwrap();
    assert_eq!(outcome.run_id, original.owner);
    assert_eq!(outcome.binding.target_id, original.target_id().unwrap());
    assert_eq!(f.broker.executable_grant_count("managed-app"), 1);
    assert_eq!(f.broker.continuation_count("managed-app"), 1);
    assert!(approval.commit_authority().is_err());
    let AccessApprovalReady::Already(repeated) = f
        .broker
        .reserve_host_managed_access_approval(
            &f.token,
            &f.broker
                .host_task_view(&f.token)
                .unwrap()
                .request
                .unwrap()
                .key,
        )
        .unwrap()
    else {
        panic!("idempotent committed decision")
    };
    assert_eq!(repeated.run_id, original.owner);
    assert_eq!(*f.worker.opens.lock(), 1);
    let observed = crate::tools::dispatch_with_transport(
        &f.broker,
        &f.token,
        "computer_observe",
        json!({"runId":outcome.run_id,"screenshot":false}),
        None,
    );
    assert_eq!(observed["isError"], false);
    assert_eq!(f.worker.capture_options.lock().len(), 1);
}

#[test]
fn wrong_resource_proof_fails_and_retry_cannot_reuse_original_run_or_profile() {
    for wrong in ["owner", "profile", "context", "missing"] {
        let f = Fixture::new();
        let mut approval = f.approval();
        let original = f.worker.scope_witness.lock().clone().unwrap();
        match wrong {
            "owner" => f.worker.scope_witness.lock().as_mut().unwrap().owner = "foreign-run".into(),
            "profile" => {
                f.worker.scope_witness.lock().as_mut().unwrap().profile = "foreign-profile".into()
            }
            "context" => f
                .worker
                .scope_witness
                .lock()
                .as_mut()
                .unwrap()
                .context_instance_id
                .clear(),
            _ => *f.worker.scope_witness.lock() = None,
        }
        approval.publish_feature().unwrap();
        assert!(approval.prepare_execution().is_err(), "{wrong}");
        assert!(approval.commit_authority().is_err());
        f.zero_authority();
        drop(approval);
        for ticket in f.broker.take_cleanup_tickets() {
            f.broker.finish_stop_cleanup(&ticket).unwrap();
        }
        assert_eq!(
            f.broker.stop_state(&original.owner).unwrap(),
            StopState::Stopped
        );
        assert!(f.broker.tabs.managed_run_idle(&original.owner));
        assert!(!f.broker.feature_enabled());
        let retry = f.approval();
        let fresh = f.worker.scope_witness.lock().clone().unwrap();
        assert_ne!(original.owner, fresh.owner);
        assert_ne!(original.profile, fresh.profile);
        drop(retry);
    }
}

fn barrier() -> RecordingGotoBarrier {
    Arc::new((std::sync::Mutex::new(false), std::sync::Condvar::new()))
}

fn signal(gate: &RecordingGotoBarrier) {
    *gate.0.lock().unwrap() = true;
    gate.1.notify_all();
}

fn wait(gate: &RecordingGotoBarrier) {
    let (entered, result) = gate
        .1
        .wait_timeout_while(gate.0.lock().unwrap(), Duration::from_secs(4), |entered| {
            !*entered
        })
        .unwrap();
    assert!(
        *entered && !result.timed_out(),
        "original worker did not reach gate"
    );
}

#[test]
fn stop_cancel_feature_off_reconnect_and_expiry_fence_late_resource_results() {
    for phase in ["open", "proof"] {
        for terminal in ["stop", "cancel", "feature-off", "reconnect", "expiry"] {
            let f = Fixture::new();
            let mut approval = f.approval();
            let cancel = approval.cancel_handle();
            let owner = f
                .worker
                .scope_witness
                .lock()
                .as_ref()
                .unwrap()
                .owner
                .clone();
            let started = barrier();
            let release = barrier();
            if phase == "open" {
                *f.worker.open_started.lock() = Some(started.clone());
                *f.worker.open_release.lock() = Some(release.clone());
            } else {
                *f.worker.scope_started.lock() = Some(started.clone());
                *f.worker.scope_release.lock() = Some(release.clone());
            }
            approval.publish_feature().unwrap();
            let join = std::thread::spawn(move || {
                let prepared = approval.prepare_execution();
                let committed = approval.commit_authority();
                (approval, prepared, committed)
            });
            let checked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                wait(&started);
                // Original local owner is visible without querying the worker.
                let view = f.broker.host_task_view(&f.token).unwrap();
                assert_eq!(view.executions.len(), 1);
                assert_eq!(view.executions[0].run_id, owner);
                f.zero_authority();
                match terminal {
                    "stop" => f.broker.fence_task_consent("managed-app"),
                    "cancel" => cancel.fence(),
                    "feature-off" => {
                        f.broker.fence_feature_enabled(false);
                    }
                    "reconnect" => f.broker.rotate_connection("managed-app").unwrap(),
                    _ => f.broker.advance_consent_clock(Duration::from_secs(601)),
                }
                // Expiry must also hand off cleanup while physical open/query is pending.
                assert_eq!(
                    f.broker.stop_state(&owner).unwrap(),
                    StopState::StopRequested,
                    "{phase}/{terminal}"
                );
            }));
            // Always release and join the original worker, even when a red
            // authority assertion panics. Never lose it during fixture Drop.
            signal(&release);
            let (approval, prepared, committed) = join.join().unwrap();
            if let Err(panic) = checked {
                drop(approval);
                std::panic::resume_unwind(panic);
            }
            assert!(prepared.is_err(), "{phase}/{terminal}");
            assert!(committed.is_err());
            drop(approval);
            for ticket in f.broker.take_cleanup_tickets() {
                f.broker.finish_stop_cleanup(&ticket).unwrap();
            }
            assert_eq!(f.broker.stop_state(&owner).unwrap(), StopState::Stopped);
            assert!(f.broker.tabs.managed_run_idle(&owner));
            f.zero_authority();
        }
    }
}

#[test]
fn context_replacement_after_binding_cannot_publish_scope() {
    let f = Fixture::new();
    let mut approval = f.approval();
    approval.publish_feature().unwrap();
    approval.prepare_execution().unwrap();
    f.worker
        .scope_witness
        .lock()
        .as_mut()
        .unwrap()
        .context_instance_id = Uuid::new_v4().to_string();
    assert!(approval.prepare_publication().is_err());
    assert!(approval.commit_authority().is_err());
    f.zero_authority();
}

#[test]
fn single_tab_task_cannot_expand_with_legacy_new_tab_or_popup_entry_points() {
    let f = Fixture::new();
    let mut approval = f.approval();
    approval.publish_feature().unwrap();
    approval.prepare_execution().unwrap();
    let outcome = approval.commit().unwrap();
    let profile = f
        .worker
        .scope_witness
        .lock()
        .as_ref()
        .unwrap()
        .profile
        .clone();
    assert!(f
        .broker
        .browser_open_tab(
            "managed-app",
            &outcome.run_id,
            "outside-profile",
            "outside-new"
        )
        .is_err());
    assert!(f
        .broker
        .browser_popup(&outcome.run_id, &profile, "outside-popup", "unapproved-ref")
        .is_err());
    assert_eq!(*f.worker.opens.lock(), 1);
}

#[test]
fn strict_browser_inventory_never_returns_unapproved_owned_profile_metadata() {
    let f = Fixture::new();
    let mut approval = f.approval();
    approval.publish_feature().unwrap();
    approval.prepare_execution().unwrap();
    let outcome = approval.commit().unwrap();
    let outside = f
        .broker
        .tabs
        .open_managed_profile(
            "managed-app",
            &outcome.run_id,
            "unapproved-synthetic-profile",
        )
        .unwrap();
    let result = crate::tools::dispatch_with_transport(
        &f.broker,
        &f.token,
        "browser_list_tabs",
        json!({"runId":outcome.run_id}),
        None,
    );
    assert_eq!(result["isError"], false);
    let body: serde_json::Value =
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    let rows = body["tabs"].as_array().unwrap();
    assert_eq!(rows.len(), 1, "ambient profile metadata leaked");
    assert_eq!(rows[0]["tabId"], outcome.binding.target_id);
    assert_ne!(rows[0]["tabId"], outside.tab_id);
}

#[test]
fn compatibility_browser_tools_cannot_read_or_touch_an_unapproved_owned_tab() {
    let f = Fixture::new();
    let mut approval = f.approval();
    approval.publish_feature().unwrap();
    approval.prepare_execution().unwrap();
    let outcome = approval.commit().unwrap();
    let outside = f
        .broker
        .tabs
        .open_managed_profile("managed-app", &outcome.run_id, "outside-synthetic-tab")
        .unwrap();
    let opened = crate::tools::dispatch_with_transport(
        &f.broker,
        &f.token,
        "browser_open",
        json!({"runId":outcome.run_id,"tabId":outside.tab_id}),
        None,
    );
    let observed = crate::tools::dispatch_with_transport(
        &f.broker,
        &f.token,
        "browser_observe",
        json!({"runId":outcome.run_id,"tabId":outside.tab_id,"pageGeneration":1}),
        None,
    );
    let raw_observe = f
        .broker
        .observe_managed_tab(&outcome.run_id, &outside.tab_id, 1);
    let navigated = f.broker.browser_navigate(
        &outcome.run_id,
        &outside.tab_id,
        "outside-nav",
        "https://owned.test/not-approved",
    );
    assert_eq!(
        opened["isError"], true,
        "legacy open leaked unapproved URL/metadata"
    );
    assert_eq!(
        observed["isError"], true,
        "legacy observe read an unapproved page"
    );
    assert!(raw_observe.is_err());
    assert!(navigated.is_err());
    assert!(f.worker.capture_options.lock().is_empty());
    assert!(f.worker.gotos.lock().is_empty());
}

#[path = "tests_managed_scope_preparation/boundaries.rs"]
mod boundaries;

#[path = "tests_managed_scope_preparation/recovery.rs"]
mod recovery;
