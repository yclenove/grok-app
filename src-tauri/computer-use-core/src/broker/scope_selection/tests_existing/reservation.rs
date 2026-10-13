//! Actual reservation entry; synthetic registry mutation, no browser or input.
use super::*;
use std::sync::atomic::{AtomicBool, Ordering};

fn reservation_at_original_entry(revoked: &'static str, rejected: bool) {
    let f = Fixture::new();
    for (tab, sequence) in [("102", 2), ("103", 3)] {
        f.broker
            .tabs()
            .offer_connected_tab_document(
                &connection(&f.pairing),
                sequence,
                SharedTabOffer {
                    pairing_token: &f.pairing.session_key,
                    origin: &f.origin,
                    extension_id: Some(EXTENSION_ID),
                    tab_id: tab,
                    title: "Reservation fixture",
                    url: "https://fixture.invalid/",
                    browser_id: "chromium",
                    profile_id: "fixture",
                    document_generation: 7,
                    connection_generation: f.pairing.generation,
                    focused: true,
                },
                Some("0123456789abcdef0123456789abcdef"),
            )
            .unwrap();
    }
    let selected = f
        .broker
        .capture_task_scope_group(
            &f.token,
            &f.key.request_id,
            f.key.revision,
            &[
                (SurfaceKind::ExistingTab, "tab:101"),
                (SurfaceKind::ExistingTab, "tab:102"),
            ],
        )
        .unwrap();
    let broker = f.broker.clone();
    let pairing = f.pairing.clone();
    let origin = f.origin.clone();
    let entered = Arc::new(AtomicBool::new(false));
    let observed = entered.clone();
    f.broker
        .arm_scope_reservation_hook_for_test(Box::new(move || {
            observed.store(true, Ordering::SeqCst);
            // This would deadlock if the hook still held the Broker/Host lock.
            assert!(!broker.feature_enabled());
            assert_eq!(broker.executable_grant_count("existing-app"), 0);
            assert_eq!(broker.continuation_count("existing-app"), 0);
            assert!(broker
                .tabs()
                .unoffer_connected_tab(
                    &origin,
                    &pairing.session_key,
                    &connection(&pairing),
                    4,
                    revoked,
                    7,
                )
                .unwrap());
        }));
    let result = f
        .broker
        .reserve_selected_access_approval(&f.token, selected);
    assert!(entered.load(Ordering::SeqCst));
    assert_eq!(result.is_err(), rejected);
    f.no_execution();
    assert!(!f.broker.feature_enabled());
    assert!(f.broker.tabs().existing_target_info("101").is_none());
    assert!(f.broker.tabs().existing_target_info("102").is_none());
    assert_eq!(f.broker.tabs().list_shared_candidates().len(), 2);
    drop(result);
}

#[test]
fn existing_task_reservation_active_unshare_at_original_entry_rejects_selection() {
    reservation_at_original_entry("101", true);
}

#[test]
fn existing_task_reservation_inactive_unshare_at_original_entry_rejects_group() {
    reservation_at_original_entry("102", true);
}

#[test]
fn existing_task_reservation_unselected_unshare_does_not_reject_original_group() {
    reservation_at_original_entry("103", false);
}
