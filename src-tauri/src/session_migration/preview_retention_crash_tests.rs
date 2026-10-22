//! Real process-exit boundary between durable lease retention and SQL COMMIT.

use super::*;

#[test]
fn crash_before_sql_commit_keeps_the_unaccepted_unexpired_preview_reusable() {
    let home = IsolatedAppHome::acquire("preview-retention-before-commit-crash");
    let (mut child, signal) = start_child(&home, "retention-before-commit");
    let status = child.0.wait().unwrap();
    assert_eq!(
        status.code(),
        Some(73),
        "the exact owned child exited at COMMIT"
    );

    let report: serde_json::Value =
        serde_json::from_slice(&std::fs::read(signal).unwrap()).unwrap();
    let path = std::path::PathBuf::from(report["path"].as_str().unwrap());
    let job_id = report["jobId"].as_str().unwrap();
    let original: jobs::PreviewLease =
        serde_json::from_value(report["originalLease"].clone()).unwrap();
    catalog::ensure_inside_imports(&path).unwrap();
    let conn = catalog::open_catalog().unwrap();
    assert!(jobs::get_job(&conn, job_id).unwrap().is_none());
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM job_items", [], |row| row
            .get::<_, u64>(0))
            .unwrap(),
        0,
        "SQLite rollback left no accepted input owner"
    );
    assert!(!original.retained_by_job);
    assert!(original.expires_unix > chrono::Utc::now().timestamp());
    let orphan = jobs::read_preview_lease(&path).unwrap().unwrap();
    assert!(
        orphan.retained_by_job,
        "the real durable write preceded the crash"
    );
    assert_eq!(orphan.file_hashes, original.file_hashes);
    assert_eq!(jobs::prepared_hashes(&path).unwrap(), original.file_hashes);

    assert_eq!(
        jobs::cleanup_unreferenced_preview_dirs().unwrap(),
        0,
        "an unaccepted crash orphan is not a completed job's disposable input"
    );
    assert!(
        path.is_dir(),
        "the original unexpired preview must remain available"
    );
    assert_eq!(jobs::prepared_hashes(&path).unwrap(), original.file_hashes);
    let repaired = jobs::read_preview_lease(&path).unwrap().unwrap();
    assert!(
        !repaired.retained_by_job,
        "reinspection must not remain SourceBusy"
    );
    assert_eq!(repaired.expires_unix, original.expires_unix);
    assert_eq!(repaired.prepare_batch_id, original.prepare_batch_id);
    assert_eq!(repaired.file_hashes, original.file_hashes);
}

#[test]
fn crash_after_sql_commit_keeps_the_accepted_input_until_worker_completion() {
    let home = IsolatedAppHome::acquire("preview-retention-after-commit-crash");
    let (mut child, signal) = start_child(&home, "retention-after-commit");
    assert_eq!(child.0.wait().unwrap().code(), Some(74));
    let report: serde_json::Value =
        serde_json::from_slice(&std::fs::read(signal).unwrap()).unwrap();
    let path = std::path::PathBuf::from(report["path"].as_str().unwrap());
    let job_id = report["jobId"].as_str().unwrap();
    catalog::ensure_inside_imports(&path).unwrap();
    let conn = catalog::open_catalog().unwrap();
    let accepted = jobs::get_job(&conn, job_id).unwrap().unwrap();
    assert_eq!(accepted.state, JobState::Queued);
    let before = jobs::read_preview_lease(&path).unwrap().unwrap();
    assert!(before.retained_by_job);
    assert_eq!(jobs::cleanup_unreferenced_preview_dirs().unwrap(), 0);
    assert_eq!(
        std::fs::read(path.join("lease.json")).unwrap(),
        serde_json::to_vec(&before).unwrap()
    );
    assert_eq!(job_control::run_pending().unwrap(), 1);
    assert_eq!(
        jobs::get_job(&conn, job_id).unwrap().unwrap().state,
        JobState::Completed
    );
    assert!(
        !path.exists(),
        "only real worker completion releases this input"
    );
}

