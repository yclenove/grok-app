//! Receipt/transaction tests use explicit synthetic witnesses, never native claims.
use super::*;
use crate::fake::FakeAdapter;
use crate::task_consent::SYNTHETIC_DECISION_CLASS;
use serde_json::json;

fn fixture() -> (Arc<ComputerUseBroker>, Arc<FakeAdapter>, String, String) {
    let adapter = Arc::new(FakeAdapter::new());
    let broker = Arc::new(ComputerUseBroker::new(
        adapter.clone(),
        BrokerOptions {
            feature_enabled: false,
            lease_path: std::env::temp_dir().join(format!("cu-scope-{}.lease", Uuid::new_v4())),
            ..BrokerOptions::default()
        },
    ));
    let token = broker.issue_transport_credential("scope-app").unwrap();
    broker
        .bind_agent_session("scope-app", "synthetic-agent")
        .unwrap();
    let request = broker.submit_access(token.token(), json!({
        "action":"request", "purpose":"synthetic selected instance",
        "scopeIntent":"owned synthetic window", "surfaces":["desktop"], "capabilities":["observe"]
    })).unwrap();
    (
        broker,
        adapter,
        token.token().into(),
        request["requestId"].as_str().unwrap().into(),
    )
}

fn select(
    broker: &Arc<ComputerUseBroker>,
    adapter: &FakeAdapter,
    token: &str,
    request: &str,
) -> HostScopeSelection {
    broker
        .capture_task_scope_selection(
            token,
            request,
            1,
            SurfaceKind::Desktop,
            &adapter.fixture_id(),
        )
        .unwrap()
}

fn observe(broker: &ComputerUseBroker, token: &str, run: &str) -> serde_json::Value {
    crate::tools::dispatch_with_transport(
        broker,
        token,
        "computer_observe",
        json!({"runId":run,"screenshot":false}),
        None,
    )
}

fn finish(broker: &ComputerUseBroker) {
    broker.fence_task_consent("scope-app");
    for ticket in broker.take_cleanup_tickets() {
        broker.finish_stop_cleanup(&ticket).unwrap();
    }
}

#[test]
fn cancelled_selection_is_fenced_before_its_first_identity_query() {
    let (broker, adapter, token, request) = fixture();
    adapter.set_synthetic_scope_instance(Some(1));
    adapter.arm_liveness_hook(Box::new(|| {
        panic!("cancelled picker queried native identity")
    }));
    let call = broker
        .begin_task_scope_group(
            &token,
            &request,
            1,
            &[(SurfaceKind::Desktop, &adapter.fixture_id())],
        )
        .unwrap();
    call.cancel_handle().fence();
    assert!(call.capture().is_err());
    assert!(!broker.feature_enabled());
    assert_eq!(broker.test_run_count(), 0);
    assert_eq!(adapter.observe_calls(), 0);
}

#[test]
fn cancellation_during_first_identity_stops_later_group_members() {
    let (broker, adapter, token, request) = fixture();
    adapter.set_synthetic_scope_instance(Some(1));
    let first = adapter.fixture_id();
    let second = "fixture:synthetic-second";
    adapter.pin_synthetic_member(second);
    let call = broker
        .begin_task_scope_group(
            &token,
            &request,
            1,
            &[
                (SurfaceKind::Desktop, &first),
                (SurfaceKind::Desktop, second),
            ],
        )
        .unwrap();
    let cancel = call.cancel_handle();
    let original = adapter.clone();
    adapter.arm_liveness_hook(Box::new(move || {
        cancel.fence();
        original.arm_liveness_hook(Box::new(|| panic!("late picker queried another member")));
    }));
    assert!(call.capture().is_err());
    assert_eq!(broker.test_run_count(), 0);
    assert_eq!(broker.continuation_count("scope-app"), 0);
    assert_eq!(adapter.list_calls(), 0);
}

#[test]
fn old_picker_cancel_cannot_fence_the_new_picker_or_a_committed_grant() {
    let (broker, adapter, token, request) = fixture();
    adapter.set_synthetic_scope_instance(Some(1));
    let old = broker
        .begin_task_scope_group(
            &token,
            &request,
            1,
            &[(SurfaceKind::Desktop, &adapter.fixture_id())],
        )
        .unwrap();
    let cancel = old.cancel_handle();
    let fresh = select(&broker, &adapter, &token, &request);
    drop(old);
    cancel.fence();
    let AccessApprovalReady::Prepare(mut approval) = broker
        .reserve_selected_access_approval(&token, fresh)
        .unwrap()
    else {
        panic!("prepare")
    };
    approval.publish_feature().unwrap();
    approval.prepare_execution().unwrap();
    let committed = approval.commit().unwrap();
    cancel.fence();
    assert_eq!(
        observe(&broker, &token, &committed.run_id)["isError"],
        false
    );
    assert_eq!(broker.continuation_count("scope-app"), 1);
    finish(&broker);
}

