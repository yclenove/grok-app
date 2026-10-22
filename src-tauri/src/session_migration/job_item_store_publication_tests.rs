//! Real signed acceptance exercises compact-header publication and old storage.
use super::*;
use crate::session_migration::{codec_profile, job_actions, publication_write_fence};
use rusqlite::params;

fn many_plan(home: &IsolatedAppHome, count: usize) -> engine::PlanState {
    let _initial = plan(home, false);
    for index in 1..count {
        let records = [
            json!({"type":"session_meta","payload":{"id":format!("signed-jobstore-{index}"),"cwd":home.path.join("workspace")}}),
            json!({"type":"event_msg","payload":{"type":"user_message","message":"Job storage neighbor"}}),
        ];
        std::fs::write(
            home.path.join(format!("source/sessions/{index}.jsonl")),
            records
                .iter()
                .map(|row| format!("{row}\n"))
                .collect::<String>(),
        )
        .unwrap();
    }
    engine::reset_runtime();
    let scanned = scan::scan_start(vec![(
        Provider::Codex,
        home.path.join("source").to_string_lossy().into(),
    )])
    .unwrap();
    assert_eq!(scanned.discovered_count as usize, count);
    let mut candidates = Vec::new();
    let mut cursor = None;
    loop {
        let (page, next, _, complete) =
            scan::page(&scanned.scan_id, scanned.epoch, cursor.as_deref(), None).unwrap();
        assert!(complete);
        candidates.extend(page);
        let Some(next) = next else { break };
        cursor = Some(next);
    }
    assert_eq!(candidates.len(), count);
    let previews = candidates
        .iter()
        .map(|candidate| {
            engine::preview(&candidate.candidate_id, Some(&candidate.source_revision))
                .unwrap()
                .0
        })
        .collect();
    engine::plan(previews, vec![]).unwrap()
}

