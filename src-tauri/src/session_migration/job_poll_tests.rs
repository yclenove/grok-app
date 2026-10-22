//! Real SQLite read/lock regressions against an isolated synthetic catalog.
use super::{catalog, engine, errors::ErrorCode, jobs, test_home::IsolatedAppHome};
use rusqlite::Connection;

fn stored_job() -> jobs::JobRecord {
    let job = jobs::JobRecord {
        job_id: "synthetic-job".into(),
        request_id: "synthetic-request".into(),
        plan_id: "synthetic-plan".into(),
        plan_hash: "synthetic-plan-hash".into(),
        state: jobs::JobState::Running,
        state_version: 3,
        items: vec![jobs::JobItem {
            candidate_id: "synthetic-candidate".into(),
            title: Some("Synthetic title".into()),
            phase: "queued".into(),
            import_id: None,
            revision_id: None,
            outcome: None,
            error: None,
            mapping_project_id: None,
        }],
        created_at: "2026-10-04T00:00:00Z".into(),
        item_count: 1,
        next_item_cursor: None,
        progress: None,
    };
    jobs::save_job(&catalog::open_catalog().unwrap(), &job).unwrap();
    job
}

fn bytes() -> Vec<u8> {
    std::fs::read(catalog::catalog_path()).unwrap()
}

#[test]
fn polling_a_missing_catalog_does_not_create_import_storage() {
    let home = IsolatedAppHome::acquire("job-poll-missing");
    // The normal fixture creates imports directories. An uninitialized child
    // home stays inside its owned root and is restored by the same RAII guard.
    std::env::set_var("GROK_APP_HOME", home.path.join("uninitialized"));
    assert!(!catalog::imports_dir().exists());
    let error = engine::job_get("synthetic-missing").unwrap_err();
    assert_eq!(error.code, ErrorCode::SourceNotFound);
    assert!(!catalog::imports_dir().exists());
}

#[test]
fn polling_does_not_repair_unrelated_optional_tables() {
    let _home = IsolatedAppHome::acquire("job-poll-readonly");
    let job = stored_job();
    let conn = catalog::open_catalog().unwrap();
    conn.execute_batch("DROP TABLE drafts;").unwrap();
    let before = bytes();
    let page = engine::job_get(&job.job_id).unwrap();
    assert_eq!(page.state_version, 3);
    assert_eq!(page.items[0].title.as_deref(), Some("Synthetic title"));
    assert_eq!(page.item_count, 1);
    assert_eq!(
        before,
        bytes(),
        "progress reads must not initialize unrelated tables"
    );
    let draft_tables: u32 = conn
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE name='drafts'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(draft_tables, 0);
}

#[test]
fn polling_works_during_an_uncommitted_writer_without_schema_writes() {
    let _home = IsolatedAppHome::acquire("job-poll-reserved");
    let job = stored_job();
    let conn = catalog::open_catalog().unwrap();
    conn.execute_batch("DROP TABLE drafts; BEGIN IMMEDIATE;")
        .unwrap();
    conn.execute(
        "UPDATE jobs SET state_version=4 WHERE job_id=?1",
        [&job.job_id],
    )
    .unwrap();
    let before = bytes();
    let result = engine::job_get(&job.job_id);
    conn.execute_batch("ROLLBACK;").unwrap();
    let page = result.expect("polling is a reader, not a competing schema writer");
    assert_eq!(
        page.state_version, 3,
        "uncommitted data must stay invisible"
    );
    assert_eq!(before, bytes());
}

#[test]
fn polling_reports_exclusive_contention_as_retryable_without_mutation() {
    let _home = IsolatedAppHome::acquire("job-poll-exclusive");
    let job = stored_job();
    let blocker = Connection::open(catalog::catalog_path()).unwrap();
    blocker.execute_batch("BEGIN EXCLUSIVE;").unwrap();
    let before = bytes();
    let result = engine::job_get(&job.job_id);
    blocker.execute_batch("ROLLBACK;").unwrap();
    let error = result.unwrap_err();
    assert_eq!(error.code, ErrorCode::SourceBusy, "{error}");
    assert_eq!(error.retry, "retry");
    assert_eq!(before, bytes());
    assert_eq!(engine::job_get(&job.job_id).unwrap().state_version, 3);
}

#[test]
fn polling_distinguishes_a_missing_job_from_storage_corruption() {
    let _home = IsolatedAppHome::acquire("job-poll-errors");
    let job = stored_job();
    let missing = engine::job_get("synthetic-missing").unwrap_err();
    assert_eq!(missing.code, ErrorCode::SourceNotFound);
    let conn = catalog::open_catalog().unwrap();
    conn.execute(
        "UPDATE jobs SET json='not json' WHERE job_id=?1",
        [&job.job_id],
    )
    .unwrap();
    let before = bytes();
    let damaged = engine::job_get(&job.job_id).unwrap_err();
    assert_eq!(damaged.code, ErrorCode::SnapshotUnavailable, "{damaged}");
    assert_eq!(damaged.retry, "none");
    assert_eq!(before, bytes());
}

#[test]
fn polling_legacy_titles_and_progress_keeps_committed_writer_visibility() {
    let _home = IsolatedAppHome::acquire("job-poll-legacy-writer");
    let mut job = stored_job();
    job.items[0].title = None;
    let conn = catalog::open_catalog().unwrap();
    jobs::save_job(&conn, &job).unwrap();
    conn.execute(
        "INSERT INTO job_items(job_id,candidate_id,phase,result_json) VALUES (?1,?2,'queued',?3)",
        rusqlite::params![
            job.job_id,
            job.items[0].candidate_id,
            r#"{"adapter":{"graph":{"title":"Committed legacy title"}}}"#
        ],
    )
    .unwrap();
    conn.execute_batch("DROP TABLE drafts; BEGIN IMMEDIATE;")
        .unwrap();
    let before = bytes();
    job.state_version = 4;
    job.items[0].phase = "committed".into();
    jobs::save_job(&conn, &job).unwrap();
    conn.execute(
        "UPDATE job_items SET result_json=?2 WHERE job_id=?1",
        rusqlite::params![
            job.job_id,
            r#"{"adapter":{"graph":{"title":"Next legacy title"}}}"#
        ],
    )
    .unwrap();
    let page = engine::job_get(&job.job_id).unwrap();
    assert_eq!(page.state_version, 3);
    assert_eq!(page.items[0].phase, "queued");
    assert_eq!(page.progress.as_ref().unwrap().done, 0);
    assert_eq!(
        page.items[0].title.as_deref(),
        Some("Committed legacy title")
    );
    assert_eq!(before, bytes(), "polling must not commit the writer's data");

    conn.execute_batch("COMMIT;").unwrap();
    let page = engine::job_get(&job.job_id).unwrap();
    assert_eq!(page.state_version, 4);
    assert_eq!(page.items[0].phase, "committed");
    assert_eq!(page.progress.as_ref().unwrap().done, 1);
    assert_eq!(page.items[0].title.as_deref(), Some("Next legacy title"));
}
