//! Original dispatcher task ownership, not a real App/CLI permission scenario.
use super::*;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancelled_dispatcher_join_keeps_original_task_for_next_shutdown_observer() {
    let manager = Arc::new(SessionManager::new());
    let (release, blocked) = tokio::sync::oneshot::channel();
    let (entered, started) = tokio::sync::oneshot::channel();
    let (settled, original_settled) = tokio::sync::oneshot::channel();
    *manager.computer_use_dispatcher.task.lock() = Some(Arc::new(tokio::sync::Mutex::new(Some(
        tauri::async_runtime::spawn(async move {
            entered.send(()).unwrap();
            blocked.await.unwrap();
            settled.send(()).unwrap();
        }),
    ))));
    tokio::time::timeout(Duration::from_secs(2), started)
        .await
        .unwrap()
        .unwrap();
    let first_owner = manager.clone();
    let first = tokio::spawn(async move { first_owner.join_computer_use_dispatcher().await });
    tokio::time::timeout(
        Duration::from_secs(2),
        manager.computer_use_dispatcher.join_entered.notified(),
    )
    .await
    .unwrap();
    first.abort();
    assert!(first.await.unwrap_err().is_cancelled());
    let retained = manager.computer_use_dispatcher.task.lock().is_some();
    let next_owner = manager.clone();
    let mut next = tokio::spawn(async move { next_owner.join_computer_use_dispatcher().await });
    tokio::time::timeout(
        Duration::from_secs(2),
        manager.computer_use_dispatcher.join_entered.notified(),
    )
    .await
    .unwrap();
    let premature = tokio::time::timeout(Duration::from_millis(25), &mut next)
        .await
        .is_ok();
    release.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(2), original_settled)
        .await
        .unwrap()
        .unwrap();
    if !premature {
        next.await.unwrap().unwrap();
    }
    assert!(
        retained,
        "cancelled shutdown discarded the original dispatcher handle"
    );
    assert!(
        !premature,
        "shutdown reported success while the original dispatcher was still blocked"
    );
    assert!(manager.computer_use_dispatcher.task.lock().is_none());
}

#[tokio::test]
async fn failed_original_dispatcher_join_cannot_be_rewritten_as_success_by_later_shutdown() {
    let manager = SessionManager::new();
    *manager.computer_use_dispatcher.task.lock() = Some(Arc::new(tokio::sync::Mutex::new(Some(
        tauri::async_runtime::spawn(async {
            panic!("synthetic original decision dispatcher failure");
        }),
    ))));
    assert!(manager.join_computer_use_dispatcher().await.is_err());
    assert!(
        manager.join_computer_use_dispatcher().await.is_err(),
        "original dispatcher join failure was discarded"
    );
}
