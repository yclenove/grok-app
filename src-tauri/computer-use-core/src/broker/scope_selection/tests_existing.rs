//! Real Host registry and reservation APIs; pairing/user choice are synthetic.
//! No browser, page content or real user permission is used in these unit tests.
use super::*;
use crate::browser::SharedTabOffer;
use crate::fake::FakeAdapter;
use crate::pairing::{ExtensionPairing, PairingConnection, PairingSession, EXTENSION_ID};
use crate::task_consent::HostRequestKey;
use serde_json::json;

mod discovery;
mod empty_retry;
mod first_use;
mod identity;
mod picker;
mod publication;
mod reservation;
mod switch;

struct Fixture {
    broker: Arc<ComputerUseBroker>,
    desktop: Arc<FakeAdapter>,
    pairing: PairingSession,
    origin: String,
    token: String,
    key: HostRequestKey,
}

impl Fixture {
    fn new() -> Self {
        Self::with_existing(|broker| {
            broker.register_existing_browser_adapter().unwrap();
        })
    }

    fn with_existing(register: impl FnOnce(&Arc<ComputerUseBroker>)) -> Self {
        let desktop = Arc::new(FakeAdapter::new());
        let broker = Arc::new(ComputerUseBroker::new(
            desktop.clone(),
            BrokerOptions {
                feature_enabled: false,
                lease_path: std::env::temp_dir()
                    .join(format!("cu-task-existing-{}.lease", Uuid::new_v4())),
                ..Default::default()
            },
        ));
        register(&broker);
        let host = broker.tabs();
        let challenge = host.begin_pairing_challenge();
        host.confirm_pairing_app_for(&challenge.nonce).unwrap();
        let pairing = host
            .complete_pairing_request(&ExtensionPairing::proof_for(
                &challenge,
                "00000000-0000-4000-8000-000000000001",
            ))
            .unwrap();
        let origin = format!("chrome-extension://{EXTENSION_ID}");
        host.offer_connected_tab_document(
            &connection(&pairing),
            1,
            SharedTabOffer {
                pairing_token: &pairing.session_key,
                origin: &origin,
                extension_id: Some(EXTENSION_ID),
                tab_id: "101",
                title: "Shared fixture",
                url: "https://fixture.invalid/",
                browser_id: "chromium",
                profile_id: "fixture",
                document_generation: 7,
                connection_generation: pairing.generation,
                focused: true,
            },
            Some("0123456789abcdef0123456789abcdef"),
        )
        .unwrap();
        let issued = broker.reserve_transport_connection("existing-app").unwrap();
        broker
            .bind_transport_agent(issued.token(), "synthetic-existing-agent")
            .unwrap();
        broker
            .submit_access(
                issued.token(),
                json!({
                    "action":"request", "purpose":"synthetic shared tab task",
                    "surfaces":["existing-tabs"], "scopeIntent":"original shared document",
                    "capabilities":["observe", "input"]
                }),
            )
            .unwrap();
        let key = broker
            .host_task_view(issued.token())
            .unwrap()
            .request
            .unwrap()
            .key;
        Self {
            broker,
            desktop,
            pairing,
            origin,
            token: issued.token().into(),
            key,
        }
    }

    fn select(&self) -> HostScopeSelection {
        self.broker
            .capture_task_scope_selection(
                &self.token,
                &self.key.request_id,
                self.key.revision,
                SurfaceKind::ExistingTab,
                "tab:101",
            )
            .unwrap()
    }

    fn no_execution(&self) {
        assert_eq!(self.broker.executable_grant_count("existing-app"), 0);
        assert_eq!(self.broker.continuation_count("existing-app"), 0);
        self.no_io();
    }