#[test]
fn picker_cancellation_fences_its_reserved_approval_and_allows_a_fresh_retry() {
    let (broker, adapter, token, request) = fixture();
    adapter.set_synthetic_scope_instance(Some(1));
    let call = broker
        .begin_task_scope_group(
            &token,
            &request,
            1,
            &[(SurfaceKind::Desktop, &adapter.fixture_id())],
        )
        .unwrap();
    let cancel = call.cancel_handle();
    let AccessApprovalReady::Prepare(mut approval) = broker
        .reserve_selected_access_approval(&token, call.capture().unwrap())
        .unwrap()
    else {
        panic!("prepare")
    };
    approval.publish_feature().unwrap();
    cancel.fence();
    assert!(!approval.is_current());
    assert!(approval.prepare_execution().is_err());
    drop(approval);
    assert!(!broker.feature_enabled());
    assert_eq!(broker.executable_grant_count("scope-app"), 0);
    assert_eq!(broker.continuation_count("scope-app"), 0);
    let selected = select(&broker, &adapter, &token, &request);
    assert!(matches!(
        broker
            .reserve_selected_access_approval(&token, selected)
            .unwrap(),
        AccessApprovalReady::Prepare(_)
    ));
}

#[test]
fn committed_selection_retry_cannot_silently_expand_or_replace_members() {
    let (broker, adapter, token, request) = fixture();
    adapter.set_synthetic_scope_instance(Some(1));
    let first = adapter.fixture_id();
    let second = "fixture:synthetic-second";
    adapter.pin_synthetic_member(second);
    let targets = [
        (SurfaceKind::Desktop, first.as_str()),
        (SurfaceKind::Desktop, second),
    ];
    let call = broker
        .begin_task_scope_group(&token, &request, 1, &targets)
        .unwrap();
    let cancel = call.cancel_handle();
    let AccessApprovalReady::Prepare(mut approval) = broker
        .reserve_selected_access_approval(&token, call.capture().unwrap())
        .unwrap()
    else {
        panic!("prepare")
    };
    approval.publish_feature().unwrap();
    approval.prepare_execution().unwrap();
    let committed = approval.commit().unwrap();
    cancel.fence();
    let retry = broker
        .committed_scope_selection(
            &token,
            &request,
            1,
            &[
                (SurfaceKind::Desktop, second),
                (SurfaceKind::Desktop, &first),
            ],
        )
        .unwrap()
        .unwrap();
    assert_eq!(retry.run_id, committed.run_id);
    assert_eq!(retry.continuation_id, committed.continuation_id);
    assert!(broker
        .committed_scope_selection(&token, &request, 1, &[(SurfaceKind::Desktop, &first)])
        .is_err());
    assert!(broker
        .committed_scope_selection(
            &token,
            &request,
            1,
            &[
                (SurfaceKind::Desktop, &first),
                (SurfaceKind::Desktop, &first)
            ]
        )
        .is_err());
    assert_eq!(broker.executable_grant_count("scope-app"), 1);
    assert_eq!(broker.continuation_count("scope-app"), 1);
    assert_eq!(adapter.list_calls(), 0);
    assert_eq!(adapter.observe_calls(), 0);
    finish(&broker);
}

#[test]
fn cancellation_during_reservation_proof_does_not_query_the_next_member() {
    let (broker, adapter, token, request) = fixture();
    adapter.set_synthetic_scope_instance(Some(1));
    let first = adapter.fixture_id();
    adapter.pin_synthetic_member("fixture:synthetic-second");
    let call = broker
        .begin_task_scope_group(
            &token,
            &request,
            1,
            &[
                (SurfaceKind::Desktop, &first),
                (SurfaceKind::Desktop, "fixture:synthetic-second"),
            ],
        )
        .unwrap();
    let cancel = call.cancel_handle();
    let captured = call.capture().unwrap();
    let original = adapter.clone();
    adapter.arm_liveness_hook(Box::new(move || {
        cancel.fence();
        original.arm_liveness_hook(Box::new(|| {
            panic!("retired reservation queried next member")
        }));
    }));
    assert!(broker
        .reserve_selected_access_approval(&token, captured)
        .is_err());
    assert_eq!(broker.test_run_count(), 0);
    assert!(!broker.feature_enabled());
    assert_eq!(broker.continuation_count("scope-app"), 0);
}