#[test]
fn publishing_one_signed_item_reads_and_writes_constant_job_header_bytes() {
    let mut costs = Vec::new();
    for count in [1, 64] {
        let home = IsolatedAppHome::acquire("jobstore-publication-complexity");
        let _reset = ResetRuntime;
        let planned = many_plan(&home, count);
        let queued =
            engine::commit(&planned.plan_id, &planned.plan_hash, "jobstore-complexity").unwrap();
        let conn = catalog::open_catalog().unwrap();
        let _fence = publication_write_fence::install(&conn);
        let crypto = JobCrypto::prepare(&conn, &queued.job_id).unwrap();
        // The full accepted snapshot is checked at worker start. Measure only
        // the selected item's real publisher, including its final atomic ACK.
        jobs::get_job_with_crypto(&conn, &queued.job_id, &crypto)
            .unwrap()
            .unwrap();
        let input =
            job_control::load_input(&conn, &queued.job_id, &queued.items[0].candidate_id).unwrap();
        conn.execute_batch(
            "CREATE TABLE jobstore_writes(bytes INTEGER);
             CREATE TRIGGER measure_jobstore AFTER UPDATE OF json ON jobs
             BEGIN INSERT INTO jobstore_writes VALUES(length(CAST(NEW.json AS BLOB))); END;",
        )
        .unwrap();
        let profile = codec_profile::Session::default();
        let scope = profile.enter("item");
        let result = jobs::publish_for_worker(&conn, &queued.job_id, 0, &input, &|| false, &crypto)
            .unwrap()
            .expect_recorded();
        drop(scope);
        assert_eq!(result.phase, "committed");
        let (writes, bytes): (u64, u64) = conn
            .query_row(
                "SELECT COUNT(*),COALESCE(SUM(bytes),0) FROM jobstore_writes",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(writes, 1, "one atomic progress ACK");
        assert_eq!(profile.calls("item", "job-progress-header-write"), 1);
        assert_eq!(profile.bytes("item", "job-progress-header-write"), bytes);
        let decodes = profile.calls("item", "job-header-decode");
        let decoded_bytes = profile.bytes("item", "job-header-decode");
        assert!((1..=4).contains(&decodes), "{decodes} header decodes");
        assert!(bytes < 1024, "{count} items rewrote {bytes} header bytes");
        assert!(
            decoded_bytes < decodes * 1024,
            "{count} items decoded {decoded_bytes} header bytes"
        );
        costs.push((bytes, decodes, decoded_bytes));
        let current = jobs::get_job_with_crypto(&conn, &queued.job_id, &crypto)
            .unwrap()
            .unwrap();
        assert_eq!(current.state_version, queued.state_version + 1);
        assert_eq!(current.items.len(), count);
        assert!(current.items[1..].iter().all(|item| item.phase == "queued"));
        assert_eq!(catalog::list_entries(&conn).unwrap().len(), 1);
    }
    assert!(costs[1].0 <= costs[0].0 + 32);
    assert_eq!(costs[0].1, costs[1].1);
    assert!(costs[1].2 <= costs[0].2 + 128);
}

#[test]
fn legacy_embedded_signed_job_keeps_controls_recovery_receipts_and_undo() {
    signed_controls(true);
}

#[test]
fn rows_v1_signed_job_keeps_public_cancel_retry_undo_and_replay_after_restart() {
    signed_controls(false);
}

fn signed_controls(legacy: bool) {
    let home = IsolatedAppHome::acquire("jobstore-legacy-signed-controls");
    let _reset = ResetRuntime;
    let planned = many_plan(&home, 2);
    let queued = engine::commit(
        &planned.plan_id,
        &planned.plan_hash,
        "jobstore-legacy-controls",
    )
    .unwrap();
    let conn = catalog::open_catalog().unwrap();
    if legacy {
        let original = jobs::get_job(&conn, &queued.job_id).unwrap().unwrap();
        // Only the display layout changes; every original input and acceptance
        // header/member seal still comes from the real registered producer.
        let tx = conn.unchecked_transaction().unwrap();
        tx.execute(
            "DELETE FROM job_item_states WHERE job_id=?1",
            [&queued.job_id],
        )
        .unwrap();
        tx.execute(
            "UPDATE jobs SET json=?2 WHERE job_id=?1",
            params![queued.job_id, serde_json::to_string(&original).unwrap()],
        )
        .unwrap();
        tx.commit().unwrap();
    }
    assert_eq!(
        jobs::item_store::uses_rows(&conn, &queued.job_id).unwrap(),
        !legacy
    );
    let action = |job: &jobs::JobRecord| job_actions::JobActionRequest {
        expected_state_version: job.state_version,
        request_id: uuid::Uuid::new_v4().to_string(),
    };
    let cancel = action(&queued);
    let cancelling = job_actions::cancel(&queued.job_id, cancel.clone()).unwrap();
    assert_eq!(
        job_actions::cancel(&queued.job_id, cancel)
            .unwrap()
            .state_version,
        cancelling.state_version
    );
    assert_eq!(job_control::run_pending().unwrap(), 1);
    let cancelled = engine::job_get(&queued.job_id).unwrap();
    assert_eq!(cancelled.state, jobs::JobState::Cancelled);
    let retry = action(&cancelled);
    let retrying = job_actions::retry(&queued.job_id, retry.clone()).unwrap();
    assert_eq!(retrying.state, jobs::JobState::Queued);
    assert_eq!(
        job_actions::retry(&queued.job_id, retry)
            .unwrap()
            .state_version,
        retrying.state_version
    );
    engine::reset_runtime();
    std::fs::rename(home.path.join("source"), home.path.join("offline-source")).unwrap();
    assert_eq!(job_control::run_pending().unwrap(), 1);
    let completed = engine::job_get(&queued.job_id).unwrap();
    assert_eq!(completed.state, jobs::JobState::Completed, "{completed:?}");
    for item in &completed.items {
        assert_eq!(
            jobs::latest_receipt(&conn, &queued.job_id, &item.candidate_id)
                .unwrap()
                .unwrap()
                .phase,
            "committed"
        );
    }
    engine::reset_runtime();
    let undo = action(&completed);
    let undone = job_actions::undo(&queued.job_id, undo.clone()).unwrap();
    assert!(undone.items.iter().all(|item| item.phase == "undone"));
    assert!(undone.state_version > completed.state_version);
    assert_eq!(
        job_actions::undo(&queued.job_id, undo)
            .unwrap()
            .state_version,
        undone.state_version
    );
    assert_eq!(
        jobs::item_store::uses_rows(&conn, &queued.job_id).unwrap(),
        !legacy
    );
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM tombstones", [], |r| r
            .get::<_, u64>(0))
            .unwrap(),
        2
    );
    assert_eq!(
        conn.query_row(
            "SELECT COUNT(*) FROM meta WHERE key LIKE 'job_action:v1:%'",
            [],
            |r| r.get::<_, u64>(0)
        )
        .unwrap(),
        3
    );
}

