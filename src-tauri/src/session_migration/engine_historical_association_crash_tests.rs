//! Real association crash/rollback through the existing child and retention owner.
use super::*;
use crate::session_migration::{preview_lease_io, registration_provenance_store as proof_store};
use rusqlite::Connection;

struct HistoricalSnapshot {
    entry: serde_json::Value,
    source: serde_json::Value,
    paths: Vec<String>,
    old_job: serde_json::Value,
    receipts: Vec<(String, String)>,
    witnesses: Vec<(String, String)>,
    old_text: String,
}

impl HistoricalSnapshot {
    fn capture(conn: &Connection) -> Self {
        let source = sources::get(conn, INSTALLATION).unwrap();
        let mut statement = conn.prepare("SELECT key,value FROM meta WHERE key LIKE 'registration_association_accepted:%' OR key LIKE 'preview_retention_accepted:%' ORDER BY key").unwrap();
        Self {
            entry: serde_json::to_value(get_entry(conn, IMPORT).unwrap()).unwrap(),
            source: serde_json::to_value(&source).unwrap(),
            paths: sources::registered_paths(conn, &source).unwrap(),
            old_job: serde_json::to_value(jobs::get_job(conn, OLD_JOB).unwrap()).unwrap(),
            receipts: receipts(conn),
            witnesses: statement
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap(),
            old_text: text(IMPORT, REVISION),
        }
    }

    fn unaccepted(&self, conn: &Connection, requested: &JobRecord) {
        assert!(jobs::get_job(conn, &requested.job_id).unwrap().is_none());
        let count: u64 = conn
            .query_row(
                "SELECT count(*) FROM job_items WHERE job_id=?1",
                [&requested.job_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
        assert!(!proof_store::verify_schema(conn).unwrap());
        let after = Self::capture(conn);
        assert_eq!(after.entry, self.entry);
        assert_eq!(after.source, self.source);
        assert_eq!(after.paths, self.paths);
        assert_eq!(after.old_job, self.old_job);
        assert_eq!(after.receipts, self.receipts);
        assert_eq!(after.witnesses, self.witnesses);
        assert_eq!(after.old_text, self.old_text);
    }
}

fn recovery_file() -> PathBuf {
    let paths: Vec<_> = std::fs::read_dir(
        crate::session_migration::catalog::imports_dir().join("preview-retention"),
    )
    .unwrap()
    .map(|entry| entry.unwrap().path())
    .collect();
    assert_eq!(paths.len(), 1);
    paths[0].clone()
}

#[test]
#[ignore = "requires explicit hash-pinned V243 synthetic seed and registered source fixture"]
fn historical_seed_association_acceptance_before_commit_crash() {
    let home = seed_home();
    let _runtime = RuntimeReset;
    let conn = open_catalog().unwrap();
    let snapshot = HistoricalSnapshot::capture(&conn);
    let report = crate::session_migration::preview_lifecycle_tests::association_crash_report_at(
        &home,
        "association-before-commit",
    );
    let requested: JobRecord = serde_json::from_value(report["job"].clone()).unwrap();
    let original: jobs::PreviewLease =
        serde_json::from_value(report["originalLease"].clone()).unwrap();
    let path = jobs::find_preview_dir(report["previewId"].as_str().unwrap())
        .unwrap()
        .unwrap();
    snapshot.unaccepted(&conn, &requested);
    assert!(job_control::preview_retention::pending());
    assert!(!original.retained_by_job);
    let retained = jobs::read_preview_lease(&path).unwrap().unwrap();
    assert!(
        retained.retained_by_job,
        "the real child retained its input before exit73"
    );
    assert_eq!(retained.file_hashes, original.file_hashes);
    let recovery = recovery_file();
    let raw: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&recovery).unwrap()).unwrap();
    assert_eq!(raw["jobId"], requested.job_id);
    assert_eq!(raw["entries"].as_array().unwrap().len(), 1);
    jobs::renew_preview_lease(&path, 1500).unwrap();
    jobs::adopt_preview_for_batch(&path, "newer-association-crash-batch").unwrap();
    let renewed = jobs::read_preview_lease(&path).unwrap().unwrap();
    assert_eq!(jobs::cleanup_unreferenced_preview_dirs().unwrap(), 0);
    assert!(!recovery.exists());
    assert!(!job_control::preview_retention::pending());
    let repaired = jobs::read_preview_lease(&path).unwrap().unwrap();
    assert!(!repaired.retained_by_job);
    assert_eq!(repaired.expires_unix, renewed.expires_unix);
    assert_eq!(repaired.prepare_batch_id, renewed.prepare_batch_id);
    assert_eq!(repaired.file_hashes, original.file_hashes);
    snapshot.unaccepted(&conn, &requested);
    reset_runtime();
    let confirmed =
        engine_prepare_plan::load_durable_plan(&requested.plan_id, &requested.plan_hash).unwrap();
    let items = engine_commit::commit_items_for_plan(&confirmed, &requested, false).unwrap();
    let accepted = job_control::enqueue_confirmed_associations(requested.clone(), items).unwrap();
    assert_eq!(accepted.job_id, requested.job_id);
    assert_eq!(accepted.state, JobState::Queued);
    assert_eq!(text(IMPORT, REVISION), snapshot.old_text);
    assert_eq!(receipts(&conn), snapshot.receipts);
    reset_runtime();
    assert_eq!(text(IMPORT, REVISION), snapshot.old_text);
    assert_eq!(
        job_control::enqueue_confirmed_associations(requested, vec![])
            .unwrap()
            .job_id,
        accepted.job_id
    );
    println!("ASSOCIATION_BEFORE_COMMIT_CHILD={report}");
}

