//! Concrete mixed-job Undo, real acceptance and rollback, no synthetic grants.
use super::*;
use rusqlite::{params, Connection};

pub(super) fn mixed_plan(home: &IsolatedAppHome) -> engine::PlanState {
    let _initial = plan(home, false);
    let records = [
        json!({"type":"session_meta","payload":{"id":"signed-second-source","cwd":home.path.join("workspace")}}),
        json!({"type":"event_msg","payload":{"type":"user_message","message":"第二个真实签名成员"}}),
    ];
    std::fs::write(
        home.path.join("source/sessions/second.jsonl"),
        records
            .iter()
            .map(|row| format!("{row}\n"))
            .collect::<String>(),
    )
    .unwrap();
    engine::reset_runtime();
    let scanned = scan::scan_start(vec![(
        Provider::Codex,
        home.path.join("source").to_string_lossy().into(),
    )])
    .unwrap();
    assert_eq!(scanned.candidates.len(), 2);
    let previews = scanned
        .candidates
        .iter()
        .map(|candidate| {
            engine::preview(&candidate.candidate_id, Some(&candidate.source_revision))
                .unwrap()
                .0
        })
        .collect();
    engine::plan(previews, vec![]).unwrap()
}

fn snapshot(conn: &Connection, job: &jobs::JobRecord) -> serde_json::Value {
    let header: (String, u64, String) = conn
        .query_row(
            "SELECT state,state_version,json FROM jobs WHERE job_id=?1",
            [&job.job_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    let receipts = conn
        .prepare("SELECT json FROM receipts ORDER BY receipt_id")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let phases = conn
        .prepare("SELECT candidate_id,phase FROM job_items WHERE job_id=?1 ORDER BY candidate_id")
        .unwrap()
        .query_map([&job.job_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let tombstones: u64 = conn
        .query_row("SELECT COUNT(*) FROM tombstones", [], |row| row.get(0))
        .unwrap();
    json!({"header":header,"entries":catalog::list_entries(conn).unwrap(),"receipts":receipts,"phases":phases,"tombstones":tombstones})
}

#[test]
fn normal_signed_mixed_undo_refuses_later_corrupt_receipt_before_any_target_write() {
    let home = IsolatedAppHome::acquire("signed-mixed-undo-refusal");
    let _reset = ResetRuntime;
    let completed = finish(&mixed_plan(&home), "signed-mixed-undo-refusal");
    assert_eq!(completed.items.len(), 2);
    let conn = catalog::open_catalog().unwrap();
    conn.execute(
        "UPDATE receipts SET json=json_set(json,'$.mappingProjectId','foreign') WHERE job_id=?1 AND candidate_id=?2 AND phase='committed'",
        params![completed.job_id, completed.items[1].candidate_id],
    ).unwrap();
    let before = snapshot(&conn, &completed);
    let changes = conn.total_changes();
    let error = engine::undo_import_job(&completed.job_id).unwrap_err();
    assert_eq!(error.code, ErrorCode::HistoryAccessUnavailable);
    assert_eq!(conn.total_changes(), changes);
    assert_eq!(snapshot(&conn, &completed), before);
    for item in &completed.items {
        assert!(
            !catalog::get_entry(&conn, item.import_id.as_deref().unwrap())
                .unwrap()
                .unwrap()
                .hidden
        );
    }
}

#[test]
fn normal_signed_late_undo_trigger_rolls_back_the_whole_mixed_job_and_can_retry() {
    let home = IsolatedAppHome::acquire("signed-mixed-undo-late-trigger");
    let _reset = ResetRuntime;
    let completed = finish(&mixed_plan(&home), "signed-mixed-undo-late-trigger");
    assert_eq!(completed.items.len(), 2);
    let conn = catalog::open_catalog().unwrap();
    let before = snapshot(&conn, &completed);
    conn.execute_batch(&format!(
        "CREATE TRIGGER corrupt_late_undo AFTER UPDATE OF json ON jobs
         WHEN NEW.job_id='{}' AND json_extract(NEW.json,'$.items[1].phase')='undone'
         BEGIN UPDATE receipts SET json=json_set(json,'$.optionsHash','changed')
         WHERE job_id=NEW.job_id AND candidate_id='{}' AND phase='committed'; END;",
        completed.job_id, completed.items[1].candidate_id,
    ))
    .unwrap();
    let error = engine::undo_import_job(&completed.job_id).unwrap_err();
    assert_eq!(error.code, ErrorCode::HistoryAccessUnavailable);
    // total_changes includes rolled-back statements. Compare the actual rows,
    // including both ACK projections, to prove final atomic rollback instead.
    assert_eq!(snapshot(&conn, &completed), before);
    conn.execute_batch("DROP TRIGGER corrupt_late_undo")
        .unwrap();
    let undone = engine::undo_import_job(&completed.job_id).unwrap();
    assert!(
        undone
            .items
            .iter()
            .all(|item| item.phase == "undone" && item.error.is_none()),
        "{undone:?}"
    );
    for item in &completed.items {
        assert!(
            catalog::get_entry(&conn, item.import_id.as_deref().unwrap())
                .unwrap()
                .unwrap()
                .hidden
        );
    }
    let repeated = engine::undo_import_job(&completed.job_id).unwrap();
    assert_eq!(
        serde_json::to_value(repeated.items).unwrap(),
        serde_json::to_value(undone.items).unwrap()
    );
}

#[test]
fn normal_signed_undo_with_initial_failed_member_keeps_it_failed_and_retries_its_allocation() {
    let home = IsolatedAppHome::acquire("signed-mixed-undo-initial-failed");
    let _reset = ResetRuntime;
    let planned = mixed_plan(&home);
    let queued = engine::commit(
        &planned.plan_id,
        &planned.plan_hash,
        "signed-mixed-undo-initial-failed",
    )
    .unwrap();
    let conn = catalog::open_catalog().unwrap();
    conn.execute_batch(&format!(
        "CREATE TRIGGER fail_second_publication AFTER INSERT ON receipts
         WHEN NEW.job_id='{}' AND NEW.candidate_id='{}' AND NEW.phase='committed'
         BEGIN SELECT RAISE(ABORT,'synthetic second publication failure'); END;",
        queued.job_id, queued.items[1].candidate_id,
    ))
    .unwrap();
    assert_eq!(job_control::run_pending().unwrap(), 1);
    let completed = engine::job_get(&queued.job_id).unwrap();
    assert_eq!(
        completed.state,
        jobs::JobState::CompletedWithIssues,
        "{completed:?}"
    );
    assert_eq!(completed.items[0].phase, "committed");
    assert_eq!(completed.items[1].phase, "failed");
    let receipt = jobs::latest_receipt(&conn, &queued.job_id, &queued.items[1].candidate_id)
        .unwrap()
        .unwrap();
    assert_eq!(receipt.phase, "linked");
    conn.execute_batch("DROP TRIGGER fail_second_publication")
        .unwrap();
    let undone = engine::undo_import_job(&queued.job_id).unwrap();
    assert_eq!(undone.items[0].phase, "undone", "{undone:?}");
    assert_eq!(
        serde_json::to_value(&undone.items[1]).unwrap(),
        serde_json::to_value(&completed.items[1]).unwrap()
    );
    let repeated = engine::undo_import_job(&queued.job_id).unwrap();
    assert_eq!(
        serde_json::to_value(&repeated.items).unwrap(),
        serde_json::to_value(&undone.items).unwrap()
    );
    engine::reset_runtime();
    std::fs::rename(home.path.join("source"), home.path.join("source-offline")).unwrap();
    engine::retry_failed(&queued.job_id).unwrap();
    assert_eq!(job_control::run_pending().unwrap(), 1);
    let recovered = engine::job_get(&queued.job_id).unwrap();
    assert_eq!(recovered.items[0].phase, "undone");
    assert_eq!(recovered.items[1].phase, "committed", "{recovered:?}");
    assert_eq!(recovered.items[1].import_id, receipt.import_id);
    assert_eq!(recovered.items[1].revision_id, receipt.revision_id);
}
