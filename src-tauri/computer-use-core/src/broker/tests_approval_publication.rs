//! Explicit synthetic Host decision, real adapter-query/authority boundaries.
use super::*;
use crate::fake::FakeAdapter;
use serde_json::json;
use std::sync::atomic::AtomicUsize;

fn fixture() -> (Arc<ComputerUseBroker>, Arc<FakeAdapter>, AccessApproval) {
    let adapter = Arc::new(FakeAdapter::new());
    adapter.set_synthetic_scope_instance(Some(1));
    let broker = Arc::new(ComputerUseBroker::new(
        adapter.clone(),
        BrokerOptions {
            feature_enabled: false,
            lease_path: std::env::temp_dir().join(format!("cu-publish-{}.lease", Uuid::new_v4())),
            ..BrokerOptions::default()
        },
    ));
    let transport = broker.issue_transport_credential("publish-app").unwrap();
    broker
        .bind_agent_session("publish-app", "synthetic-agent")
        .unwrap();
    let request = broker.submit_access(transport.token(), json!({
        "action":"request", "purpose":"synthetic final approval boundary", "scopeIntent":"owned fake instance",
        "surfaces":["desktop"], "capabilities":["observe"]
    })).unwrap();
    let selected = broker
        .capture_task_scope_selection(
            transport.token(),
            request["requestId"].as_str().unwrap(),
            1,
            SurfaceKind::Desktop,
            &adapter.fixture_id(),
        )
        .unwrap();
    let AccessApprovalReady::Prepare(mut approval) = broker
        .reserve_selected_access_approval(transport.token(), selected)
        .unwrap()
    else {
        panic!("prepare")
    };
    approval.publish_feature().unwrap();
    approval.prepare_execution().unwrap();
    (broker, adapter, approval)
}

fn cleanup(broker: &ComputerUseBroker) {
    broker.fence_task_consent("publish-app");
    for ticket in broker.take_cleanup_tickets() {
        broker.finish_stop_cleanup(&ticket).unwrap();
    }
}

#[test]
fn final_authority_publication_does_not_query_native_under_app_pin() {
    let (broker, adapter, mut approval) = fixture();
    approval.prepare_publication().unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let query_count = calls.clone();
    adapter.arm_liveness_hook(Box::new(move || {
        query_count.fetch_add(1, Ordering::SeqCst);
    }));
    let outcome = approval.commit_authority();
    // Leave the synthetic resource terminal even on the initial failing run.
    let grants = broker.executable_grant_count("publish-app");
    cleanup(&broker);
    assert!(outcome.is_ok());
    assert_eq!(grants, 1);
    assert_eq!(
        calls.load(Ordering::SeqCst),
        0,
        "final App authority pin performed a native query"
    );
}

#[test]
fn authority_commit_requires_explicit_native_publication_preparation() {
    let (broker, adapter, mut approval) = fixture();
    assert!(approval.commit_authority().is_err());
    assert_eq!(broker.executable_grant_count("publish-app"), 0);
    assert_eq!(broker.continuation_count("publish-app"), 0);
    assert_eq!(adapter.observe_calls(), 0);
    drop(approval);
    cleanup(&broker);
}

#[test]
fn replacement_during_native_publication_preparation_never_becomes_a_grant() {
    let (broker, adapter, mut approval) = fixture();
    adapter.set_synthetic_scope_instance(Some(2));
    assert!(approval.prepare_publication().is_err());
    assert!(approval.commit_authority().is_err());
    assert_eq!(broker.executable_grant_count("publish-app"), 0);
    assert_eq!(broker.continuation_count("publish-app"), 0);
    assert_eq!(adapter.observe_calls(), 0);
    drop(approval);
    cleanup(&broker);
}

#[test]
fn stop_after_native_preparation_rejects_publication_without_another_query() {
    let (broker, adapter, mut approval) = fixture();
    approval.prepare_publication().unwrap();
    adapter.arm_liveness_hook(Box::new(|| {
        panic!("Stop must not query native under App pin")
    }));
    approval.cancel_handle().fence();
    assert!(approval.commit_authority().is_err());
    assert_eq!(broker.executable_grant_count("publish-app"), 0);
    assert_eq!(broker.continuation_count("publish-app"), 0);
    drop(approval);
    cleanup(&broker);
}

#[test]
fn repeated_borrowed_publication_does_not_create_a_second_grant_or_continuation() {
    let (broker, _, mut approval) = fixture();
    approval.prepare_publication().unwrap();
    approval.commit_authority().unwrap();
    assert!(approval.commit_authority().is_err());
    assert_eq!(broker.executable_grant_count("publish-app"), 1);
    assert_eq!(broker.continuation_count("publish-app"), 1);
    cleanup(&broker);
}

#[test]
fn a_live_foreign_transport_cannot_borrow_the_prepared_approval() {
    let (broker, adapter, mut approval) = fixture();
    let foreign = broker.issue_transport_credential("foreign-app").unwrap();
    broker
        .bind_agent_session("foreign-app", "foreign-agent")
        .unwrap();
    approval.prepare_publication().unwrap();
    adapter.arm_liveness_hook(Box::new(|| panic!("foreign publication queried native")));
    assert!(approval.commit_for_transport(foreign.token()).is_err());
    assert!(approval
        .commit_for_transport("synthetic-invalid-bearer")
        .is_err());
    assert_eq!(broker.executable_grant_count("publish-app"), 0);
    assert_eq!(broker.continuation_count("publish-app"), 0);
    drop(approval);
    cleanup(&broker);
}
