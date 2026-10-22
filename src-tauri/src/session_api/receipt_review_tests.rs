use super::{
    idempotency,
    idempotency_tests::Fixture,
    receipt_lease,
    receipt_review::{self, Decision, ReviewRequest},
    receipt_store::{self, Request},
    TurnResult, TurnStatus,
};
use serde_json::{json, Value};
use std::{cell::Cell, fs, time::Duration};
fn seed(f: &Fixture) {
    receipt_store::begin(&Request::new(&f.path(), "s", "original prompt", "old")).unwrap();
}
fn request(f: &Fixture, decision: Decision) -> ReviewRequest {
    let snap = receipt_store::snapshot(&f.path()).unwrap();
    let row = snap.items.iter().find(|r| r.key == "old").unwrap();
    serde_json::from_value(json!({"sessionId":"s","key":"old","revision":row.revision,"prompt":"original prompt","decision":decision,"reviewed":true,"confirmed":true})).unwrap()
}
#[test]
fn snapshot_is_read_only_and_does_not_disclose_prompt_or_claim_nonce() {
    let f = Fixture::new();
    assert!(receipt_store::snapshot(&f.path()).unwrap().items.is_empty());
    assert!(!f.path().exists());
    seed(&f);
    let before = fs::read(f.path()).unwrap();
    let snap = serde_json::to_string(&receipt_store::snapshot(&f.path()).unwrap()).unwrap();
    let value: Value = serde_json::from_slice(&before).unwrap();
    let nonce = value["entries"][0]["claim_id"].as_str().unwrap();
    assert!(!snap.contains(nonce));
    assert!(!snap.contains("original prompt"));
    assert_eq!(fs::read(f.path()).unwrap(), before);
}
#[tokio::test]
async fn dismiss_records_review_but_old_identity_stays_unknown_forever() {
    let f = Fixture::new();
    seed(&f);
    let r = request(&f, Decision::Dismiss);
    let ack = receipt_review::review_at(&f.path(), r, || async { panic!("dismiss cannot send") })
        .await
        .unwrap();
    assert_eq!(ack.decision, Decision::Dismiss);
    assert!(ack.replacement_key.is_none());
    let before = fs::read(f.path()).unwrap();
    assert_eq!(
        receipt_review::review_at(&f.path(), request(&f, Decision::Dismiss), || async {
            panic!()
        })
        .await
        .unwrap(),
        ack
    );
    assert_eq!(fs::read(f.path()).unwrap(), before);
    let (result, replay) =
        idempotency::run_at(&f.path(), "s", "original prompt", Some("old"), || async {
            panic!()
        })
        .await;
    assert_eq!(result.status, TurnStatus::Unknown);
    assert!(!replay);
}
#[tokio::test]
async fn retry_atomically_reserves_new_identity_before_dispatch_and_replays_ack_once() {
    let f = Fixture::new();
    seed(&f);
    let calls = Cell::new(0);
    let ack = receipt_review::review_at(&f.path(), request(&f, Decision::Retry), || async {
        let value: Value = serde_json::from_slice(&fs::read(f.path()).unwrap()).unwrap();
        assert_eq!(value["version"], 3);
        assert_eq!(value["entries"].as_array().unwrap().len(), 2);
        assert_eq!(value["entries"][1]["state"], "pending");
        assert_eq!(value["entries"][0]["result"]["status"], "unknown");
        assert!(!String::from_utf8(fs::read(f.path()).unwrap())
            .unwrap()
            .contains("original prompt"));
        calls.set(calls.get() + 1);
        TurnResult::started("s")
    })
    .await
    .unwrap();
    assert_eq!(calls.get(), 1);
    assert_ne!(ack.replacement_key.as_deref(), Some("old"));
    assert_eq!(ack.replacement_status, Some(TurnStatus::TurnStarted));
    let before = fs::read(f.path()).unwrap();
    assert_eq!(
        receipt_review::review_at(&f.path(), request(&f, Decision::Retry), || async {
            panic!()
        })
        .await
        .unwrap(),
        ack
    );
    assert_eq!(fs::read(f.path()).unwrap(), before);
    let old = idempotency::run_at(&f.path(), "s", "original prompt", Some("old"), || async {
        panic!()
    })
    .await
    .0;
    assert_eq!(old.status, TurnStatus::Unknown);
    let child = idempotency::run_at(
        &f.path(),
        "s",
        "original prompt",
        ack.replacement_key.as_deref(),
        || async { panic!() },
    )
    .await;
    assert_eq!(child.0.status, TurnStatus::TurnStarted);
    assert!(child.1);
}
#[tokio::test]
async fn dropped_review_future_never_resends_reserved_child() {
    let f = Fixture::new();
    seed(&f);
    let path = f.path();
    let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
    let mut review = Box::pin(receipt_review::review_at(
        &path,
        request(&f, Decision::Retry),
        || async move {
            entered_tx.send(()).expect("dispatch readiness receiver");
            std::future::pending::<TurnResult>().await
        },
    ));
    // Cancel at dispatch, not during blocking lease/reservation I/O. The timeout
    // only bounds a broken readiness handshake; it never triggers cancellation.
    tokio::select! {
        result = &mut review => panic!("review ended before dispatch readiness: {result:?}"),
        entered = tokio::time::timeout(Duration::from_secs(30), entered_rx) => {
            entered.expect("dispatch readiness watchdog expired")
                .expect("dispatch readiness sender dropped");
        }
    }
    let reserved: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(reserved["entries"].as_array().unwrap().len(), 2);
    assert_eq!(reserved["entries"][1]["state"], "pending");
    assert_eq!(reserved["entries"][0]["result"]["status"], "unknown");
    drop(review);
    let ack = receipt_review::review_at(&f.path(), request(&f, Decision::Retry), || async {
        panic!("never resume dispatch")
    })
    .await
    .unwrap();
    assert_eq!(ack.replacement_status, Some(TurnStatus::Unknown));
    let r = idempotency::run_at(
        &f.path(),
        "s",
        "original prompt",
        ack.replacement_key.as_deref(),
        || async { panic!() },
    )
    .await
    .0;
    assert_eq!(r.status, TurnStatus::Unknown);
}
#[tokio::test]
async fn crash_after_atomic_reservation_before_callback_is_unknown_not_an_outbox() {
    let f = Fixture::new();
    seed(&f);
    receipt_store::review(&f.path(), &request(&f, Decision::Retry)).unwrap();
    let ack = receipt_review::review_at(&f.path(), request(&f, Decision::Retry), || async {
        panic!()
    })
    .await
    .unwrap();
    assert_eq!(ack.replacement_status, Some(TurnStatus::Unknown));
}
#[tokio::test]
async fn conflicting_decision_prompt_revision_and_missing_confirmations_do_not_write() {
    let f = Fixture::new();
    seed(&f);
    let before = fs::read(f.path()).unwrap();
    for patch in [
        json!({"prompt":"other"}),
        json!({"reviewed":false}),
        json!({"confirmed":false}),
        json!({"revision":"0".repeat(64)}),
        json!({"sessionId":"imp:x"}),
        json!({"key":"old "}),
    ] {
        let mut raw = json!({"sessionId":"s","key":"old","revision":request(&f,Decision::Retry).revision,"prompt":"original prompt","decision":"retry","reviewed":true,"confirmed":true});
        for (k, v) in patch.as_object().unwrap() {
            raw[k] = v.clone();
        }
        let r = serde_json::from_value(raw).unwrap();
        assert!(
            receipt_review::review_at(&f.path(), r, || async { panic!() })
                .await
                .is_err()
        );
        assert_eq!(fs::read(f.path()).unwrap(), before);
    }
    receipt_review::review_at(&f.path(), request(&f, Decision::Dismiss), || async {
        panic!()
    })
    .await
    .unwrap();
    let before = fs::read(f.path()).unwrap();
    assert_eq!(
        receipt_review::review_at(&f.path(), request(&f, Decision::Retry), || async {
            panic!()
        })
        .await
        .unwrap_err(),
        "SESSION_RECEIPT_CHANGED"
    );
    assert_eq!(fs::read(f.path()).unwrap(), before);
}
#[tokio::test]
async fn original_normalized_prompt_is_required_only_for_retry() {
    let f = Fixture::new();
    seed(&f);
    let mut r = request(&f, Decision::Retry);
    r.prompt = " original prompt  ".into();
    assert!(
        receipt_review::review_at(&f.path(), r, || async { TurnResult::started("s") })
            .await
            .is_ok()
    );
    let f = Fixture::new();
    seed(&f);
    let mut r = request(&f, Decision::Dismiss);
    r.prompt.clear();
    assert!(
        receipt_review::review_at(&f.path(), r, || async { panic!() })
            .await
            .is_ok()
    );
}
#[test]
fn leases_allow_parallel_readers_but_exclude_review_across_canonical_aliases() {
    let f = Fixture::new();
    let a = receipt_lease::try_acquire(&f.path(), false)
        .unwrap()
        .unwrap();
    let b = receipt_lease::try_acquire(&f.0.join(".").join("receipts.json"), false)
        .unwrap()
        .unwrap();
    assert!(receipt_lease::try_acquire(&f.path(), true)
        .unwrap()
        .is_none());
    drop(a);
    assert!(receipt_lease::try_acquire(&f.path(), true)
        .unwrap()
        .is_none());
    drop(b);
    let c = receipt_lease::try_acquire(&f.path(), true)
        .unwrap()
        .unwrap();
    assert!(receipt_lease::try_acquire(&f.path(), false)
        .unwrap()
        .is_none());
    drop(c);
    assert!(receipt_lease::try_acquire(&f.path(), false)
        .unwrap()
        .is_some());
}
#[tokio::test]
async fn active_dispatch_prevents_review_before_any_store_change() {
    let f = Fixture::new();
    seed(&f);
    let before = fs::read(f.path()).unwrap();
    let _reader = receipt_lease::try_acquire(&f.path(), false)
        .unwrap()
        .unwrap();
    assert_eq!(
        receipt_review::review_at(&f.path(), request(&f, Decision::Retry), || async {
            panic!()
        })
        .await
        .unwrap_err(),
        "SESSION_RECEIPT_ACTIVE"
    );
    assert_eq!(fs::read(f.path()).unwrap(), before);
}
#[tokio::test]
async fn active_review_prevents_new_dispatch_without_creating_a_claim() {
    let f = Fixture::new();
    let _writer = receipt_lease::try_acquire(&f.path(), true)
        .unwrap()
        .unwrap();
    let result = idempotency::run_at(&f.path(), "s", "p", Some("key"), || async { panic!() })
        .await
        .0;
    assert_eq!(result.status, TurnStatus::Unknown);
    assert!(!f.path().exists());
}
#[test]
fn legacy_and_v2_reads_do_not_upgrade_or_offer_unverifiable_review() {
    let f = Fixture::new();
    fs::write(f.path(),serde_json::to_vec(&json!({"entries":[{"key":"old","result":{"ok":false,"status":"unknown","sessionId":"s"}}]})).unwrap()).unwrap();
    let before = fs::read(f.path()).unwrap();
    let snap = receipt_store::snapshot(&f.path()).unwrap();
    assert!(!snap.items[0].can_review);
    assert_eq!(fs::read(f.path()).unwrap(), before);
    let f = Fixture::new();
    seed(&f);
    let mut v: Value = serde_json::from_slice(&fs::read(f.path()).unwrap()).unwrap();
    v["version"] = json!(2);
    fs::write(f.path(), serde_json::to_vec(&v).unwrap()).unwrap();
    let before = fs::read(f.path()).unwrap();
    assert!(receipt_store::snapshot(&f.path()).unwrap().items[0].can_review);
    assert_eq!(fs::read(f.path()).unwrap(), before);
}
#[tokio::test]
async fn reviewed_replacement_is_pinned_against_capacity_eviction() {
    let f = Fixture::new();
    seed(&f);
    let ack = receipt_review::review_at(&f.path(), request(&f, Decision::Retry), || async {
        TurnResult::started("s")
    })
    .await
    .unwrap();
    for n in 0..receipt_store::CAP + 5 {
        let key = format!("key-{n}");
        idempotency::run_at(&f.path(), "other", "p", Some(&key), || async {
            TurnResult::started("other")
        })
        .await;
    }
    assert_eq!(
        receipt_review::review_at(&f.path(), request(&f, Decision::Retry), || async {
            panic!()
        })
        .await
        .unwrap(),
        ack
    );
    let v: Value = serde_json::from_slice(&fs::read(f.path()).unwrap()).unwrap();
    assert_eq!(v["entries"].as_array().unwrap().len(), receipt_store::CAP);
}
#[tokio::test]
async fn all_unknown_capacity_blocks_new_retry_without_recording_review() {
    let f = Fixture::new();
    seed(&f);
    for n in 1..receipt_store::CAP {
        receipt_store::begin(&Request::new(&f.path(), "s", "p", &format!("u-{n}"))).unwrap();
    }
    let before = fs::read(f.path()).unwrap();
    assert_eq!(
        receipt_review::review_at(&f.path(), request(&f, Decision::Retry), || async {
            panic!()
        })
        .await
        .unwrap_err(),
        "SESSION_RECEIPT_CAPACITY"
    );
    assert_eq!(fs::read(f.path()).unwrap(), before);
}
#[tokio::test]
async fn late_original_finish_cannot_overwrite_reviewed_unknown() {
    let f = Fixture::new();
    let req = Request::new(&f.path(), "s", "original prompt", "old");
    let nonce = match receipt_store::begin(&req).unwrap() {
        receipt_store::Begin::Reserved(n) => n,
        _ => panic!(),
    };
    receipt_review::review_at(&f.path(), request(&f, Decision::Dismiss), || async {
        panic!()
    })
    .await
    .unwrap();
    let before = fs::read(f.path()).unwrap();
    assert!(receipt_store::finish(&req, &nonce, TurnResult::started("s")).is_err());
    assert_eq!(fs::read(f.path()).unwrap(), before);
}
#[tokio::test]
async fn missing_or_corrupt_child_fails_closed_and_never_recreates_it() {
    let f = Fixture::new();
    seed(&f);
    receipt_review::review_at(&f.path(), request(&f, Decision::Retry), || async {
        TurnResult::started("s")
    })
    .await
    .unwrap();
    let r = request(&f, Decision::Retry);
    let mut v: Value = serde_json::from_slice(&fs::read(f.path()).unwrap()).unwrap();
    v["entries"].as_array_mut().unwrap().pop();
    fs::write(f.path(), serde_json::to_vec(&v).unwrap()).unwrap();
    let before = fs::read(f.path()).unwrap();
    assert!(receipt_store::snapshot(&f.path()).is_err());
    assert_eq!(
        receipt_review::review_at(&f.path(), r, || async { panic!() })
            .await
            .unwrap_err(),
        "SESSION_RECEIPT_UNAVAILABLE"
    );
    assert_eq!(fs::read(f.path()).unwrap(), before);
}
#[tokio::test]
async fn wrong_session_and_retry_later_results_are_unknown() {
    for result in [
        TurnResult::started("wrong"),
        TurnResult::fail(TurnStatus::RetryLater, "s", "legacy"),
    ] {
        let f = Fixture::new();
        seed(&f);
        let ack =
            receipt_review::review_at(&f.path(), request(&f, Decision::Retry), || async { result })
                .await
                .unwrap();
        assert_eq!(ack.replacement_status, Some(TurnStatus::Unknown));
    }
}

