use std::future::Future;
use std::sync::mpsc;
use std::task::{Context, Poll};
use std::time::Duration;

use super::{
    run_prepare_command, session_import_prepare_cancel, session_import_prepare_get,
    session_import_prepare_items, session_import_prepare_retry,
};
use crate::session_migration::errors::{ErrorCode, ImportError};
use crate::session_migration::{catalog, test_home::IsolatedAppHome};

#[test]
fn prepare_command_yields_while_blocking_work_waits_on_another_thread() {
    let caller = std::thread::current().id();
    let (entered, entry) = mpsc::channel();
    let (release, released) = mpsc::channel();
    let mut future = Box::pin(run_prepare_command(move || {
        entered.send(std::thread::current().id()).unwrap();
        // A regressed inline implementation fails within a bounded interval;
        // it cannot strand the test thread indefinitely waiting for release.
        let allowed = released.recv_timeout(Duration::from_secs(3)).is_ok();
        Ok(allowed)
    }));
    let waker = futures_util::task::noop_waker();
    let first = future.as_mut().poll(&mut Context::from_waker(&waker));
    let worker = entry.recv_timeout(Duration::from_secs(5));
    let _ = release.send(());
    let yielded = matches!(first, Poll::Pending);
    let result = match first {
        Poll::Ready(result) => result,
        Poll::Pending => tauri::async_runtime::block_on(future),
    };
    assert!(
        yielded,
        "blocking prepare work ran before the command yielded"
    );
    assert_ne!(
        worker.unwrap(),
        caller,
        "prepare work ran on the caller thread"
    );
    assert!(result.unwrap(), "the worker was not released while pending");
}

#[test]
fn prepare_command_preserves_typed_domain_errors() {
    let expected =
        ImportError::new(ErrorCode::SourceBusy, "synthetic lock contention").into_command_err();
    let actual = tauri::async_runtime::block_on(run_prepare_command(|| {
        Err::<(), _>(ImportError::new(
            ErrorCode::SourceBusy,
            "synthetic lock contention",
        ))
    }));
    assert_eq!(actual.unwrap_err(), expected);
}

#[test]
fn prepare_command_facades_keep_completed_state_paging_and_retry_contracts() {
    let _home = IsolatedAppHome::acquire("prepare-command-facade");
    let batch_id = uuid::Uuid::new_v4().to_string();
    let root = catalog::imports_dir().join("prepare-batches");
    std::fs::create_dir_all(&root).unwrap();
    let connection =
        crate::session_migration::open_fixture_sqlite(root.join(format!("{batch_id}.sqlite")))
            .unwrap();
    connection
        .execute_batch(
            "CREATE TABLE meta(id INTEGER PRIMARY KEY,schema_version INTEGER,batch_id TEXT,
          scan_id TEXT,epoch INTEGER,selection_id TEXT,selection_version INTEGER,
          state TEXT,state_version INTEGER,cancel_requested INTEGER,created_at TEXT,
          updated_at TEXT,retention_expires_unix INTEGER);
         CREATE TABLE items(seq INTEGER PRIMARY KEY,candidate_id TEXT,title TEXT,provider TEXT,
          source_revision TEXT,input_json TEXT,state TEXT,attempt INTEGER,preview_id TEXT,
          selection_json TEXT,error_json TEXT);",
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO meta VALUES(1,1,?1,'synthetic-scan',1,'synthetic-selection',1,
         'completed',2,0,'2026-10-01','2026-10-01',0)",
            [&batch_id],
        )
        .unwrap();
    drop(connection);

    let get = tauri::async_runtime::block_on(session_import_prepare_get(batch_id.clone())).unwrap();
    assert_eq!(get.batch_id, batch_id);
    assert_eq!(
        get.state,
        crate::session_migration::prepare::PrepareBatchState::Completed
    );
    assert_eq!(get.total, 0);
    let page = tauri::async_runtime::block_on(session_import_prepare_items(
        batch_id.clone(),
        None,
        Some(50),
    ))
    .unwrap();
    assert!(page.items.is_empty());
    assert!(page.next_cursor.is_none());
    let cancelled =
        tauri::async_runtime::block_on(session_import_prepare_cancel(batch_id.clone())).unwrap();
    assert_eq!(cancelled.state, get.state);
    assert_eq!(cancelled.state_version, get.state_version);
    let retry = tauri::async_runtime::block_on(session_import_prepare_retry(batch_id)).unwrap_err();
    assert!(retry.contains("PLAN_STALE"), "{retry}");
}

#[test]
fn prepare_command_facades_keep_invalid_batch_errors() {
    let _home = IsolatedAppHome::acquire("prepare-command-invalid");
    let id = "not-an-import-batch".to_string();
    for error in [
        tauri::async_runtime::block_on(session_import_prepare_get(id.clone())).unwrap_err(),
        tauri::async_runtime::block_on(session_import_prepare_items(id.clone(), None, None))
            .unwrap_err(),
        tauri::async_runtime::block_on(session_import_prepare_cancel(id.clone())).unwrap_err(),
        tauri::async_runtime::block_on(session_import_prepare_retry(id)).unwrap_err(),
    ] {
        assert!(error.contains("PLAN_STALE"), "{error}");
    }
}
