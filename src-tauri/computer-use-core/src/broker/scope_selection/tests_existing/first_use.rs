//! Production Broker first-use path. Host gesture and extension proof are synthetic.
use super::*;

fn unpaired() -> (
    Arc<ComputerUseBroker>,
    Arc<FakeAdapter>,
    String,
    HostRequestKey,
) {
    let desktop = Arc::new(FakeAdapter::new());
    let broker = Arc::new(ComputerUseBroker::new(
        desktop.clone(),
        BrokerOptions {
            feature_enabled: false,
            lease_path: std::env::temp_dir().join(format!("cu-first-use-{}.lease", Uuid::new_v4())),
            ..Default::default()
        },
    ));
    broker.register_existing_browser_adapter().unwrap();
    let issued = broker
        .reserve_transport_connection("first-use-app")
        .unwrap();
    broker
        .bind_transport_agent(issued.token(), "synthetic-first-use-agent")
        .unwrap();
    broker
        .submit_access(
            issued.token(),
            json!({
                "action":"request", "purpose":"synthetic first-use task",
                "surfaces":["existing-tabs"], "scopeIntent":"user shared page",
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
    (broker, desktop, issued.token().into(), key)
}

#[test]
fn existing_first_use_picker_can_prepare_pairing_without_enabling_execution() {
    let (broker, desktop, token, key) = unpaired();
    assert!(
        broker.begin_pairing().is_err(),
        "generic feature-off pairing stays gated"
    );
    assert!(broker.tabs().pending_pairing_challenge().is_none());
    let picker = broker
        .begin_task_scope_picker(&token, &key, SurfaceKind::ExistingTab)
        .unwrap();
    let challenge = broker
        .tabs()
        .pending_pairing_challenge()
        .expect("explicit original Host ExistingTab selection must prepare a challenge while off");
    assert!(!challenge.expired());
    assert!(!broker.feature_enabled());
    assert_eq!(broker.test_run_count(), 0);
    assert_eq!(broker.executable_grant_count("first-use-app"), 0);
    assert_eq!(broker.continuation_count("first-use-app"), 0);
    assert_eq!(desktop.observe_calls(), 0);
    assert!(desktop.executions().is_empty());
    drop(picker);
    assert!(broker.tabs().pending_pairing_challenge().is_none());
}

#[test]
fn existing_preparation_readback_tracks_confirmation_share_compatibility_and_occupancy_without_io()
{
    let (broker, desktop, token, key) = unpaired();
    let picker = broker
        .begin_task_scope_picker(&token, &key, SurfaceKind::ExistingTab)
        .unwrap();
    assert_eq!(picker.surface(), SurfaceKind::ExistingTab);
    let initial = picker.existing_share_preparation().unwrap();
    assert!(initial.challenge.is_some());
    assert!(!initial.pairing_confirmed);
    assert_eq!(initial.available_tabs, 0);
    let pairing = pair_original(&broker, &picker);
    let empty = picker.existing_share_preparation().unwrap();
    assert!(empty.challenge.is_none());
    assert_eq!(empty.available_tabs, 0);
    for (seq, tab, doc) in [
        (1, "101", Some("0123456789abcdef0123456789abcdef")),
        (2, "102", None),
    ] {
        broker
            .offer_connected_tab_document(
                &connection(&pairing),
                seq,
                SharedTabOffer {
                    pairing_token: &pairing.session_key,
                    origin: &format!("chrome-extension://{EXTENSION_ID}"),
                    extension_id: Some(EXTENSION_ID),
                    tab_id: tab,
                    title: "Synthetic preparation metadata",
                    url: "https://fixture.invalid/",
                    browser_id: "chromium",
                    profile_id: "synthetic",
                    document_generation: 7,
                    connection_generation: pairing.generation,
                    focused: true,
                },
                doc,
            )
            .unwrap();
    }
    let ready = picker.existing_share_preparation().unwrap();
    assert_eq!(
        (
            ready.available_tabs,
            ready.incompatible_tabs,
            ready.borrowed_tabs
        ),
        (1, 1, 0)
    );
    let selector = broker
        .tabs()
        .list_shared_candidates()
        .into_iter()
        .find(|(selector, _, _)| selector.starts_with("101@"))
        .unwrap()
        .0;
    broker
        .tabs()
        .grant_picker_tab("foreign", "foreign-run", &selector)
        .unwrap();
    let busy = picker.existing_share_preparation().unwrap();
    assert_eq!(
        (
            busy.available_tabs,
            busy.incompatible_tabs,
            busy.borrowed_tabs
        ),
        (0, 1, 1)
    );
    broker.tabs().return_borrowed("foreign-run", "101").unwrap();
    assert_eq!(
        picker.existing_share_preparation().unwrap().available_tabs,
        1
    );
    picker.cancel_handle().fence();
    assert!(picker.existing_share_preparation().is_err());
    assert_eq!(broker.tabs().list_shared_candidates().len(), 2);
    no_authority(&broker, &desktop);
}

fn pair_original(broker: &ComputerUseBroker, picker: &HostScopePickerCall) -> PairingSession {
    let challenge = picker.pairing_challenge().unwrap().unwrap();
    picker.confirm_existing_pairing(&challenge.nonce).unwrap();
    broker
        .complete_pairing(&ExtensionPairing::proof_for(
            &challenge,
            "00000000-0000-4000-8000-000000000001",
        ))
        .unwrap()
}

fn no_authority(broker: &ComputerUseBroker, desktop: &FakeAdapter) {
    assert!(!broker.feature_enabled());
    assert_eq!(broker.test_run_count(), 0);
    assert_eq!(broker.executable_grant_count("first-use-app"), 0);
    assert!(broker.claim_continuation("first-use-app").is_none());
    assert_eq!(desktop.list_calls(), 0);
    assert_eq!(desktop.observe_calls(), 0);
    assert!(desktop.executions().is_empty());
}

#[test]
fn existing_first_use_wrong_full_key_surface_and_model_readback_never_prepare_pairing() {
    let (broker, desktop, token, key) = unpaired();
    for wrong in [
        HostRequestKey {
            connection_id: "foreign".into(),
            ..key.clone()
        },
        HostRequestKey {
            task_id: "foreign".into(),
            ..key.clone()
        },
        HostRequestKey {
            request_id: "foreign".into(),
            ..key.clone()
        },
        HostRequestKey {
            revision: key.revision + 1,
            ..key.clone()
        },
    ] {
        assert!(broker
            .begin_task_scope_picker(&token, &wrong, SurfaceKind::ExistingTab)
            .is_err());
    }
    assert!(broker
        .begin_task_scope_picker(&token, &key, SurfaceKind::Desktop)
        .is_err());
    assert!(broker
        .begin_task_scope_picker("foreign-token", &key, SurfaceKind::ExistingTab)
        .is_err());
    broker.host_task_view(&token).unwrap();
    broker
        .submit_access(&token, json!({"action":"status"}))
        .unwrap();
    assert!(broker.tabs().pending_pairing_challenge().is_none());
    no_authority(&broker, &desktop);
}

#[test]
fn existing_first_use_all_original_fences_retire_preparation_before_and_after_explicit_pair() {
    for paired in [false, true] {
        for mode in [
            "cancel",
            "model-cancel",
            "deny",
            "stop",
            "request",
            "revision",
            "agent",
            "transport",
            "epoch",
            "feature-off",
            "deadline",
            "request-ttl",
        ] {
            let (broker, desktop, token, key) = unpaired();
            let picker = broker
                .begin_task_scope_picker(&token, &key, SurfaceKind::ExistingTab)
                .unwrap();
            let challenge = picker.pairing_challenge().unwrap().unwrap();
            let session = paired.then(|| pair_original(&broker, &picker));
            match mode {
                "cancel" => picker.cancel_handle().fence(),
                "model-cancel" => {
                    broker
                        .submit_access(&token, json!({"action":"cancel"}))
                        .unwrap();
                }
                "deny" => {
                    broker
                        .decide_host_task(&token, &key, HostTaskDecision::Deny)
                        .unwrap();
                }
                "stop" => {
                    broker
                        .decide_host_task(&token, &key, HostTaskDecision::Stop)
                        .unwrap();
                }
                "request" => {
                    broker.submit_access(&token, json!({"action":"request", "purpose":"replacement task",
                    "surfaces":["existing-tabs"], "scopeIntent":"replacement share", "capabilities":["observe"]})).unwrap();
                }
                "revision" => {
                    broker.submit_access(&token, json!({"action":"request", "purpose":"synthetic first-use task",
                    "surfaces":["existing-tabs"], "scopeIntent":"user shared page", "capabilities":["observe"]})).unwrap();
                }
                "agent" => broker
                    .bind_agent_session("first-use-app", "replacement-agent")
                    .unwrap(),
                "transport" => {
                    broker.retire_transport_connection(&token);
                    broker
                        .reserve_transport_connection("first-use-app")
                        .unwrap();
                }
                "epoch" => {
                    broker.set_feature_enabled(true);
                    broker.set_feature_enabled(false);
                }
                "feature-off" => broker.set_feature_enabled(false),
                "deadline" => broker.expire_existing_share_preparation_for_test(),
                "request-ttl" => broker.advance_consent_clock(Duration::from_secs(601)),
                _ => unreachable!(),
            }
            assert!(
                picker.pairing_challenge().is_err(),
                "{mode} paired={paired}"
            );
            assert!(picker.confirm_existing_pairing(&challenge.nonce).is_err());
            assert!(
                picker.existing_share_preparation().is_err(),
                "{mode} paired={paired}"
            );
            assert!(broker
                .complete_pairing(&ExtensionPairing::proof_for(
                    &challenge,
                    "00000000-0000-4000-8000-000000000001"
                ))
                .is_err());
            assert!(broker.tabs().pending_pairing_challenge().is_none());
            if let Some(session) = session {
                assert!(broker
                    .check_pairing_connection(
                        &format!("chrome-extension://{EXTENSION_ID}"),
                        &session.session_key,
                        &connection(&session),
                        false
                    )
                    .is_err());
            }
            no_authority(&broker, &desktop);
        }
    }
}

#[test]
fn existing_first_use_old_cancel_and_drop_cannot_retire_a_new_challenge() {
    let (broker, desktop, token, key) = unpaired();
    let old = broker
        .begin_task_scope_picker(&token, &key, SurfaceKind::ExistingTab)
        .unwrap();
    let prior = old.pairing_challenge().unwrap().unwrap();
    let fresh = broker
        .begin_task_scope_picker(&token, &key, SurfaceKind::ExistingTab)
        .unwrap();
    let current = fresh.pairing_challenge().unwrap().unwrap();
    assert_ne!(prior.nonce, current.nonce);
    old.cancel_handle().fence();
    drop(old);
    assert_eq!(
        fresh.pairing_challenge().unwrap().unwrap().nonce,
        current.nonce
    );
    assert!(broker
        .complete_pairing(&ExtensionPairing::proof_for(
            &prior,
            "00000000-0000-4000-8000-000000000001"
        ))
        .is_err());
    let session = pair_original(&broker, &fresh);
    assert_eq!(
        broker.check_pairing_connection(
            &format!("chrome-extension://{EXTENSION_ID}"),
            &session.session_key,
            &connection(&session),
            false
        ),
        Ok(false)
    );
    no_authority(&broker, &desktop);
}

#[test]
fn existing_first_use_foreign_pending_is_not_overwritten_and_foreign_share_is_not_released() {
    let (broker, desktop, token, key) = unpaired();
    // Simulate an independent Host UI's pending device setup, not a task grant.
    let foreign = broker.tabs().begin_pairing_challenge();
    assert!(broker
        .begin_task_scope_picker(&token, &key, SurfaceKind::ExistingTab)
        .is_err());
    assert_eq!(
        broker.tabs().pending_pairing_challenge().unwrap().nonce,
        foreign.nonce
    );
    no_authority(&broker, &desktop);
    let f = Fixture::new();
    let shared = f.broker.tabs().list_shared_candidates()[0].0.clone();
    f.broker
        .tabs()
        .grant_picker_tab("foreign-app", "foreign-run", &shared)
        .unwrap();
    let picker = f
        .broker
        .begin_task_scope_picker(&f.token, &f.key, SurfaceKind::ExistingTab)
        .unwrap();
    assert!(picker.pairing_challenge().unwrap().is_none());
    drop(picker);
    assert_eq!(
        f.broker.tabs().existing_target_info("101").unwrap().run_id,
        "foreign-run"
    );
    assert_eq!(f.broker.tabs().list_shared_candidates()[0].0, shared);
    f.broker
        .tabs()
        .return_borrowed("foreign-run", "101")
        .unwrap();
    f.no_execution();
}

#[test]
fn existing_first_use_pair_completion_and_cancel_share_the_original_authority_lock() {
    for _ in 0..16 {
        let (broker, desktop, token, key) = unpaired();
        let picker = broker
            .begin_task_scope_picker(&token, &key, SurfaceKind::ExistingTab)
            .unwrap();
        let challenge = picker.pairing_challenge().unwrap().unwrap();
        picker.confirm_existing_pairing(&challenge.nonce).unwrap();
        let proof = ExtensionPairing::proof_for(&challenge, "00000000-0000-4000-8000-000000000001");
        let barrier = Arc::new(std::sync::Barrier::new(2));
        let peer = broker.clone();
        let ready = barrier.clone();
        let completion = std::thread::spawn(move || {
            ready.wait();
            peer.complete_pairing(&proof)
        });
        barrier.wait();
        picker.cancel_handle().fence();
        if let Ok(session) = completion.join().unwrap() {
            assert!(broker
                .check_pairing_connection(
                    &format!("chrome-extension://{EXTENSION_ID}"),
                    &session.session_key,
                    &connection(&session),
                    false
                )
                .is_err());
        }
        assert!(broker.tabs().pending_pairing_challenge().is_none());
        assert!(picker.pairing_challenge().is_err());
        no_authority(&broker, &desktop);
    }
}
