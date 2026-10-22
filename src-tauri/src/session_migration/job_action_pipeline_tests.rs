//! Public controls over real source -> preview -> plan -> accepted job writers.
//! No manufactured positive seals, jobs, inputs or terminal result rows.
use super::*;
use crate::session_migration::{errors::ImportError, job_actions as controls};
use rusqlite::{params, Connection};

#[path = "job_action_fault_tests.rs"]
mod faults;

#[path = "job_action_undo_tests.rs"]
mod undo;

fn request(job: &jobs::JobRecord) -> controls::JobActionRequest {
    controls::JobActionRequest {
        expected_state_version: job.state_version,
        request_id: uuid::Uuid::new_v4().to_string(),
    }
}

fn queued(home: &IsolatedAppHome) -> jobs::JobRecord {
    let planned = plan(home, false);
    engine::commit(
        &planned.plan_id,
        &planned.plan_hash,
        "public-control-source",
    )
    .unwrap()
}

fn key(request: &controls::JobActionRequest) -> String {
    format!("job_action:v1:{}", request.request_id)
}

#[derive(Debug, PartialEq)]
enum Cell {
    Null,
    Integer(i64),
    Real(u64),
    Text(Vec<u8>),
    Blob(Vec<u8>),
}

type Rows = Vec<(String, Vec<Vec<Cell>>)>;

// Compare all persisted rows/types/bytes even for intentionally invalid UTF8.
// total_changes on a different connection cannot prove public-call rollback.
fn snapshot(conn: &Connection) -> Rows {
    let tables = conn
        .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    tables
        .into_iter()
        .map(|table| {
            let quoted = table.replace('"', "\"\"");
            let mut statement = conn
                .prepare(&format!("SELECT rowid,* FROM \"{quoted}\" ORDER BY rowid"))
                .unwrap();
            let columns = statement.column_count();
            let rows = statement
                .query_map([], |row| {
                    (0..columns)
                        .map(|column| {
                            use rusqlite::types::ValueRef;
                            Ok(match row.get_ref(column)? {
                                ValueRef::Null => Cell::Null,
                                ValueRef::Integer(value) => Cell::Integer(value),
                                ValueRef::Real(value) => Cell::Real(value.to_bits()),
                                ValueRef::Text(value) => Cell::Text(value.to_vec()),
                                ValueRef::Blob(value) => Cell::Blob(value.to_vec()),
                            })
                        })
                        .collect::<rusqlite::Result<Vec<_>>>()
                })
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            (table, rows)
        })
        .collect()
}

fn refuse_unchanged(
    conn: &Connection,
    code: ErrorCode,
    operation: impl FnOnce() -> Result<jobs::JobRecord, ImportError>,
) {
    let before = snapshot(conn);
    let error = operation().unwrap_err();
    assert_eq!(error.code, code, "{error:?}");
    assert_eq!(snapshot(conn), before);
}

fn unchanged_replay(
    conn: &Connection,
    operation: impl FnOnce() -> Result<jobs::JobRecord, ImportError>,
) -> jobs::JobRecord {
    let before = snapshot(conn);
    let result = operation().unwrap();
    assert_eq!(snapshot(conn), before);
    result
}