    fn no_io(&self) {
        assert!(self
            .broker
            .tabs()
            .poll_extension_request(
                &self.origin,
                &self.pairing.session_key,
                &connection(&self.pairing),
            )
            .unwrap()
            .is_none());
        assert_eq!(self.desktop.observe_calls(), 0);
        assert!(self.desktop.executions().is_empty());
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.broker.fence_task_consent("existing-app");
        for ticket in self.broker.take_cleanup_tickets() {
            self.broker.finish_stop_cleanup(&ticket).unwrap();
        }
    }
}

fn connection(pairing: &PairingSession) -> PairingConnection {
    PairingConnection {
        instance_id: pairing.instance_id.clone(),
        connection_nonce: pairing.connection_nonce.clone(),
        generation: pairing.generation,
    }
}

#[test]
fn existing_task_selection_uses_shared_instance_without_borrow_or_content() {
    let f = Fixture::new();
    let selected = f.select();
    assert_eq!(selected.target_id(), "tab:101");
    assert!(selected.members()[0].native.is_some());
    assert!(f.broker.tabs().existing_target_info("101").is_none());
    assert!(!f.broker.feature_enabled());
    f.no_execution();
}

#[test]
fn existing_task_original_reservation_borrows_then_stop_returns_without_close() {
    let f = Fixture::new();
    let AccessApprovalReady::Prepare(mut approval) = f
        .broker
        .reserve_selected_access_approval(&f.token, f.select())
        .unwrap()
    else {
        panic!("prepare")
    };
    f.no_execution();
    approval.publish_feature().unwrap();
    approval.prepare_execution().unwrap();
    f.no_execution();
    let granted = approval.commit().unwrap();
    assert_eq!(granted.binding.target_id, "tab:101");
    let borrowed = f.broker.tabs().existing_target_info("101").unwrap();
    assert_eq!(borrowed.run_id, granted.run_id);
    assert_eq!(borrowed.session, "existing-app");
    assert!(borrowed.borrowed && borrowed.user_owned && !borrowed.closed);
    f.broker.fence_task_consent("existing-app");
    for ticket in f.broker.take_cleanup_tickets() {
        f.broker.finish_stop_cleanup(&ticket).unwrap();
    }
    assert!(f.broker.tabs().existing_target_info("101").is_none());
    assert_eq!(
        f.broker.tabs().list_shared_candidates().len(),
        1,
        "Stop must not close/unshare user tab"
    );
    assert_eq!(f.broker.executable_grant_count("existing-app"), 0);
    assert!(f
        .broker
        .execution_run(&f.token, "computer_observe")
        .is_err());
    assert!(!f.broker.tabs().is_closed("101"));
    f.no_io();
}

fn reoffer(f: &Fixture, sequence: u64, document: Option<&str>) -> Result<(), String> {
    f.broker.tabs().offer_connected_tab_document(
        &connection(&f.pairing),
        sequence,
        SharedTabOffer {
            pairing_token: &f.pairing.session_key,
            origin: &f.origin,
            extension_id: Some(EXTENSION_ID),
            tab_id: "101",
            title: "Shared fixture",
            url: "https://fixture.invalid/",
            browser_id: "chromium",
            profile_id: "fixture",
            document_generation: 7,
            connection_generation: f.pairing.generation,
            focused: true,
        },
        document,
    )
}

#[test]
fn existing_task_reshare_same_tab_document_and_generation_retires_original_receipt() {
    let f = Fixture::new();
    let old = f.select();
    reoffer(&f, 2, Some("0123456789abcdef0123456789abcdef")).unwrap();
    assert!(f
        .broker
        .reserve_selected_access_approval(&f.token, old)
        .is_err());
    assert!(f.broker.tabs().existing_target_info("101").is_none());
    f.no_execution();
}

