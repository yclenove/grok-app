use super::{
    queue_regression_tests::{item, Fixture},
    queue_store, TurnResult,
};
use std::fs;
#[test]
fn confirmed_dispatch_keeps_an_atomic_durable_outcome_after_active_item_removal() {
    let f = Fixture::new();
    let q = item("original");
    queue_store::insert(&f.path(), q.clone()).unwrap();
    let nonce = queue_store::claim(&f.path(), &q).unwrap().unwrap();
    assert!(queue_store::finish(&f.path(), &q, &nonce, &TurnResult::started("s")).unwrap());
    assert!(queue_store::list(&f.path()).unwrap().is_empty());
    let disk: serde_json::Value = serde_json::from_slice(&fs::read(f.path()).unwrap()).unwrap();
    assert_eq!(disk["outcomes"][0]["itemId"], "original");
    assert_eq!(disk["outcomes"][0]["status"], "started");
}
#[test]
fn confirmed_handoff_keeps_original_to_replacement_lineage_on_disk() {
    let f = Fixture::new();
    let q = item("original");
    queue_store::insert(&f.path(), q.clone()).unwrap();
    let nonce = queue_store::claim(&f.path(), &q).unwrap().unwrap();
    let next = item("replacement");
    queue_store::insert(&f.path(), next.clone()).unwrap();
    let mut result = TurnResult::started("s");
    result.status = super::TurnStatus::Queued;
    result.queue_item_id = Some(next.item_id.clone());
    result.queued_prompt = Some(next.prompt);
    assert!(queue_store::finish(&f.path(), &q, &nonce, &result).unwrap());
    let disk: serde_json::Value = serde_json::from_slice(&fs::read(f.path()).unwrap()).unwrap();
    assert_eq!(disk["outcomes"][0]["status"], "handed_off");
    assert_eq!(disk["outcomes"][0]["replacementId"], "replacement");
}