#[test]
#[ignore = "requires explicit hash-pinned V243 synthetic seed and registered source fixture"]
fn historical_seed_association_acceptance_rollback_io() {
    use preview_lease_io::{TestIoFault, TestIoPoint};
    use std::cell::RefCell;
    use std::io::ErrorKind;
    use std::rc::Rc;

    for point in [
        Some(TestIoPoint::Write),
        Some(TestIoPoint::Sync),
        Some(TestIoPoint::Rename),
        None,
    ] {
        let _home = seed_home();
        let _runtime = RuntimeReset;
        let conn = open_catalog().unwrap();
        let confirmed = confirmed_plan();
        let snapshot = HistoricalSnapshot::capture(&conn);
        let requested = requested_job(
            &confirmed,
            &format!("synthetic-association-rollback-{point:?}"),
        );
        let items = engine_commit::commit_items_for_plan(&confirmed, &requested, false).unwrap();
        let path = items[0].prepared.as_ref().unwrap().path.clone();
        let original = jobs::read_preview_lease(&path).unwrap().unwrap();
        let fault: Rc<RefCell<Option<TestIoFault>>> = Rc::new(RefCell::new(None));
        let hook_fault = Rc::clone(&fault);
        let expected = original.clone();
        crate::session_migration::preview_lifecycle_tests::test_before_enqueue_commit(move || {
            if let Some(point) = point {
                *hook_fault.borrow_mut() =
                    Some(TestIoFault::arm(point, ErrorKind::StorageFull, &expected));
            } else {
                job_control::preview_retention::fail_receipt_once("remove");
            }
        });
        let mode: String = conn
            .query_row("PRAGMA journal_mode=DELETE", [], |row| row.get(0))
            .unwrap();
        assert_eq!(mode, "delete");
        let reader = conn.unchecked_transaction().unwrap();
        reader
            .query_row("SELECT count(*) FROM jobs", [], |row| row.get::<_, u64>(0))
            .unwrap();
        let result = job_control::enqueue_confirmed_associations(requested.clone(), items);
        drop(reader);
        if let Some(guard) = fault.borrow_mut().take() {
            guard.assert_consumed();
        }
        let error = result.unwrap_err();
        assert!(
            error.message.contains("locked"),
            "actual COMMIT failure: {error:?}"
        );
        assert!(
            error.message.contains("retention rollback deferred"),
            "{error:?}"
        );
        assert!(
            error.message.contains(if point.is_some() {
                "bounded synthetic lease I/O fault"
            } else {
                "synthetic recovery remove failure"
            }),
            "{error:?}"
        );
        assert_eq!(error.code, ErrorCode::SourceBusy);
        snapshot.unaccepted(&conn, &requested);
        assert!(job_control::preview_retention::pending());
        let recovery = recovery_file();
        let retained = jobs::read_preview_lease(&path).unwrap().unwrap();
        assert_eq!(retained.retained_by_job, point.is_some());
        assert_eq!(retained.file_hashes, original.file_hashes);
        assert_eq!(retained.expires_unix, original.expires_unix);
        assert_eq!(jobs::cleanup_unreferenced_preview_dirs().unwrap(), 0);
        assert!(!recovery.exists());
        assert!(!job_control::preview_retention::pending());
        let repaired = jobs::read_preview_lease(&path).unwrap().unwrap();
        assert!(!repaired.retained_by_job);
        assert_eq!(repaired.file_hashes, original.file_hashes);
        let items = engine_commit::commit_items_for_plan(&confirmed, &requested, false).unwrap();
        let accepted =
            job_control::enqueue_confirmed_associations(requested.clone(), items).unwrap();
        assert_eq!(accepted.job_id, requested.job_id);
        assert_eq!(receipts(&conn), snapshot.receipts);
        assert_eq!(text(IMPORT, REVISION), snapshot.old_text);
        println!(
            "ASSOCIATION_ROLLBACK_IO={}",
            json!({"point": format!("{point:?}"),
            "error": error.code, "actualSqlCommitFailed": true, "sameRequestAccepted": true})
        );
    }
}
