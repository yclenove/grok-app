//! A shutdown observer may disappear, but the original consumer must not.
use super::*;

fn fixture() -> Arc<Watcher> {
    Arc::new(Watcher {
        stopping: AtomicBool::new(false),
        join_failed: AtomicBool::new(false),
        wake: Notify::new(),
        task: Mutex::new(None),
        cleanup_errors: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        join_entered: Notify::new(),
    })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancelled_cleanup_join_retains_original_consumer_for_the_next_join() {
    let state = fixture();
    let (release, blocked) = tokio::sync::oneshot::channel();
    let (entered, started) = tokio::sync::oneshot::channel();
    *state.task.lock() = Some(Arc::new(tokio::sync::Mutex::new(Some(
        tauri::async_runtime::spawn(async move {
            entered.send(()).unwrap();
            blocked.await.unwrap();
        }),
    ))));
    started.await.unwrap();
    let first_state = state.clone();
    let first = tokio::spawn(async move { join_with(&first_state).await });
    tokio::time::timeout(Duration::from_secs(2), state.join_entered.notified())
        .await
        .unwrap();
    first.abort();
    assert!(first.await.unwrap_err().is_cancelled());
    let retained = state.task.lock().is_some();
    let next_state = state.clone();
    let mut next = tokio::spawn(async move { join_with(&next_state).await });
    tokio::time::timeout(Duration::from_secs(2), state.join_entered.notified())
        .await
        .unwrap();
    let premature = tokio::time::timeout(Duration::from_millis(25), &mut next)
        .await
        .is_ok();
    release.send(()).unwrap();
    if !premature {
        next.await.unwrap().unwrap();
    }
    assert!(
        retained,
        "cancelled join abandoned the original consumer handle"
    );
    assert!(
        !premature,
        "a later shutdown claimed success with original cleanup still busy"
    );
    assert!(state.task.lock().is_none());
}

#[tokio::test]
async fn a_failed_original_consumer_join_cannot_be_relabelled_as_clean_by_a_later_join() {
    let state = fixture();
    *state.task.lock() = Some(Arc::new(tokio::sync::Mutex::new(Some(
        tauri::async_runtime::spawn(async {
            panic!("synthetic cleanup consumer failure");
        }),
    ))));
    assert!(join_with(&state).await.is_err());
    assert!(
        join_with(&state).await.is_err(),
        "lost original cleanup join failure"
    );
}
