//! Legacy compatibility and atomic split-display updates through real job APIs.
use super::*;
use crate::session_migration::{engine, job_input_association, job_progress, jobs::item_store};
use rusqlite::params;

fn embed_legacy(conn: &rusqlite::Connection, job: &JobRecord) {
    let tx = conn.unchecked_transaction().unwrap();
    tx.execute("DELETE FROM job_item_states WHERE job_id=?1", [&job.job_id])
        .unwrap();
    tx.execute(
        "UPDATE jobs SET json=?2 WHERE job_id=?1",
        params![job.job_id, serde_json::to_string(job).unwrap()],
    )
    .unwrap();
    tx.commit().unwrap();
}

#[test]
fn legacy_pages_work_without_the_new_table_and_do_not_rewrite_storage() {
    let _home = Home::new();
    let queued = enqueue(
        job(),
        (0..3)
            .map(|n| input_named(&format!("c{n}"), "legacy"))
            .collect(),
    )
    .unwrap();
    let conn = open_catalog().unwrap();
    embed_legacy(&conn, &queued);
    let mut stored = queued.clone();
    stored.items[0].phase = "tombstoned".into();
    stored.items[0].outcome = Some("tombstoned".into());
    stored.items[1].phase = "cancelled".into();
    stored.state_version += 1;
    save_job(&conn, &stored).unwrap();
    conn.execute_batch("DROP TABLE job_item_states").unwrap();
    let before = std::fs::read(super::super::catalog::catalog_path()).unwrap();
    let first = engine::job_items_page(&queued.job_id, None, Some(2)).unwrap();
    assert_eq!(first.item_count, 3);
    assert_eq!(first.state_version, stored.state_version);
    assert_eq!(
        first.progress.unwrap(),
        super::super::jobs::JobProgress {
            done: 2,
            retryable: 1
        }
    );
    assert_eq!(first.items[0].phase, "tombstoned");
    let second =
        engine::job_items_page(&queued.job_id, first.next_item_cursor.as_deref(), Some(2)).unwrap();
    assert_eq!(second.items[0].candidate_id, "c2");
    assert!(second.next_item_cursor.is_none());
    assert_eq!(
        std::fs::read(super::super::catalog::catalog_path()).unwrap(),
        before
    );
    assert_eq!(
        get_job(&conn, &queued.job_id).unwrap().unwrap().items.len(),
        3
    );
}

#[test]
fn legacy_and_split_jobs_cancel_restart_retry_publish_and_undo() {
    for legacy in [true, false] {
        let _home = Home::new();
        let queued = enqueue(
            job(),
            vec![input_named("one", "one"), input_named("two", "two")],
        )
        .unwrap();
        let conn = open_catalog().unwrap();
        if legacy {
            embed_legacy(&conn, &queued);
        }
        let cancelling = cancel(&queued.job_id).unwrap();
        drop(conn);
        assert_eq!(run_pending().unwrap(), 1);
        let conn = open_catalog().unwrap();
        let cancelled = get_job(&conn, &queued.job_id).unwrap().unwrap();
        assert_eq!(cancelled.state, JobState::Cancelled);
        assert!(cancelled.state_version > cancelling.state_version);
        assert!(cancelled.items.iter().all(|item| item.phase == "cancelled"));
        let retrying = retry(&queued.job_id).unwrap();
        assert!(retrying.state_version > cancelled.state_version);
        assert_eq!(run_pending().unwrap(), 1);
        let completed = get_job(&conn, &queued.job_id).unwrap().unwrap();
        assert_eq!(completed.state, JobState::Completed);
        assert!(completed.state_version > retrying.state_version);
        assert!(completed.items.iter().all(|item| item.phase == "committed"));
        let undone = engine::undo_import_job(&queued.job_id).unwrap();
        assert!(undone.state_version > completed.state_version);
        assert!(undone.items.iter().all(|item| item.phase == "undone"));
        assert_eq!(
            item_store::uses_rows(&conn, &queued.job_id).unwrap(),
            !legacy
        );
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM tombstones", [], |r| r
                .get::<_, u32>(0))
                .unwrap(),
            2
        );
    }
}

