use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

const DOCUMENT: &str = "0123456789abcdef0123456789abcdef";

fn offer_tab(f: &Fixture, tab: &str, sequence: u64) {
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
                title: "",
                url: "https://fixture.invalid/",
                browser_id: "chromium",
                profile_id: "fixture",
                document_generation: 7,
                connection_generation: f.pairing.generation,
                focused: true,
            },
            Some(DOCUMENT),
        )
        .unwrap();
}

#[test]
fn existing_task_candidates_require_canonical_tab_ids_without_borrow_or_content() {
    let f = Fixture::new();
    for (index, tab) in ["001", "x", "9007199254740992", "102"]
        .into_iter()
        .enumerate()
    {
        offer_tab(&f, tab, index as u64 + 2);
    }
    for invalid in [
        "tab:001",
        "tab:x",
        "tab:9007199254740992",
        "101",
        "tab:+101",
    ] {
        assert!(f.broker.tabs().existing_scope_identity(invalid).is_err());
    }
    let host = f.broker.tabs();
    let page = host.existing_scope_candidates(64, &|| true).unwrap();
    assert_eq!(
        page.candidates
            .iter()
            .map(|row| row.info.target_id.as_str())
            .collect::<Vec<_>>(),
        ["tab:101", "tab:102"]
    );
    assert!(!page.truncated);
    for row in page.candidates {
        assert!(host
            .existing_target_info(row.info.target_id.strip_prefix("tab:").unwrap())
            .is_none());
    }
    f.no_execution();
}

#[test]
fn existing_task_candidate_limit_cancellation_and_occupied_tab_never_read_content() {
    let f = Fixture::new();
    offer_tab(&f, "102", 2);
    let host = f.broker.tabs();
    let page = host.existing_scope_candidates(1, &|| true).unwrap();
    assert_eq!(page.candidates.len(), 1);
    assert!(page.truncated);
    let empty = host.existing_scope_candidates(0, &|| true).unwrap();
    assert!(empty.candidates.is_empty() && empty.truncated);
    let calls = AtomicUsize::new(0);
    assert!(host
        .existing_scope_candidates(64, &|| calls.fetch_add(1, Ordering::SeqCst) < 2)
        .is_err());
    f.no_execution();
    let selector = host
        .list_shared_candidates()
        .into_iter()
        .find(|(id, _, _)| id.starts_with("101@"))
        .unwrap()
        .0;
    host.grant_picker_tab("foreign", "foreign-run", &selector)
        .unwrap();
    let page = host.existing_scope_candidates(64, &|| true).unwrap();
    assert_eq!(page.candidates.len(), 1);
    assert_eq!(page.candidates[0].info.target_id, "tab:102");
    assert_eq!(
        host.existing_target_info("101").unwrap().run_id,
        "foreign-run"
    );
    host.return_borrowed("foreign-run", "101").unwrap();
    f.no_execution();
}

#[test]
fn existing_task_original_proof_cannot_acquire_replacement_connection() {
    let mut f = Fixture::new();
    let old_selection = f.select();
    let host = f.broker.tabs();
    let original = host.existing_scope_identity("tab:101").unwrap();
    let challenge = host.begin_pairing_challenge();
    host.confirm_pairing_app_for(&challenge.nonce).unwrap();
    f.pairing = host
        .complete_pairing_request(&ExtensionPairing::proof_for(
            &challenge,
            "00000000-0000-4000-8000-000000000002",
        ))
        .unwrap();
    reoffer(&f, 1, Some(DOCUMENT)).unwrap();
    assert_ne!(host.existing_scope_identity("tab:101").unwrap(), original);
    assert!(host
        .acquire_scope_tab("existing-app", "old-run", &original)
        .is_err());
    assert!(f
        .broker
        .reserve_selected_access_approval(&f.token, old_selection)
        .is_err());
    assert!(host.existing_target_info("101").is_none());
    assert_eq!(f.select().target_id(), "tab:101");
    f.no_execution();
}

#[test]
fn existing_task_expired_pairing_cannot_borrow_and_does_not_close_user_tab() {
    let f = Fixture::new();
    let host = f.broker.tabs();
    let proof = host.existing_scope_identity("tab:101").unwrap();
    // Keep an original borrowed record, so closed=false is an actual lifecycle
    // assertion, not an inference from an absent/unshared candidate.
    assert!(
        host.acquire_scope_tab("existing-app", "old-run", &proof)
            .unwrap()
            .0
            .borrowed
    );
    host.expire_pairing_connection_for_test();
    assert!(host
        .acquire_scope_tab("existing-app", "old-run", &proof)
        .is_err());
    assert!(host
        .existing_scope_candidates(64, &|| true)
        .unwrap()
        .candidates
        .is_empty());
    assert!(host.existing_target_info("101").is_none());
    assert!(!host.is_closed("101"));
    assert_eq!(f.broker.executable_grant_count("existing-app"), 0);
    assert_eq!(f.broker.continuation_count("existing-app"), 0);
    assert_eq!(f.desktop.observe_calls(), 0);
    assert!(f.desktop.executions().is_empty());
}

#[test]
fn existing_task_proof_is_host_specific_and_debug_is_redacted() {
    let f = Fixture::new();
    let other = Fixture::new();
    let proof = f.broker.tabs().existing_scope_identity("tab:101").unwrap();
    assert!(other
        .broker
        .tabs()
        .acquire_scope_tab("existing-app", "old-run", &proof)
        .is_err());
    assert_eq!(format!("{proof:?}"), "ExistingScopeIdentity { .. }");
    for value in [
        &f.pairing.session_key,
        &f.pairing.connection_nonce,
        &f.pairing.instance_id,
    ] {
        assert!(!format!("{proof:?}").contains(value));
    }
    f.no_execution();
    other.no_execution();
}