#[test]
fn selected_identity_requires_a_witness_and_never_uses_liveness_as_proof() {
    let (broker, adapter, token, request) = fixture();
    assert!(adapter.target_alive(&adapter.fixture_id()));
    assert!(broker
        .capture_task_scope_selection(
            &token,
            &request,
            1,
            SurfaceKind::Desktop,
            &adapter.fixture_id()
        )
        .is_err());
    assert!(broker.reserve_access_approval(&token, &request, 1).is_err());
    assert!(!broker.feature_enabled());
    assert_eq!(broker.test_run_count(), 0);
    assert_eq!(adapter.list_calls(), 0);
    assert_eq!(adapter.observe_calls(), 0);
    assert!(adapter.executions().is_empty());
}

#[test]
fn selected_receipt_stages_without_effect_and_commits_only_its_original_instance() {
    let (broker, adapter, token, request) = fixture();
    adapter.set_synthetic_scope_instance(Some(1));
    let selected = select(&broker, &adapter, &token, &request);
    assert_eq!(selected.target_id(), adapter.fixture_id());
    assert_eq!(selected.surface(), SurfaceKind::Desktop);
    assert_eq!(adapter.list_calls(), 0);
    assert_eq!(adapter.observe_calls(), 0);
    assert_eq!(broker.test_run_count(), 0);
    let AccessApprovalReady::Prepare(mut prep) = broker
        .reserve_selected_access_approval(&token, selected)
        .unwrap()
    else {
        panic!("new selection prepares")
    };
    assert!(!broker.feature_enabled());
    prep.publish_feature().unwrap();
    prep.prepare_execution().unwrap();
    assert_eq!(broker.executable_grant_count("scope-app"), 0);
    assert!(broker.begin_model_call(&token, "computer_observe").is_err());
    let outcome = prep.commit().unwrap();
    assert_eq!(outcome.decision_class, SYNTHETIC_DECISION_CLASS);
    assert_eq!(observe(&broker, &token, &outcome.run_id)["isError"], false);
    let status = broker
        .submit_access(&token, json!({"action":"status"}))
        .unwrap();
    assert_eq!(status["decisionClass"], SYNTHETIC_DECISION_CLASS);
    adapter.set_synthetic_scope_instance(Some(2));
    assert_eq!(observe(&broker, &token, &outcome.run_id)["isError"], true);
    assert_eq!(adapter.observe_calls(), 1);
    assert!(adapter.executions().is_empty());
    finish(&broker);
}

#[test]
fn refreshed_picker_retires_old_receipt_without_reading_target_content() {
    let (broker, adapter, token, request) = fixture();
    adapter.set_synthetic_scope_instance(Some(1));
    let old = select(&broker, &adapter, &token, &request);
    let fresh = select(&broker, &adapter, &token, &request);
    assert!(broker
        .reserve_selected_access_approval(&token, old)
        .is_err());
    assert!(matches!(
        broker
            .reserve_selected_access_approval(&token, fresh)
            .unwrap(),
        AccessApprovalReady::Prepare(_)
    ));
    assert_eq!(broker.test_run_count(), 0);
    assert_eq!(adapter.list_calls(), 0);
    assert_eq!(adapter.observe_calls(), 0);
}

#[test]
fn receipt_cannot_cross_broker_even_when_owner_counters_and_fixture_names_match() {
    let (original, adapter, token, request) = fixture();
    adapter.set_synthetic_scope_instance(Some(1));
    let selected = select(&original, &adapter, &token, &request);
    let (other, _, other_token, _) = fixture();
    assert!(other
        .reserve_selected_access_approval(&other_token, selected)
        .is_err());
    assert_eq!(other.executable_grant_count("scope-app"), 0);
    assert_eq!(original.executable_grant_count("scope-app"), 0);
    assert_eq!(adapter.observe_calls(), 0);
}