#[test]
fn signed_split_membership_rejects_coherent_duplicate_and_missing_display_rows() {
    let home = IsolatedAppHome::acquire("jobstore-signed-display-membership");
    let _reset = ResetRuntime;
    let planned = many_plan(&home, 2);
    let queued =
        engine::commit(&planned.plan_id, &planned.plan_hash, "jobstore-membership").unwrap();
    let conn = catalog::open_catalog().unwrap();
    let crypto = JobCrypto::prepare(&conn, &queued.job_id).unwrap();
    for sql in [
        "UPDATE job_item_states SET candidate_id=(SELECT candidate_id FROM job_item_states WHERE item_index=0),json=(SELECT json FROM job_item_states WHERE item_index=0) WHERE item_index=1",
        "DELETE FROM job_item_states WHERE item_index=1",
        "UPDATE jobs SET json=json_remove(json,'$.itemStorage')",
    ] {
        let tx = conn.unchecked_transaction().unwrap();
        tx.execute(sql, []).unwrap();
        let changes = tx.total_changes();
        assert!(jobs::get_job_with_crypto(&tx, &queued.job_id, &crypto).is_err(), "{sql}");
        assert_eq!(tx.total_changes(), changes);
        tx.rollback().unwrap();
    }
    assert_eq!(engine::job_get(&queued.job_id).unwrap().item_count, 2);
}

#[test]
fn signed_publication_rolls_back_a_receipt_trigger_changing_a_neighbor_display_row() {
    refuse_receipt_trigger(
        "UPDATE job_item_states SET json=json_set(json,'$.title','foreign')
         WHERE job_id=NEW.job_id AND item_index=1;",
    );
}

#[test]
fn signed_publication_rolls_back_receipt_trigger_layout_and_neighbor_phase_forgeries() {
    for phase in ["committed", "cancelled", "tombstoned"] {
        for delete_rows in [false, true] {
            refuse_receipt_trigger(&format!(
                "UPDATE jobs SET json=json_remove(json_set(json,'$.items',json_array(
                   json((SELECT json FROM job_item_states WHERE job_id=NEW.job_id AND item_index=0)),
                   json_set((SELECT json FROM job_item_states WHERE job_id=NEW.job_id AND item_index=1),
                            '$.phase','{phase}','$.outcome','{phase}'))),'$.itemStorage')
                 WHERE job_id=NEW.job_id; {}",
                if delete_rows { "DELETE FROM job_item_states WHERE job_id=NEW.job_id;" } else { "" },
            ));
        }
    }
    refuse_receipt_trigger(
        "UPDATE jobs SET json=json_remove(json,'$.itemStorage') WHERE job_id=NEW.job_id;",
    );
    // Even an unchanged header write is foreign outside the worker's ACK.
    refuse_receipt_trigger("UPDATE jobs SET json=json WHERE job_id=NEW.job_id;");
}

#[test]
fn signed_publication_rolls_back_deleted_moved_and_replaced_neighbor_rows() {
    for body in [
        "DELETE FROM job_item_states WHERE job_id=NEW.job_id AND item_index=1;",
        "DELETE FROM job_item_states;",
        "UPDATE job_item_states SET job_id='other-job' WHERE job_id=NEW.job_id AND item_index=1;",
        "UPDATE job_item_states SET job_id='other-job',rowid=rowid+100000 WHERE job_id=NEW.job_id AND item_index=1;",
        "UPDATE job_item_states SET job_id='other-job' WHERE job_id=NEW.job_id AND item_index=1;
         INSERT INTO job_item_states SELECT NEW.job_id,item_index,candidate_id,
           json_set(json,'$.phase','tombstoned') FROM job_item_states WHERE job_id='other-job';",
        "UPDATE job_items SET job_id='other-job' WHERE job_id=NEW.job_id AND candidate_id<>NEW.candidate_id;",
        "UPDATE OR REPLACE job_item_states SET item_index=0,candidate_id=NEW.candidate_id,
           json=json_set(json,'$.candidateId',NEW.candidate_id)
         WHERE job_id=NEW.job_id AND item_index=1;",
        "UPDATE OR REPLACE job_item_states SET item_index=1 WHERE job_id=NEW.job_id AND item_index=0;",
        "INSERT INTO meta VALUES('jobstore-trigger-scratch','scratch');
         DELETE FROM meta WHERE key='jobstore-trigger-scratch';
         UPDATE job_item_states SET json=json_set(json,'$.phase','tombstoned')
         WHERE job_id=NEW.job_id AND item_index=1;",
    ] {
        refuse_receipt_trigger(body);
    }
}