#[test]
fn public_cancel_and_retry_replay_survive_worker_progress_and_offline_restart() {
    let home = IsolatedAppHome::acquire("public-control-replay");
    let _reset = ResetRuntime;
    let queued = queued(&home);
    let conn = catalog::open_catalog().unwrap();
    // A cancel request must not rewrite even an unchanged item phase/input.
    conn.execute_batch(
        "CREATE TRIGGER no_cancel_item_rewrite BEFORE UPDATE ON job_items
         BEGIN SELECT RAISE(ABORT,'cancel rewrote an item'); END;",
    )
    .unwrap();
    let cancel = request(&queued);
    let cancelling = controls::cancel(&queued.job_id, cancel.clone()).unwrap();
    assert_eq!(cancelling.state, jobs::JobState::Cancelling);
    assert_eq!(cancelling.state_version, queued.state_version + 1);
    assert_eq!(cancelling.items[0].phase, "queued");
    let replay = unchanged_replay(&conn, || controls::cancel(&queued.job_id, cancel.clone()));
    assert_eq!(replay.state_version, cancelling.state_version);
    conn.execute_batch("DROP TRIGGER no_cancel_item_rewrite")
        .unwrap();
    assert_eq!(job_control::run_pending().unwrap(), 1);
    let cancelled = engine::job_get(&queued.job_id).unwrap();
    assert_eq!(cancelled.state, jobs::JobState::Cancelled);
    assert_eq!(cancelled.items[0].phase, "cancelled");
    let replay = unchanged_replay(&conn, || controls::cancel(&queued.job_id, cancel.clone()));
    assert_eq!(replay.state_version, cancelled.state_version);
    engine::reset_runtime();
    std::fs::rename(home.path.join("source"), home.path.join("offline-source")).unwrap();
    let retry = request(&cancelled);
    let resumed = controls::retry(&queued.job_id, retry.clone()).unwrap();
    assert_eq!(resumed.state, jobs::JobState::Queued);
    let replay = unchanged_replay(&conn, || controls::retry(&queued.job_id, retry.clone()));
    assert_eq!(replay.state_version, resumed.state_version);
    assert_eq!(job_control::run_pending().unwrap(), 1);
    let completed = engine::job_get(&queued.job_id).unwrap();
    assert_eq!(completed.state, jobs::JobState::Completed, "{completed:?}");
    assert_eq!(completed.items[0].phase, "committed");
    jobs::load_published_auth(
        completed.items[0].import_id.as_deref().unwrap(),
        completed.items[0].revision_id.as_deref(),
        None,
    )
    .unwrap();
    engine::reset_runtime();
    for _ in 0..2 {
        let replay = unchanged_replay(&conn, || controls::retry(&queued.job_id, retry.clone()));
        assert_eq!(replay.state_version, completed.state_version);
        let replay = unchanged_replay(&conn, || controls::cancel(&queued.job_id, cancel.clone()));
        assert_eq!(replay.state, jobs::JobState::Completed);
        assert_eq!(replay.state_version, completed.state_version);
    }
    assert_eq!(job_control::run_pending().unwrap(), 0);
}

#[test]
fn public_stale_control_and_reused_request_identity_refuse_without_claiming_journal() {
    let home = IsolatedAppHome::acquire("public-control-stale");
    let _reset = ResetRuntime;
    let first = queued(&home);
    let conn = catalog::open_catalog().unwrap();
    let stale = request(&first);
    assert_eq!(job_control::run_pending().unwrap(), 1);
    let completed = engine::job_get(&first.job_id).unwrap();
    assert!(completed.state_version > stale.expected_state_version);
    for retry in [false, true] {
        refuse_unchanged(&conn, ErrorCode::JobStateChanged, || {
            if retry {
                controls::retry(&first.job_id, stale.clone())
            } else {
                controls::cancel(&first.job_id, stale.clone())
            }
        });
    }
    assert!(!conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM meta WHERE key=?1)",
            [key(&stale)],
            |row| row.get::<_, bool>(0)
        )
        .unwrap());
    let action = request(&completed);
    let updated = controls::cancel(&first.job_id, action.clone()).unwrap();
    assert_eq!(updated.state, jobs::JobState::Completed);
    refuse_unchanged(&conn, ErrorCode::JobActionConflict, || {
        controls::retry(&first.job_id, action.clone())
    });
    let mut changed = action.clone();
    changed.expected_state_version = updated.state_version;
    refuse_unchanged(&conn, ErrorCode::JobActionConflict, || {
        controls::cancel(&first.job_id, changed)
    });
    let next = plan(&home, false);
    let other = engine::commit(&next.plan_id, &next.plan_hash, "public-control-other").unwrap();
    refuse_unchanged(&conn, ErrorCode::JobActionConflict, || {
        controls::cancel(&other.job_id, action.clone())
    });
    // A stale rejection did not consume the request ID. Explicitly reviewing
    // and issuing it at the current version is still a fresh intent.
    let mut reviewed = stale;
    reviewed.expected_state_version = updated.state_version;
    let reviewed = controls::cancel(&first.job_id, reviewed).unwrap();
    assert_eq!(reviewed.state_version, updated.state_version + 1);
}

