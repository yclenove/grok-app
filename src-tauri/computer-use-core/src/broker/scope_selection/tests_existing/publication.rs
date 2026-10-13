//! Original authority commit after native preparation, synthetic registry updates.
use super::*;

fn offer(f: &Fixture, tab: &str, sequence: u64) {
    f.broker
        .offer_connected_tab_document(
            &connection(&f.pairing),
            sequence,
            SharedTabOffer {
                pairing_token: &f.pairing.session_key,
                origin: &f.origin,
                extension_id: Some(EXTENSION_ID),
                tab_id: tab,
                title: "Publication fixture",
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

fn unshare(f: &Fixture, tab: &str) {
    assert!(f
        .broker
        .unoffer_connected_tab(
            &f.origin,
            &f.pairing.session_key,
            &connection(&f.pairing),
            4,
            tab,
            7,
        )
        .unwrap());
}

fn prepared(f: &Fixture) -> AccessApproval {
    // Offers before enable use the explicitly synthetic fixture registry.
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
                    title: "Publication fixture",
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
    let selection = f
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
    let AccessApprovalReady::Prepare(mut approval) = f
        .broker
        .reserve_selected_access_approval(&f.token, selection)
        .unwrap()
    else {
        panic!("original reservation")
    };
    approval.publish_feature().unwrap();
    approval.prepare_execution().unwrap();
    approval.prepare_publication().unwrap();
    assert!(f.broker.tabs().existing_target_info("102").is_none());
    f.no_execution();
    approval
}

fn rejected_after_preparation(mutation: &str) {
    let f = Fixture::new();
    let mut approval = prepared(&f);
    match mutation {
        "active-unshare" => unshare(&f, "101"),
        "inactive-unshare" => unshare(&f, "102"),
        "inactive-reshare" => offer(&f, "102", 4),
        "expired-connection" => f.broker.tabs().expire_pairing_connection_for_test(),
        _ => panic!("fixed mutation"),
    }
    let result = approval.commit_authority();
    let grants = f.broker.executable_grant_count("existing-app");
    let continuations = f.broker.continuation_count("existing-app");
    let mut shares = f.broker.tabs().list_shared_candidates();
    shares.sort();
    drop(approval);
    for ticket in f.broker.take_cleanup_tickets() {
        f.broker.finish_stop_cleanup(&ticket).unwrap();
    }
    assert!(
        result.is_err(),
        "{mutation} published a stale prepared scope"
    );
    assert_eq!(grants, 0, "{mutation}");
    assert_eq!(continuations, 0, "{mutation}");
    assert!(!f.broker.feature_enabled());
    assert!(!f.broker.tabs().is_closed("101"));
    // Inactive B was never borrowed, so it has no TabRecord. is_closed()
    // returns true for an absent record; it is not a physical browser oracle.
    assert!(f.broker.tabs().existing_target_info("102").is_none());
    let mut retained = f.broker.tabs().list_shared_candidates();
    retained.sort();
    assert_eq!(retained, shares, "cleanup changed another user share");
    assert_eq!(f.desktop.observe_calls(), 0);
    assert!(f.desktop.executions().is_empty());
}

#[test]
fn existing_task_publication_active_unshare_after_prepare_rejects_authority() {
    rejected_after_preparation("active-unshare");
}
#[test]
fn existing_task_publication_inactive_unshare_after_prepare_rejects_entire_group() {
    rejected_after_preparation("inactive-unshare");
}
#[test]
fn existing_task_publication_same_document_inactive_reshare_rejects_old_proof() {
    rejected_after_preparation("inactive-reshare");
}
#[test]
fn existing_task_publication_connection_expiry_after_prepare_rejects_authority() {
    rejected_after_preparation("expired-connection");
}
#[test]
fn existing_task_publication_executor_replacement_is_rejected_without_changing_authority() {
    let f = Fixture::new();
    let mut approval = prepared(&f);
    let original = f
        .broker
        .require_surface_executor(SurfaceKind::ExistingTab)
        .unwrap();
    // Registered executors are immutable. A rejected registration must not be
    // mislabeled as an actual replacement or a stale-authority counterexample.
    assert!(f
        .broker
        .register_surface_adapter(SurfaceKind::ExistingTab, Arc::new(FakeAdapter::new()))
        .is_err());
    let retained = f
        .broker
        .require_surface_executor(SurfaceKind::ExistingTab)
        .unwrap();
    assert_eq!(retained.generation, original.generation);
    assert!(Arc::ptr_eq(&retained.adapter, &original.adapter));
    approval.commit_authority().unwrap();
    assert_eq!(f.broker.executable_grant_count("existing-app"), 1);
    assert_eq!(f.broker.continuation_count("existing-app"), 1);
    f.no_io();
}
#[test]
fn existing_task_publication_unselected_share_change_preserves_original_group() {
    let f = Fixture::new();
    let mut approval = prepared(&f);
    unshare(&f, "103");
    let outcome = approval.commit_authority().unwrap();
    assert_eq!(outcome.binding.target_id, "tab:101");
    assert_eq!(f.broker.executable_grant_count("existing-app"), 1);
    assert_eq!(f.broker.continuation_count("existing-app"), 1);
    assert_eq!(
        f.broker.host_task_view(&f.token).unwrap().grants[0]
            .members
            .len(),
        2
    );
    f.no_io();
}
