use super::{
    queue_dispatch as dispatch,
    queue_regression_tests::{item, Fixture},
    queue_store as store, TurnResult, TurnStatus,
};
use std::{cell::Cell, fs};
#[tokio::test]
async fn successful_enqueue_is_durable_before_acknowledgement() {
    let f = Fixture::new();
    let q = item("q");
    let result = dispatch::enqueue_at(&f.0, q.clone()).await;
    assert!(result.ok);
    assert_eq!(result.status, TurnStatus::Queued);
    assert_eq!(store::list(&f.path()).unwrap(), vec![q]);
}
#[tokio::test]
async fn queue_read_write_or_lock_failure_never_reports_queued() {
    for mode in ["directory", "corrupt", "lock-directory"] {
        let f = Fixture::new();
        match mode {
            "directory" => fs::create_dir(f.path()).unwrap(),
            "corrupt" => fs::write(f.path(), b"corrupt").unwrap(),
            _ => fs::create_dir(crate::store_lock::lock_path_for(&f.path())).unwrap(),
        }
        let result = dispatch::enqueue_at(&f.0, item("q")).await;
        assert!(!result.ok);
        assert_eq!(result.status, TurnStatus::Unknown);
        assert!(result.queue_item_id.is_none());
        assert!(result.queued_prompt.is_none());
        if mode == "corrupt" {
            assert_eq!(fs::read(f.path()).unwrap(), b"corrupt");
        }
    }
}
#[tokio::test]
async fn full_queue_rejects_new_request_without_false_queued() {
    let f = Fixture::new();
    for i in 0..super::QUEUE_CAP {
        store::insert(&f.path(), item(&format!("q-{i}"))).unwrap();
    }
    let before = fs::read(f.path()).unwrap();
    let result = dispatch::enqueue_at(&f.0, item("overflow")).await;
    assert!(!result.ok);
    assert_eq!(fs::read(f.path()).unwrap(), before);
}
#[tokio::test]
async fn confirmed_dispatch_removes_only_its_item() {
    let f = Fixture::new();
    let q = item("q");
    store::insert(&f.path(), q.clone()).unwrap();
    store::insert(&f.path(), item("next")).unwrap();
    let result = dispatch::run_at(&f.0, &q, || async { TurnResult::started("s") })
        .await
        .unwrap();
    assert!(result.ok);
    assert_eq!(store::list(&f.path()).unwrap()[0].item_id, "next");
    assert!(dispatch::run_at(&f.0, &q, || async {
        panic!("stale item must not dispatch")
    })
    .await
    .is_none());
}
#[tokio::test]
async fn concurrent_drains_dispatch_once() {
    let f = Fixture::new();
    let q = item("q");
    store::insert(&f.path(), q.clone()).unwrap();
    let calls = Cell::new(0);
    let send = || async {
        calls.set(calls.get() + 1);
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        TurnResult::started("s")
    };
    let (a, b) = tokio::join!(
        dispatch::run_at(&f.0, &q, send),
        dispatch::run_at(&f.0, &q, send)
    );
    assert_eq!(calls.get(), 1);
    assert_eq!(usize::from(a.is_some()) + usize::from(b.is_some()), 1);
    assert!(store::list(&f.path()).unwrap().is_empty());
}
#[tokio::test]
async fn unknown_is_held_without_duplicate_dispatch_or_payload_loss() {
    let f = Fixture::new();
    let q = item("q");
    store::insert(&f.path(), q.clone()).unwrap();
    let result = dispatch::run_at(&f.0, &q, || async {
        TurnResult::fail(TurnStatus::Unknown, "s", "timeout")
    })
    .await
    .unwrap();
    assert_eq!(result.status, TurnStatus::Unknown);
    let before = fs::read(f.path()).unwrap();
    for _ in 0..5 {
        assert!(dispatch::run_at(&f.0, &q, || async {
            panic!("uncertain item must not dispatch")
        })
        .await
        .is_none());
    }
    assert_eq!(fs::read(f.path()).unwrap(), before);
    assert_eq!(store::list(&f.path()).unwrap()[0].prompt, "prompt");
}
#[tokio::test]
async fn canceled_drain_retains_claim_and_never_reenters_dispatch() {
    let f = Fixture::new();
    let q = item("q");
    store::insert(&f.path(), q.clone()).unwrap();
    let (entered, wait) = tokio::sync::oneshot::channel();
    {
        let mut future = Box::pin(dispatch::run_at(&f.0, &q, || async {
            entered.send(()).unwrap();
            std::future::pending::<TurnResult>().await
        }));
        tokio::select! {_=&mut future=>panic!("must remain pending"),_=wait=>{}}
    }
    assert_eq!(store::list(&f.path()).unwrap().len(), 1);
    assert!(
        dispatch::run_at(&f.0, &q, || async { panic!("canceled claim") })
            .await
            .is_none()
    );
}
#[tokio::test]
async fn failed_ack_remains_protected_even_after_receipt_eviction() {
    let f = Fixture::new();
    let q = item("q");
    store::insert(&f.path(), q.clone()).unwrap();
    let parked = f.0.join("retained-queue.json");
    let result = dispatch::run_at(&f.0, &q, || async {
        fs::rename(f.path(), &parked).unwrap();
        fs::create_dir(f.path()).unwrap();
        TurnResult::started("s")
    })
    .await
    .unwrap();
    assert_eq!(result.status, TurnStatus::Unknown);
    fs::remove_dir(f.path()).unwrap();
    fs::rename(&parked, f.path()).unwrap();
    for i in 0..=super::receipt_store::CAP {
        let r = super::idempotency::run_queued_at(
            &f.0,
            "other",
            "p",
            &format!("other-{i}"),
            || async { TurnResult::started("other") },
        )
        .await;
        assert!(r.ok);
    }
    let receipts: serde_json::Value = serde_json::from_slice(
        &fs::read(f.0.join("session-api-queue-dispatch-receipts.json")).unwrap(),
    )
    .unwrap();
    assert!(!receipts["entries"]
        .as_array()
        .unwrap()
        .iter()
        .any(|r| r["key"] == "q"));
    let before = fs::read(f.path()).unwrap();
    assert!(dispatch::run_at(&f.0, &q, || async {
        panic!("receipt eviction must not reset queue claim")
    })
    .await
    .is_none());
    assert_eq!(fs::read(f.path()).unwrap(), before);
}
#[tokio::test]
async fn replacement_must_be_durable_before_original_is_acknowledged() {
    let f = Fixture::new();
    let q = item("q");
    store::insert(&f.path(), q.clone()).unwrap();
    let result = dispatch::run_at(&f.0, &q, || dispatch::enqueue_at(&f.0, item("replacement")))
        .await
        .unwrap();
    assert_eq!(result.status, TurnStatus::Queued);
    assert!(result.ok);
    let items = store::list(&f.path()).unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].item_id, "replacement");
}
#[tokio::test]
async fn v90_pending_receipt_is_honored_when_claiming_legacy_item() {
    let f = Fixture::new();
    let q = item("q");
    store::insert(&f.path(), q.clone()).unwrap();
    let request = super::receipt_store::Request::new(
        &f.0.join("session-api-queue-dispatch-receipts.json"),
        "s",
        "prompt",
        "q",
    );
    assert!(matches!(
        super::receipt_store::begin(&request).unwrap(),
        super::receipt_store::Begin::Reserved(_)
    ));
    let result = dispatch::run_at(&f.0, &q, || async {
        panic!("V90 pending receipt must block")
    })
    .await
    .unwrap();
    assert_eq!(result.status, TurnStatus::Unknown);
    assert_eq!(store::list(&f.path()).unwrap().len(), 1);
}
