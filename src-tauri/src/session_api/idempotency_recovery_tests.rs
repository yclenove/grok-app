use super::{
    idempotency::run_at, idempotency_tests::Fixture, receipt_store, TurnResult, TurnStatus,
};
use serde_json::{json, Value};
use std::fs;

fn read(f: &Fixture) -> Value {
    serde_json::from_slice(&fs::read(f.path()).unwrap()).unwrap()
}
fn write(f: &Fixture, v: &Value) {
    fs::write(f.path(), serde_json::to_vec(v).unwrap()).unwrap();
}
async fn started(f: &Fixture, sid: &str, key: &str) {
    let (r, _) = run_at(&f.path(), sid, "p", Some(key), || async {
        TurnResult::started(sid)
    })
    .await;
    assert!(r.ok);
}
async fn rejected(f: &Fixture, sid: &str, prompt: &str, key: &str) -> TurnResult {
    run_at(&f.path(), sid, prompt, Some(key), || async {
        panic!("must not dispatch")
    })
    .await
    .0
}

#[tokio::test]
async fn claim_is_flushed_before_dispatch_and_contains_no_prompt_copy() {
    let f = Fixture::new();
    let (r, _) = run_at(
        &f.path(),
        "s",
        "private synthetic prompt",
        Some("key"),
        || async {
            let value = read(&f);
            let entry = &value["entries"][0];
            assert_eq!(value["version"], 3);
            assert_eq!(entry["state"], "pending");
            assert_eq!(entry["result"]["status"], "unknown");
            assert_eq!(entry["prompt_sha256"].as_str().unwrap().len(), 64);
            assert!(!fs::read_to_string(f.path())
                .unwrap()
                .contains("private synthetic prompt"));
            TurnResult::started("s")
        },
    )
    .await;
    assert!(r.ok);
    assert_eq!(read(&f)["entries"][0]["state"], "completed");
}

#[tokio::test]
async fn receipt_write_failure_after_dispatch_is_unknown_and_never_replayed_as_success() {
    let f = Fixture::new();
    let path = f.path();
    let displaced = f.0.join("saved-claim.json");
    let (r, _) = run_at(&path, "s", "p", Some("key"), || async {
        fs::rename(&path, &displaced).unwrap();
        fs::create_dir(&path).unwrap();
        TurnResult::started("s")
    })
    .await;
    assert_eq!(r.status, TurnStatus::Unknown);
    fs::remove_dir(&path).unwrap();
    fs::rename(displaced, &path).unwrap();
    assert_eq!(
        rejected(&f, "s", "p", "key").await.status,
        TurnStatus::Unknown
    );
}

#[tokio::test]
async fn unsupported_or_incomplete_store_is_not_overwritten() {
    for value in [
        json!({"version":999,"entries":[]}),
        json!({}),
        json!({"version":2,"entries":[],"future":"keep"}),
    ] {
        let f = Fixture::new();
        write(&f, &value);
        let before = fs::read(f.path()).unwrap();
        assert_eq!(
            rejected(&f, "s", "p", "key").await.status,
            TurnStatus::Unknown
        );
        assert_eq!(fs::read(f.path()).unwrap(), before);
    }
}

