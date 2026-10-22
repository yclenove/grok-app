//! Real signed producers exercise the writer used by publication and dedup.
use super::*;
use crate::session_migration::{cursor::InstallationAuthenticator, jobs::entry};
use rusqlite::{params, Connection};

fn original_allocation(conn: &Connection, job: &jobs::JobRecord) -> String {
    let auth = InstallationAuthenticator::load().unwrap();
    let tx = conn.unchecked_transaction().unwrap();
    let accepted =
        job_control::publication_acceptance::journal::load(&tx, &auth, &job.job_id).unwrap();
    accepted
        .allocated(&tx, &job.items[0].candidate_id)
        .unwrap()
        .digest
}

#[test]
fn normal_signed_writer_allocation_is_bound_to_its_transaction_job_and_candidate() {
    let home = IsolatedAppHome::acquire("signed-writer-allocation-snapshot");
    let _reset = ResetRuntime;
    let planned = plan(&home, false);
    let job = engine::commit(&planned.plan_id, &planned.plan_hash, "writer-snapshot").unwrap();
    let conn = catalog::open_catalog().unwrap();
    let expected = original_allocation(&conn, &job);
    let crypto = JobCrypto::prepare(&conn, &job.job_id).unwrap();
    let other = catalog::open_catalog().unwrap();
    let tx = conn.unchecked_transaction().unwrap();
    let other_tx = other.unchecked_transaction().unwrap();
    let writer = entry::authenticate_writer(&tx, &job.job_id, &crypto).unwrap();
    let changes = tx.total_changes();
    let candidate = &job.items[0].candidate_id;
    assert_eq!(
        writer
            .allocated(&tx, &job.job_id, candidate)
            .unwrap()
            .digest,
        expected
    );
    assert!(writer.allocated(&other_tx, &job.job_id, candidate).is_err());
    assert!(writer.allocated(&tx, "foreign-job", candidate).is_err());
    assert!(writer
        .allocated(&tx, &job.job_id, "foreign-candidate")
        .is_err());
    assert_eq!(tx.total_changes(), changes);
    assert_eq!(other_tx.total_changes(), 0);
    writer.require_writer(&tx, &job.job_id).unwrap();
}

#[test]
fn normal_signed_writer_allocation_expires_after_even_a_rolled_back_sql_write() {
    let home = IsolatedAppHome::acquire("signed-writer-allocation-write");
    let _reset = ResetRuntime;
    let planned = plan(&home, false);
    let job = engine::commit(&planned.plan_id, &planned.plan_hash, "writer-write").unwrap();
    let conn = catalog::open_catalog().unwrap();
    let expected = original_allocation(&conn, &job);
    let crypto = JobCrypto::prepare(&conn, &job.job_id).unwrap();
    let tx = conn.unchecked_transaction().unwrap();
    let writer = entry::authenticate_writer(&tx, &job.job_id, &crypto).unwrap();
    tx.execute_batch("SAVEPOINT writer_snapshot_probe").unwrap();
    tx.execute("UPDATE jobs SET state=state WHERE job_id=?1", [&job.job_id])
        .unwrap();
    tx.execute_batch("ROLLBACK TO writer_snapshot_probe; RELEASE writer_snapshot_probe")
        .unwrap();
    let changes = tx.total_changes();
    assert_eq!(
        writer
            .allocated(&tx, &job.job_id, &job.items[0].candidate_id)
            .err()
            .unwrap()
            .code,
        ErrorCode::PlanStale
    );
    assert_eq!(
        writer.require_writer(&tx, &job.job_id).unwrap_err().code,
        ErrorCode::PlanStale
    );
    assert_eq!(tx.total_changes(), changes);
    let fresh = entry::authenticate_writer(&tx, &job.job_id, &crypto).unwrap();
    assert_eq!(
        fresh
            .allocated(&tx, &job.job_id, &job.items[0].candidate_id)
            .unwrap()
            .digest,
        expected
    );
    fresh.require_writer(&tx, &job.job_id).unwrap();
    assert_eq!(tx.total_changes(), changes);
}

#[test]
fn normal_signed_writer_allocation_rechecks_original_input_with_fresh_authentication() {
    let home = IsolatedAppHome::acquire("signed-writer-allocation-input");
    let _reset = ResetRuntime;
    let planned = plan(&home, false);
    let job = engine::commit(&planned.plan_id, &planned.plan_hash, "writer-input").unwrap();
    let conn = catalog::open_catalog().unwrap();
    let crypto = JobCrypto::prepare(&conn, &job.job_id).unwrap();
    let tx = conn.unchecked_transaction().unwrap();
    tx.execute(
        "UPDATE job_items SET result_json=result_json || ' ' WHERE job_id=?1",
        [&job.job_id],
    )
    .unwrap();
    let writer = entry::authenticate_writer(&tx, &job.job_id, &crypto).unwrap();
    let changes = tx.total_changes();
    assert!(writer
        .allocated(&tx, &job.job_id, &job.items[0].candidate_id)
        .is_err());
    assert_eq!(tx.total_changes(), changes);
}