#[test]
fn split_job_page_header_counts_and_items_see_one_committed_snapshot() {
    let _home = Home::new();
    let queued = enqueue(
        job(),
        vec![input_named("one", "one"), input_named("two", "two")],
    )
    .unwrap();
    let conn = open_catalog().unwrap();
    let tx = conn.unchecked_transaction().unwrap();
    let mut next = queued.clone();
    next.state_version += 1;
    next.items[0].phase = "tombstoned".into();
    save_job(&tx, &next).unwrap();
    let page = engine::job_get(&queued.job_id).unwrap();
    assert_eq!(page.state_version, queued.state_version);
    assert_eq!(page.progress.unwrap().done, 0);
    assert!(page.items.iter().all(|item| item.phase == "queued"));
    tx.commit().unwrap();
    let page = engine::job_get(&queued.job_id).unwrap();
    assert_eq!(page.state_version, next.state_version);
    assert_eq!(page.progress.unwrap().done, 1);
    assert_eq!(page.items[0].phase, "tombstoned");
    assert_eq!(page.items[1].phase, "queued");
}

#[test]
fn split_display_rows_keep_association_record_budget_before_decoding_items() {
    let _home = Home::new();
    let mut queued = enqueue(job(), vec![input("bounded")]).unwrap();
    let conn = open_catalog().unwrap();
    queued.items[0].title = Some("x".repeat(16 * 1024 * 1024));
    save_job(&conn, &queued).unwrap();
    super::super::jobs::take_job_item_deserializes();
    let error = job_input_association::bounded_job_metadata(&conn, &queued.job_id).unwrap_err();
    assert_eq!(error.code, ErrorCode::ResourceLimit);
    assert_eq!(super::super::jobs::take_job_item_deserializes(), 0);
}

