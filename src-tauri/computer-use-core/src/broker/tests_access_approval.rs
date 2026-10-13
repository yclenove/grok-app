//! Staged Host transaction tests use only explicitly synthetic scope decisions.
use super::*;
use crate::fake::FakeAdapter;
use serde_json::json;

fn fixture() -> (Arc<ComputerUseBroker>, Arc<FakeAdapter>, String, String) {
    let adapter = Arc::new(FakeAdapter::new());
    let broker = Arc::new(ComputerUseBroker::new(
        adapter.clone(),
        BrokerOptions {
            feature_enabled: false,
            lease_path: std::env::temp_dir()
                .join(format!("cu-approval-{}.lease", uuid::Uuid::new_v4())),
            ..BrokerOptions::default()
        },
    ));
    let token = broker.issue_transport_credential("approval-app").unwrap();
    broker
        .bind_agent_session("approval-app", "synthetic-agent")
        .unwrap();
    let request = broker.submit_access(token.token(), json!({
        "action": "request", "purpose": "synthetic staged approval",
        "scopeIntent": "owned fake fixture", "surfaces": ["desktop"], "capabilities": ["observe"]
    })).unwrap();
    let request = request["requestId"].as_str().unwrap().to_string();
    broker
        .bind_synthetic_scope(&request, &adapter.fixture_id())
        .unwrap();
    (broker, adapter, token.token().into(), request)
}

fn reserve(broker: &Arc<ComputerUseBroker>, token: &str, request: &str) -> AccessApproval {
    match broker.reserve_access_approval(token, request, 1).unwrap() {
        AccessApprovalReady::Prepare(prep) => prep,
        AccessApprovalReady::Already(_) => panic!("first approval prepares"),
    }
}

fn cleanup(broker: &ComputerUseBroker) {
    broker.fence_task_consent("approval-app");
    for ticket in broker.take_cleanup_tickets() {
        broker.finish_stop_cleanup(&ticket).unwrap();
    }
}

#[test]
fn staged_webview_scope_uses_only_its_reserved_executor() {
    let (broker, desktop, token, _) = fixture();
    let view = Arc::new(FakeAdapter::new());
    let target = "wv:synthetic-owned-view";
    view.pin_synthetic_member(target);
    broker
        .register_surface_adapter(SurfaceKind::WebView, view.clone())
        .unwrap();
    let request = broker.submit_access(&token, json!({
        "action":"request", "purpose":"synthetic WebView reservation",
        "scopeIntent":"owned fake view", "surfaces":["app-webview"], "capabilities":["observe"]
    })).unwrap();
    let request = request["requestId"].as_str().unwrap();
    broker
        .bind_synthetic_scope_on_surface(request, target, SurfaceKind::WebView)
        .unwrap();
    let mut prep = reserve(&broker, &token, request);
    assert_eq!(prep.surface(), SurfaceKind::WebView);
    prep.publish_feature().unwrap();
    prep.prepare_execution().unwrap();
    assert_eq!(broker.executable_grant_count("approval-app"), 0);
    let outcome = prep.commit().unwrap();
    let observed = crate::tools::dispatch_with_transport(
        &broker,
        &token,
        "computer_observe",
        json!({"runId":outcome.run_id,"targetId":target,"screenshot":false}),
        None,
    );
    assert_eq!(observed["isError"], false, "{observed}");
    assert_eq!(view.observe_calls(), 1);
    assert_eq!(desktop.observe_calls(), 0);
    assert_eq!(desktop.list_calls(), 0);
    cleanup(&broker);
}

#[test]
fn missing_reserved_webview_never_falls_back_to_desktop() {
    let (broker, desktop, token, _) = fixture();
    let target = "wv:synthetic-missing-view";
    desktop.pin_synthetic_member(target);
    let request = broker.submit_access(&token, json!({
        "action":"request", "purpose":"synthetic missing WebView",
        "scopeIntent":"missing fake view", "surfaces":["app-webview"], "capabilities":["observe"]
    })).unwrap();
    let request = request["requestId"].as_str().unwrap();
    broker
        .bind_synthetic_scope_on_surface(request, target, SurfaceKind::WebView)
        .unwrap();
    let mut prep = reserve(&broker, &token, request);
    prep.publish_feature().unwrap();
    assert!(matches!(
        prep.prepare_execution(),
        Err(BrokerError::SurfaceUnavailable {
            surface: SurfaceKind::WebView
        })
    ));
    prep.abort();
    assert_eq!(desktop.list_calls(), 0);
    assert_eq!(desktop.observe_calls(), 0);
    assert_eq!(broker.executable_grant_count("approval-app"), 0);
    assert_eq!(broker.continuation_count("approval-app"), 0);
    assert!(!broker.feature_enabled());
    cleanup(&broker);
}

