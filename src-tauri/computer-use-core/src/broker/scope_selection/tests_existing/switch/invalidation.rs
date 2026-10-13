use super::*;

#[test]
fn existing_task_group_original_destination_proof_rejects_reshare_document_unshare_and_new_connection(
) {
    for mode in ["reshare", "document", "unshare", "connection", "expiry"] {
        let f = Fixture::new();
        let outcome = approve_group(&f);
        let before = f.broker.authorized_target(&outcome.run_id).unwrap();
        let host = f.broker.tabs();
        match mode {
            "reshare" => share(&f, 4, "102", "Second"),
            "document" => {
                share_document(&f, 4, "102", "Second", "fedcba9876543210fedcba9876543210")
            }
            "unshare" => {
                host.unoffer_connected_tab(
                    &f.origin,
                    &f.pairing.session_key,
                    &connection(&f.pairing),
                    4,
                    "102",
                    7,
                )
                .unwrap();
            }
            _ => {
                if mode == "expiry" {
                    host.expire_pairing_connection_for_test();
                    assert!(host.expire_pairing_connection());
                }
                let challenge = host.begin_pairing_challenge();
                host.confirm_pairing_app_for(&challenge.nonce).unwrap();
                host.complete_pairing_request(&ExtensionPairing::proof_for(
                    &challenge,
                    "00000000-0000-4000-8000-000000000002",
                ))
                .unwrap();
            }
        }
        assert!(open(&f, &outcome.run_id, "102").is_err(), "{mode}");
        assert_eq!(f.broker.authorized_target(&outcome.run_id).unwrap(), before);
        assert!(host.existing_target_info("102").is_none());
        assert!(f.desktop.executions().is_empty());
        assert_eq!(f.desktop.observe_calls(), 0);
    }
}

#[test]
fn existing_task_group_same_run_inactive_legacy_borrow_is_not_adopted_or_released() {
    let f = Fixture::new();
    let outcome = approve_group(&f);
    let before = f.broker.authorized_target(&outcome.run_id).unwrap();
    let host = f.broker.tabs();
    let selector = host
        .list_shared_candidates()
        .into_iter()
        .find(|(id, _, _)| id.starts_with("102@"))
        .unwrap()
        .0;
    let original = host
        .grant_picker_tab("existing-app", &outcome.run_id, &selector)
        .unwrap();
    assert!(open(&f, &outcome.run_id, "102").is_err());
    assert_eq!(f.broker.authorized_target(&outcome.run_id).unwrap(), before);
    assert_eq!(
        host.existing_target_info("102").unwrap().generation,
        original.generation
    );
    host.return_borrowed(&outcome.run_id, "102").unwrap();
    open(&f, &outcome.run_id, "102").unwrap();
    f.no_io();
}
