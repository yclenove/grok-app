use super::*;

fn binding(tab: &str) -> CompletionBinding {
    CompletionBinding {
        protocol: ACTION_COMPLETION_PROTOCOL,
        request_id: uuid::Uuid::new_v4().to_string(),
        connection: PairingConnection {
            instance_id: uuid::Uuid::new_v4().to_string(),
            connection_nonce: uuid::Uuid::new_v4().to_string(),
            generation: 1,
        },
        session: "owner".into(),
        run_id: format!("run-{tab}"),
        tab_id: tab.into(),
        document_id: "0123456789abcdef0123456789abcdef".into(),
        document_generation: 1,
        grant_generation: 1,
        snapshot_id: uuid::Uuid::new_v4().to_string(),
    }
}

#[test]
fn browser_exit_releases_only_an_exact_cleanup_or_unknown_owner() {
    let mut registry = CompletionRegistry::default();
    let old = binding("102");
    let kept = binding("101");
    registry
        .reserve_with_browser(
            old.clone(),
            ActionCancellation::default(),
            false,
            Some("old-browser".into()),
        )
        .unwrap();
    registry
        .reserve_with_browser(
            kept.clone(),
            ActionCancellation::default(),
            false,
            Some("new-browser".into()),
        )
        .unwrap();
    let mismatched = registry.converge_browser_exit(
        "old-browser",
        &[BrowserExitCleanup {
            browser_id: "other".into(),
            request_id: old.request_id.clone(),
            document_id: old.document_id.clone(),
            phase: "applied".into(),
        }],
    );
    assert!(mismatched.is_empty());
    assert_eq!(registry.len(), 2);
    let released = registry.converge_browser_exit(
        "old-browser",
        &[BrowserExitCleanup {
            browser_id: "old-browser".into(),
            request_id: old.request_id.clone(),
            document_id: old.document_id.clone(),
            phase: "prepared".into(),
        }],
    );
    assert_eq!(released, vec![(old.request_id, "unknown")]);
    assert_eq!(registry.len(), 1);
    assert!(registry.has_tab(&kept.tab_id));
    let wrong_document = registry.converge_browser_exit(
        "new-browser",
        &[BrowserExitCleanup {
            browser_id: "new-browser".into(),
            request_id: kept.request_id.clone(),
            document_id: "ffffffffffffffffffffffffffffffff".into(),
            phase: "physicallySettled".into(),
        }],
    );
    assert!(wrong_document.is_empty());
    assert_eq!(registry.len(), 1);
}

#[test]
fn canceled_or_expired_unclaimed_offer_can_never_be_claimed() {
    for timeout in [false, true] {
        let mut registry = CompletionRegistry::default();
        let cancellation = ActionCancellation::default();
        let proof = registry
            .offer(binding("101"), cancellation.clone())
            .unwrap();
        if timeout {
            registry.pending[0].deadline = Instant::now();
        } else {
            cancellation.cancel();
        }
        assert!(registry.claim(&proof).is_err());
        assert!(registry.idle("run-101"));
        assert_eq!(
            registry.status(&proof).unwrap().phase,
            CompletionPhase::Settled
        );
        registry.settle(&proof).unwrap();
    }
}

#[test]
fn claimed_offer_stays_busy_past_cancellation_and_deadline_until_authenticated_settlement() {
    let mut registry = CompletionRegistry::default();
    let cancellation = ActionCancellation::default();
    let proof = registry
        .offer(binding("101"), cancellation.clone())
        .unwrap();
    registry.claim(&proof).unwrap();
    assert!(
        registry.claim(&proof).is_err(),
        "claim response loss must not replay"
    );
    cancellation.cancel();
    registry.pending[0].deadline = Instant::now() - Duration::from_secs(600);
    let status = registry.status(&proof).unwrap();
    assert_eq!(status.phase, CompletionPhase::Claimed);
    assert!(status.cancel_requested);
    assert!(!registry.idle("run-101"));
    assert!(registry
        .offer(binding("101"), ActionCancellation::default())
        .is_err());
    let other = registry
        .offer(binding("102"), ActionCancellation::default())
        .unwrap();
    registry.settle(&proof).unwrap();
    registry.settle(&proof).unwrap();
    assert!(registry.idle("run-101"));
    assert!(!registry.idle("run-102"));
    registry.claim(&other).unwrap();
    assert!(registry.claim(&proof).is_err());
}

#[test]
fn every_identity_field_and_key_is_authenticated_before_mutating_occupancy() {
    let mut registry = CompletionRegistry::default();
    let proof = registry
        .offer(binding("101"), ActionCancellation::default())
        .unwrap();
    let original = serde_json::to_value(&proof).unwrap();
    let mut pointers = original["binding"]
        .as_object()
        .unwrap()
        .keys()
        .filter(|key| *key != "connection")
        .map(|key| format!("/binding/{key}"))
        .collect::<Vec<_>>();
    pointers.extend(
        ["instanceId", "connectionNonce", "generation"]
            .map(|key| format!("/binding/connection/{key}")),
    );
    pointers.push("/completionKey".into());
    for pointer in pointers {
        let mut changed = original.clone();
        let field = changed.pointer_mut(&pointer).unwrap();
        *field = if let Some(number) = field.as_u64() {
            serde_json::json!(number + 1)
        } else {
            serde_json::json!("f".repeat(64))
        };
        let changed = serde_json::from_value(changed).unwrap();
        assert!(registry.claim(&changed).is_err(), "{pointer}");
        assert!(registry.status(&changed).is_err(), "{pointer}");
        assert!(registry.settle(&changed).is_err(), "{pointer}");
        assert!(registry.retirement(&changed).is_err(), "{pointer}");
        assert!(!registry.idle("run-101"), "{pointer}");
    }
    assert!(!format!("{proof:?}").contains(&proof.completion_key.0));
    assert_eq!(
        registry.pending[0]
            .authentication
            .as_ref()
            .unwrap()
            .tag
            .len(),
        32
    );
    registry.claim(&proof).unwrap();
    registry.settle(&proof).unwrap();
}

