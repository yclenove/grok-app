//! Explicitly synthetic picker witnesses; no user/native approval.
use super::*;
use crate::{
    fake::FakeAdapter,
    owned::HostOwnedAdapter,
    task_consent::{HostRequestKey, HostRequestPhase},
};
use serde_json::json;

struct Fixture {
    broker: Arc<ComputerUseBroker>,
    fake: Arc<FakeAdapter>,
    token: String,
    key: HostRequestKey,
}

impl Fixture {
    fn new() -> Self {
        let fake = Arc::new(FakeAdapter::new());
        fake.set_synthetic_scope_instance(Some(1));
        fake.pin_synthetic_member("fixture:second");
        let broker = Arc::new(ComputerUseBroker::new(
            Arc::new(HostOwnedAdapter::new(fake.clone())),
            BrokerOptions {
                feature_enabled: false,
                lease_path: std::env::temp_dir()
                    .join(format!("cu-scope-picker-{}.lease", Uuid::new_v4())),
                ..BrokerOptions::default()
            },
        ));
        let token = broker.reserve_transport_connection("picker-app").unwrap();
        broker
            .bind_transport_agent(token.token(), "synthetic-agent")
            .unwrap();
        broker.submit_access(token.token(), json!({"action":"request", "purpose":"synthetic picker", "surfaces":["desktop"], "scopeIntent":"owned fixture group", "capabilities":["observe"]})).unwrap();
        let key = broker
            .host_task_view(token.token())
            .unwrap()
            .request
            .unwrap()
            .key;
        Self {
            broker,
            fake,
            token: token.token().into(),
            key,
        }
    }
    fn begin(&self) -> HostScopePickerCall {
        self.broker
            .begin_task_scope_picker(&self.token, &self.key, SurfaceKind::Desktop)
            .unwrap()
    }
    fn ids(picker: &HostScopePicker) -> Vec<String> {
        picker
            .views()
            .unwrap()
            .iter()
            .map(|view| view.candidate_id.clone())
            .collect()
    }
    fn no_effects(&self) {
        assert!(!self.broker.feature_enabled());
        assert_eq!(self.broker.executable_grant_count("picker-app"), 0);
        assert_eq!(self.broker.continuation_count("picker-app"), 0);
        assert_eq!(self.fake.observe_calls(), 0);
        assert!(self.fake.executions().is_empty());
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.broker.fence_task_consent("picker-app");
        for ticket in self.broker.take_cleanup_tickets() {
            self.broker.finish_stop_cleanup(&ticket).unwrap();
        }
    }
}

#[test]
fn opening_reserves_original_request_but_does_not_discover_or_execute() {
    let fixture = Fixture::new();
    let call = fixture.begin();
    assert_eq!(fixture.fake.list_calls(), 0);
    assert_eq!(
        fixture
            .broker
            .host_task_view(&fixture.token)
            .unwrap()
            .request
            .unwrap()
            .phase,
        HostRequestPhase::Preparing
    );
    fixture.no_effects();
    drop(call);
    assert_eq!(
        fixture
            .broker
            .host_task_view(&fixture.token)
            .unwrap()
            .request
            .unwrap()
            .phase,
        HostRequestPhase::Pending
    );
}

#[test]
fn original_wrapper_forwards_bounded_picker_without_worker_or_grant() {
    let fixture = Fixture::new();
    let picker = fixture.begin().discover().unwrap();
    let views = picker.views().unwrap();
    assert_eq!(views.len(), 2);
    assert_eq!(fixture.fake.list_calls(), 1);
    assert!(!picker.truncated());
    let serialized = serde_json::to_string(&views).unwrap();
    assert!(!serialized.contains(&fixture.token));
    assert!(!serialized.contains(&fixture.fake.fixture_id()));
    assert!(!serialized.contains("identity"));
    assert!(views.iter().all(|view| view.surface == "desktop"));
    fixture.no_effects();
}