#[test]
fn split_display_budget_counts_candidate_bytes_before_decoding_items() {
    let _home = Home::new();
    let queued = enqueue(job(), vec![input("bounded-candidate")]).unwrap();
    let conn = open_catalog().unwrap();
    let omitted_candidate: usize = conn
        .query_row(
            "SELECT length(CAST(j.json AS BLOB))+length(CAST(s.json AS BLOB))+1
             FROM jobs j JOIN job_item_states s ON s.job_id=j.job_id WHERE j.job_id=?1",
            [&queued.job_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        item_store::require_budget(&conn, &queued.job_id, omitted_candidate)
            .unwrap_err()
            .code,
        ErrorCode::ResourceLimit
    );
    // SQLite character counts would miss both multibyte IDs and embedded NULs.
    for candidate in ["é".repeat(8193), "\0".repeat(16 * 1024 * 1024 + 1)] {
        let tx = conn.unchecked_transaction().unwrap();
        tx.execute("UPDATE job_item_states SET candidate_id=?1", [&candidate])
            .unwrap();
        super::super::jobs::take_job_item_deserializes();
        let changes = tx.total_changes();
        assert_eq!(
            job_input_association::bounded_job_metadata(&tx, &queued.job_id)
                .unwrap_err()
                .code,
            ErrorCode::ResourceLimit
        );
        assert_eq!(super::super::jobs::take_job_item_deserializes(), 0);
        assert_eq!(tx.total_changes(), changes);
        // An invalid UTF8 body proves the candidate check precedes body reads.
        tx.execute("UPDATE job_item_states SET json=CAST(x'ff' AS TEXT)", [])
            .unwrap();
        assert_eq!(
            item_store::page::<JobItem>(&tx, &queued.job_id, 0, 1)
                .unwrap_err()
                .message,
            "invalid durable job item state"
        );
        assert_eq!(
            item_store::item_json(&tx, &queued.job_id, 0)
                .unwrap_err()
                .message,
            "invalid durable job item state"
        );
        tx.rollback().unwrap();
    }
}

#[test]
fn a_downgrade_header_cannot_hide_surviving_display_rows() {
    let _home = Home::new();
    let queued = enqueue(job(), vec![input("downgrade")]).unwrap();
    let conn = open_catalog().unwrap();
    for embedded in [Vec::new(), queued.items.clone()] {
        let tx = conn.unchecked_transaction().unwrap();
        let mut downgraded = queued.clone();
        downgraded.items = embedded;
        tx.execute(
            "UPDATE jobs SET json=?2 WHERE job_id=?1",
            params![queued.job_id, serde_json::to_string(&downgraded).unwrap()],
        )
        .unwrap();
        let changes = tx.total_changes();
        for error in [
            item_store::uses_rows(&tx, &queued.job_id).unwrap_err(),
            item_store::page::<JobItem>(&tx, &queued.job_id, 0, 1).unwrap_err(),
            item_store::item_json(&tx, &queued.job_id, 0).unwrap_err(),
            get_job(&tx, &queued.job_id).unwrap_err(),
            job_input_association::ordinary_job_metadata(&tx, &queued.job_id).unwrap_err(),
        ] {
            assert_eq!(error.code, ErrorCode::HistoryAccessUnavailable, "{error}");
        }
        assert_eq!(tx.total_changes(), changes);
        tx.rollback().unwrap();
    }
    let display = item_store::item_json(&conn, &queued.job_id, 0).unwrap();
    embed_legacy(&conn, &queued);
    let _fence = crate::session_migration::publication_write_fence::install(&conn);
    job_input_association::ordinary_job_metadata(&conn, &queued.job_id).unwrap();
    conn.execute(
        "INSERT INTO job_item_states VALUES(?1,0,?2,?3)",
        params![queued.job_id, queued.items[0].candidate_id, display],
    )
    .unwrap();
    // Even the decoded-header cache must recheck orphan rows on a cache hit.
    assert_eq!(
        job_input_association::ordinary_job_metadata(&conn, &queued.job_id)
            .unwrap_err()
            .code,
        ErrorCode::HistoryAccessUnavailable
    );
    assert_eq!(
        engine::job_get(&queued.job_id).unwrap_err().code,
        ErrorCode::HistoryAccessUnavailable
    );
}

#[test]
fn split_display_slot_holes_and_mismatched_candidate_columns_are_not_readable() {
    let _home = Home::new();
    let queued = enqueue(
        job(),
        vec![input_named("one", "one"), input_named("two", "two")],
    )
    .unwrap();
    let conn = open_catalog().unwrap();
    let crypto =
        crate::session_migration::publication_crypto::JobCrypto::prepare(&conn, &queued.job_id)
            .unwrap();
    for sql in [
        "DELETE FROM job_item_states WHERE item_index=0",
        "UPDATE job_item_states SET item_index=4 WHERE item_index=1",
        "UPDATE job_item_states SET candidate_id='foreign' WHERE item_index=0",
        "UPDATE job_item_states SET json=CAST(json AS BLOB)",
        "UPDATE jobs SET json=json_set(json,'$.itemStorage','rows-v2')",
        "UPDATE jobs SET json=json_set(json,'$.items',json('[{}]'))",
        "DROP TABLE job_item_states",
    ] {
        let tx = conn.unchecked_transaction().unwrap();
        tx.execute(sql, []).unwrap();
        assert!(
            super::super::jobs::get_job_with_crypto(&tx, &queued.job_id, &crypto).is_err(),
            "{sql}"
        );
        tx.rollback().unwrap();
        assert_eq!(engine::job_get(&queued.job_id).unwrap().item_count, 2);
    }
}

#[test]
fn item_state_version_overflow_rolls_back_both_item_projections() {
    for legacy in [true, false] {
        let _home = Home::new();
        let mut queued = enqueue(job(), vec![input("overflow")]).unwrap();
        let conn = open_catalog().unwrap();
        if legacy {
            embed_legacy(&conn, &queued);
        }
        queued.state_version = u32::MAX;
        save_job(&conn, &queued).unwrap();
        let before = std::fs::read(super::super::catalog::catalog_path()).unwrap();
        let mut item = queued.items[0].clone();
        item.phase = "cancelled".into();
        assert!(job_progress::set_item(&conn, &queued.job_id, 0, &item).is_err());
        assert_eq!(
            std::fs::read(super::super::catalog::catalog_path()).unwrap(),
            before
        );
        let stored = get_job(&conn, &queued.job_id).unwrap().unwrap();
        assert_eq!(stored.state_version, u32::MAX);
        assert_eq!(stored.items[0].phase, "queued");
    }
}

#[test]
fn saving_a_split_job_without_a_caller_transaction_rolls_back_a_late_item_failure() {
    let _home = Home::new();
    let mut queued = enqueue(
        job(),
        vec![input_named("one", "one"), input_named("two", "two")],
    )
    .unwrap();
    let conn = open_catalog().unwrap();
    conn.execute_batch(
        "CREATE TEMP TRIGGER fail_second_state BEFORE UPDATE ON job_item_states
         WHEN NEW.item_index=1 BEGIN SELECT RAISE(ABORT,'synthetic second display failure'); END;",
    )
    .unwrap();
    let before = std::fs::read(super::super::catalog::catalog_path()).unwrap();
    queued.state_version += 1;
    queued.state = JobState::Running;
    queued.items[0].phase = "cancelled".into();
    assert!(save_job(&conn, &queued).is_err());
    assert!(conn.is_autocommit());
    assert_eq!(
        std::fs::read(super::super::catalog::catalog_path()).unwrap(),
        before
    );
    let stored = get_job(&conn, &queued.job_id).unwrap().unwrap();
    assert_eq!(stored.state, JobState::Queued);
    assert_eq!(stored.state_version, queued.state_version - 1);
    assert!(stored.items.iter().all(|item| item.phase == "queued"));
}
