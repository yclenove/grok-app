use super::*;
use crate::browser::extension_completion::CompletionPhase;
mod fixture;
use fixture::Fixture;

#[test]
fn negotiation_is_explicit_authenticated_immutable_and_cleared_on_repair() {
    let f = Fixture::new(false);
    let command = f.observe("101", 1);
    assert!(f
        .enqueue("101", command.clone(), ActionCancellation::default())
        .is_err());
    assert!(f
        .host
        .poll_existing_action(&f.origin, &f.session.session_key, &f.connection)
        .is_err());
    assert!(f
        .host
        .negotiate_existing_actions(&f.origin, "wrong", f.negotiation())
        .is_err());
    for version in [0, 1, 3] {
        let mut n = f.negotiation();
        n.protocol = version;
        assert!(f
            .host
            .negotiate_existing_actions(&f.origin, &f.session.session_key, n)
            .is_err());
    }
    f.negotiate();
    f.negotiate();
    let mut n = f.negotiation();
    n.actions.pop();
    assert!(f
        .host
        .negotiate_existing_actions(&f.origin, &f.session.session_key, n)
        .is_err());
    f.enqueue("101", command, ActionCancellation::default())
        .unwrap();
    f.host.begin_pairing_challenge();
    assert!(f.host.inner.lock().action_negotiation.is_none());
    assert!(f.host.existing_operations_idle("run-101"));
}

#[test]
fn legacy_poll_never_delivers_action_and_v2_delivery_is_once_only() {
    let f = Fixture::new(true);
    let command = f.observe("101", 1);
    let (_, _receiver) = f
        .enqueue("101", command, ActionCancellation::default())
        .unwrap();
    assert!(f
        .host
        .poll_extension_request(&f.origin, &f.session.session_key, &f.connection)
        .unwrap()
        .is_none());
    let dispatch = f.poll().unwrap();
    assert!(f.poll().is_none());
    assert!(f
        .host
        .claim_existing_completion(&f.origin, &f.session.session_key, &dispatch.proof)
        .is_err());
    f.claim(&dispatch).unwrap();
    assert!(f.claim(&dispatch).is_err());
}

#[test]
fn claim_binds_full_command_identity_and_original_ref() {
    let f = Fixture::new(true);
    let command = f.observe("101", 1);
    let (_, _receiver) = f
        .enqueue("101", command, ActionCancellation::default())
        .unwrap();
    let dispatch = f.poll().unwrap();
    let original = serde_json::to_value(&dispatch).unwrap();
    for (pointer, value) in [
        ("/request/sequence", serde_json::json!(999)),
        ("/request/deadlineMs", serde_json::json!(1)),
        ("/request/session", serde_json::json!("other")),
        ("/request/runId", serde_json::json!("other")),
        ("/request/documentId", serde_json::json!("f".repeat(32))),
        ("/request/grantGeneration", serde_json::json!(999)),
        ("/request/documentGeneration", serde_json::json!(999)),
        (
            "/request/command/elementRef",
            serde_json::json!(format!("{}-2", dispatch.request.command.snapshot_id())),
        ),
        ("/request/command/parameters/count", serde_json::json!(2)),
        ("/proof/completionKey", serde_json::json!("f".repeat(64))),
    ] {
        let mut changed = original.clone();
        *changed.pointer_mut(pointer).unwrap() = value;
        let changed: ActionDispatch = serde_json::from_value(changed).unwrap();
        assert!(f.claim(&changed).is_err(), "{pointer}");
        assert!(
            f.result(&changed, ActionStatus::Unknown).is_err(),
            "{pointer}"
        );
    }
    f.claim(&dispatch).unwrap();
}

#[test]
fn admission_rejects_unknown_ref_and_stale_generations_without_occupancy() {
    let f = Fixture::new(true);
    let command = f.observe("101", 1);
    let mut wire = serde_json::to_value(&command).unwrap();
    wire["elementRef"] = serde_json::json!(format!("{}-missing", command.snapshot_id()));
    assert!(f
        .enqueue(
            "101",
            serde_json::from_value(wire).unwrap(),
            ActionCancellation::default()
        )
        .is_err());
    let info = f.host.inner.lock().tabs["101"].info.clone();
    for (grant, document) in [
        (info.generation + 1, info.document_generation),
        (info.generation, info.document_generation + 1),
    ] {
        assert!(f
            .host
            .enqueue_existing_action(ExistingActionInput {
                session: "owner",
                run: "run-101",
                tab: "101",
                grant_generation: grant,
                document_generation: document,
                command: command.clone(),
                cancellation: ActionCancellation::default()
            })
            .is_err());
    }
    assert!(f.host.existing_operations_idle("run-101"));
}