#[test]
fn stop_inside_native_identity_query_retires_the_picker_result() {
    let (broker, adapter, token, request) = fixture();
    adapter.set_synthetic_scope_instance(Some(1));
    let stopped = broker.clone();
    adapter.arm_liveness_hook(Box::new(move || stopped.fence_task_consent("scope-app")));
    assert!(broker
        .capture_task_scope_selection(
            &token,
            &request,
            1,
            SurfaceKind::Desktop,
            &adapter.fixture_id()
        )
        .is_err());
    assert_eq!(broker.test_run_count(), 0);
    assert_eq!(adapter.list_calls(), 0);
    assert_eq!(adapter.observe_calls(), 0);
}

#[test]
fn instance_replacement_between_native_prepare_and_commit_has_no_grant_or_effect() {
    let (broker, adapter, token, request) = fixture();
    adapter.set_synthetic_scope_instance(Some(1));
    let selected = select(&broker, &adapter, &token, &request);
    let AccessApprovalReady::Prepare(mut prep) = broker
        .reserve_selected_access_approval(&token, selected)
        .unwrap()
    else {
        panic!("new selection prepares")
    };
    prep.publish_feature().unwrap();
    prep.prepare_execution().unwrap();
    adapter.set_synthetic_scope_instance(Some(2));
    assert!(prep.commit().is_err());
    assert_eq!(broker.executable_grant_count("scope-app"), 0);
    assert_eq!(broker.continuation_count("scope-app"), 0);
    assert!(!broker.feature_enabled());
    assert_eq!(adapter.observe_calls(), 0);
    assert!(adapter.executions().is_empty());
    finish(&broker);
}

#[test]
fn group_receipt_commits_all_selected_members_once_without_reading_inactive_content() {
    let (broker, adapter, token, request) = fixture();
    adapter.set_synthetic_scope_instance(Some(1));
    let a = adapter.fixture_id();
    adapter.pin_synthetic_member(&a);
    adapter.rotate_identity();
    let b = adapter.fixture_id();
    let selected = broker
        .capture_task_scope_group(
            &token,
            &request,
            1,
            &[(SurfaceKind::Desktop, &b), (SurfaceKind::Desktop, &a)],
        )
        .unwrap();
    assert_eq!(selected.members().len(), 2);
    assert_eq!(broker.test_run_count(), 0);
    assert_eq!(adapter.list_calls(), 0);
    // Discovery is not needed again after the Host chose exact instances.
    // A missing discovery service must not force a broad title enumeration.
    adapter.set_listing_error(true);
    let AccessApprovalReady::Prepare(mut prep) = broker
        .reserve_selected_access_approval(&token, selected)
        .unwrap()
    else {
        panic!("new group prepares")
    };
    prep.publish_feature().unwrap();
    prep.prepare_execution().unwrap();
    let outcome = prep.commit().unwrap();
    assert_eq!(outcome.decision_class, SYNTHETIC_DECISION_CLASS);
    let rows = broker.approved_task_targets(&token, &json!({})).unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows.iter().filter(|row| row["active"] == true).count(), 1);
    assert!(rows
        .iter()
        .any(|row| row["targetId"] == a && row["targetGeneration"].is_null()));
    assert!(rows.iter().all(|row| row["title"] != row["targetId"]));
    broker
        .open_approved_task_target(&token, &json!({"runId":outcome.run_id,"targetId":a}))
        .unwrap();
    let info = broker
        .authorized_target_info(&outcome.run_id, SurfaceKind::Desktop)
        .unwrap();
    assert_eq!(info.target_id, a);
    assert_eq!(info.title, "SyntheticMember");
    assert_eq!(adapter.list_calls(), 0);
    assert_eq!(broker.executable_grant_count("scope-app"), 1);
    assert_eq!(broker.continuation_count("scope-app"), 1);
    let AccessApprovalReady::Already(repeated) =
        broker.reserve_access_approval(&token, &request, 1).unwrap()
    else {
        panic!("duplicate group must return original outcome")
    };
    assert_eq!(repeated.run_id, outcome.run_id);
    assert_eq!(repeated.continuation_id, outcome.continuation_id);
    assert_eq!(adapter.observe_calls(), 0);
    assert!(adapter.executions().is_empty());
    finish(&broker);
}