#[tokio::test]
async fn legacy_owner_is_not_replayed_and_different_session_can_use_key() {
    let f = Fixture::new();
    let mut legacy = TurnResult::started("old-session");
    legacy.idempotency_key = Some("key".into());
    write(&f, &json!({"entries":[{"key":"key","result":legacy}]}));
    let before = fs::read(f.path()).unwrap();
    let r = rejected(&f, "old-session", "p", "key").await;
    assert_eq!(r.status, TurnStatus::Unknown);
    assert_eq!(fs::read(f.path()).unwrap(), before);
    let legacy_entry = read(&f)["entries"][0].clone();
    started(&f, "new-session", "key").await;
    assert_eq!(read(&f)["entries"][0], legacy_entry);
    assert_eq!(read(&f)["entries"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn conflicting_prompt_returns_409_and_leaves_receipt_unchanged() {
    let f = Fixture::new();
    started(&f, "s", "key").await;
    let before = fs::read(f.path()).unwrap();
    let r = rejected(&f, "s", "different", "key").await;
    assert_eq!(r.status, TurnStatus::IdempotencyConflict);
    assert_eq!(super::status_for(&r.status), 409);
    assert_eq!(
        serde_json::to_value(&r).unwrap()["status"],
        "idempotency_conflict"
    );
    assert_eq!(fs::read(f.path()).unwrap(), before);
}

#[tokio::test]
async fn unknown_serializes_to_explicit_non_success_conflict() {
    let f = Fixture::new();
    let (r, _) = run_at(&f.path(), "s", "p", Some("key"), || async {
        TurnResult::fail(TurnStatus::RetryLater, "s", "timeout")
    })
    .await;
    assert_eq!(r.status, TurnStatus::Unknown);
    assert!(!r.ok);
    assert_eq!(super::status_for(&r.status), 409);
    assert_eq!(serde_json::to_value(&r).unwrap()["status"], "unknown");
    let stored = read(&f);
    assert_eq!(stored["entries"][0]["state"], "completed");
    assert_eq!(
        rejected(&f, "s", "p", "key").await.status,
        TurnStatus::Unknown
    );
}

#[tokio::test]
async fn imported_history_cannot_bypass_readonly_via_any_cache() {
    let f = Fixture::new();
    let path = f.path();
    fs::write(&path, b"not even JSON").unwrap();
    let before = fs::read(&path).unwrap();
    for key in [None, Some("key")] {
        let (r, replayed) = run_at(
            &path,
            "imp:83761701-94e0-4ae4-904a-4917ca98ddcb",
            "p",
            key,
            || async { panic!("read-only") },
        )
        .await;
        assert_eq!(r.status, TurnStatus::Error);
        assert!(!replayed);
        assert_eq!(r.session_id, "imp:83761701-94e0-4ae4-904a-4917ca98ddcb");
    }
    assert_eq!(fs::read(path).unwrap(), before);
}

#[tokio::test]
async fn queued_replay_retains_its_session_prompt_and_single_queue_item() {
    let f = Fixture::new();
    let path = f.path();
    let (first, _) = run_at(&path, "s", "p", Some("key"), || async {
        let mut r = TurnResult::started("s");
        r.status = TurnStatus::Queued;
        r.queue_item_id = Some("q-one".into());
        r.queued_prompt = Some("p".into());
        r
    })
    .await;
    let (replayed, hit) = run_at(&path, "s", "p", Some("key"), || async {
        panic!("double queue")
    })
    .await;
    assert!(hit);
    assert_eq!(
        serde_json::to_value(&first).unwrap(),
        serde_json::to_value(&replayed).unwrap()
    );
    let wrong = rejected(&f, "s", "not-p", "key").await;
    assert!(wrong.queue_item_id.is_none());
    assert!(wrong.queued_prompt.is_none());
    let (other, hit) = run_at(&path, "other", "p", Some("key"), || async {
        TurnResult::started("other")
    })
    .await;
    assert!(other.ok);
    assert!(!hit);
    assert!(other.queued_prompt.is_none());
}

#[tokio::test]
async fn corrupt_result_identity_and_payload_fail_closed() {
    let f = Fixture::new();
    started(&f, "s", "key").await;
    let original = read(&f);
    let mut cases = Vec::new();
    let mut v = original.clone();
    v["entries"][0]["result"]["idempotencyKey"] = json!("wrong");
    cases.push(v);
    let mut v = original.clone();
    v["entries"][0]["prompt_sha256"] = json!("bad");
    cases.push(v);
    let mut v = original.clone();
    v["entries"][0]["state"] = json!("pending");
    cases.push(v);
    let mut v = original.clone();
    v["entries"][0]["result"]["ok"] = json!(false);
    cases.push(v);
    let mut v = original.clone();
    let entry = v["entries"][0].clone();
    v["entries"].as_array_mut().unwrap().push(entry);
    cases.push(v);
    let mut v = original;
    v["entries"][0]["result"]["status"] = json!("queued");
    v["entries"][0]["result"]["queueItemId"] = json!("q");
    v["entries"][0]["result"]["queuedPrompt"] = json!("wrong");
    cases.push(v);
    for v in cases {
        write(&f, &v);
        let before = fs::read(f.path()).unwrap();
        assert_eq!(
            rejected(&f, "s", "p", "key").await.status,
            TurnStatus::Unknown
        );
        assert_eq!(fs::read(f.path()).unwrap(), before);
    }
}

#[tokio::test]
async fn fully_unresolved_capacity_rejects_without_evicting_any_claim() {
    let f = Fixture::new();
    let seed = receipt_store::Request::new(&f.path(), "seed", "p", "seed");
    receipt_store::begin(&seed).unwrap();
    let mut v = read(&f);
    let template = v["entries"][0].clone();
    let mut entries = Vec::new();
    for i in 0..receipt_store::CAP {
        let mut entry = template.clone();
        entry["key"] = json!(format!("k-{i}"));
        entry["result"]["idempotencyKey"] = entry["key"].clone();
        entries.push(entry);
    }
    v["entries"] = json!(entries);
    write(&f, &v);
    let before = fs::read(f.path()).unwrap();
    assert_eq!(
        rejected(&f, "s", "p", "new").await.status,
        TurnStatus::Unknown
    );
    assert_eq!(fs::read(f.path()).unwrap(), before);
}

#[tokio::test]
async fn capacity_only_evicts_confirmed_receipts_never_pending_or_unknown() {
    let f = Fixture::new();
    started(&f, "s", "seed").await;
    let mut v = read(&f);
    let template = v["entries"][0].clone();
    let mut entries = Vec::new();
    for i in 0..receipt_store::CAP {
        let mut entry = template.clone();
        entry["key"] = json!(format!("k-{i}"));
        entry["result"]["idempotencyKey"] = entry["key"].clone();
        entries.push(entry);
    }
    entries[0]["state"] = json!("pending");
    entries[0]["result"]["status"] = json!("unknown");
    entries[0]["result"]["ok"] = json!(false);
    entries[1]["result"]["status"] = json!("unknown");
    entries[1]["result"]["ok"] = json!(false);
    let pending = entries[0].clone();
    let unknown = entries[1].clone();
    v["entries"] = json!(entries);
    write(&f, &v);
    started(&f, "s", "new").await;
    let result = read(&f);
    let list = result["entries"].as_array().unwrap();
    assert_eq!(list.len(), receipt_store::CAP);
    assert_eq!(list[0], pending);
    assert_eq!(list[1], unknown);
    assert!(list.iter().all(|e| e["key"] != "k-2"));
}

#[tokio::test]
async fn completion_with_wrong_claim_nonce_cannot_overwrite() {
    let f = Fixture::new();
    let request = receipt_store::Request::new(&f.path(), "s", "p", "key");
    receipt_store::begin(&request).unwrap();
    let before = fs::read(f.path()).unwrap();
    let mut result = TurnResult::started("s");
    result.idempotency_key = Some("key".into());
    assert!(receipt_store::finish(&request, "wrong-nonce", result).is_err());
    assert_eq!(fs::read(f.path()).unwrap(), before);
}

#[tokio::test]
async fn unrelated_concurrent_requests_do_not_lose_receipts() {
    let f = Fixture::new();
    let path = f.path();
    let mut tasks = Vec::new();
    for i in 0..24 {
        let path = path.clone();
        tasks.push(tokio::spawn(async move {
            let key = format!("k-{i}");
            run_at(&path, "s", "p", Some(&key), || async {
                tokio::task::yield_now().await;
                TurnResult::started("s")
            })
            .await
            .0
        }));
    }
    for task in tasks {
        assert!(task.await.unwrap().ok);
    }
    assert_eq!(read(&f)["entries"].as_array().unwrap().len(), 24);
}

#[test]
fn unknown_and_conflict_cli_results_never_signal_success_or_app_not_running() {
    for status in [TurnStatus::Unknown, TurnStatus::IdempotencyConflict] {
        let result = TurnResult::fail(status, "s", "synthetic");
        assert_eq!(super::turn_exit_code(&result), 1);
    }
    for error in [
        "operation timed out",
        "connection reset",
        "HTTP 503",
        "invalid response body",
    ] {
        let (status, code, message) = super::classify_cli_transport_error(error, "s");
        assert_eq!(status, TurnStatus::Unknown);
        assert_eq!(code, 1);
        assert!(message.contains("do not automatically resend"));
    }
}

#[tokio::test]
async fn keyless_dispatch_timeout_is_unknown_after_callback_started() {
    let f = Fixture::new();
    let entered = std::cell::Cell::new(false);
    let (result, _) = run_at(&f.path(), "s", "p", None, || async {
        super::with_dispatch_timeout_for(std::time::Duration::from_millis(10), "s".into(), async {
            entered.set(true);
            std::future::pending::<TurnResult>().await
        })
        .await
    })
    .await;
    assert!(entered.get());
    assert_eq!(result.status, TurnStatus::Unknown);
    assert!(!result.ok);
    assert!(result.idempotency_key.is_none());
    assert!(!f.path().exists());
}

#[tokio::test]
async fn unknown_result_fields_are_preserved_instead_of_silently_dropped() {
    let f = Fixture::new();
    started(&f, "s", "key").await;
    let mut value = read(&f);
    value["entries"][0]["result"]["futureField"] = json!({"preserve":true});
    write(&f, &value);
    let before = fs::read(f.path()).unwrap();
    assert_eq!(
        rejected(&f, "other", "p", "new-key").await.status,
        TurnStatus::Unknown
    );
    assert_eq!(fs::read(f.path()).unwrap(), before);
}

#[tokio::test]
async fn rescheduled_unknown_queue_item_does_not_dispatch_again() {
    let f = Fixture::new();
    let calls = std::cell::Cell::new(0);
    for _ in 0..5 {
        let result = super::idempotency::run_queued_at(&f.0, "s", "p", "q-one", || async {
            calls.set(calls.get() + 1);
            TurnResult::fail(TurnStatus::Unknown, "s", "ambiguous drain")
        })
        .await;
        assert_eq!(result.status, TurnStatus::Unknown);
    }
    assert_eq!(calls.get(), 1);
}

#[tokio::test]
async fn queue_identity_cannot_collide_with_an_external_request_key() {
    let f = Fixture::new();
    let path = f.0.join("session-api-idempotency.json");
    let (external, _) = run_at(&path, "s", "external", Some("q-one"), || async {
        TurnResult::started("s")
    })
    .await;
    assert!(external.ok);
    let queue = super::idempotency::run_queued_at(
        &f.0,
        "s",
        "different queued prompt",
        "q-one",
        || async { TurnResult::started("s") },
    )
    .await;
    assert!(queue.ok);
    let invalid = super::idempotency::run_queued_at(&f.0, "s", "p", " ", || async {
        panic!("identity missing")
    })
    .await;
    assert_eq!(invalid.status, TurnStatus::Error);
}
