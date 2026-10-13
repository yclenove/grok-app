//! U04 task inventory starts from approved identities, never ambient discovery.
//! Every Host decision and target in this module is explicitly synthetic.
use super::*;

fn inventory(broker: &ComputerUseBroker, token: &str) -> Vec<Value> {
    let response = crate::tools::dispatch_with_transport(
        broker,
        token,
        "computer_list_targets",
        json!({}),
        None,
    );
    assert_eq!(
        response["isError"], false,
        "approved inventory failed: {response}"
    );
    serde_json::from_str::<Vec<Value>>(response["content"][0]["text"].as_str().unwrap()).unwrap()
}

#[test]
fn two_approved_members_are_listed_without_ambient_target_enumeration() {
    let (broker, adapter) = review_pair();
    let token = review_token(&broker);
    let a = adapter.fixture_id();
    adapter.pin_synthetic_member(&a);
    let grant_a = review_commit(&broker, token.token(), "A", &["observe", "input"], &a);
    adapter.rotate_identity();
    let b = adapter.fixture_id();
    let grant_b = review_commit(&broker, token.token(), "B", &["observe", "input"], &b);
    adapter.set_include_decoy(true);
    let before = adapter.list_calls();
    // Discovery now fails. Reading already-approved inventory must not need it.
    adapter.set_listing_error(true);
    let rows = inventory(&broker, token.token());
    let ids = rows
        .iter()
        .map(|row| row["targetId"].as_str().unwrap())
        .collect::<HashSet<_>>();
    assert_eq!(ids, HashSet::from([a.as_str(), b.as_str()]));
    assert!(rows.iter().any(|row| row["runId"] == grant_a.run_id));
    assert!(rows.iter().any(|row| row["runId"] == grant_b.run_id));
    assert_eq!(adapter.list_calls(), before);
    assert_eq!(adapter.observe_calls(), 0);
    assert!(adapter.executions().is_empty());
}

#[test]
fn inventory_never_borrows_input_only_or_pending_scope_for_reading() {
    let (broker, adapter) = review_pair();
    let token = review_token(&broker);
    let a = adapter.fixture_id();
    adapter.pin_synthetic_member(&a);
    review_commit(&broker, token.token(), "input-only", &["input"], &a);
    adapter.rotate_identity();
    let b = adapter.fixture_id();
    review_commit(&broker, token.token(), "readable", &["observe"], &b);
    broker
        .submit_access(
            token.token(),
            review_args("unapproved-C", &["observe", "input"]),
        )
        .unwrap();
    let before = adapter.list_calls();
    let rows = inventory(&broker, token.token());
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["targetId"], b);
    assert_eq!(adapter.list_calls(), before);
    assert_eq!(adapter.observe_calls(), 0);
    assert!(adapter.executions().is_empty());
}

#[test]
fn a_dead_approved_member_is_not_replaced_with_an_ambient_target() {
    let (broker, adapter) = review_pair();
    let token = review_token(&broker);
    let a = adapter.fixture_id();
    review_commit(&broker, token.token(), "A", &["observe"], &a);
    adapter.rotate_identity();
    let before = adapter.list_calls();
    let rows = inventory(&broker, token.token());
    assert!(rows.is_empty(), "a replacement inherited old scope");
    assert_eq!(adapter.list_calls(), before);
    assert_eq!(adapter.observe_calls(), 0);
    assert!(adapter.executions().is_empty());
}

#[test]
fn stop_during_member_liveness_never_publishes_cached_scope_metadata() {
    let (broker, adapter) = review_pair();
    let broker = Arc::new(broker);
    let token = review_token(&broker);
    review_commit(
        &broker,
        token.token(),
        "A",
        &["observe"],
        &adapter.fixture_id(),
    );
    let weak = Arc::downgrade(&broker);
    adapter.arm_liveness_hook(Box::new(move || {
        weak.upgrade().unwrap().fence_task_consent("review-app");
    }));
    let before = adapter.list_calls();
    let result = crate::tools::dispatch_with_transport(
        &broker,
        token.token(),
        "computer_list_targets",
        json!({}),
        None,
    );
    assert_eq!(result["isError"], true, "stopped scope published: {result}");
    assert_eq!(adapter.list_calls(), before);
    assert_eq!(adapter.observe_calls(), 0);
    assert!(adapter.executions().is_empty());
    for ticket in broker.take_cleanup_tickets() {
        broker.finish_stop_cleanup(&ticket).unwrap();
    }
}

#[test]
fn same_id_reauthorization_during_liveness_rejects_old_generation_metadata() {
    let (broker, adapter) = review_pair();
    let broker = Arc::new(broker);
    let token = review_token(&broker);
    let target = adapter.fixture_id();
    let granted = review_commit(&broker, token.token(), "A", &["observe"], &target);
    let weak = Arc::downgrade(&broker);
    adapter.arm_liveness_hook(Box::new(move || {
        weak.upgrade()
            .unwrap()
            .authorize_desktop_window(&granted.run_id, &target)
            .unwrap();
    }));
    let result = crate::tools::dispatch_with_transport(
        &broker,
        token.token(),
        "computer_list_targets",
        json!({}),
        None,
    );
    assert_eq!(
        result["isError"], true,
        "old generation published: {result}"
    );
    assert_eq!(adapter.observe_calls(), 0);
    assert!(adapter.executions().is_empty());
    assert_eq!(inventory(&broker, token.token()).len(), 1);
}