fn receipt(q: &queue_store::Item) -> TurnResult {
    let mut r = TurnResult::started(&q.session_id);
    r.status = super::TurnStatus::Queued;
    r.queue_item_id = Some(q.item_id.clone());
    r.queued_prompt = Some(q.prompt.clone());
    r.idempotency_key = Some("original-key".into());
    r
}
#[test]
fn cancel_is_durable_idempotent_and_prevents_later_claim_or_id_reuse() {
    let f = Fixture::new();
    let q = item("cancel");
    queue_store::insert(&f.path(), q.clone()).unwrap();
    queue_store::cancel(&f.path(), &q).unwrap();
    let bytes = fs::read(f.path()).unwrap();
    queue_store::cancel(&f.path(), &q).unwrap();
    assert_eq!(bytes, fs::read(f.path()).unwrap());
    assert!(queue_store::claim(&f.path(), &q).unwrap().is_none());
    assert!(queue_store::insert(&f.path(), q.clone()).is_err());
    assert!(matches!(
        queue_store::resolve(&f.path(), "s", "cancel", "prompt").unwrap(),
        super::queue_outcomes::Resolved::Cancelled
    ));
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(json["version"], 4);
    assert!(json["items"].as_array().unwrap().is_empty());
    assert!(json["outcomes"][0].get("prompt").is_none());
}
#[test]
fn cancel_compare_checks_session_prompt_and_timestamp_without_mutating() {
    let f = Fixture::new();
    let q = item("a");
    queue_store::insert(&f.path(), q.clone()).unwrap();
    let bytes = fs::read(f.path()).unwrap();
    for changed in [
        queue_store::Item::new("wrong", "a", "prompt", 1),
        queue_store::Item::new("s", "a", " prompt ", 1),
        queue_store::Item::new("s", "a", "prompt", 2),
        item("missing"),
    ] {
        assert!(queue_store::cancel(&f.path(), &changed).is_err());
        assert_eq!(bytes, fs::read(f.path()).unwrap());
    }
    queue_store::cancel(&f.path(), &q).unwrap();
    assert!(
        queue_store::cancel(&f.path(), &queue_store::Item::new("s", "a", "prompt", 2)).is_err()
    );
}
#[test]
fn pending_and_held_are_not_cancellable_or_automatically_resolved_as_sent() {
    let f = Fixture::new();
    let q = item("a");
    queue_store::insert(&f.path(), q.clone()).unwrap();
    let nonce = queue_store::claim(&f.path(), &q).unwrap().unwrap();
    for held in [false, true] {
        if held {
            queue_store::finish(
                &f.path(),
                &q,
                &nonce,
                &TurnResult::fail(super::TurnStatus::Unknown, "s", "unknown"),
            )
            .unwrap();
        }
        let before = fs::read(f.path()).unwrap();
        assert_eq!(
            queue_store::cancel(&f.path(), &q).unwrap_err(),
            "SESSION_QUEUE_CLAIMED"
        );
        assert_eq!(before, fs::read(f.path()).unwrap());
        assert!(matches!(
            queue_store::resolve(&f.path(), "s", "a", "prompt").unwrap(),
            super::queue_outcomes::Resolved::Unknown
        ));
    }
}
#[test]
fn cancel_and_claim_race_has_exactly_one_winner() {
    for n in 0..16 {
        let f = Fixture::new();
        let q = item(&format!("race-{n}"));
        queue_store::insert(&f.path(), q.clone()).unwrap();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let path = f.path();
        let expected = q.clone();
        let b = barrier.clone();
        let cancel = std::thread::spawn(move || {
            b.wait();
            queue_store::cancel(&path, &expected)
        });
        barrier.wait();
        let claim = queue_store::claim(&f.path(), &q).unwrap();
        let cancelled = cancel.join().unwrap();
        assert_ne!(claim.is_some(), cancelled.is_ok());
        if claim.is_some() {
            assert_eq!(cancelled.unwrap_err(), "SESSION_QUEUE_CLAIMED");
        } else {
            assert!(queue_store::list(&f.path()).unwrap().is_empty());
        }
    }
}
#[tokio::test]
async fn original_receipt_follows_handoff_then_started_preserving_key() {
    let f = Fixture::new();
    let q = item("original");
    let r = receipt(&q);
    queue_store::insert(&f.path(), q.clone()).unwrap();
    let live = super::queue_result::resolve_at(&f.0, r.clone()).await;
    assert_eq!(live.queue_item_id.as_deref(), Some("original"));
    let nonce = queue_store::claim(&f.path(), &q).unwrap().unwrap();
    let next = item("next");
    queue_store::insert(&f.path(), next.clone()).unwrap();
    queue_store::finish(&f.path(), &q, &nonce, &receipt(&next)).unwrap();
    let live = super::queue_result::resolve_at(&f.0, r.clone()).await;
    assert_eq!(live.queue_item_id.as_deref(), Some("next"));
    let nonce = queue_store::claim(&f.path(), &next).unwrap().unwrap();
    queue_store::finish(&f.path(), &next, &nonce, &TurnResult::started("s")).unwrap();
    let done = super::queue_result::resolve_at(&f.0, r).await;
    assert_eq!(done.status, super::TurnStatus::TurnStarted);
    assert!(done.ok);
    assert_eq!(done.idempotency_key.as_deref(), Some("original-key"));
    assert!(done.queue_item_id.is_none());
}
#[tokio::test]
async fn original_receipt_follows_handoff_cancellation_and_never_requeues() {
    let f = Fixture::new();
    let q = item("original");
    let next = item("next");
    queue_store::insert(&f.path(), q.clone()).unwrap();
    let nonce = queue_store::claim(&f.path(), &q).unwrap().unwrap();
    queue_store::insert(&f.path(), next.clone()).unwrap();
    queue_store::finish(&f.path(), &q, &nonce, &receipt(&next)).unwrap();
    queue_store::cancel(&f.path(), &next).unwrap();
    let before = fs::read(f.path()).unwrap();
    let done = super::queue_result::resolve_at(&f.0, receipt(&q)).await;
    assert_eq!(done.status, super::TurnStatus::Cancelled);
    assert!(!done.ok);
    assert!(done.queue_item_id.is_none());
    assert_eq!(done.idempotency_key.as_deref(), Some("original-key"));
    assert_eq!(before, fs::read(f.path()).unwrap());
    assert_eq!(super::status_for(&done.status), axum::http::StatusCode::OK);
}
#[tokio::test]
async fn missing_corrupt_payload_mismatch_and_evicted_results_fail_closed() {
    let f = Fixture::new();
    let q = item("old");
    assert_eq!(
        super::queue_result::resolve_at(&f.0, receipt(&q))
            .await
            .status,
        super::TurnStatus::Unknown
    );
    queue_store::insert(&f.path(), q.clone()).unwrap();
    queue_store::cancel(&f.path(), &q).unwrap();
    let wrong = queue_store::Item::new("s", "old", " prompt ", 1);
    assert_eq!(
        super::queue_result::resolve_at(&f.0, receipt(&wrong))
            .await
            .status,
        super::TurnStatus::Unknown
    );
    for i in 0..super::QUEUE_CAP {
        let other = item(&format!("new-{i}"));
        queue_store::insert(&f.path(), other.clone()).unwrap();
        queue_store::cancel(&f.path(), &other).unwrap();
    }
    assert_eq!(
        super::queue_result::resolve_at(&f.0, receipt(&q))
            .await
            .status,
        super::TurnStatus::Unknown
    );
    let disk: serde_json::Value = serde_json::from_slice(&fs::read(f.path()).unwrap()).unwrap();
    assert_eq!(disk["outcomes"].as_array().unwrap().len(), super::QUEUE_CAP);
    fs::write(f.path(), b"{broken").unwrap();
    let unknown = super::queue_result::resolve_at(&f.0, receipt(&q)).await;
    assert_eq!(unknown.status, super::TurnStatus::Unknown);
    assert!(!unknown
        .message
        .unwrap()
        .contains(&f.0.to_string_lossy().to_string()));
    assert_eq!(fs::read(f.path()).unwrap(), b"{broken");
}
#[test]
fn legacy_versions_load_without_rewrite_and_upgrade_only_on_mutation() {
    let f = Fixture::new();
    for version in [None, Some(2)] {
        let mut v = serde_json::json!({"items":[item("legacy")]});
        if let Some(n) = version {
            v["version"] = n.into();
        }
        let bytes = serde_json::to_vec(&v).unwrap();
        fs::write(f.path(), &bytes).unwrap();
        assert_eq!(queue_store::list(&f.path()).unwrap().len(), 1);
        assert_eq!(bytes, fs::read(f.path()).unwrap());
        queue_store::cancel(&f.path(), &item("legacy")).unwrap();
        let upgraded: serde_json::Value =
            serde_json::from_slice(&fs::read(f.path()).unwrap()).unwrap();
        assert_eq!(upgraded["version"], 4);
    }
}
#[test]
fn malformed_outcomes_legacy_extension_duplicates_and_cycles_fail_closed() {
    let f = Fixture::new();
    let q = item("a");
    queue_store::insert(&f.path(), q.clone()).unwrap();
    queue_store::cancel(&f.path(), &q).unwrap();
    let valid: serde_json::Value = serde_json::from_slice(&fs::read(f.path()).unwrap()).unwrap();
    for kind in 0..6 {
        let mut v = valid.clone();
        match kind {
            0 => v["version"] = 2.into(),
            1 => v["outcomes"][0]["promptHash"] = "not-a-hash".into(),
            2 => v["outcomes"][0]["replacementId"] = "unexpected".into(),
            3 => v["items"] = serde_json::json!([q]),
            4 => v["outcomes"][0]["extra"] = true.into(),
            _ => {
                v.as_object_mut().unwrap().remove("outcomes");
            }
        }
        let bytes = serde_json::to_vec(&v).unwrap();
        fs::write(f.path(), &bytes).unwrap();
        assert!(queue_store::list(&f.path()).is_err(), "kind {kind}");
        assert_eq!(bytes, fs::read(f.path()).unwrap());
    }
    let mut cycle = valid.clone();
    let mut b = cycle["outcomes"][0].clone();
    b["itemId"] = "b".into();
    cycle["outcomes"][0]["status"] = "handed_off".into();
    cycle["outcomes"][0]["replacementId"] = "b".into();
    b["status"] = "handed_off".into();
    b["replacementId"] = "a".into();
    cycle["outcomes"].as_array_mut().unwrap().push(b);
    fs::write(f.path(), serde_json::to_vec(&cycle).unwrap()).unwrap();
    assert!(matches!(
        queue_store::resolve(&f.path(), "s", "a", "prompt").unwrap(),
        super::queue_outcomes::Resolved::Unknown
    ));
}
#[tokio::test]
async fn cancellation_command_validates_identity_and_redacts_storage_failures() {
    let f = Fixture::new();
    let q = item("q");
    queue_store::insert(&f.path(), q.clone()).unwrap();
    for sid in ["", "__draft__", "imp:history", " s "] {
        let r = serde_json::from_value(
            serde_json::json!({"sessionId":sid,"itemId":"q","prompt":"prompt","createdAt":1}),
        )
        .unwrap();
        assert_eq!(
            super::queue_cancel::cancel_at(&f.0, r).await.unwrap_err(),
            "SESSION_QUEUE_INVALID"
        );
    }
    let mut v = serde_json::json!({"sessionId":"s","itemId":"q","prompt":"prompt","createdAt":1});
    v["filePath"] = "do-not-use".into();
    assert!(serde_json::from_value::<super::queue_cancel::QueueCancelRequest>(v).is_err());
    fs::write(f.path(), b"{broken").unwrap();
    let r = serde_json::from_value(
        serde_json::json!({"sessionId":"s","itemId":"q","prompt":"prompt","createdAt":1}),
    )
    .unwrap();
    assert_eq!(
        super::queue_cancel::cancel_at(&f.0, r).await.unwrap_err(),
        "SESSION_QUEUE_UNAVAILABLE"
    );
    assert_eq!(fs::read(f.path()).unwrap(), b"{broken");
}

