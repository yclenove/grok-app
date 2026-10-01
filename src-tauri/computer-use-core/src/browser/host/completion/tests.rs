use super::*;
use crate::browser::extension_completion::CompletionPhase;
use crate::browser::extension_protocol::*;
use crate::browser::{SharedTabOffer, TabDisconnect};
use crate::pairing::{ExtensionPairing, PairingConnection, PairingSession, EXTENSION_ID};

fn pair(host: &ExistingTabHost) -> PairingSession {
    let challenge = host.begin_pairing_challenge();
    host.confirm_pairing_app_for(&challenge.nonce).unwrap();
    host.complete_pairing_request(&ExtensionPairing::proof_for(
        &challenge,
        &uuid::Uuid::new_v4().to_string(),
    ))
    .unwrap()
}

fn connection(session: &PairingSession) -> PairingConnection {
    PairingConnection {
        instance_id: session.instance_id.clone(),
        connection_nonce: session.connection_nonce.clone(),
        generation: session.generation,
    }
}

fn share(host: &ExistingTabHost, session: &PairingSession, tab: &str, sequence: u64) -> String {
    host.offer_connected_tab(
        &connection(session),
        sequence,
        SharedTabOffer {
            pairing_token: &session.session_key,
            origin: &format!("chrome-extension://{EXTENSION_ID}"),
            extension_id: Some(EXTENSION_ID),
            tab_id: tab,
            title: "fixture",
            url: "https://fixture.invalid/",
            browser_id: "chromium",
            profile_id: "fixture",
            document_generation: 7,
            connection_generation: session.generation,
            focused: true,
        },
    )
    .unwrap();
    host.list_shared_candidates()
        .into_iter()
        .find(|(id, _, _)| id.starts_with(&format!("{tab}@")))
        .unwrap()
        .0
}

fn observe(host: &ExistingTabHost, session: &PairingSession, origin: &str, tab: &str) -> String {
    let (request, receiver) = host
        .enqueue_extension_observe("owner", &format!("run-{tab}"), tab, false)
        .unwrap();
    host.poll_extension_request(origin, &session.session_key, &connection(session))
        .unwrap()
        .unwrap();
    let ExtensionCommand::Observe { snapshot_id, .. } = &request.command;
    let snapshot = snapshot_id.clone();
    host.complete_extension_request(
        origin,
        &session.session_key,
        ExtensionResult {
            outcome: ExtensionOutcome::Observation {
                observation: ExtensionObservation {
                    snapshot_id: snapshot.clone(),
                    document_id: "0123456789abcdef0123456789abcdef".into(),
                    title: "fixture".into(),
                    url: "https://fixture.invalid/".into(),
                    text: "visible fixture".into(),
                    nodes: vec![],
                    viewport_width: 800,
                    viewport_height: 600,
                    truncated: false,
                    screenshot: None,
                },
            },
            request,
        },
    )
    .unwrap();
    receiver.recv().unwrap().unwrap();
    snapshot
}

fn fixture() -> (ExistingTabHost, PairingSession, String, String) {
    let host = ExistingTabHost::new();
    let session = pair(&host);
    let origin = format!("chrome-extension://{EXTENSION_ID}");
    let selector = share(&host, &session, "101", 1);
    host.grant_picker_tab("owner", "run-101", &selector)
        .unwrap();
    let snapshot = observe(&host, &session, &origin, "101");
    (host, session, origin, snapshot)
}

#[test]
fn observation_and_action_cannot_replace_each_others_live_snapshot() {
    let (host, session, origin, snapshot) = fixture();
    let (request, receiver) = host
        .enqueue_extension_observe("owner", "run-101", "101", false)
        .unwrap();
    assert!(host
        .offer_existing_completion(
            "owner",
            "run-101",
            "101",
            &snapshot,
            ActionCancellation::default()
        )
        .is_err());
    host.cancel_existing_operations("run-101");
    assert!(receiver.recv().unwrap().is_err());
    assert!(host
        .poll_extension_request(&origin, &session.session_key, &request.connection)
        .unwrap()
        .is_none());
    let selector = share(&host, &session, "101", 2);
    host.grant_picker_tab("owner", "run-101", &selector)
        .unwrap();
    let snapshot = observe(&host, &session, &origin, "101");
    let proof = host
        .offer_existing_completion(
            "owner",
            "run-101",
            "101",
            &snapshot,
            ActionCancellation::default(),
        )
        .unwrap();
    assert!(host
        .enqueue_extension_observe("owner", "run-101", "101", false)
        .is_err());
    host.settle_existing_completion(&origin, &proof).unwrap();
    assert!(host
        .enqueue_extension_observe("owner", "run-101", "101", false)
        .is_ok());
}