#[test]
fn normal_signed_publish_rechecks_unrelated_input_after_its_first_receipt_write() {
    let home = IsolatedAppHome::acquire("signed-writer-late-unrelated-input");
    let _reset = ResetRuntime;
    let _initial = plan(&home, false);
    let source = home.path.join("source");
    let second = [
        json!({"type":"session_meta","payload":{"id":"second-normal-source"}}),
        json!({"type":"event_msg","payload":{"type":"user_message","message":"healthy neighbor"}}),
    ];
    std::fs::write(
        source.join("sessions/second.jsonl"),
        second
            .iter()
            .map(|row| format!("{row}\n"))
            .collect::<String>(),
    )
    .unwrap();
    let scan = scan::scan_start(vec![(Provider::Codex, source.to_string_lossy().into())]).unwrap();
    assert_eq!(scan.candidates.len(), 2);
    let previews = scan
        .candidates
        .iter()
        .map(|candidate| {
            engine::preview(&candidate.candidate_id, Some(&candidate.source_revision))
                .unwrap()
                .0
        })
        .collect();
    let planned = engine::plan(previews, vec![]).unwrap();
    let job = engine::commit(
        &planned.plan_id,
        &planned.plan_hash,
        "writer-unrelated-input",
    )
    .unwrap();
    let conn = catalog::open_catalog().unwrap();
    let first = &job.items[0].candidate_id;
    let second = &job.items[1].candidate_id;
    let expected_text = if scan
        .candidates
        .iter()
        .find(|candidate| &candidate.candidate_id == second)
        .unwrap()
        .native_session_id
        == "second-normal-source"
    {
        "healthy neighbor"
    } else {
        "正常导入与撤销 🚀"
    };
    let original: String = conn
        .query_row(
            "SELECT result_json FROM job_items WHERE job_id=?1 AND candidate_id=?2",
            params![job.job_id, second],
            |row| row.get(0),
        )
        .unwrap();
    // The first real publication writer has already authenticated when this
    // trigger mutates a DIFFERENT original body. Its post-write check must
    // roll the receipt and corruption back, then allow healthy queued work.
    conn.execute_batch(&format!(
        "CREATE TRIGGER corrupt_neighbor AFTER INSERT ON receipts
         WHEN NEW.job_id='{}' AND NEW.candidate_id='{first}' AND NEW.phase='prepared'
         BEGIN UPDATE job_items SET result_json=result_json || ' '
         WHERE job_id=NEW.job_id AND candidate_id='{second}'; END;",
        job.job_id
    ))
    .unwrap();
    assert_eq!(job_control::run_pending().unwrap(), 1);
    let completed = engine::job_get(&job.job_id).unwrap();
    assert_eq!(
        completed.state,
        jobs::JobState::CompletedWithIssues,
        "{completed:?}"
    );
    assert_eq!(completed.items[0].phase, "failed", "{completed:?}");
    assert!(completed.items[0].import_id.is_none() && completed.items[0].revision_id.is_none());
    assert_eq!(completed.items[1].phase, "committed", "{completed:?}");
    assert_eq!(
        conn.query_row(
            "SELECT COUNT(*) FROM receipts WHERE job_id=?1 AND candidate_id=?2",
            params![job.job_id, first],
            |row| row.get::<_, usize>(0)
        )
        .unwrap(),
        0
    );
    assert_eq!(
        conn.query_row(
            "SELECT result_json FROM job_items WHERE job_id=?1 AND candidate_id=?2",
            params![job.job_id, second],
            |row| row.get::<_, String>(0)
        )
        .unwrap(),
        original
    );
    assert_eq!(catalog::list_entries(&conn).unwrap().len(), 1);
    engine::reset_runtime();
    let healthy = &completed.items[1];
    let (events, _, total) = engine::events_page(
        healthy.import_id.as_deref().unwrap(),
        healthy.revision_id.as_deref(),
        None,
        Some(50),
        None,
    )
    .unwrap();
    assert_eq!(events.len(), usize::try_from(total).unwrap());
    assert!(events
        .iter()
        .any(|event| event.to_string().contains(expected_text)));
    assert_eq!(job_control::run_pending().unwrap(), 0);
}