#[tokio::test]
async fn retained_original_client_key_replays_terminal_outcome_without_dispatch_or_receipt_rewrite()
{
    for cancelled in [true, false] {
        let f = Fixture::new();
        let q = item("original");
        let path = f.0.join("session-api-idempotency.json");
        let (accepted, replayed) =
            super::idempotency::run_at(&path, "s", "prompt", Some("client-key"), || async {
                queue_store::insert(&f.path(), q.clone()).unwrap();
                receipt(&q)
            })
            .await;
        assert!(!replayed);
        assert_eq!(accepted.status, super::TurnStatus::Queued);
        if cancelled {
            queue_store::cancel(&f.path(), &q).unwrap();
        } else {
            let nonce = queue_store::claim(&f.path(), &q).unwrap().unwrap();
            queue_store::finish(&f.path(), &q, &nonce, &TurnResult::started("s")).unwrap();
        }
        let receipt_before = fs::read(&path).unwrap();
        let queue_before = fs::read(f.path()).unwrap();
        for _ in 0..2 {
            let (cached, replayed) =
                super::idempotency::run_at(&path, "s", "prompt", Some("client-key"), || async {
                    panic!("A retained client key must never dispatch twice")
                })
                .await;
            assert!(replayed);
            assert_eq!(cached.status, super::TurnStatus::Queued);
            let resolved = super::queue_result::resolve_at(&f.0, cached).await;
            assert_eq!(
                resolved.status,
                if cancelled {
                    super::TurnStatus::Cancelled
                } else {
                    super::TurnStatus::TurnStarted
                }
            );
            assert_eq!(resolved.idempotency_key.as_deref(), Some("client-key"));
            assert!(resolved.queue_item_id.is_none());
        }
        assert_eq!(receipt_before, fs::read(&path).unwrap());
        assert_eq!(queue_before, fs::read(f.path()).unwrap());
    }
}
