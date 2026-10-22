//! Real SQLite commit failures after lease publication, not manufactured jobs.

use super::*;
use crate::session_migration::errors::ErrorCode;

#[path = "preview_retention_crash_tests.rs"]
mod crash_tests;
#[path = "preview_retention_recovery_tests.rs"]
mod recovery_tests;

fn hold_reader(conn: &rusqlite::Connection) -> rusqlite::Transaction<'_> {
    // Rollback journal permits the enqueue writer reservation and lease writes,
    // but this read snapshot prevents its final EXCLUSIVE commit lock.
    let mode: String = conn
        .query_row("PRAGMA journal_mode=DELETE", [], |row| row.get(0))
        .unwrap();
    assert_eq!(mode, "delete");
    let reader = conn.unchecked_transaction().unwrap();
    reader
        .query_row("SELECT count(*) FROM jobs", [], |row| row.get::<_, u64>(0))
        .unwrap();
    reader
}

#[test]
fn failed_sql_commit_restores_unaccepted_retention_and_same_request_retries() {
    let _home = IsolatedAppHome::acquire("preview-retention-commit-failure");
    let conn = catalog::open_catalog().unwrap();
    let item = input();
    let path = item.prepared.as_ref().unwrap().path.clone();
    let before = jobs::read_preview_lease(&path).unwrap().unwrap();
    assert!(!before.retained_by_job);
    let requested = job();
    let reader = hold_reader(&conn);
    let error = job_control::enqueue(requested.clone(), vec![item.clone()]).unwrap_err();
    // Release before assertions so a red never leaves an active writer blocker.
    drop(reader);
    assert!(
        matches!(
            error.code,
            ErrorCode::SourceBusy | ErrorCode::SnapshotUnavailable
        ),
        "the original SQLite commit failure must remain visible: {error:?}"
    );
    assert!(error.message.contains("locked"), "{error:?}");
    assert!(jobs::get_job(&conn, &requested.job_id).unwrap().is_none());
    let after = jobs::read_preview_lease(&path).unwrap().unwrap();
    assert!(
        !after.retained_by_job,
        "a rejected SQL acceptance must not leave its preview permanently busy"
    );
    assert_eq!(after.expires_unix, before.expires_unix);
    assert_eq!(after.prepare_batch_id, before.prepare_batch_id);
    assert_eq!(after.file_hashes, before.file_hashes);
    let accepted = job_control::enqueue(requested.clone(), vec![item]).unwrap();
    assert_eq!(accepted.job_id, requested.job_id);
    assert!(
        jobs::read_preview_lease(&path)
            .unwrap()
            .unwrap()
            .retained_by_job
    );
    assert_eq!(job_control::run_pending().unwrap(), 1);
    assert_eq!(
        jobs::get_job(&conn, &accepted.job_id)
            .unwrap()
            .unwrap()
            .state,
        JobState::Completed
    );
}

#[test]
fn failed_sql_commit_never_clears_an_already_accepted_preview_owner() {
    let _home = IsolatedAppHome::acquire("preview-retention-existing-owner");
    let conn = catalog::open_catalog().unwrap();
    let item = input();
    let path = item.prepared.as_ref().unwrap().path.clone();
    let first = job_control::enqueue(job(), vec![item.clone()]).unwrap();
    let before = jobs::read_preview_lease(&path).unwrap().unwrap();
    assert!(before.retained_by_job);
    let rejected = job();
    let reader = hold_reader(&conn);
    let error = job_control::enqueue(rejected.clone(), vec![item]).unwrap_err();
    drop(reader);
    assert!(error.message.contains("locked"), "{error:?}");
    assert!(jobs::get_job(&conn, &rejected.job_id).unwrap().is_none());
    assert_eq!(
        jobs::get_job(&conn, &first.job_id).unwrap().unwrap().state,
        JobState::Queued
    );
    let after = jobs::read_preview_lease(&path).unwrap().unwrap();
    assert!(
        after.retained_by_job,
        "the successful owner's input remains protected"
    );
    assert_eq!(after.file_hashes, before.file_hashes);
    assert_eq!(jobs::cleanup_unreferenced_preview_dirs().unwrap(), 0);
    assert!(path.is_dir());
    assert_eq!(job_control::run_pending().unwrap(), 1);
    assert_eq!(
        jobs::get_job(&conn, &first.job_id).unwrap().unwrap().state,
        JobState::Completed
    );
}

