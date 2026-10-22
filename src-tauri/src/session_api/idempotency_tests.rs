use super::{idempotency::run_at, TurnResult, TurnStatus};
use std::{cell::Cell, fs, path::PathBuf};

pub(super) struct Fixture(pub(super) PathBuf);
impl Fixture {
    pub(super) fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("grok-session-receipts-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        Self(root)
    }
    pub(super) fn path(&self) -> PathBuf {
        self.0.join("receipts.json")
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[tokio::test]
async fn same_key_in_different_sessions_never_replays_another_session() {
    let f = Fixture::new();
    let path = f.path();
    let calls = Cell::new(0);
    for sid in ["session-a", "session-b"] {
        let (result, replay) = run_at(&path, sid, "prompt", Some("key"), || async {
            calls.set(calls.get() + 1);
            TurnResult::started(sid)
        })
        .await;
        assert_eq!(result.session_id, sid);
        assert!(!replay);
    }
    assert_eq!(calls.get(), 2);
}

#[tokio::test]
async fn changed_prompt_is_conflict_not_a_successful_replay() {
    let f = Fixture::new();
    let path = f.path();
    run_at(&path, "s", "first", Some("key"), || async {
        TurnResult::started("s")
    })
    .await;
    let (result, _) = run_at(&path, "s", "different", Some("key"), || async {
        panic!("must not dispatch")
    })
    .await;
    assert!(!result.ok, "a key cannot acknowledge a different prompt");
}

#[tokio::test]
async fn concurrent_duplicate_dispatches_once() {
    let f = Fixture::new();
    let path = f.path();
    let calls = Cell::new(0);
    let dispatch = || async {
        calls.set(calls.get() + 1);
        tokio::task::yield_now().await;
        TurnResult::started("s")
    };
    let (a, b) = tokio::join!(
        run_at(&path, "s", "p", Some("key"), dispatch),
        run_at(&path, "s", "p", Some("key"), dispatch)
    );
    assert!(a.0.ok || b.0.ok);
    assert_eq!(calls.get(), 1);
}

#[tokio::test]
async fn timed_out_dispatch_never_automatically_retries_the_same_key() {
    let f = Fixture::new();
    let path = f.path();
    let calls = Cell::new(0);
    run_at(&path, "s", "p", Some("key"), || async {
        calls.set(calls.get() + 1);
        TurnResult::fail(TurnStatus::RetryLater, "s", "dispatch timed out")
    })
    .await;
    let (result, _) = run_at(&path, "s", "p", Some("key"), || async {
        calls.set(calls.get() + 1);
        TurnResult::started("s")
    })
    .await;
    assert!(!result.ok);
    assert_eq!(calls.get(), 1);
}

#[tokio::test]
async fn cancelled_dispatch_leaves_a_durable_no_redispatch_claim() {
    let f = Fixture::new();
    let path = f.path();
    let calls = Cell::new(0);
    let (started, entered) = tokio::sync::oneshot::channel();
    let mut operation = Box::pin(run_at(&path, "s", "p", Some("key"), || async {
        calls.set(calls.get() + 1);
        started.send(()).unwrap();
        std::future::pending::<TurnResult>().await
    }));
    tokio::select! {
        _ = &mut operation => panic!("dispatch must remain in flight"),
        result = entered => result.unwrap(),
    }
    drop(operation); // Cancellation occurs after the actual dispatch callback, not a timer race.
    let (result, _) = run_at(&path, "s", "p", Some("key"), || async {
        calls.set(calls.get() + 1);
        TurnResult::started("s")
    })
    .await;
    assert!(!result.ok);
    assert_eq!(calls.get(), 1);
}

#[tokio::test]
async fn corrupt_receipts_fail_closed_and_preserve_bytes() {
    let f = Fixture::new();
    let path = f.path();
    let original = b"{broken receipt";
    fs::write(&path, original).unwrap();
    let calls = Cell::new(0);
    let (result, _) = run_at(&path, "s", "p", Some("key"), || async {
        calls.set(calls.get() + 1);
        TurnResult::started("s")
    })
    .await;
    assert!(!result.ok);
    assert_eq!(calls.get(), 0);
    assert_eq!(fs::read(&path).unwrap(), original);
}

#[tokio::test]
async fn unwritable_claim_does_not_dispatch() {
    let f = Fixture::new();
    let path = f.path();
    fs::create_dir(&path).unwrap();
    let calls = Cell::new(0);
    let (result, _) = run_at(&path, "s", "p", Some("key"), || async {
        calls.set(calls.get() + 1);
        TurnResult::started("s")
    })
    .await;
    assert!(!result.ok);
    assert_eq!(calls.get(), 0);
    assert!(path.is_dir());
}

#[tokio::test]
async fn exact_normalized_prompt_replays_without_dispatch() {
    let f = Fixture::new();
    let path = f.path();
    run_at(&path, "s", " p ", Some("key"), || async {
        TurnResult::started("s")
    })
    .await;
    let (result, replay) = run_at(&path, "s", "p", Some("key"), || async {
        panic!("must not dispatch")
    })
    .await;
    assert!(result.ok);
    assert!(replay);
    assert_eq!(result.idempotency_key.as_deref(), Some("key"));
}

#[tokio::test]
async fn requests_without_keys_keep_independent_dispatch() {
    let f = Fixture::new();
    let path = f.path();
    let calls = Cell::new(0);
    for _ in 0..2 {
        run_at(&path, "s", "p", None, || async {
            calls.set(calls.get() + 1);
            TurnResult::started("s")
        })
        .await;
    }
    assert_eq!(calls.get(), 2);
    assert!(!path.exists());
}