#[test]
fn success_requires_claim_and_physical_settlement_then_single_result() {
    let f = Fixture::new(true);
    let command = f.observe("101", 1);
    let (_, receiver) = f
        .enqueue("101", command, ActionCancellation::default())
        .unwrap();
    let dispatch = f.poll().unwrap();
    assert!(f.result(&dispatch, ActionStatus::Verified).is_err());
    f.claim(&dispatch).unwrap();
    assert!(f.result(&dispatch, ActionStatus::Applied).is_err());
    f.host
        .settle_existing_completion(&f.origin, &dispatch.proof)
        .unwrap();
    assert!(
        !f.host.existing_operations_idle("run-101"),
        "business result still owns the slot"
    );
    assert!(f
        .host
        .enqueue_extension_observe("owner", "run-101", "101", false)
        .is_err());
    f.result(&dispatch, ActionStatus::Verified).unwrap();
    assert_eq!(
        receiver.recv().unwrap().unwrap().status,
        ActionStatus::Verified
    );
    assert!(f.result(&dispatch, ActionStatus::Verified).is_err());
    assert!(f.host.existing_operations_idle("run-101"));
}

#[test]
fn settled_success_survives_completion_tombstone_expiry() {
    let f = Fixture::new(true);
    let command = f.observe("101", 1);
    let (_, receiver) = f
        .enqueue("101", command, ActionCancellation::default())
        .unwrap();
    let dispatch = f.poll().unwrap();
    f.claim(&dispatch).unwrap();
    f.host
        .settle_existing_completion(&f.origin, &dispatch.proof)
        .unwrap();
    f.host.expire_completion_tombstones_for_test();
    assert!(f
        .host
        .existing_completion_status(&f.origin, &dispatch.proof)
        .is_err());
    assert_eq!(
        f.host
            .existing_completion_retirement(&f.origin, &dispatch.proof)
            .unwrap(),
        crate::browser::extension_completion::CompletionRetirement::Absent
    );
    f.result(&dispatch, ActionStatus::Applied).unwrap();
    assert_eq!(
        receiver.recv().unwrap().unwrap().status,
        ActionStatus::Applied
    );
    assert!(f.result(&dispatch, ActionStatus::Applied).is_err());
    assert!(f.host.existing_operations_idle("run-101"));
}

#[test]
fn settlement_without_claim_cannot_publish_success() {
    let f = Fixture::new(true);
    let command = f.observe("101", 1);
    let (_, _receiver) = f
        .enqueue("101", command, ActionCancellation::default())
        .unwrap();
    let dispatch = f.poll().unwrap();
    f.host
        .settle_existing_completion(&f.origin, &dispatch.proof)
        .unwrap();
    assert!(f.result(&dispatch, ActionStatus::Verified).is_err());
    f.result(&dispatch, ActionStatus::Rejected).unwrap();
}

#[test]
fn unknown_result_does_not_release_physical_occupancy_or_allow_generic_claim() {
    let f = Fixture::new(true);
    let command = f.observe("101", 1);
    let (_, receiver) = f
        .enqueue("101", command, ActionCancellation::default())
        .unwrap();
    let dispatch = f.poll().unwrap();
    f.claim(&dispatch).unwrap();
    f.result(&dispatch, ActionStatus::Unknown).unwrap();
    assert_eq!(
        receiver.recv().unwrap().unwrap().status,
        ActionStatus::Unknown
    );
    assert!(!f.host.existing_operations_idle("run-101"));
    assert!(f
        .host
        .claim_existing_completion(&f.origin, &f.session.session_key, &dispatch.proof)
        .is_err());
    f.host
        .settle_existing_completion(&f.origin, &dispatch.proof)
        .unwrap();
    assert!(f.host.existing_operations_idle("run-101"));
}