#[test]
fn rollback_preserves_newer_lease_renewal_and_batch_adoption() {
    let _home = IsolatedAppHome::acquire("preview-retention-renewal");
    let conn = catalog::open_catalog().unwrap();
    let item = input();
    let path = item.prepared.as_ref().unwrap().path.clone();
    let expected_expiry = chrono::Utc::now().timestamp() + 1200;
    let hook_path = path.clone();
    BEFORE_ENQUEUE_COMMIT.with(|hook| {
        *hook.borrow_mut() = Some(Box::new(move || {
            jobs::renew_preview_lease(&hook_path, 1200).unwrap();
            jobs::adopt_preview_for_batch(&hook_path, "synthetic-newer-batch").unwrap();
        }));
    });
    let requested = job();
    let reader = hold_reader(&conn);
    let error = job_control::enqueue(requested.clone(), vec![item]).unwrap_err();
    drop(reader);
    assert!(error.message.contains("locked"), "{error:?}");
    assert!(jobs::get_job(&conn, &requested.job_id).unwrap().is_none());
    let lease = jobs::read_preview_lease(&path).unwrap().unwrap();
    assert!(!lease.retained_by_job);
    assert!(lease.expires_unix >= expected_expiry);
    assert_eq!(
        lease.prepare_batch_id.as_deref(),
        Some("synthetic-newer-batch")
    );
}

#[cfg(windows)]
#[test]
fn later_lease_failure_restores_earlier_inputs_and_retries_the_whole_job() {
    use std::os::windows::fs::OpenOptionsExt;
    let _home = IsolatedAppHome::acquire("preview-retention-multi-input");
    let conn = catalog::open_catalog().unwrap();
    let first = input_with_candidate("first");
    let second = input_with_candidate("second");
    let first_path = first.prepared.as_ref().unwrap().path.clone();
    let second_path = second.prepared.as_ref().unwrap().path.clone();
    let locked = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(second_path.join("lease.json"))
        .unwrap();
    let requested = job();
    let result = job_control::enqueue(requested.clone(), vec![first.clone(), second.clone()]);
    drop(locked);
    assert!(result.is_err());
    assert!(jobs::get_job(&conn, &requested.job_id).unwrap().is_none());
    for path in [&first_path, &second_path] {
        assert!(
            !jobs::read_preview_lease(path)
                .unwrap()
                .unwrap()
                .retained_by_job
        );
        assert!(path.is_dir());
    }
    let accepted = job_control::enqueue(requested, vec![first, second]).unwrap();
    assert_eq!(accepted.item_count, 2);
    assert_eq!(job_control::run_pending().unwrap(), 1);
    assert_eq!(
        jobs::get_job(&conn, &accepted.job_id)
            .unwrap()
            .unwrap()
            .state,
        JobState::Completed
    );
}

#[test]
fn other_process_acceptance_owner_rejects_without_writes_then_allows_retry() {
    use std::io::Write;
    let home = IsolatedAppHome::acquire("preview-retention-cross-process");
    let conn = catalog::open_catalog().unwrap();
    let item = input();
    let path = item.prepared.as_ref().unwrap().path.clone();
    let original = std::fs::read(path.join("lease.json")).unwrap();
    let (mut child, _) = start_child(&home, "retention");
    let requested = job();
    let rejected = job_control::enqueue(requested.clone(), vec![item.clone()]);
    child.0.stdin.take().unwrap().write_all(&[0]).unwrap();
    let child_exit = child.0.wait().unwrap();
    assert!(child_exit.success());
    assert_eq!(rejected.unwrap_err().code, ErrorCode::SourceBusy);
    assert!(jobs::get_job(&conn, &requested.job_id).unwrap().is_none());
    assert_eq!(std::fs::read(path.join("lease.json")).unwrap(), original);
    let accepted = job_control::enqueue(requested, vec![item]).unwrap();
    assert_eq!(job_control::run_pending().unwrap(), 1);
    assert_eq!(
        jobs::get_job(&conn, &accepted.job_id)
            .unwrap()
            .unwrap()
            .state,
        JobState::Completed
    );
}