#[test]
fn foreign_keys_unrequested_surfaces_and_retired_owner_reject_before_discovery() {
    let fixture = Fixture::new();
    for key in [
        HostRequestKey {
            connection_id: "foreign".into(),
            ..fixture.key.clone()
        },
        HostRequestKey {
            task_id: "foreign".into(),
            ..fixture.key.clone()
        },
        HostRequestKey {
            request_id: "foreign".into(),
            ..fixture.key.clone()
        },
        HostRequestKey {
            revision: 2,
            ..fixture.key.clone()
        },
    ] {
        assert!(fixture
            .broker
            .begin_task_scope_picker(&fixture.token, &key, SurfaceKind::Desktop)
            .is_err());
    }
    for surface in [
        SurfaceKind::ManagedBrowser,
        SurfaceKind::ExistingTab,
        SurfaceKind::WebView,
    ] {
        assert!(fixture
            .broker
            .begin_task_scope_picker(&fixture.token, &fixture.key, surface)
            .is_err());
    }
    fixture.broker.retire_transport_connection(&fixture.token);
    assert!(fixture
        .broker
        .begin_task_scope_picker(&fixture.token, &fixture.key, SurfaceKind::Desktop)
        .is_err());
    assert_eq!(fixture.fake.list_calls(), 0);
}

#[test]
fn canceled_before_discovery_never_reaches_native_producer() {
    let fixture = Fixture::new();
    let call = fixture.begin();
    call.cancel_handle().fence();
    assert!(call.discover().is_err());
    assert_eq!(fixture.fake.list_calls(), 0);
    fixture.no_effects();
}

#[test]
fn stopped_during_discovery_drops_late_result_and_does_not_query_second_candidate() {
    let fixture = Fixture::new();
    let call = fixture.begin();
    let broker = fixture.broker.clone();
    let token = fixture.token.clone();
    let key = fixture.key.clone();
    fixture.fake.arm_liveness_hook(Box::new(move || {
        broker
            .decide_host_task(&token, &key, HostTaskDecision::Stop)
            .unwrap();
    }));
    assert!(call.discover().is_err());
    assert_eq!(fixture.fake.list_calls(), 1);
    fixture.no_effects();
}

#[test]
fn new_picker_invalidates_original_candidate_references_and_late_close_cannot_fence_newer() {
    let fixture = Fixture::new();
    let old = fixture.begin().discover().unwrap();
    let old_ids = Fixture::ids(&old);
    let new = fixture.begin().discover().unwrap();
    let new_ids = Fixture::ids(&new);
    assert!(old.views().is_err());
    assert!(old.select(&old_ids).is_err());
    old.cancel_handle().fence();
    drop(old);
    assert!(new.is_current());
    assert!(new.select(&old_ids).is_err());
    assert!(new.select(&new_ids).is_ok());
    fixture.no_effects();
}

#[test]
fn same_target_string_with_replaced_native_instance_cannot_be_approved_from_old_picker() {
    let fixture = Fixture::new();
    let picker = fixture.begin().discover().unwrap();
    let ids = Fixture::ids(&picker);
    fixture.fake.set_synthetic_scope_instance(Some(2));
    let selected = picker.select(&ids).unwrap();
    assert!(fixture
        .broker
        .reserve_selected_access_approval(&fixture.token, selected)
        .is_err());
    fixture.no_effects();
}

#[test]
fn another_executor_cannot_replace_the_original_picker_producer() {
    let fixture = Fixture::new();
    let picker = fixture.begin().discover().unwrap();
    let ids = Fixture::ids(&picker);
    assert!(fixture
        .broker
        .register_surface_adapter(
            SurfaceKind::Desktop,
            Arc::new(HostOwnedAdapter::new(fixture.fake.clone()))
        )
        .is_err());
    let AccessApprovalReady::Prepare(approval) = fixture
        .broker
        .reserve_selected_access_approval(&fixture.token, picker.select(&ids).unwrap())
        .unwrap()
    else {
        panic!("original producer prepares")
    };
    drop(approval);
    fixture.no_effects();
}