#[test]
fn completed_job_with_the_same_candidate_does_not_own_a_new_crash_orphan() {
    let home = IsolatedAppHome::acquire("preview-retention-completed-candidate");
    let conn = catalog::open_catalog().unwrap();
    let item = input();
    let completed_path = item.prepared.as_ref().unwrap().path.clone();
    let accepted = job_control::enqueue(job(), vec![item]).unwrap();
    assert_eq!(job_control::run_pending().unwrap(), 1);
    assert!(!completed_path.exists());
    assert_eq!(
        jobs::get_job(&conn, &accepted.job_id)
            .unwrap()
            .unwrap()
            .state,
        JobState::Completed
    );

    let (mut child, signal) = start_child(&home, "retention-before-commit");
    assert_eq!(child.0.wait().unwrap().code(), Some(73));
    let report: serde_json::Value =
        serde_json::from_slice(&std::fs::read(signal).unwrap()).unwrap();
    let path = std::path::PathBuf::from(report["path"].as_str().unwrap());
    let original: jobs::PreviewLease =
        serde_json::from_value(report["originalLease"].clone()).unwrap();
    assert_ne!(path, completed_path);
    assert_eq!(original.candidate_id, "concurrent");
    assert!(jobs::get_job(&conn, report["jobId"].as_str().unwrap())
        .unwrap()
        .is_none());
    assert_eq!(
        jobs::cleanup_unreferenced_preview_dirs().unwrap(),
        0,
        "a different preview from a real completed job is not this orphan's owner"
    );
    assert_eq!(jobs::prepared_hashes(&path).unwrap(), original.file_hashes);
    assert!(
        !jobs::read_preview_lease(&path)
            .unwrap()
            .unwrap()
            .retained_by_job
    );
}

#[cfg(windows)]
#[test]
fn failed_rollback_io_keeps_the_unaccepted_input_for_explicit_reconciliation() {
    use std::os::windows::fs::OpenOptionsExt;
    use std::sync::{Arc, Mutex};

    let _home = IsolatedAppHome::acquire("preview-retention-rollback-io");
    let conn = catalog::open_catalog().unwrap();
    let item = input();
    let path = item.prepared.as_ref().unwrap().path.clone();
    let original = jobs::read_preview_lease(&path).unwrap().unwrap();
    let locked = Arc::new(Mutex::new(None));
    let hook_locked = Arc::clone(&locked);
    let hook_path = path.clone();
    BEFORE_ENQUEUE_COMMIT.with(|hook| {
        *hook.borrow_mut() = Some(Box::new(move || {
            *hook_locked.lock().unwrap() = Some(
                std::fs::OpenOptions::new()
                    .read(true)
                    .share_mode(1)
                    .open(hook_path.join("lease.json"))
                    .unwrap(),
            );
        }));
    });
    let requested = job();
    let reader = hold_reader(&conn);
    let error = job_control::enqueue(requested.clone(), vec![item.clone()]).unwrap_err();
    drop(reader);
    drop(locked.lock().unwrap().take());
    assert!(
        error.message.contains("retention rollback deferred"),
        "{error:?}"
    );
    assert!(jobs::get_job(&conn, &requested.job_id).unwrap().is_none());
    assert!(
        jobs::read_preview_lease(&path)
            .unwrap()
            .unwrap()
            .retained_by_job
    );
    assert_eq!(
        jobs::cleanup_unreferenced_preview_dirs().unwrap(),
        0,
        "repair I/O failure is not permission to delete an unexpired unaccepted input"
    );
    let repaired = jobs::read_preview_lease(&path).unwrap().unwrap();
    assert!(!repaired.retained_by_job);
    assert_eq!(repaired.expires_unix, original.expires_unix);
    assert_eq!(repaired.file_hashes, original.file_hashes);
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
