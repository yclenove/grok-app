use super::{
    queue_dispatch, queue_lease,
    queue_outcomes::Resolved,
    queue_recovery::{self, Decision, RecoveryRequest},
    queue_regression_tests::{item, Fixture},
    queue_result, queue_store, TurnResult, TurnStatus,
};
use serde_json::{json, Value};
use std::fs;
fn request(q: &queue_store::Item, decision: &str) -> RecoveryRequest {
    serde_json::from_value(json!({"sessionId":q.session_id,"itemId":q.item_id,"prompt":q.prompt,"createdAt":q.created_at,"decision":decision,"reviewed":true,"confirmed":true})).unwrap()
}
fn pending(f: &Fixture) -> (queue_store::Item, String) {
    let q = item("q");
    queue_store::insert(&f.path(), q.clone()).unwrap();
    let nonce = queue_store::claim(&f.path(), &q).unwrap().unwrap();
    (q, nonce)
}
fn receipt(q: &queue_store::Item) -> TurnResult {
    TurnResult {
        ok: true,
        status: TurnStatus::Queued,
        session_id: q.session_id.clone(),
        idempotency_key: Some("original-key".into()),
        message: None,
        queue_item_id: Some(q.item_id.clone()),
        queued_prompt: Some(q.prompt.clone()),
    }
}
#[tokio::test]
async fn pending_and_held_manual_dispositions_preserve_original_unknown_and_fifo() {
    for held in [false, true] {
        for decision in ["dismiss", "retry"] {
            let f = Fixture::new();
            let (q, nonce) = pending(&f);
            let tail = item("tail");
            queue_store::insert(&f.path(), tail.clone()).unwrap();
            if held {
                assert!(!queue_store::finish(
                    &f.path(),
                    &q,
                    &nonce,
                    &TurnResult::fail(TurnStatus::Unknown, "s", "uncertain")
                )
                .unwrap());
            }
            let ack = queue_recovery::recover_at(&f.0, request(&q, decision))
                .await
                .unwrap();
            let after = fs::read(f.path()).unwrap();
            assert_eq!(
                queue_recovery::recover_at(&f.0, request(&q, decision))
                    .await
                    .unwrap(),
                ack
            );
            assert_eq!(fs::read(f.path()).unwrap(), after);
            let rows = queue_store::list(&f.path()).unwrap();
            if decision == "retry" {
                let value = serde_json::to_value(&ack).unwrap();
                assert_eq!(rows.len(), 2);
                assert_eq!(rows[0].item_id, value["replacementId"].as_str().unwrap());
                assert_ne!(rows[0].item_id, q.item_id);
                assert_eq!(rows[0].prompt, q.prompt);
                assert!(rows[0].delivery.is_none());
                assert_eq!(rows[1], tail);
                let new_nonce = queue_store::claim(&f.path(), &rows[0]).unwrap().unwrap();
                assert!(queue_store::finish(
                    &f.path(),
                    &rows[0],
                    &new_nonce,
                    &TurnResult::started("s")
                )
                .unwrap());
            } else {
                assert_eq!(rows, vec![tail]);
            }
            assert!(matches!(
                queue_store::resolve(&f.path(), "s", "q", "prompt").unwrap(),
                Resolved::Unknown
            ));
            let replay = queue_result::resolve_at(&f.0, receipt(&q)).await;
            assert_eq!(replay.status, TurnStatus::Unknown);
            assert_eq!(replay.idempotency_key.as_deref(), Some("original-key"));
            assert!(queue_store::claim(&f.path(), &q).unwrap().is_none());
            assert!(queue_store::finish(&f.path(), &q, &nonce, &TurnResult::started("s")).is_err());
            assert!(queue_store::insert(&f.path(), q.clone()).is_err());
            let other = if decision == "retry" {
                "dismiss"
            } else {
                "retry"
            };
            assert_eq!(
                queue_recovery::recover_at(&f.0, request(&q, other))
                    .await
                    .unwrap_err(),
                "SESSION_QUEUE_CHANGED"
            );
        }
    }
}
#[tokio::test]
async fn recovery_requires_both_confirmations_and_exact_identity() {
    let f = Fixture::new();
    let (q, _) = pending(&f);
    let before = fs::read(f.path()).unwrap();
    for (key, value) in [
        ("reviewed", json!(false)),
        ("confirmed", json!(false)),
        ("sessionId", json!("imp:any")),
        ("sessionId", json!("__draft__")),
        ("prompt", json!(" ")),
    ] {
        let mut v = json!({"sessionId":"s","itemId":"q","prompt":"prompt","createdAt":1,"decision":"retry","reviewed":true,"confirmed":true});
        v[key] = value;
        assert_eq!(
            queue_recovery::recover_at(&f.0, serde_json::from_value(v).unwrap())
                .await
                .unwrap_err(),
            "SESSION_QUEUE_INVALID"
        );
        assert_eq!(fs::read(f.path()).unwrap(), before);
    }
    for q2 in [
        queue_store::Item::new("other", "q", "prompt", 1),
        queue_store::Item::new("s", "q", "prompt ", 1),
        queue_store::Item::new("s", "q", "prompt", 2),
        item("missing"),
    ] {
        assert_eq!(
            queue_recovery::recover_at(&f.0, request(&q2, "retry"))
                .await
                .unwrap_err(),
            "SESSION_QUEUE_CHANGED"
        );
        assert_eq!(fs::read(f.path()).unwrap(), before);
    }
    let mut v=serde_json::to_value(json!({"sessionId":"s","itemId":"q","prompt":"prompt","createdAt":1,"decision":"retry","reviewed":true,"confirmed":true})).unwrap();
    v["unsafeRelease"] = json!(true);
    assert!(serde_json::from_value::<RecoveryRequest>(v).is_err());
    assert!(queue_recovery::recover_at(&f.0, request(&q, "dismiss"))
        .await
        .is_ok());
}
#[tokio::test]
async fn ready_item_is_not_misclassified_as_unknown_or_duplicated() {
    let f = Fixture::new();
    let q = item("q");
    queue_store::insert(&f.path(), q.clone()).unwrap();
    let before = fs::read(f.path()).unwrap();
    assert_eq!(
        queue_recovery::recover_at(&f.0, request(&q, "retry"))
            .await
            .unwrap_err(),
        "SESSION_QUEUE_CHANGED"
    );
    assert_eq!(fs::read(f.path()).unwrap(), before);
}
#[tokio::test]
async fn active_dispatch_lease_blocks_recovery_and_releases_after_unknown() {
    let f = Fixture::new();
    let q = item("q");
    queue_store::insert(&f.path(), q.clone()).unwrap();
    let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
    let (done_tx, done_rx) = tokio::sync::oneshot::channel();
    let root = f.0.clone();
    let cloned = q.clone();
    let worker = tokio::spawn(async move {
        queue_dispatch::run_at(&root, &cloned, || async {
            entered_tx.send(()).unwrap();
            done_rx.await.unwrap();
            TurnResult::fail(TurnStatus::Unknown, "s", "unknown")
        })
        .await
    });
    tokio::time::timeout(std::time::Duration::from_secs(5), entered_rx)
        .await
        .unwrap()
        .unwrap();
    let before = fs::read(f.path()).unwrap();
    assert_eq!(
        queue_recovery::recover_at(&f.0, request(&q, "retry"))
            .await
            .unwrap_err(),
        "SESSION_QUEUE_ACTIVE"
    );
    assert_eq!(fs::read(f.path()).unwrap(), before);
    assert_eq!(
        queue_store::list(&f.path()).unwrap()[0].display_state(),
        "pending"
    );
    done_tx.send(()).unwrap();
    worker.await.unwrap();
    assert!(queue_recovery::recover_at(&f.0, request(&q, "retry"))
        .await
        .is_ok());
}
#[tokio::test]
async fn aborted_dispatch_retains_original_claim_until_explicit_review() {
    let f = Fixture::new();
    let q = item("q");
    queue_store::insert(&f.path(), q.clone()).unwrap();
    let root = f.0.clone();
    let cloned = q.clone();
    let (tx, rx) = tokio::sync::oneshot::channel();
    let worker = tokio::spawn(async move {
        queue_dispatch::run_at(&root, &cloned, || async {
            tx.send(()).unwrap();
            std::future::pending::<TurnResult>().await
        })
        .await
    });
    tokio::time::timeout(std::time::Duration::from_secs(5), rx)
        .await
        .unwrap()
        .unwrap();
    worker.abort();
    assert!(worker.await.unwrap_err().is_cancelled());
    assert_eq!(
        queue_store::list(&f.path()).unwrap()[0].display_state(),
        "pending"
    );
    assert!(
        queue_dispatch::run_at(&f.0, &q, || async { panic!("old claim redispatched") })
            .await
            .is_none()
    );
    assert!(queue_recovery::recover_at(&f.0, request(&q, "dismiss"))
        .await
        .is_ok());
}
#[test]
fn concurrent_review_retries_create_exactly_one_new_identity() {
    let f = Fixture::new();
    let (q, _) = pending(&f);
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
    let workers = (0..8)
        .map(|_| {
            let p = f.path();
            let q = q.clone();
            let b = barrier.clone();
            std::thread::spawn(move || {
                b.wait();
                queue_store::recover(&p, &q, Decision::Retry).unwrap()
            })
        })
        .collect::<Vec<_>>();
    let results = workers
        .into_iter()
        .map(|w| w.join().unwrap())
        .collect::<Vec<_>>();
    assert!(results.iter().all(|r| r == &results[0]));
    assert_eq!(queue_store::list(&f.path()).unwrap().len(), 1);
}
#[tokio::test]
async fn retained_client_receipt_cannot_become_a_second_send_after_manual_retry() {
    let f = Fixture::new();
    let (q, _) = pending(&f);
    let path = f.0.join("session-api-idempotency.json");
    let (original, _) =
        super::idempotency::run_at(&path, "s", "prompt", Some("client"), || async {
            receipt(&q)
        })
        .await;
    assert_eq!(original.status, TurnStatus::Queued);
    let before = fs::read(&path).unwrap();
    queue_recovery::recover_at(&f.0, request(&q, "retry"))
        .await
        .unwrap();
    for _ in 0..2 {
        let (result, replayed) =
            super::idempotency::run_at(&path, "s", "prompt", Some("client"), || async {
                panic!("original request sent again")
            })
            .await;
        assert!(replayed);
        let resolved = queue_result::resolve_at(&f.0, result).await;
        assert_eq!(resolved.status, TurnStatus::Unknown);
        assert_eq!(resolved.idempotency_key.as_deref(), Some("client"));
    }
    assert_eq!(fs::read(path).unwrap(), before);
}
#[tokio::test]
async fn malformed_store_and_missing_outcome_never_authorize_a_new_attempt() {
    let f = Fixture::new();
    let (q, _) = pending(&f);
    queue_recovery::recover_at(&f.0, request(&q, "retry"))
        .await
        .unwrap();
    let mut v: Value = serde_json::from_slice(&fs::read(f.path()).unwrap()).unwrap();
    v["version"] = json!(3);
    fs::write(f.path(), serde_json::to_vec(&v).unwrap()).unwrap();
    let before = fs::read(f.path()).unwrap();
    assert_eq!(
        queue_recovery::recover_at(&f.0, request(&q, "retry"))
            .await
            .unwrap_err(),
        "SESSION_QUEUE_UNAVAILABLE"
    );
    assert_eq!(fs::read(f.path()).unwrap(), before);
    fs::write(f.path(), b"{broken").unwrap();
    assert_eq!(
        queue_recovery::recover_at(&f.0, request(&q, "retry"))
            .await
            .unwrap_err(),
        "SESSION_QUEUE_UNAVAILABLE"
    );
    assert_eq!(fs::read(f.path()).unwrap(), b"{broken");
    fs::write(f.path(), br#"{"version":4,"items":[],"outcomes":[]}"#).unwrap();
    assert_eq!(
        queue_recovery::recover_at(&f.0, request(&q, "retry"))
            .await
            .unwrap_err(),
        "SESSION_QUEUE_CHANGED"
    );
}
#[tokio::test]
async fn version_three_claim_is_read_only_until_an_explicit_version_four_disposition() {
    let f = Fixture::new();
    let (q, _) = pending(&f);
    let mut v: Value = serde_json::from_slice(&fs::read(f.path()).unwrap()).unwrap();
    v["version"] = json!(3);
    fs::write(f.path(), serde_json::to_vec(&v).unwrap()).unwrap();
    let before = fs::read(f.path()).unwrap();
    assert_eq!(
        queue_store::list(&f.path()).unwrap()[0].display_state(),
        "pending"
    );
    assert_eq!(fs::read(f.path()).unwrap(), before);
    queue_recovery::recover_at(&f.0, request(&q, "dismiss"))
        .await
        .unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(&fs::read(f.path()).unwrap()).unwrap()["version"],
        4
    );
}
#[test]
fn live_lease_is_root_scoped_and_drop_releases_it() {
    let a = Fixture::new();
    let b = Fixture::new();
    let lease = queue_lease::try_acquire(&a.0).unwrap().unwrap();
    assert!(queue_lease::try_acquire(&a.0).unwrap().is_none());
    assert!(queue_lease::try_acquire(&b.0).unwrap().is_some());
    drop(lease);
    assert!(queue_lease::try_acquire(&a.0).unwrap().is_some());
}