#[test]
fn concurrent_claim_is_once_only_and_does_not_release_a_lost_reply() {
    let (host, session, origin, snapshot) = fixture();
    let proof = host
        .offer_existing_completion(
            "owner",
            "run-101",
            "101",
            &snapshot,
            ActionCancellation::default(),
        )
        .unwrap();
    let barrier = std::sync::Barrier::new(8);
    std::thread::scope(|scope| {
        let barrier = &barrier;
        let results = (0..8)
            .map(|_| {
                scope.spawn(|| {
                    barrier.wait();
                    host.claim_existing_completion(&origin, &session.session_key, &proof)
                        .is_ok()
                })
            })
            .collect::<Vec<_>>();
        assert_eq!(
            results
                .into_iter()
                .map(|result| result.join().unwrap())
                .filter(|claimed| *claimed)
                .count(),
            1
        );
    });
    assert!(!host.existing_operations_idle("run-101"));
    host.settle_existing_completion(&origin, &proof).unwrap();
    assert!(host.existing_operations_idle("run-101"));
}

#[test]
fn every_revocation_cancels_immediately_but_retains_claimed_occupancy() {
    for mode in [
        "pause",
        "stop",
        "return",
        "release",
        "navigate",
        "disconnect",
        "unshare",
        "reshare",
        "revoke",
        "re-pair",
    ] {
        let (host, session, origin, snapshot) = fixture();
        let cancellation = ActionCancellation::default();
        let proof = host
            .offer_existing_completion("owner", "run-101", "101", &snapshot, cancellation.clone())
            .unwrap();
        host.claim_existing_completion(&origin, &session.session_key, &proof)
            .unwrap();
        match mode {
            "pause" => {
                host.cancel_actions("run-101").unwrap();
            }
            "stop" => {
                host.cancel_run("run-101").unwrap();
            }
            "return" => {
                host.return_borrowed("run-101", "101").unwrap();
            }
            "release" => {
                assert!(host.release_existing_grant(&host.observe("run-101", "101").unwrap()));
            }
            "navigate" => {
                host.note_user_navigation("101", "https://fixture.invalid/next")
                    .unwrap();
            }
            "disconnect" => {
                host.disconnect_tab("101", TabDisconnect::Disconnect)
                    .unwrap();
            }
            "unshare" => {
                host.unoffer_connected_tab(
                    &origin,
                    &session.session_key,
                    &connection(&session),
                    2,
                    "101",
                    7,
                )
                .unwrap();
            }
            "reshare" => {
                share(&host, &session, "101", 2);
            }
            "revoke" => host.revoke_pairing(),
            _ => {
                pair(&host);
            }
        }
        assert!(
            cancellation.check().is_err(),
            "{mode} must fence without a later status query"
        );
        assert!(!host.existing_operations_idle("run-101"), "{mode}");
        assert!(host
            .claim_existing_completion(&origin, &session.session_key, &proof)
            .is_err());
        let status = host.existing_completion_status(&origin, &proof).unwrap();
        assert_eq!(status.phase, CompletionPhase::Claimed);
        assert!(status.cancel_requested);
        host.settle_existing_completion(&origin, &proof).unwrap();
        host.settle_existing_completion(&origin, &proof).unwrap();
        assert!(host.existing_operations_idle("run-101"));
        assert!(!host.is_closed("101"));
    }
}

#[test]
fn replacement_pairing_cannot_reuse_tab_until_old_receipt_settles() {
    let (host, session, origin, snapshot) = fixture();
    let proof = host
        .offer_existing_completion(
            "owner",
            "run-101",
            "101",
            &snapshot,
            ActionCancellation::default(),
        )
        .unwrap();
    host.claim_existing_completion(&origin, &session.session_key, &proof)
        .unwrap();
    let replacement = pair(&host);
    assert!(host
        .pairing_session_key()
        .is_some_and(|key| key != session.session_key));
    let selector = share(&host, &replacement, "101", 1);
    assert!(host
        .grant_picker_tab("new-owner", "new-run", &selector)
        .is_err());
    assert!(host
        .reconnect_tab("run-101", "101", 7, replacement.generation + 1)
        .is_err());
    assert!(host
        .existing_completion_status("https://fixture.invalid", &proof)
        .is_err());
    assert!(host
        .settle_existing_completion("https://fixture.invalid", &proof)
        .is_err());
    host.settle_existing_completion(&origin, &proof).unwrap();
    host.grant_picker_tab("new-owner", "new-run", &selector)
        .unwrap();
    host.settle_existing_completion(&origin, &proof).unwrap();
    assert_eq!(host.existing_target_info("101").unwrap().run_id, "new-run");
    assert!(host
        .check_pairing_connection(&origin, &session.session_key, &connection(&session), false)
        .is_err());
}