#[test]
fn inactive_member_retiring_before_commit_rejects_the_entire_group() {
    let (broker, adapter, token, request) = fixture();
    adapter.set_synthetic_scope_instance(Some(1));
    let inactive = adapter.fixture_id();
    adapter.pin_synthetic_member(&inactive);
    adapter.rotate_identity();
    let active = adapter.fixture_id();
    let selected = broker
        .capture_task_scope_group(
            &token,
            &request,
            1,
            &[
                (SurfaceKind::Desktop, &active),
                (SurfaceKind::Desktop, &inactive),
            ],
        )
        .unwrap();
    let AccessApprovalReady::Prepare(mut prep) = broker
        .reserve_selected_access_approval(&token, selected)
        .unwrap()
    else {
        panic!("new group prepares")
    };
    prep.publish_feature().unwrap();
    prep.prepare_execution().unwrap();
    adapter.unpin_synthetic_member(&inactive);
    assert!(prep.commit().is_err());
    assert!(adapter.target_alive(&active));
    assert_eq!(broker.executable_grant_count("scope-app"), 0);
    assert_eq!(broker.continuation_count("scope-app"), 0);
    assert!(broker.scope_memories("scope-app").is_empty());
    assert!(!broker.feature_enabled());
    assert_eq!(adapter.observe_calls(), 0);
    assert!(adapter.executions().is_empty());
    finish(&broker);
}

#[test]
fn invalid_or_cross_surface_group_is_rejected_before_instance_queries() {
    let (broker, adapter, token, request) = fixture();
    let target = adapter.fixture_id();
    for targets in [
        vec![],
        vec![(SurfaceKind::Desktop, target.as_str()); 9],
        vec![
            (SurfaceKind::Desktop, target.as_str()),
            (SurfaceKind::WebView, "wv:foreign"),
        ],
    ] {
        assert!(broker
            .capture_task_scope_group(&token, &request, 1, &targets)
            .is_err());
    }
    assert_eq!(broker.test_run_count(), 0);
    assert!(!broker.feature_enabled());
    assert_eq!(adapter.list_calls(), 0);
    assert_eq!(adapter.observe_calls(), 0);
}

#[test]
fn stop_or_identity_retirement_inside_exact_metadata_prevents_preparation() {
    for stop in [false, true] {
        let (broker, adapter, token, request) = fixture();
        adapter.set_synthetic_scope_instance(Some(1));
        let selection = select(&broker, &adapter, &token, &request);
        let AccessApprovalReady::Prepare(mut prep) = broker
            .reserve_selected_access_approval(&token, selection)
            .unwrap()
        else {
            panic!("new selected target prepares")
        };
        prep.publish_feature().unwrap();
        let interrupted = broker.clone();
        let replaced = adapter.clone();
        adapter.arm_scope_metadata_hook(Box::new(move || {
            if stop {
                interrupted.fence_task_consent("scope-app");
            } else {
                replaced.set_synthetic_scope_instance(Some(2));
            }
        }));
        assert!(prep.prepare_execution().is_err());
        prep.abort();
        assert_eq!(broker.executable_grant_count("scope-app"), 0);
        assert_eq!(broker.continuation_count("scope-app"), 0);
        assert!(broker.scope_memories("scope-app").is_empty());
        assert_eq!(adapter.list_calls(), 0);
        assert_eq!(adapter.observe_calls(), 0);
        assert!(adapter.executions().is_empty());
        finish(&broker);
    }
}

#[test]
fn stop_inside_preparation_proof_prevents_the_first_selected_metadata_read() {
    let (broker, adapter, token, request) = fixture();
    adapter.set_synthetic_scope_instance(Some(1));
    let selection = select(&broker, &adapter, &token, &request);
    let AccessApprovalReady::Prepare(mut prep) = broker
        .reserve_selected_access_approval(&token, selection)
        .unwrap()
    else {
        panic!("new selected target prepares")
    };
    prep.publish_feature().unwrap();
    let stopped = broker.clone();
    let second_proof = adapter.clone();
    adapter.arm_liveness_hook(Box::new(move || {
        // The first group-proof query precedes prepare_authorize. Stop in the
        // next query, inside scope_member_info after that initial state check.
        second_proof.arm_liveness_hook(Box::new(move || stopped.fence_task_consent("scope-app")));
    }));
    assert!(prep.prepare_execution().is_err());
    prep.abort();
    assert_eq!(adapter.scope_metadata_calls(), 0);
    assert_eq!(adapter.list_calls(), 0);
    assert_eq!(adapter.observe_calls(), 0);
    assert_eq!(broker.executable_grant_count("scope-app"), 0);
    assert_eq!(broker.continuation_count("scope-app"), 0);
    assert!(adapter.executions().is_empty());
    finish(&broker);
}