#[test]
fn cancellation_or_deadline_releases_business_wait_but_keeps_claimed_receipt() {
    for timeout in [false, true] {
        for claimed in [false, true] {
            let f = Fixture::new(true);
            let command = f.observe("101", 1);
            let (_, receiver) = f
                .enqueue("101", command, ActionCancellation::default())
                .unwrap();
            let dispatch = f.poll().unwrap();
            if claimed {
                f.claim(&dispatch).unwrap();
            }
            if timeout {
                f.host.inner.lock().extension_actions[0].deadline = Instant::now();
            } else {
                f.host.cancel_existing_operations("run-101");
            }
            assert_eq!(f.host.existing_operations_idle("run-101"), !claimed);
            assert!(receiver.recv().unwrap().is_err());
            assert!(f.result(&dispatch, ActionStatus::Verified).is_err());
            let status = f
                .host
                .existing_completion_status(&f.origin, &dispatch.proof)
                .unwrap();
            assert!(status.cancel_requested);
            assert_eq!(
                status.phase,
                if claimed {
                    CompletionPhase::Claimed
                } else {
                    CompletionPhase::Settled
                }
            );
            f.host
                .settle_existing_completion(&f.origin, &dispatch.proof)
                .unwrap();
            assert!(f.host.existing_operations_idle("run-101"));
            assert!(!f.host.inner.lock().tabs["101"].info.closed);
        }
    }
}

#[test]
fn mixed_queue_capacity_counts_each_action_once_including_after_settlement() {
    let f = Fixture::new(true);
    let mut commands = Vec::new();
    for index in 0..9 {
        commands.push(f.observe(&(100 + index).to_string(), index + 1));
    }
    let mut receivers = Vec::new();
    for (index, command) in commands.iter().enumerate().take(8) {
        receivers.push(
            f.enqueue(
                &(100 + index).to_string(),
                command.clone(),
                ActionCancellation::default(),
            )
            .unwrap()
            .1,
        );
    }
    assert!(f
        .enqueue("108", commands[8].clone(), ActionCancellation::default())
        .is_err());
    let dispatch = f.poll().unwrap();
    f.claim(&dispatch).unwrap();
    f.host
        .settle_existing_completion(&f.origin, &dispatch.proof)
        .unwrap();
    assert!(f
        .host
        .enqueue_extension_observe("owner", "run-108", "108", false)
        .is_err());
    f.result(&dispatch, ActionStatus::Applied).unwrap();
    assert!(f
        .host
        .enqueue_extension_observe("owner", "run-108", "108", false)
        .is_ok());
    assert!(f
        .enqueue("100", commands[0].clone(), ActionCancellation::default())
        .is_err());
}

#[test]
fn settled_result_wait_still_blocks_reborrow_and_pause_retires_it_immediately() {
    let f = Fixture::new(true);
    let command = f.observe("101", 1);
    let (_, receiver) = f
        .enqueue("101", command, ActionCancellation::default())
        .unwrap();
    let dispatch = f.poll().unwrap();
    f.claim(&dispatch).unwrap();
    f.host
        .settle_existing_completion(&f.origin, &dispatch.proof)
        .unwrap();
    let selector = f.host.list_shared_candidates()[0].0.clone();
    assert!(
        f.host
            .grant_picker_tab("owner", "run-101", &selector)
            .is_err(),
        "settled receipt does not finish result admission"
    );
    f.host.cancel_actions("run-101").unwrap();
    assert!(receiver
        .try_recv()
        .expect("Pause must retire the result waiter synchronously")
        .is_err());
}

#[test]
fn concurrent_poll_and_claim_have_one_winner() {
    let f = Fixture::new(true);
    let command = f.observe("101", 1);
    let (_, _receiver) = f
        .enqueue("101", command, ActionCancellation::default())
        .unwrap();
    let dispatches = std::thread::scope(|scope| {
        (0..8)
            .map(|_| scope.spawn(|| f.poll()))
            .collect::<Vec<_>>()
            .into_iter()
            .filter_map(|h| h.join().unwrap())
            .collect::<Vec<_>>()
    });
    assert_eq!(dispatches.len(), 1);
    let dispatch = &dispatches[0];
    let wins = std::thread::scope(|scope| {
        (0..8)
            .map(|_| scope.spawn(|| f.claim(dispatch).is_ok()))
            .collect::<Vec<_>>()
            .into_iter()
            .map(|h| h.join().unwrap())
            .filter(|won| *won)
            .count()
    });
    // A fresh claim cannot be accepted after the racing callers completed.
    assert_eq!(wins, 1);
    assert!(f.claim(dispatch).is_err());
    assert_eq!(
        f.host
            .existing_completion_status(&f.origin, &dispatch.proof)
            .unwrap()
            .phase,
        CompletionPhase::Claimed
    );
}