#[test]
fn signed_final_ack_refuses_extra_header_events_during_and_after_its_update() {
    for point in [
        "AFTER UPDATE OF json ON jobs WHEN NEW.state_version<>OLD.state_version",
        "AFTER INSERT ON receipts WHEN NEW.phase='committed'",
    ] {
        let home = IsolatedAppHome::acquire("jobstore-final-header-fence");
        let _reset = ResetRuntime;
        let planned = many_plan(&home, 2);
        let queued =
            engine::commit(&planned.plan_id, &planned.plan_hash, "jobstore-final-fence").unwrap();
        let conn = catalog::open_catalog().unwrap();
        let _fence = publication_write_fence::install(&conn);
        let crypto = JobCrypto::prepare(&conn, &queued.job_id).unwrap();
        let input =
            job_control::load_input(&conn, &queued.job_id, &queued.items[0].candidate_id).unwrap();
        let final_rows = || {
            catalog_rows(&conn)
                .into_iter()
                .filter(|(table, _)| {
                    matches!(
                        table.as_str(),
                        "jobs" | "job_items" | "job_item_states" | "entries" | "revisions"
                    )
                })
                .collect::<CatalogRows>()
        };
        let before = final_rows();
        conn.execute_batch(&format!(
            "CREATE TRIGGER extra_header_event {point}
             BEGIN UPDATE jobs SET json=json WHERE job_id=NEW.job_id; END;",
        ))
        .unwrap();
        let error = jobs::publish_for_worker(&conn, &queued.job_id, 0, &input, &|| false, &crypto)
            .err()
            .unwrap();
        assert_eq!(
            error.code,
            ErrorCode::HistoryAccessUnavailable,
            "{point}: {error:?}"
        );
        assert_eq!(final_rows(), before);
        assert_eq!(
            jobs::latest_receipt(&conn, &queued.job_id, &queued.items[0].candidate_id)
                .unwrap()
                .unwrap()
                .phase,
            "linked"
        );
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM receipts WHERE phase='committed'",
                [],
                |r| r.get::<_, u64>(0)
            )
            .unwrap(),
            0
        );
        conn.execute_batch("DROP TRIGGER extra_header_event")
            .unwrap();
        assert_eq!(
            jobs::publish_for_worker(&conn, &queued.job_id, 0, &input, &|| false, &crypto)
                .unwrap()
                .expect_recorded()
                .phase,
            "committed"
        );
    }
}

type CatalogRows = Vec<(String, Vec<Vec<rusqlite::types::Value>>)>;

fn catalog_rows(conn: &rusqlite::Connection) -> CatalogRows {
    let tables = conn
        .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name")
        .unwrap()
        .query_map([], |r| r.get::<_, String>(0))
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
                .query_map([], |r| {
                    (0..columns)
                        .map(|index| r.get::<_, rusqlite::types::Value>(index))
                        .collect::<rusqlite::Result<Vec<_>>>()
                })
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            (table, rows)
        })
        .collect()
}

fn refuse_receipt_trigger(body: &str) {
    let home = IsolatedAppHome::acquire("jobstore-display-fence");
    let _reset = ResetRuntime;
    let planned = many_plan(&home, 2);
    let queued = engine::commit(&planned.plan_id, &planned.plan_hash, "jobstore-fence").unwrap();
    let conn = catalog::open_catalog().unwrap();
    let _fence = publication_write_fence::install(&conn);
    let crypto = JobCrypto::prepare(&conn, &queued.job_id).unwrap();
    let input =
        job_control::load_input(&conn, &queued.job_id, &queued.items[0].candidate_id).unwrap();
    let before = catalog_rows(&conn);
    conn.execute_batch(&format!(
        "CREATE TRIGGER corrupt_jobstore_neighbor AFTER INSERT ON receipts
         WHEN NEW.job_id='{}' AND NEW.phase='prepared'
         BEGIN {body} END;",
        queued.job_id
    ))
    .unwrap();
    let error = jobs::publish_for_worker(&conn, &queued.job_id, 0, &input, &|| false, &crypto)
        .err()
        .unwrap();
    assert!(
        matches!(
            error.code,
            ErrorCode::HistoryAccessUnavailable | ErrorCode::SnapshotUnavailable
        ),
        "{body}: {error:?}"
    );
    assert_eq!(catalog_rows(&conn), before, "{body}");
    assert_eq!(
        conn.query_row(
            "SELECT COUNT(*) FROM receipts WHERE job_id=?1",
            [&queued.job_id],
            |r| r.get::<_, u64>(0)
        )
        .unwrap(),
        0
    );
    assert_eq!(
        engine::job_get(&queued.job_id).unwrap().state_version,
        queued.state_version
    );
    conn.execute_batch("DROP TRIGGER corrupt_jobstore_neighbor")
        .unwrap();
    assert_eq!(
        jobs::publish_for_worker(&conn, &queued.job_id, 0, &input, &|| false, &crypto)
            .unwrap()
            .expect_recorded()
            .phase,
        "committed"
    );
    assert_eq!(
        jobs::item_store::page::<jobs::JobItem>(&conn, &queued.job_id, 1, 1).unwrap()[0].phase,
        "queued"
    );
}