#[tokio::test]
async fn uncertain_finish_cannot_recreate_or_redispatch_replacement() {
    let f = Fixture::new();
    seed(&f);
    let saved = f.0.join("reserved.json");
    let result = receipt_review::review_at(&f.path(), request(&f, Decision::Retry), || async {
        fs::rename(f.path(), &saved).unwrap();
        fs::create_dir(f.path()).unwrap();
        TurnResult::started("s")
    })
    .await;
    assert_eq!(result.unwrap_err(), "SESSION_RECEIPT_UNAVAILABLE");
    fs::remove_dir(f.path()).unwrap();
    fs::rename(saved, f.path()).unwrap();
    let ack = receipt_review::review_at(&f.path(), request(&f, Decision::Retry), || async {
        panic!()
    })
    .await
    .unwrap();
    assert_eq!(ack.replacement_status, Some(TurnStatus::Unknown));
}
#[test]
fn process_child() {
    let Ok(root) = std::env::var("GROK_RECEIPT_REVIEW_CHILD") else {
        return;
    };
    let path = std::path::PathBuf::from(root).join("receipts.json");
    let mode = std::env::var("GROK_RECEIPT_REVIEW_CHILD_MODE").unwrap();
    if mode == "reader" {
        let _lease = receipt_lease::try_acquire(&path, false).unwrap().unwrap();
        fs::write(path.with_extension("ready"), b"ready").unwrap();
        let now = std::time::Instant::now();
        while !path.with_extension("release").exists() {
            assert!(now.elapsed() < Duration::from_secs(10));
            std::thread::sleep(Duration::from_millis(10));
        }
        return;
    }
    // Intentionally exit only this isolated unit-test child, not the native App.
    let fixture = Fixture(path.parent().unwrap().to_path_buf());
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            receipt_review::review_at(&path, request(&fixture, Decision::Retry), || async {
                fs::write(path.with_extension("dispatched"), b"one").unwrap();
                std::process::exit(47)
            })
            .await
            .unwrap();
        });
}
fn child(f: &Fixture, mode: &str) -> std::process::Child {
    std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "session_api::receipt_review_tests::process_child",
            "--test-threads=1",
        ])
        .env("GROK_RECEIPT_REVIEW_CHILD", &f.0)
        .env("GROK_RECEIPT_REVIEW_CHILD_MODE", mode)
        .env("GROK_APP_HOME", f.0.join("isolated-app"))
        .env("GROK_HOME", f.0.join("isolated-agent"))
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::inherit())
        .spawn()
        .unwrap()
}
#[test]
fn live_other_process_reader_excludes_review_until_actual_exit() {
    let f = Fixture::new();
    let mut p = child(&f, "reader");
    let now = std::time::Instant::now();
    while !f.path().with_extension("ready").exists() {
        assert!(p.try_wait().unwrap().is_none());
        assert!(now.elapsed() < Duration::from_secs(5));
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(p.try_wait().unwrap().is_none());
    let denied = receipt_lease::try_acquire(&f.path(), true);
    let shared = receipt_lease::try_acquire(&f.path(), false).unwrap();
    assert!(shared.is_some());
    drop(shared);
    fs::write(f.path().with_extension("release"), b"done").unwrap();
    assert!(p.wait().unwrap().success());
    assert!(denied.unwrap().is_none());
    assert!(receipt_lease::try_acquire(&f.path(), true)
        .unwrap()
        .is_some());
}
#[tokio::test]
async fn process_exit_after_callback_leaves_unknown_without_automatic_repeat() {
    let f = Fixture::new();
    seed(&f);
    let mut p = child(&f, "exit");
    assert_eq!(p.wait().unwrap().code(), Some(47));
    assert_eq!(
        fs::read(f.path().with_extension("dispatched")).unwrap(),
        b"one"
    );
    let before = fs::read(f.path()).unwrap();
    let ack = receipt_review::review_at(&f.path(), request(&f, Decision::Retry), || async {
        panic!()
    })
    .await
    .unwrap();
    assert_eq!(ack.replacement_status, Some(TurnStatus::Unknown));
    assert_eq!(fs::read(f.path()).unwrap(), before);
}