fn retry_failed_binding(cleanup_before_retry: bool) {
    let (broker, adapter, token, request) = fixture();
    let mut first = reserve(&broker, &token, &request);
    let first_cancel = first.cancel_handle();
    first.publish_feature().unwrap();
    adapter.set_listing_error(true);
    assert!(first.prepare_execution().is_err());
    let aborted = first.abort();
    assert!(aborted.opened_run);
    assert!(!broker.feature_enabled());
    assert_eq!(broker.executable_grant_count("approval-app"), 0);
    assert_eq!(broker.continuation_count("approval-app"), 0);
    assert_eq!(adapter.observe_calls(), 0);
    let original_cleanup = broker.take_cleanup_tickets();
    assert_eq!(original_cleanup.len(), 1);
    assert_eq!(original_cleanup[0].run_id, aborted.run_id);
    if cleanup_before_retry {
        broker.finish_stop_cleanup(&original_cleanup[0]).unwrap();
    }

    // A new, explicit Host attempt retries the same request and revision.
    adapter.set_listing_error(false);
    let mut retry = reserve(&broker, &token, &request);
    retry.publish_feature().unwrap();
    retry
        .prepare_execution()
        .expect("same request must recover");
    let committed = retry.commit().unwrap();
    assert_ne!(committed.run_id, aborted.run_id);
    let observe = || {
        let result = crate::tools::dispatch_with_transport(
            &broker,
            &token,
            "computer_observe",
            json!({"runId":committed.run_id,"screenshot":false}),
            None,
        );
        assert_eq!(result["isError"], false, "{result}");
    };
    observe();
    // Late callbacks retain the original attempt's owner, even after commit.
    first_cancel.fence();
    assert!(first.abort().feature.is_none());
    broker.finish_stop_cleanup(&original_cleanup[0]).unwrap();
    assert!(broker.feature_enabled());
    assert_eq!(
        broker.stop_state(&committed.run_id).unwrap(),
        StopState::Running
    );
    observe();
    assert_eq!(adapter.observe_calls(), 2);
    assert!(adapter.executions().is_empty());
    assert_eq!(broker.executable_grant_count("approval-app"), 1);
    let repeated = broker.reserve_access_approval(&token, &request, 1).unwrap();
    let AccessApprovalReady::Already(repeated) = repeated else {
        panic!("committed retry must not allocate another attempt")
    };
    assert_eq!(repeated.run_id, committed.run_id);
    assert_eq!(repeated.continuation_id, committed.continuation_id);
    assert_eq!(broker.continuation_count("approval-app"), 1);
    cleanup(&broker);
}

#[test]
fn failed_binding_can_retry_same_request_after_original_cleanup() {
    retry_failed_binding(true);
}

#[test]
fn failed_binding_retry_survives_late_original_cleanup_and_cancel() {
    retry_failed_binding(false);
}

#[test]
fn stop_during_reserved_webview_binding_never_publishes_a_grant() {
    let (broker, desktop, token, _) = fixture();
    let view = Arc::new(FakeAdapter::new());
    let target = "wv:synthetic-stopped-view";
    view.pin_synthetic_member(target);
    broker
        .register_surface_adapter(SurfaceKind::WebView, view.clone())
        .unwrap();
    let request = broker.submit_access(&token, json!({
        "action":"request", "purpose":"synthetic stopped WebView",
        "scopeIntent":"owned fake view", "surfaces":["app-webview"], "capabilities":["observe"]
    })).unwrap();
    let request = request["requestId"].as_str().unwrap();
    broker
        .bind_synthetic_scope_on_surface(request, target, SurfaceKind::WebView)
        .unwrap();
    let mut prep = reserve(&broker, &token, request);
    prep.publish_feature().unwrap();
    let cancel = prep.cancel_handle();
    view.arm_liveness_hook(Box::new(move || cancel.fence()));
    assert!(prep.prepare_execution().is_err());
    prep.abort();
    assert_eq!(view.observe_calls(), 0);
    assert_eq!(desktop.list_calls(), 0);
    assert_eq!(broker.executable_grant_count("approval-app"), 0);
    assert_eq!(broker.continuation_count("approval-app"), 0);
    cleanup(&broker);
}

#[test]
fn publication_and_native_preparation_never_admit_execution_before_final_commit() {
    let (broker, adapter, token, request) = fixture();
    let mut prep = reserve(&broker, &token, &request);
    assert!(!prep.initially_enabled());
    prep.publish_feature().unwrap();
    assert_eq!(broker.executable_grant_count("approval-app"), 0);
    prep.prepare_execution().unwrap();
    assert!(broker.begin_model_call(&token, "computer_observe").is_err());
    assert_eq!(broker.continuation_count("approval-app"), 0);
    assert_eq!(adapter.observe_calls(), 0);
    let committed = prep.commit().unwrap();
    assert!(broker.begin_model_call(&token, "computer_observe").is_ok());
    assert_eq!(broker.continuation_count("approval-app"), 1);
    let repeated = broker.reserve_access_approval(&token, &request, 1).unwrap();
    let AccessApprovalReady::Already(repeated) = repeated else {
        panic!("duplicate must reuse commit")
    };
    assert_eq!(repeated.continuation_id, committed.continuation_id);
    assert_eq!(broker.continuation_count("approval-app"), 1);
    cleanup(&broker);
}

