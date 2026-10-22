use super::*;
use crate::session_migration::{job_progress, jobs};

#[test]
fn batch_cancel_writes_header_once_and_recovers_after_restart() {
    let _home = Home::new();
    let queued = enqueue(
        job(),
        (0..61)
            .map(|i| input_named(&format!("c{i}"), "batch"))
            .collect(),
    )
    .unwrap();
    let cursor = crate::session_migration::engine::job_get(&queued.job_id)
        .unwrap()
        .next_item_cursor
        .unwrap();
    let cancelling = cancel(&queued.job_id).unwrap();
    let conn = open_catalog().unwrap();
    conn.execute_batch("CREATE TABLE header_writes(n INTEGER); INSERT INTO header_writes VALUES(0);
        CREATE TRIGGER count_header AFTER UPDATE ON jobs BEGIN UPDATE header_writes SET n=n+1; END;").unwrap();
    let cancelled = job_progress::cancel_unstarted_queued(&conn, &queued.job_id).unwrap();
    assert_eq!(cancelled.len(), 61);
    let current = get_job(&conn, &queued.job_id).unwrap().unwrap();
    assert_eq!(current.state, JobState::Cancelling);
    assert_eq!(current.state_version, cancelling.state_version + 1);
    assert!(current
        .items
        .iter()
        .all(|item| item.phase == "cancelled" && item.outcome.as_deref() == Some("cancelled")));
    let writes: u32 = conn
        .query_row("SELECT n FROM header_writes", [], |r| r.get(0))
        .unwrap();
    assert_eq!(writes, 1);
    assert!(job_progress::cancel_unstarted_queued(&conn, &queued.job_id)
        .unwrap()
        .is_empty());
    assert_eq!(
        get_job(&conn, &queued.job_id)
            .unwrap()
            .unwrap()
            .state_version,
        current.state_version
    );
    let page =
        crate::session_migration::engine::job_items_page(&queued.job_id, Some(&cursor), None)
            .unwrap();
    assert_eq!(page.items.len(), 11);
    assert!(page.items.iter().all(|item| item.phase == "cancelled"));
    assert_eq!(page.state_version, current.state_version);
    assert_eq!(run_pending().unwrap(), 1);
    assert_eq!(
        get_job(&conn, &queued.job_id).unwrap().unwrap().state,
        JobState::Cancelled
    );
    assert!(list_entries(&conn).unwrap().is_empty());
    assert_eq!(run_pending().unwrap(), 0);
}

#[test]
fn batch_cancel_preserves_raw_receipts_and_repairs_existing_projections() {
    let _home = Home::new();
    let inputs = ["untouched", "missing", "started"]
        .map(|id| input_named(id, id))
        .to_vec();
    let missing_path = inputs[1].prepared.as_ref().unwrap().path.clone();
    let queued = enqueue(job(), inputs).unwrap();
    cancel(&queued.job_id).unwrap();
    let conn = open_catalog().unwrap();
    conn.execute(
        "UPDATE job_items SET phase='running' WHERE job_id=?1 AND candidate_id='untouched'",
        [&queued.job_id],
    )
    .unwrap();
    conn.execute(
        "DELETE FROM job_items WHERE job_id=?1 AND candidate_id='missing'",
        [&queued.job_id],
    )
    .unwrap();
    conn.execute("INSERT INTO receipts(receipt_id,job_id,candidate_id,phase,json,created_at) VALUES('damaged',?1,'started','prepared','{bad','t')", [&queued.job_id]).unwrap();
    let cancelled = job_progress::cancel_unstarted_queued(&conn, &queued.job_id).unwrap();
    assert_eq!(cancelled.len(), 2);
    assert!(!cancelled.contains("started"));
    let current = get_job(&conn, &queued.job_id).unwrap().unwrap();
    assert_eq!(current.items[2].phase, "queued");
    let phase: String = conn
        .query_row(
            "SELECT phase FROM job_items WHERE job_id=?1 AND candidate_id='untouched'",
            [&queued.job_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(phase, "cancelled");
    let missing: u32 = conn
        .query_row(
            "SELECT COUNT(*) FROM job_items WHERE job_id=?1 AND candidate_id='missing'",
            [&queued.job_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(missing, 0);
    assert_eq!(run_pending().unwrap(), 1);
    let stopped = get_job(&conn, &queued.job_id).unwrap().unwrap();
    assert_eq!(stopped.items[2].phase, "failed");
    assert!(stopped.items[2].error.is_some());
    let raw: String = conn
        .query_row(
            "SELECT json FROM receipts WHERE receipt_id='damaged'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(raw, "{bad");
    assert!(list_entries(&conn).unwrap().is_empty());
    assert!(
        missing_path.is_dir(),
        "ambiguous cancelled input must not be deleted"
    );
    assert!(jobs::cleanup_unreferenced_preview_dirs()
        .unwrap_err()
        .message
        .contains("deferred"));
}

#[test]
fn batch_cancel_rolls_back_all_rows_when_header_write_fails() {
    let _home = Home::new();
    let queued = enqueue(
        job(),
        vec![input_named("one", "one"), input_named("two", "two")],
    )
    .unwrap();
    cancel(&queued.job_id).unwrap();
    let conn = open_catalog().unwrap();
    let before: String = conn
        .query_row(
            "SELECT json FROM jobs WHERE job_id=?1",
            [&queued.job_id],
            |r| r.get(0),
        )
        .unwrap();
    conn.execute_batch("CREATE TEMP TRIGGER fail_batch BEFORE UPDATE ON jobs BEGIN SELECT RAISE(ABORT,'injected batch failure'); END;").unwrap();
    assert!(job_progress::cancel_unstarted_queued(&conn, &queued.job_id).is_err());
    let after: String = conn
        .query_row(
            "SELECT json FROM jobs WHERE job_id=?1",
            [&queued.job_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(before, after);
    let queued_rows: u32 = conn
        .query_row(
            "SELECT COUNT(*) FROM job_items WHERE job_id=?1 AND phase='queued'",
            [&queued.job_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(queued_rows, 2);
}

#[test]
fn missing_durable_input_reports_failure_without_blocking_later_jobs() {
    let _home = Home::new();
    let broken_input = input_named("broken", "lost input");
    let broken_path = broken_input.prepared.as_ref().unwrap().path.clone();
    let healthy_input = input_named("healthy", "retained input");
    let healthy_path = healthy_input.prepared.as_ref().unwrap().path.clone();
    let damaged = enqueue(job(), vec![broken_input]).unwrap();
    let healthy = enqueue(job(), vec![healthy_input]).unwrap();
    let conn = open_catalog().unwrap();
    conn.execute("DELETE FROM job_items WHERE job_id=?1", [&damaged.job_id])
        .unwrap();
    assert_eq!(run_pending().unwrap(), 2);
    let failed = get_job(&conn, &damaged.job_id).unwrap().unwrap();
    assert_eq!(failed.state, JobState::CompletedWithIssues);
    assert_eq!(failed.items[0].phase, "failed");
    assert!(failed.items[0].error.is_some());
    let done = get_job(&conn, &healthy.job_id).unwrap().unwrap();
    assert_eq!(done.state, JobState::Completed);
    assert_eq!(list_entries(&conn).unwrap().len(), 1);
    let inputs: u32 = conn
        .query_row(
            "SELECT COUNT(*) FROM job_items WHERE job_id=?1",
            [&damaged.job_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(inputs, 0, "failure must not fabricate missing input");
    assert!(
        broken_path.is_dir(),
        "damaged ownership must remain deferred"
    );
    assert!(
        !healthy_path.exists(),
        "unrelated authenticated completion must still dispose its preview"
    );
    assert!(jobs::cleanup_unreferenced_preview_dirs()
        .unwrap_err()
        .message
        .contains("deferred"));
    let cancelled = enqueue(job(), vec![input_named("cancelled", "lost input")]).unwrap();
    conn.execute("DELETE FROM job_items WHERE job_id=?1", [&cancelled.job_id])
        .unwrap();
    cancel(&cancelled.job_id).unwrap();
    assert_eq!(run_pending().unwrap(), 1);
    assert_eq!(
        get_job(&conn, &cancelled.job_id).unwrap().unwrap().state,
        JobState::Cancelled
    );
}

#[test]
fn worker_decodes_and_updates_linear_number_of_items() {
    let _home = Home::new();
    let count = 24;
    let inputs = (0..count)
        .map(|i| input_named(&format!("c{i}"), "progress"))
        .collect();
    let queued = enqueue(job(), inputs).unwrap();
    let conn = open_catalog().unwrap();
    conn.execute_batch(
        "CREATE TABLE progress_update_count(n INTEGER NOT NULL);
         INSERT INTO progress_update_count VALUES(0);
         CREATE TRIGGER count_progress_updates AFTER UPDATE ON job_items
         BEGIN UPDATE progress_update_count SET n=n+1; END;",
    )
    .unwrap();
    jobs::take_job_item_deserializes();
    assert_eq!(run_pending().unwrap(), 1);
    let decoded = jobs::take_job_item_deserializes();
    let updates: u32 = conn
        .query_row("SELECT n FROM progress_update_count", [], |r| r.get(0))
        .unwrap();
    assert!(
        decoded <= count * 6,
        "decoded {decoded} items for {count} sessions"
    );
    assert!(
        updates <= count * 4,
        "updated {updates} rows for {count} sessions"
    );
    let done = get_job(&conn, &queued.job_id).unwrap().unwrap();
    assert_eq!(done.state, JobState::Completed);
    assert!(done.items.iter().all(|i| i.phase == "committed"));
}

#[test]
fn item_progress_preserves_cancellation_and_other_items() {
    let _home = Home::new();
    let queued = enqueue(
        job(),
        vec![input_named("one", "one"), input_named("two", "two")],
    )
    .unwrap();
    let cancelled = cancel(&queued.job_id).unwrap();
    let conn = open_catalog().unwrap();
    assert!(job_progress::cancelling(&conn, &queued.job_id).unwrap());
    let mut item = queued.items[0].clone();
    item.phase = "cancelled".into();
    item.outcome = Some("cancelled".into());
    job_progress::set_item(&conn, &queued.job_id, 0, &item).unwrap();
    let current = get_job(&conn, &queued.job_id).unwrap().unwrap();
    assert_eq!(current.state, JobState::Cancelling);
    assert_eq!(current.state_version, cancelled.state_version + 1);
    assert_eq!(current.items[0].phase, "cancelled");
    assert_eq!(current.items[1].phase, "queued");
    assert_eq!(current.items[1].candidate_id, "two");
    let phase: String = conn
        .query_row(
            "SELECT phase FROM job_items WHERE job_id=?1 AND candidate_id='one'",
            [&queued.job_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(phase, "cancelled");
}

#[test]
fn stale_item_slot_and_missing_durable_input_do_not_change_header() {
    let _home = Home::new();
    let queued = enqueue(
        job(),
        vec![input_named("one", "one"), input_named("two", "two")],
    )
    .unwrap();
    let conn = open_catalog().unwrap();
    let header = || {
        conn.query_row(
            "SELECT json FROM jobs WHERE job_id=?1",
            [&queued.job_id],
            |r| r.get::<_, String>(0),
        )
        .unwrap()
    };
    let before = header();
    assert!(job_progress::set_item(&conn, &queued.job_id, 1, &queued.items[0]).is_err());
    assert!(job_progress::set_item(&conn, &queued.job_id, 99, &queued.items[0]).is_err());
    assert_eq!(header(), before);
    conn.execute(
        "DELETE FROM job_items WHERE job_id=?1 AND candidate_id='one'",
        [&queued.job_id],
    )
    .unwrap();
    assert!(job_progress::set_item(&conn, &queued.job_id, 0, &queued.items[0]).is_err());
    assert_eq!(header(), before);
}

#[test]
fn progress_write_failure_rolls_back_phase_and_bad_state_is_an_error() {
    let _home = Home::new();
    let queued = enqueue(job(), vec![input("one")]).unwrap();
    let conn = open_catalog().unwrap();
    conn.execute_batch("CREATE TEMP TRIGGER fail_progress BEFORE UPDATE ON jobs BEGIN SELECT RAISE(ABORT,'injected progress failure'); END;").unwrap();
    let mut item = queued.items[0].clone();
    item.phase = "cancelled".into();
    assert!(job_progress::set_item(&conn, &queued.job_id, 0, &item).is_err());
    let phase: String = conn
        .query_row(
            "SELECT phase FROM job_items WHERE job_id=?1",
            [&queued.job_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(phase, "queued");
    conn.execute_batch("DROP TRIGGER fail_progress").unwrap();
    conn.execute(
        "UPDATE jobs SET state='unknown_future_state' WHERE job_id=?1",
        [&queued.job_id],
    )
    .unwrap();
    assert!(job_progress::cancelling(&conn, &queued.job_id).is_err());
    assert!(job_progress::cancelling(&conn, "missing-job").unwrap());
}

fn progress_storage_hash() -> String {
    crate::session_migration::hash::sha256_bytes(
        &std::fs::read(crate::session_migration::catalog::catalog_path()).unwrap(),
    )
}

fn assert_progress_storage_intact(conn: &rusqlite::Connection, expected_hash: &str) {
    assert!(conn.is_autocommit());
    let integrity: String = conn
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .unwrap();
    assert_eq!(integrity, "ok");
    assert_eq!(progress_storage_hash(), expected_hash);
}

fn cap_progress_storage(conn: &rusqlite::Connection) -> u32 {
    let pages: u32 = conn
        .query_row("PRAGMA page_count", [], |row| row.get(0))
        .unwrap();
    conn.pragma_update(None, "max_page_count", pages).unwrap();
    let limit: u32 = conn
        .query_row("PRAGMA max_page_count", [], |row| row.get(0))
        .unwrap();
    assert_eq!(limit, pages);
    pages
}

fn finish_progress_storage_retry(conn: &rusqlite::Connection, id: &str) {
    assert_eq!(run_pending().unwrap(), 1);
    assert_eq!(
        get_job(conn, id).unwrap().unwrap().state,
        JobState::Cancelled
    );
    assert!(list_entries(conn).unwrap().is_empty());
    assert_eq!(retry(id).unwrap().state, JobState::Queued);
    assert_eq!(run_pending().unwrap(), 1);
    let done = get_job(conn, id).unwrap().unwrap();
    assert_eq!(done.state, JobState::Completed);
    assert_eq!(done.items.len(), 2);
    assert!(done.items.iter().all(|item| item.phase == "committed"));
    assert_eq!(run_pending().unwrap(), 0);
    assert_eq!(retry(id).unwrap().state, JobState::Completed);
    assert_eq!(run_pending().unwrap(), 0);
    assert_eq!(list_entries(conn).unwrap().len(), 2);
    let phases: Vec<(String, u32)> = conn
        .prepare(
            "SELECT phase,count(*) FROM receipts WHERE job_id=?1 GROUP BY phase ORDER BY phase",
        )
        .unwrap()
        .query_map([id], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(
        phases,
        ["committed", "linked", "prepared", "published", "staged"]
            .map(|phase| (phase.to_owned(), 2))
    );
    let duplicates: u32 = conn
        .query_row(
            "SELECT count(*) FROM (SELECT candidate_id,phase FROM receipts
             WHERE job_id=?1 GROUP BY candidate_id,phase HAVING count(*)>1)",
            [id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(duplicates, 0);
}

#[test]
fn progress_storage_busy_preserves_state_and_allows_cancel_retry() {
    let _home = Home::new();
    let queued = enqueue(
        job(),
        vec![input_named("one", "one"), input_named("two", "two")],
    )
    .unwrap();
    let cancelling = cancel(&queued.job_id).unwrap();
    let conn = open_catalog().unwrap();
    conn.busy_timeout(std::time::Duration::ZERO).unwrap();
    let journal: String = conn
        .query_row("PRAGMA journal_mode", [], |row| row.get(0))
        .unwrap();
    assert_eq!(journal, "delete");
    let before = progress_storage_hash();
    let blocker =
        rusqlite::Connection::open(crate::session_migration::catalog::catalog_path()).unwrap();
    blocker.execute_batch("BEGIN EXCLUSIVE").unwrap();
    let errors = [
        job_progress::cancelling(&conn, &queued.job_id).unwrap_err(),
        job_progress::set_item(&conn, &queued.job_id, 0, &queued.items[0]).unwrap_err(),
        job_progress::cancel_unstarted_queued(&conn, &queued.job_id).unwrap_err(),
    ];
    blocker.execute_batch("ROLLBACK").unwrap();
    drop(blocker);
    assert_progress_storage_intact(&conn, &before);
    assert_eq!(
        get_job(&conn, &queued.job_id)
            .unwrap()
            .unwrap()
            .state_version,
        cancelling.state_version
    );
    drop(conn);
    let conn = open_catalog().unwrap();
    assert_progress_storage_intact(&conn, &before);
    assert!(job_progress::cancelling(&conn, &queued.job_id).unwrap());
    assert_eq!(
        job_progress::cancel_unstarted_queued(&conn, &queued.job_id)
            .unwrap()
            .len(),
        2
    );
    finish_progress_storage_retry(&conn, &queued.job_id);
    // Check classification after durability/recovery, so red also proves those invariants.
    for error in errors {
        assert_eq!(error.code, ErrorCode::SourceBusy, "{error}");
        assert_eq!(error.retry, "retry");
    }
}

#[test]
fn progress_storage_full_item_rolls_back_phase_and_header() {
    let _home = Home::new();
    let queued = enqueue(
        job(),
        vec![input_named("one", "one"), input_named("two", "two")],
    )
    .unwrap();
    let cancelling = cancel(&queued.job_id).unwrap();
    let conn = open_catalog().unwrap();
    let pages = cap_progress_storage(&conn);
    let before = progress_storage_hash();
    let mut item = queued.items[0].clone();
    item.phase = "failed".into();
    item.outcome = Some("failed".into());
    item.error = Some("synthetic-progress-error ".repeat(32 * 1024));
    let error = job_progress::set_item(&conn, &queued.job_id, 0, &item).unwrap_err();
    assert_progress_storage_intact(&conn, &before);
    let current = get_job(&conn, &queued.job_id).unwrap().unwrap();
    assert_eq!(current.state_version, cancelling.state_version);
    assert!(current
        .items
        .iter()
        .all(|item| item.phase == "queued" && item.error.is_none()));
    drop(conn);
    let conn = open_catalog().unwrap();
    assert_progress_storage_intact(&conn, &before);
    conn.pragma_update(None, "max_page_count", pages + 1024)
        .unwrap();
    job_progress::set_item(&conn, &queued.job_id, 0, &item).unwrap();
    let current = get_job(&conn, &queued.job_id).unwrap().unwrap();
    assert_eq!(current.state, JobState::Cancelling);
    assert_eq!(current.state_version, cancelling.state_version + 1);
    assert_eq!(current.items[0].error, item.error);
    assert_eq!(current.items[1].phase, "queued");
    finish_progress_storage_retry(&conn, &queued.job_id);
    assert_eq!(error.code, ErrorCode::DiskSpaceLow, "{error}");
    assert_eq!(error.retry, "retry");
}

#[test]
fn progress_storage_full_batch_rolls_back_every_cancelled_item() {
    let _home = Home::new();
    let queued = enqueue(
        job(),
        vec![input_named("one", "one"), input_named("two", "two")],
    )
    .unwrap();
    let cancelling = cancel(&queued.job_id).unwrap();
    let conn = open_catalog().unwrap();
    // A bounded fixture-only allocation yields real SQLITE_FULL; no host disk exhaustion.
    conn.execute_batch(
        "CREATE TABLE progress_pressure(payload BLOB);
        CREATE TRIGGER fill_progress AFTER UPDATE ON job_items WHEN NEW.phase='cancelled'
        BEGIN INSERT INTO progress_pressure VALUES(zeroblob(1048576)); END;",
    )
    .unwrap();
    let pages = cap_progress_storage(&conn);
    let before = progress_storage_hash();
    let error = job_progress::cancel_unstarted_queued(&conn, &queued.job_id).unwrap_err();
    assert_progress_storage_intact(&conn, &before);
    let pressure: u32 = conn
        .query_row("SELECT count(*) FROM progress_pressure", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(pressure, 0);
    let current = get_job(&conn, &queued.job_id).unwrap().unwrap();
    assert_eq!(current.state_version, cancelling.state_version);
    assert!(current.items.iter().all(|item| item.phase == "queued"));
    drop(conn);
    let conn = open_catalog().unwrap();
    assert_progress_storage_intact(&conn, &before);
    conn.pragma_update(None, "max_page_count", pages + 1024)
        .unwrap();
    conn.execute_batch("DROP TRIGGER fill_progress").unwrap();
    assert_eq!(
        job_progress::cancel_unstarted_queued(&conn, &queued.job_id)
            .unwrap()
            .len(),
        2
    );
    let cancelled_hash = progress_storage_hash();
    assert!(job_progress::cancel_unstarted_queued(&conn, &queued.job_id)
        .unwrap()
        .is_empty());
    assert_progress_storage_intact(&conn, &cancelled_hash);
    finish_progress_storage_retry(&conn, &queued.job_id);
    assert_eq!(error.code, ErrorCode::DiskSpaceLow, "{error}");
    assert_eq!(error.retry, "retry");
}

#[test]
fn progress_storage_read_only_is_not_promoted_to_transient_retry() {
    let _home = Home::new();
    let queued = enqueue(
        job(),
        vec![input_named("one", "one"), input_named("two", "two")],
    )
    .unwrap();
    cancel(&queued.job_id).unwrap();
    let conn = open_catalog().unwrap();
    let before = progress_storage_hash();
    conn.pragma_update(None, "query_only", true).unwrap();
    assert!(job_progress::cancelling(&conn, &queued.job_id).unwrap());
    let errors = [
        job_progress::set_item(&conn, &queued.job_id, 0, &queued.items[0]).unwrap_err(),
        job_progress::cancel_unstarted_queued(&conn, &queued.job_id).unwrap_err(),
    ];
    assert_progress_storage_intact(&conn, &before);
    conn.pragma_update(None, "query_only", false).unwrap();
    assert_progress_storage_intact(&conn, &before);
    finish_progress_storage_retry(&conn, &queued.job_id);
    for error in errors {
        assert_eq!(error.code, ErrorCode::SnapshotUnavailable, "{error}");
        assert_eq!(error.retry, "none");
    }
}