#[test]
fn pending_capacity_never_evicts_uncertain_actions_and_tombstones_are_bounded() {
    let mut registry = CompletionRegistry::default();
    let mut proofs = Vec::new();
    for tab in 0..MAX_PENDING {
        let proof = registry
            .offer(binding(&tab.to_string()), ActionCancellation::default())
            .unwrap();
        registry.claim(&proof).unwrap();
        proofs.push(proof);
    }
    registry.cancel_run(None);
    assert!(registry
        .offer(binding("overflow"), ActionCancellation::default())
        .is_err());
    assert_eq!(registry.pending.len(), MAX_PENDING);
    for proof in proofs {
        registry.settle(&proof).unwrap();
    }
    let old = registry
        .offer(binding("101"), ActionCancellation::default())
        .unwrap();
    registry.settle(&old).unwrap();
    for _ in 0..COMPLETION_TOMBSTONE_LIMIT {
        let proof = registry
            .offer(binding("101"), ActionCancellation::default())
            .unwrap();
        registry.settle(&proof).unwrap();
    }
    assert_eq!(registry.finished.len(), COMPLETION_TOMBSTONE_LIMIT);
    assert!(registry.status(&old).is_err());
    assert_eq!(
        registry.retirement(&old).unwrap(),
        CompletionRetirement::Absent
    );
    let latest = registry
        .offer(binding("101"), ActionCancellation::default())
        .unwrap();
    registry.settle(&latest).unwrap();
    for (_, time) in &mut registry.finished {
        *time = Instant::now() - FINISHED_TTL;
    }
    assert!(registry.settle(&latest).is_err());
    assert!(registry.finished.is_empty());
    assert_eq!(
        registry.retirement(&latest).unwrap(),
        CompletionRetirement::Absent
    );
}

#[test]
fn retirement_is_authenticated_read_only_and_cannot_cross_registry_instances() {
    let mut registry = CompletionRegistry::default();
    let proof = registry
        .offer(binding("101"), ActionCancellation::default())
        .unwrap();
    assert_eq!(
        registry.retirement(&proof).unwrap(),
        CompletionRetirement::Pending
    );
    assert!(!registry.idle("run-101"));
    registry.claim(&proof).unwrap();
    assert_eq!(
        registry.retirement(&proof).unwrap(),
        CompletionRetirement::Pending
    );
    assert!(!registry.idle("run-101"));
    registry.settle(&proof).unwrap();
    assert_eq!(
        registry.retirement(&proof).unwrap(),
        CompletionRetirement::Settled
    );
    assert!(registry.idle("run-101"));

    let mut replacement = CompletionRegistry::default();
    assert!(replacement.retirement(&proof).is_err());
    let mut changed = proof.clone();
    changed.completion_key.0.replace_range(..2, "ff");
    assert!(registry.retirement(&changed).is_err());
}

#[test]
fn proof_wire_shape_is_strict() {
    let authority = crate::browser::csprng_bearer_token();
    let (_, proof) = Authentication::issue(binding("101"), &authority).unwrap();
    for pointer in ["", "/binding", "/binding/connection"] {
        let mut value = serde_json::to_value(&proof).unwrap();
        value
            .pointer_mut(pointer)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert("unexpected".into(), serde_json::json!(true));
        assert!(serde_json::from_value::<CompletionProof>(value).is_err());
    }
}

#[test]
fn reservation_mints_no_key_until_one_delivery_and_requires_bound_claim() {
    let mut registry = CompletionRegistry::default();
    let binding = binding("101");
    registry
        .reserve(binding.clone(), ActionCancellation::default(), true)
        .unwrap();
    assert!(registry.pending[0].authentication.is_none());
    assert!(!registry.idle("run-101"));
    let proof = registry.deliver(&binding.request_id).unwrap();
    assert!(registry.deliver(&binding.request_id).is_err());
    assert!(
        registry.claim(&proof).is_err(),
        "generic claim must not bypass command binding"
    );
    registry.claim_bound(&proof).unwrap();
    registry.settle(&proof).unwrap();
}

#[test]
fn canceled_or_expired_reservation_can_never_be_delivered() {
    for timeout in [false, true] {
        let mut registry = CompletionRegistry::default();
        let binding = binding("101");
        let cancellation = ActionCancellation::default();
        registry
            .reserve(binding.clone(), cancellation.clone(), true)
            .unwrap();
        if timeout {
            registry.pending[0].deadline = Instant::now();
        } else {
            cancellation.cancel();
        }
        assert!(registry.deliver(&binding.request_id).is_err());
        assert!(registry.idle("run-101"));
        assert!(
            registry.finished.is_empty(),
            "no undisclosed key or fabricated proof to retain"
        );
    }
}