#[test]
fn offering_requires_current_session_run_snapshot_and_grant() {
    let (host, _, _, snapshot) = fixture();
    for (owner, run, tab, snap) in [
        ("other", "run-101", "101", snapshot.as_str()),
        ("owner", "other", "101", snapshot.as_str()),
        ("owner", "run-101", "102", snapshot.as_str()),
        ("owner", "run-101", "101", "stale"),
    ] {
        assert!(host
            .offer_existing_completion(owner, run, tab, snap, ActionCancellation::default())
            .is_err());
    }
    host.return_borrowed("run-101", "101").unwrap();
    assert!(host
        .offer_existing_completion(
            "owner",
            "run-101",
            "101",
            &snapshot,
            ActionCancellation::default()
        )
        .is_err());
}

#[test]
fn completion_offer_rejects_an_observation_from_the_previous_document_generation() {
    let (host, _, _, snapshot) = fixture();
    host.note_user_navigation("101", "https://fixture.invalid/next")
        .unwrap();
    // Keep candidate metadata current to isolate the old observation check.
    host.inner
        .lock()
        .shared
        .get_mut("101")
        .unwrap()
        .document_generation = 8;
    assert!(host
        .offer_existing_completion(
            "owner",
            "run-101",
            "101",
            &snapshot,
            ActionCancellation::default()
        )
        .is_err());
}

#[test]
fn mixed_observation_and_completion_capacity_is_bounded_together() {
    let (host, session, origin, snapshot) = fixture();
    let mut snapshots = vec![("101".to_string(), snapshot)];
    for number in 102..=109 {
        let tab = number.to_string();
        let selector = share(&host, &session, &tab, number - 100);
        host.grant_picker_tab("owner", &format!("run-{tab}"), &selector)
            .unwrap();
        snapshots.push((tab.clone(), observe(&host, &session, &origin, &tab)));
    }
    let mut proofs = Vec::new();
    let mut receivers = Vec::new();
    for (tab, snapshot) in &snapshots[..4] {
        let proof = host
            .offer_existing_completion(
                "owner",
                &format!("run-{tab}"),
                tab,
                snapshot,
                ActionCancellation::default(),
            )
            .unwrap();
        host.claim_existing_completion(&origin, &session.session_key, &proof)
            .unwrap();
        proofs.push(proof);
    }
    for (tab, _) in &snapshots[4..8] {
        receivers.push(
            host.enqueue_extension_observe("owner", &format!("run-{tab}"), tab, false)
                .unwrap()
                .1,
        );
    }
    assert!(host
        .offer_existing_completion(
            "owner",
            "run-109",
            "109",
            &snapshots[8].1,
            ActionCancellation::default()
        )
        .is_err());
    assert!(host
        .enqueue_extension_observe("owner", "run-109", "109", false)
        .is_err());
    host.cancel_actions("run-101").unwrap();
    assert!(host
        .enqueue_extension_observe("owner", "run-109", "109", false)
        .is_err());
    host.settle_existing_completion(&origin, &proofs[0])
        .unwrap();
    receivers.push(
        host.enqueue_extension_observe("owner", "run-109", "109", false)
            .unwrap()
            .1,
    );
    assert!(!host.existing_operations_idle("run-102"));
}

#[test]
fn claim_racing_pause_never_loses_successfully_claimed_occupancy() {
    for _ in 0..16 {
        let (host, session, origin, snapshot) = fixture();
        let proof = host
            .offer_existing_completion(
                "owner",
                "run-101",
                "101",
                &snapshot,
                ActionCancellation::default(),
            )
            .unwrap();
        let barrier = std::sync::Barrier::new(2);
        let claimed = std::thread::scope(|scope| {
            let claim = scope.spawn(|| {
                barrier.wait();
                host.claim_existing_completion(&origin, &session.session_key, &proof)
                    .is_ok()
            });
            barrier.wait();
            host.cancel_actions("run-101").unwrap();
            claim.join().unwrap()
        });
        assert_eq!(host.existing_operations_idle("run-101"), !claimed);
        assert!(
            host.existing_completion_status(&origin, &proof)
                .unwrap()
                .cancel_requested
        );
        assert!(host
            .claim_existing_completion(&origin, &session.session_key, &proof)
            .is_err());
        host.settle_existing_completion(&origin, &proof).unwrap();
        assert!(host.existing_operations_idle("run-101"));
    }
}