#[test]
fn explicit_subset_reserves_only_original_members_and_committed_retry_has_no_discovery() {
    let fixture = Fixture::new();
    let picker = fixture.begin().discover().unwrap();
    let ids = Fixture::ids(&picker);
    let chosen = vec![ids[0].clone()];
    let AccessApprovalReady::Prepare(mut approval) = fixture
        .broker
        .reserve_selected_access_approval(&fixture.token, picker.select(&chosen).unwrap())
        .unwrap()
    else {
        panic!("prepare")
    };
    assert_eq!(fixture.broker.executable_grant_count("picker-app"), 0);
    approval.publish_feature().unwrap();
    approval.prepare_execution().unwrap();
    let outcome = approval.commit().unwrap();
    let listing = fixture.fake.list_calls();
    let retry = picker
        .committed_outcome(&fixture.token, &chosen)
        .unwrap()
        .unwrap();
    assert_eq!(retry.run_id, outcome.run_id);
    assert!(picker.committed_outcome(&fixture.token, &ids).is_err());
    assert_eq!(fixture.fake.list_calls(), listing);
    drop(picker);
    assert_eq!(fixture.broker.executable_grant_count("picker-app"), 1);
    assert_eq!(fixture.broker.continuation_count("picker-app"), 1);
    assert_eq!(fixture.fake.observe_calls(), 0);
}

#[test]
fn selection_rejects_empty_duplicate_foreign_or_oversize_refs_without_approving() {
    let fixture = Fixture::new();
    let picker = fixture.begin().discover().unwrap();
    let ids = Fixture::ids(&picker);
    for selected in [
        vec![],
        vec![ids[0].clone(), ids[0].clone()],
        vec!["foreign".into()],
        vec![ids[0].clone(); 9],
    ] {
        assert!(picker.select(&selected).is_err());
    }
    assert!(picker.select(&ids).is_ok());
    fixture.no_effects();
}

#[test]
fn oversized_candidate_inventory_is_bounded_and_never_published_as_complete() {
    let fixture = Fixture::new();
    for index in 0..140 {
        fixture
            .fake
            .pin_synthetic_member(&format!("fixture:extra-{index}"));
    }
    let picker = fixture.begin().discover().unwrap();
    assert_eq!(picker.views().unwrap().len(), MAX_SCOPE_CANDIDATES);
    assert!(picker.truncated());
    assert!(picker.select(&Fixture::ids(&picker)).is_err());
    fixture.no_effects();
}

#[test]
fn native_producer_failure_leaves_pending_request_without_execution_fallback() {
    let fixture = Fixture::new();
    fixture.fake.set_listing_error(true);
    assert!(fixture.begin().discover().is_err());
    assert_eq!(
        fixture
            .broker
            .host_task_view(&fixture.token)
            .unwrap()
            .request
            .unwrap()
            .phase,
        HostRequestPhase::Pending
    );
    fixture.no_effects();
}

#[test]
fn selected_group_retains_each_picker_name_without_metadata_rediscovery() {
    let fixture = Fixture::new();
    let picker = fixture.begin().discover().unwrap();
    let views = picker.views().unwrap();
    let ids = Fixture::ids(&picker);
    let AccessApprovalReady::Prepare(mut approval) = fixture
        .broker
        .reserve_selected_access_approval(&fixture.token, picker.select(&ids).unwrap())
        .unwrap()
    else {
        panic!("prepare original selected members")
    };
    approval.publish_feature().unwrap();
    approval.prepare_execution().unwrap();
    approval.commit().unwrap();
    let listed = fixture.fake.list_calls();
    fixture
        .fake
        .arm_liveness_hook(Box::new(|| panic!("metadata queried target liveness")));
    let metadata =
        serde_json::to_value(fixture.broker.host_task_view(&fixture.token).unwrap()).unwrap();
    let members = metadata["grants"][0]["members"].as_array().unwrap();
    assert_eq!(members.len(), 2);
    for (member, candidate) in members.iter().zip(&views) {
        assert_eq!(member["displayName"], candidate.title);
    }
    assert_eq!(fixture.fake.list_calls(), listed);
    assert_eq!(fixture.fake.observe_calls(), 0);
    fixture.fake.arm_liveness_hook(Box::new(|| {}));
}