#[test]
fn public_retry_keeps_committed_member_and_its_receipt_bytes_through_lost_reply() {
    let home = IsolatedAppHome::acquire("public-retry-partial");
    let _reset = ResetRuntime;
    let planned = super::mixed_undo::mixed_plan(&home);
    let queued = engine::commit(&planned.plan_id, &planned.plan_hash, "public-partial").unwrap();
    let conn = catalog::open_catalog().unwrap();
    conn.execute_batch(&format!(
        "CREATE TRIGGER fail_control_member AFTER INSERT ON receipts
         WHEN NEW.job_id='{}' AND NEW.candidate_id='{}' AND NEW.phase='committed'
         BEGIN SELECT RAISE(ABORT,'synthetic partial failure'); END;",
        queued.job_id, queued.items[1].candidate_id,
    ))
    .unwrap();
    assert_eq!(job_control::run_pending().unwrap(), 1);
    let failed = engine::job_get(&queued.job_id).unwrap();
    assert_eq!(failed.state, jobs::JobState::CompletedWithIssues);
    assert_eq!(failed.items[0].phase, "committed");
    assert_eq!(failed.items[1].phase, "failed");
    let receipts = || {
        conn.prepare(
            "SELECT json FROM receipts WHERE job_id=?1 AND candidate_id=?2 ORDER BY receipt_id",
        )
        .unwrap()
        .query_map(
            params![queued.job_id, queued.items[0].candidate_id],
            |row| row.get::<_, String>(0),
        )
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
    };
    let original_receipts = receipts();
    let original_item = serde_json::to_value(&failed.items[0]).unwrap();
    conn.execute_batch("DROP TRIGGER fail_control_member")
        .unwrap();
    let action = request(&failed);
    let retry = controls::retry(&queued.job_id, action.clone()).unwrap();
    assert_eq!(
        serde_json::to_value(&retry.items[0]).unwrap(),
        original_item
    );
    assert_eq!(retry.items[1].phase, "queued");
    unchanged_replay(&conn, || controls::retry(&queued.job_id, action.clone()));
    engine::reset_runtime();
    std::fs::rename(home.path.join("source"), home.path.join("offline-source")).unwrap();
    assert_eq!(job_control::run_pending().unwrap(), 1);
    let completed = engine::job_get(&queued.job_id).unwrap();
    assert_eq!(completed.state, jobs::JobState::Completed, "{completed:?}");
    assert_eq!(
        serde_json::to_value(&completed.items[0]).unwrap(),
        original_item
    );
    assert_eq!(receipts(), original_receipts);
    unchanged_replay(&conn, || controls::retry(&queued.job_id, action.clone()));
}

#[test]
fn public_control_validates_signed_original_even_on_recorded_replay() {
    let home = IsolatedAppHome::acquire("public-control-auth");
    let _reset = ResetRuntime;
    let queued = queued(&home);
    let action = request(&queued);
    controls::cancel(&queued.job_id, action.clone()).unwrap();
    let conn = catalog::open_catalog().unwrap();
    let original: String = conn
        .query_row(
            "SELECT result_json FROM job_items WHERE job_id=?1",
            [&queued.job_id],
            |row| row.get(0),
        )
        .unwrap();
    conn.execute(
        "UPDATE job_items SET result_json='{}' WHERE job_id=?1",
        [&queued.job_id],
    )
    .unwrap();
    refuse_unchanged(&conn, ErrorCode::HistoryAccessUnavailable, || {
        controls::cancel(&queued.job_id, action.clone())
    });
    conn.execute(
        "UPDATE job_items SET result_json=?2 WHERE job_id=?1",
        params![queued.job_id, original],
    )
    .unwrap();
    unchanged_replay(&conn, || controls::cancel(&queued.job_id, action));
}

#[test]
fn public_legacy_controls_preserve_original_protocol_and_do_not_mint_acceptance() {
    let home = IsolatedAppHome::acquire("public-control-legacy");
    let _reset = ResetRuntime;
    let planned = plan(&home, false);
    let queued = engine::enqueue_legacy_plan(&planned, "public-control-legacy");
    assert_eq!(
        PublicationProtocol::for_job_id(&queued.job_id).unwrap(),
        PublicationProtocol::Legacy
    );
    let cancel = request(&queued);
    controls::cancel(&queued.job_id, cancel.clone()).unwrap();
    let conn = catalog::open_catalog().unwrap();
    unchanged_replay(&conn, || controls::cancel(&queued.job_id, cancel));
    assert_eq!(job_control::run_pending().unwrap(), 1);
    let cancelled = engine::job_get(&queued.job_id).unwrap();
    let retry = request(&cancelled);
    controls::retry(&queued.job_id, retry.clone()).unwrap();
    assert_eq!(job_control::run_pending().unwrap(), 1);
    let completed = engine::job_get(&queued.job_id).unwrap();
    assert_eq!(completed.state, jobs::JobState::Completed, "{completed:?}");
    unchanged_replay(&conn, || controls::retry(&queued.job_id, retry));
    assert_eq!(
        PublicationProtocol::for_job_id(&queued.job_id).unwrap(),
        PublicationProtocol::Legacy
    );
    assert!(!conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM meta WHERE key LIKE ?1)",
            [format!("publication_acceptance:v1:{}:%", queued.job_id)],
            |row| row.get::<_, bool>(0),
        )
        .unwrap());
}