#[test]
fn existing_task_changed_document_before_commit_rolls_back_only_original_borrow() {
    let f = Fixture::new();
    let AccessApprovalReady::Prepare(mut approval) = f
        .broker
        .reserve_selected_access_approval(&f.token, f.select())
        .unwrap()
    else {
        panic!("prepare")
    };
    approval.publish_feature().unwrap();
    approval.prepare_execution().unwrap();
    reoffer(&f, 2, Some("fedcba9876543210fedcba9876543210")).unwrap();
    assert!(approval.prepare_publication().is_err());
    approval.abort();
    for ticket in f.broker.take_cleanup_tickets() {
        f.broker.finish_stop_cleanup(&ticket).unwrap();
    }
    assert!(!f.broker.feature_enabled());
    assert!(f.broker.tabs().existing_target_info("101").is_none());
    assert!(!f.broker.tabs().is_closed("101"));
    assert_eq!(f.broker.tabs().list_shared_candidates().len(), 1);
    f.no_execution();
}

#[test]
fn existing_task_preparation_cannot_adopt_a_legacy_or_foreign_borrow() {
    let f = Fixture::new();
    let selector = f.broker.tabs().list_shared_candidates()[0].0.clone();
    f.broker
        .tabs()
        .grant_picker_tab("foreign-app", "foreign-run", &selector)
        .unwrap();
    let AccessApprovalReady::Prepare(mut approval) = f
        .broker
        .reserve_selected_access_approval(&f.token, f.select())
        .unwrap()
    else {
        panic!("prepare")
    };
    approval.publish_feature().unwrap();
    assert!(approval.prepare_execution().is_err());
    approval.abort();
    for ticket in f.broker.take_cleanup_tickets() {
        f.broker.finish_stop_cleanup(&ticket).unwrap();
    }
    let borrowed = f.broker.tabs().existing_target_info("101").unwrap();
    assert_eq!(borrowed.run_id, "foreign-run");
    assert_eq!(borrowed.session, "foreign-app");
    assert!(!borrowed.closed);
    f.broker
        .tabs()
        .return_borrowed("foreign-run", "101")
        .unwrap();
    f.no_execution();
}

#[test]
fn existing_task_missing_document_proof_does_not_disable_legacy_share() {
    let f = Fixture::new();
    reoffer(&f, 2, None).unwrap();
    assert!(f
        .broker
        .capture_task_scope_selection(
            &f.token,
            &f.key.request_id,
            f.key.revision,
            SurfaceKind::ExistingTab,
            "tab:101"
        )
        .is_err());
    let selector = f.broker.tabs().list_shared_candidates()[0].0.clone();
    let legacy = f
        .broker
        .tabs()
        .grant_picker_tab("legacy-app", "legacy-run", &selector)
        .unwrap();
    assert!(legacy.borrowed && !legacy.closed);
    f.broker
        .tabs()
        .return_borrowed("legacy-run", "101")
        .unwrap();
    f.no_execution();
}

#[test]
fn existing_task_invalid_document_offer_keeps_original_proof_and_sequence() {
    let f = Fixture::new();
    let expected = f.broker.tabs().existing_scope_identity("tab:101").unwrap();
    for invalid in ["", "0123456789abcdef", "z123456789abcdef0123456789abcdef"] {
        assert!(reoffer(&f, 2, Some(invalid)).is_err());
        assert_eq!(
            f.broker.tabs().existing_scope_identity("tab:101").unwrap(),
            expected
        );
    }
    assert!(!format!("{expected:?}").contains("0123456789abcdef"));
    reoffer(&f, 2, Some("0123456789abcdef0123456789abcdef")).unwrap();
    f.no_execution();
}

#[test]
fn existing_task_cancelled_reservation_never_borrows_a_shared_tab() {
    let f = Fixture::new();
    let AccessApprovalReady::Prepare(mut approval) = f
        .broker
        .reserve_selected_access_approval(&f.token, f.select())
        .unwrap()
    else {
        panic!("prepare")
    };
    approval.cancel_handle().fence();
    assert!(approval.publish_feature().is_err());
    assert!(approval.prepare_execution().is_err());
    approval.abort();
    assert!(f.broker.tabs().existing_target_info("101").is_none());
    f.no_execution();
}
