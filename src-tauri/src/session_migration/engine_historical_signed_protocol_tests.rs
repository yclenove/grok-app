//! Real signed controls alongside the unchanged historical Legacy contracts.
use super::*;
use crate::session_migration::publication_crypto::JobCrypto;
use rusqlite::params;

#[test]
#[ignore = "requires explicit hash-pinned V243 synthetic seed and registered source fixture"]
fn historical_seed_association_acceptance_signed_input_corruption_isolates_other_jobs() {
    for fault in ["missing", "malformed", "oversized-title"] {
        let home = seed_home();
        let _runtime = RuntimeReset;
        let conn = open_catalog().unwrap();
        let confirmed = confirmed_plan();
        acceptance(&confirmed, "signed-input-association-control").unwrap();
        assert_eq!(job_control::run_pending().unwrap(), 1);
        let old_text = text(IMPORT, REVISION);
        let previews = cursor_source(&home, 2);
        let broken_plan = plan(vec![previews[0].clone()], vec![]).unwrap();
        let broken = commit(
            &broken_plan.plan_id,
            &broken_plan.plan_hash,
            "signed-broken",
        )
        .unwrap();
        assert_eq!(
            crate::session_migration::publication_protocol::PublicationProtocol::for_job_id(
                &broken.job_id
            )
            .unwrap(),
            crate::session_migration::publication_protocol::PublicationProtocol::SignedOutcomeV1
        );
        let later_plan = plan(vec![previews[1].clone()], vec![]).unwrap();
        let later = commit(&later_plan.plan_id, &later_plan.plan_hash, "signed-later").unwrap();
        let input: (i64, String, String) = conn
            .query_row(
                "SELECT rowid,phase,result_json FROM job_items WHERE job_id=?1",
                [&broken.job_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        match fault {
            "missing" => {
                conn.execute("DELETE FROM job_items WHERE job_id=?1", [&broken.job_id])
                    .unwrap();
            }
            "malformed" => {
                conn.execute(
                    "UPDATE job_items SET result_json='{' WHERE job_id=?1",
                    [&broken.job_id],
                )
                .unwrap();
            }
            _ => {
                let mut changed: serde_json::Value = serde_json::from_str(&input.2).unwrap();
                changed["adapter"]["graph"]["title"] = json!("x".repeat(16 * 1024 * 1024));
                conn.execute(
                    "UPDATE job_items SET result_json=?1 WHERE job_id=?2",
                    params![changed.to_string(), broken.job_id],
                )
                .unwrap();
            }
        }
        let damaged = isolation_tests::raw_job_rows(&conn, &broken.job_id);
        for invocation in 0..2 {
            reset_runtime();
            let result = job_control::run_pending();
            println!(
                "SIGNED_INPUT_QUEUE_REFUSAL={}",
                json!({"fault": fault, "invocation": invocation,
                    "completed": result.as_ref().ok(),
                    "error": result.as_ref().err().map(|error| error.code),
                    "brokenState": conn.query_row("SELECT state FROM jobs WHERE job_id=?1",
                        [&broken.job_id], |row| row.get::<_, String>(0)).unwrap(),
                    "brokenRowsUnchanged": isolation_tests::raw_job_rows(&conn, &broken.job_id) == damaged})
            );
            assert_eq!(
                result.as_ref().err().map(|error| error.code),
                Some(ErrorCode::HistoryAccessUnavailable),
                "signed corruption must stay visible: {fault}"
            );
            assert_eq!(
                isolation_tests::raw_job_rows(&conn, &broken.job_id),
                damaged
            );
            assert_eq!(damaged.1, 0, "no receipt may be synthesized");
            let healthy = job_get(&later.job_id).unwrap();
            assert_eq!(healthy.state, JobState::Completed);
            let item = &healthy.items[0];
            let (events, _, total) = events_page(
                item.import_id.as_deref().unwrap(),
                item.revision_id.as_deref(),
                None,
                Some(50),
                None,
            )
            .unwrap();
            assert!(total > 0 && !events.is_empty());
        }
        let later_receipts =
            jobs::latest_receipt(&conn, &later.job_id, &later.items[0].candidate_id)
                .unwrap()
                .unwrap();
        if fault == "missing" {
            conn.execute(
                "INSERT INTO job_items(rowid,job_id,candidate_id,phase,result_json) VALUES(?1,?2,?3,?4,?5)",
                params![input.0, broken.job_id, broken.items[0].candidate_id, input.1, input.2],
            )
            .unwrap();
        } else {
            conn.execute(
                "UPDATE job_items SET result_json=?1 WHERE rowid=?2",
                params![input.2, input.0],
            )
            .unwrap();
        }
        reset_runtime();
        assert_eq!(job_control::run_pending().unwrap(), 1);
        assert_eq!(job_get(&broken.job_id).unwrap().state, JobState::Completed);
        assert_eq!(
            serde_json::to_value(
                jobs::latest_receipt(&conn, &later.job_id, &later.items[0].candidate_id)
                    .unwrap()
                    .unwrap()
            )
            .unwrap(),
            serde_json::to_value(later_receipts).unwrap()
        );
        assert_eq!(text(IMPORT, REVISION), old_text);
        assert_eq!(job_control::run_pending().unwrap(), 0);
        println!("SIGNED_INPUT_QUEUE_ISOLATION={fault}:zero-write-refusal:healthy-drained:exact-original-retry");
    }
}

#[test]
#[ignore = "requires explicit hash-pinned V243 synthetic seed and registered source fixture"]
fn historical_seed_association_acceptance_signed_final_writer_records_one_atomic_ack() {
    for ordinary_count in [1, 24] {
        let home = seed_home();
        let _runtime = RuntimeReset;
        let conn = open_catalog().unwrap();
        let (job, index, input) = progress_ack_tests::mixed_input(&home, ordinary_count, true);
        let crypto = JobCrypto::prepare(&conn, &job.job_id).unwrap();
        conn.execute_batch("CREATE TABLE signed_ack_writes(n INTEGER NOT NULL); INSERT INTO signed_ack_writes VALUES(0);
            CREATE TRIGGER count_signed_ack AFTER UPDATE ON jobs BEGIN UPDATE signed_ack_writes SET n=n+1; END;").unwrap();
        let published =
            jobs::publish_for_worker(&conn, &job.job_id, index, &input, &|| false, &crypto)
                .unwrap()
                .expect_recorded();
        assert_eq!(published.phase, "committed");
        assert_eq!(
            conn.query_row("SELECT n FROM signed_ack_writes", [], |row| row
                .get::<_, u32>(0))
                .unwrap(),
            1
        );
        let recorded = jobs::get_job(&conn, &job.job_id).unwrap().unwrap();
        assert_eq!(recorded.state, job.state);
        assert_eq!(recorded.state_version, job.state_version + 1);
        assert_eq!(recorded.items[index].phase, "committed");
        assert_eq!(
            serde_json::to_value(&recorded.items[index]).unwrap(),
            serde_json::to_value(&published).unwrap()
        );
        let receipt = jobs::latest_receipt(&conn, &job.job_id, &published.candidate_id)
            .unwrap()
            .unwrap();
        drop(input);
        reset_runtime();
        assert_eq!(job_control::run_pending().unwrap(), 1);
        assert_eq!(job_get(&job.job_id).unwrap().state, JobState::Completed);
        assert_eq!(
            serde_json::to_value(
                jobs::latest_receipt(&conn, &job.job_id, &published.candidate_id)
                    .unwrap()
                    .unwrap()
            )
            .unwrap(),
            serde_json::to_value(receipt).unwrap()
        );
        assert_eq!(job_control::run_pending().unwrap(), 0);
    }
}