#[test]
fn permission_and_dispatch_reject_scope_spoofing_without_discovery() {
    let (broker, adapter) = review_pair();
    let token = review_token(&broker);
    review_commit(
        &broker,
        token.token(),
        "A",
        &["observe"],
        &adapter.fixture_id(),
    );
    let before = adapter.list_calls();
    broker
        .preflight_tool_permission(token.token(), "computer_list_targets", &json!({}))
        .unwrap();
    for args in [
        json!([]),
        json!({"targetId":"foreign"}),
        json!({"runId":"foreign"}),
        json!({"grant":true}),
        json!({"targetId":adapter.fixture_id(),"target_id":adapter.fixture_id()}),
    ] {
        assert!(broker
            .preflight_tool_permission(token.token(), "computer_list_targets", &args)
            .is_err());
        let result = crate::tools::dispatch_with_transport(
            &broker,
            token.token(),
            "computer_list_targets",
            args,
            None,
        );
        assert_eq!(result["isError"], true, "{result}");
    }
    assert_eq!(adapter.list_calls(), before);
    assert_eq!(adapter.observe_calls(), 0);
    assert!(adapter.executions().is_empty());
}

#[test]
fn a_later_member_callback_cannot_publish_an_earlier_generation() {
    let (broker, adapter) = review_pair();
    let broker = Arc::new(broker);
    let token = review_token(&broker);
    let a = adapter.fixture_id();
    adapter.pin_synthetic_member(&a);
    let first = review_commit(&broker, token.token(), "A", &["observe"], &a);
    adapter.rotate_identity();
    review_commit(
        &broker,
        token.token(),
        "B",
        &["observe"],
        &adapter.fixture_id(),
    );
    let weak_broker = Arc::downgrade(&broker);
    let weak_adapter = Arc::downgrade(&adapter);
    adapter.arm_liveness_hook(Box::new(move || {
        // A has not yet returned. Install the interleave for B's later call.
        weak_adapter
            .upgrade()
            .unwrap()
            .arm_liveness_hook(Box::new(move || {
                weak_broker
                    .upgrade()
                    .unwrap()
                    .authorize_desktop_window(&first.run_id, &a)
                    .unwrap();
            }));
    }));
    let result = crate::tools::dispatch_with_transport(
        &broker,
        token.token(),
        "computer_list_targets",
        json!({}),
        None,
    );
    assert_eq!(
        result["isError"], true,
        "earlier stale member published: {result}"
    );
    assert_eq!(adapter.observe_calls(), 0);
    assert!(adapter.executions().is_empty());
    assert_eq!(inventory(&broker, token.token()).len(), 2);
}

#[test]
fn explicit_inventory_selection_reads_that_member_and_rejects_ambiguous_defaults() {
    let (broker, adapter) = review_pair();
    let token = review_token(&broker);
    let a = adapter.fixture_id();
    adapter.pin_synthetic_member(&a);
    let grant_a = review_commit(&broker, token.token(), "A", &["observe"], &a);
    adapter.rotate_identity();
    let b = adapter.fixture_id();
    let grant_b = review_commit(&broker, token.token(), "B", &["observe"], &b);
    let defaults = crate::tools::dispatch_with_transport(
        &broker,
        token.token(),
        "computer_observe",
        json!({"screenshot":false}),
        None,
    );
    assert_eq!(
        defaults["isError"], true,
        "ambiguous defaults guessed a member"
    );
    assert_eq!(adapter.observe_calls(), 0);
    let bad_pair = json!({"runId":grant_a.run_id,"targetId":b,"screenshot":false});
    assert!(broker
        .preflight_tool_permission(token.token(), "computer_observe", &bad_pair)
        .is_err());
    let rejected = crate::tools::dispatch_with_transport(
        &broker,
        token.token(),
        "computer_observe",
        bad_pair,
        None,
    );
    assert_eq!(rejected["isError"], true);
    let exact = json!({"runId":grant_b.run_id,"targetId":b,"screenshot":false});
    broker
        .preflight_tool_permission(token.token(), "computer_observe", &exact)
        .unwrap();
    assert_eq!(adapter.observe_calls(), 0, "preflight captured a target");
    let observed = crate::tools::dispatch_with_transport(
        &broker,
        token.token(),
        "computer_observe",
        exact,
        None,
    );
    assert_eq!(observed["isError"], false, "{observed}");
    assert_eq!(adapter.observe_calls(), 1);
    assert_eq!(adapter.last_observe_target_id(), b);
    broker.fence_task_consent("review-app");
    for ticket in broker.take_cleanup_tickets() {
        broker.finish_stop_cleanup(&ticket).unwrap();
    }
}