#[test]
fn same_id_reauthorization_after_preparation_cannot_commit_the_old_decision() {
    let (broker, adapter, token, request) = fixture();
    let mut prep = reserve(&broker, &token, &request);
    prep.publish_feature().unwrap();
    prep.prepare_execution().unwrap();
    let run = broker.inner.lock().runs.keys().next().unwrap().clone();
    let before = broker.inner.lock().runs[&run]
        .target
        .as_ref()
        .unwrap()
        .target_generation;
    broker
        .authorize_desktop_window(&run, &adapter.fixture_id())
        .unwrap();
    assert!(
        broker.inner.lock().runs[&run]
            .target
            .as_ref()
            .unwrap()
            .target_generation
            > before
    );
    assert!(
        prep.commit().is_err(),
        "new binding inherited the older staged decision"
    );
    assert_eq!(broker.executable_grant_count("approval-app"), 0);
    assert_eq!(broker.continuation_count("approval-app"), 0);
    assert_eq!(adapter.observe_calls(), 0);
    assert!(adapter.executions().is_empty());
    cleanup(&broker);
}

#[test]
fn cancellation_in_open_gap_retains_original_cleanup_and_rolls_epoch_forward() {
    let (broker, adapter, token, request) = fixture();
    let mut prep = reserve(&broker, &token, &request);
    let epoch = broker.feature_epoch();
    prep.publish_feature().unwrap();
    let cancel = prep.cancel_handle();
    broker.arm_commit_interrupt_after_open(Box::new(move || cancel.fence()));
    assert!(prep.prepare_execution().is_err());
    let effect = prep.abort();
    assert!(effect.opened_run);
    assert!(!broker.feature_enabled());
    assert!(broker.feature_epoch() > epoch);
    assert_ne!(
        broker.stop_state(&effect.run_id).unwrap(),
        StopState::Running
    );
    assert_eq!(broker.executable_grant_count("approval-app"), 0);
    assert_eq!(broker.continuation_count("approval-app"), 0);
    assert_eq!(adapter.observe_calls(), 0);
    assert!(adapter.executions().is_empty());
    cleanup(&broker);
}

#[test]
fn explicit_enable_owns_preference_and_old_preparation_cannot_roll_it_back() {
    let (broker, _, token, request) = fixture();
    let mut prep = reserve(&broker, &token, &request);
    prep.publish_feature().unwrap();
    broker.supersede_access_approvals_for_explicit_enable();
    broker.set_feature_enabled(true);
    assert!(!prep.is_current());
    assert!(prep.abort().feature.is_none());
    assert!(broker.feature_enabled());
    assert_eq!(broker.executable_grant_count("approval-app"), 0);
}

#[test]
fn another_session_committing_takes_feature_ownership_from_an_unfinished_first_use() {
    let (broker, adapter, token, request) = fixture();
    let mut first = reserve(&broker, &token, &request);
    first.publish_feature().unwrap();
    let second_token = broker
        .issue_transport_credential("approval-second")
        .unwrap();
    broker
        .bind_agent_session("approval-second", "synthetic-second")
        .unwrap();
    let second = broker.submit_access(second_token.token(), json!({
        "action": "request", "purpose": "second synthetic approval",
        "scopeIntent": "owned fake fixture", "surfaces": ["desktop"], "capabilities": ["observe"]
    })).unwrap();
    let second_request = second["requestId"].as_str().unwrap();
    broker
        .bind_synthetic_scope(second_request, &adapter.fixture_id())
        .unwrap();
    let AccessApprovalReady::Prepare(mut second) = broker
        .reserve_access_approval(second_token.token(), second_request, 1)
        .unwrap()
    else {
        panic!("second session prepares")
    };
    second.publish_feature().unwrap();
    second.prepare_execution().unwrap();
    second.commit().unwrap();
    let effect = first.abort();
    assert!(
        effect.feature.is_none(),
        "old first-use rollback revoked another session's committed grant"
    );
    assert!(broker.feature_enabled());
    assert!(broker
        .begin_model_call(second_token.token(), "computer_observe")
        .is_ok());
    broker.fence_task_consent("approval-second");
    cleanup(&broker);
}

#[test]
fn skipping_publication_is_rejected_and_drop_aborts_the_reservation() {
    let (broker, adapter, token, request) = fixture();
    let mut prep = reserve(&broker, &token, &request);
    assert!(prep.prepare_execution().is_err());
    assert!(prep.commit().is_err());
    assert!(!broker.feature_enabled());
    assert_eq!(broker.reservation_count(), 0);
    assert_eq!(adapter.observe_calls(), 0);
}
