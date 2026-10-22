//! Actual SQL failures/late side effects and raw journal corruption controls.
use super::*;

#[test]
fn rows_v1_action_checks_exact_ordered_display_bytes_from_its_snapshot() {
    let home = IsolatedAppHome::acquire("public-control-display-seal");
    let _reset = ResetRuntime;
    let queued = queued(&home);
    let conn = catalog::open_catalog().unwrap();
    assert!(jobs::item_store::uses_rows(&conn, &queued.job_id).unwrap());
    let prepared = controls::PreparedAction::prepare(
        &conn,
        &queued.job_id,
        controls::Action::Cancel,
        request(&queued),
    )
    .unwrap();
    for sql in [
        "UPDATE job_item_states SET json=json || ' '",
        "UPDATE job_item_states SET json=json_set(json,'$.title','changed')",
        "UPDATE job_item_states SET candidate_id='foreign'",
        "UPDATE job_item_states SET item_index=1",
        "DELETE FROM job_item_states WHERE item_index=0",
        "INSERT INTO job_item_states SELECT job_id,1,candidate_id,json FROM job_item_states",
    ] {
        let tx = conn.unchecked_transaction().unwrap();
        let (observed, _entry) =
            jobs::get_job_for_writer(&tx, &queued.job_id, prepared.crypto()).unwrap();
        let observed = observed.unwrap();
        assert!(!prepared.replay(&tx, &observed).unwrap());
        tx.execute(sql, []).unwrap();
        let changes = tx.total_changes();
        assert_eq!(
            prepared.replay(&tx, &observed).unwrap_err().code,
            ErrorCode::HistoryAccessUnavailable,
            "{sql}"
        );
        assert_eq!(tx.total_changes(), changes);
        tx.rollback().unwrap();
    }
}

#[test]
fn public_controls_roll_back_state_and_journal_on_early_and_late_sql_faults() {
    for retry in [false, true] {
        let home = IsolatedAppHome::acquire("public-control-sql-faults");
        let _reset = ResetRuntime;
        let queued = queued(&home);
        let conn = catalog::open_catalog().unwrap();
        let before = if retry {
            controls::cancel(&queued.job_id, request(&queued)).unwrap();
            assert_eq!(job_control::run_pending().unwrap(), 1);
            engine::job_get(&queued.job_id).unwrap()
        } else {
            queued
        };
        let action = request(&before);
        let invoke = || {
            if retry {
                controls::retry(&before.job_id, action.clone())
            } else {
                controls::cancel(&before.job_id, action.clone())
            }
        };
        let faults = [
            (
                "BEFORE UPDATE ON jobs",
                "SELECT RAISE(ABORT,'synthetic job write failed');",
                ErrorCode::SnapshotUnavailable,
            ),
            (
                "AFTER UPDATE ON jobs",
                "UPDATE job_items SET result_json='{}' WHERE job_id=NEW.job_id;",
                ErrorCode::HistoryAccessUnavailable,
            ),
            (
                "AFTER INSERT ON meta WHEN NEW.key LIKE 'job_action:v1:%'",
                "SELECT RAISE(ABORT,'synthetic journal write failed');",
                ErrorCode::SnapshotUnavailable,
            ),
            (
                "AFTER INSERT ON meta WHEN NEW.key LIKE 'job_action:v1:%'",
                "UPDATE jobs SET state_version=state_version+1;",
                ErrorCode::HistoryAccessUnavailable,
            ),
            (
                "AFTER INSERT ON meta WHEN NEW.key LIKE 'job_action:v1:%'",
                "UPDATE job_item_states SET json=json_set(json,'$.phase','conflict');",
                ErrorCode::HistoryAccessUnavailable,
            ),
        ];
        for (point, body, code) in faults {
            conn.execute_batch(&format!(
                "CREATE TRIGGER control_sql_fault {point} BEGIN {body} END;"
            ))
            .unwrap();
            refuse_unchanged(&conn, code, invoke);
            conn.execute_batch("DROP TRIGGER control_sql_fault")
                .unwrap();
        }
        let completed = invoke().unwrap();
        assert_eq!(completed.state_version, before.state_version + 1);
        unchanged_replay(&conn, invoke);
    }
}

#[test]
fn public_control_replay_refuses_malformed_oversized_wrong_type_and_altered_journal() {
    let home = IsolatedAppHome::acquire("public-control-journal-faults");
    let _reset = ResetRuntime;
    let queued = queued(&home);
    let action = request(&queued);
    controls::cancel(&queued.job_id, action.clone()).unwrap();
    let conn = catalog::open_catalog().unwrap();
    let original: String = conn
        .query_row(
            "SELECT value FROM meta WHERE key=?1",
            [key(&action)],
            |row| row.get(0),
        )
        .unwrap();
    let invalid = [
        "'{}'",
        "'null'",
        "CAST(x'ff' AS TEXT)",
        "x'7b7d'",
        "CAST(zeroblob(1048576) AS TEXT)",
    ];
    for value in invalid {
        conn.execute(
            &format!("UPDATE meta SET value={value} WHERE key=?1"),
            [key(&action)],
        )
        .unwrap();
        refuse_unchanged(&conn, ErrorCode::HistoryAccessUnavailable, || {
            controls::cancel(&queued.job_id, action.clone())
        });
    }
    for value in [format!("\u{feff}{original}"), {
        let mut record: serde_json::Value = serde_json::from_str(&original).unwrap();
        record["body"]["resultStateVersion"] = json!(999);
        record.to_string()
    }] {
        conn.execute(
            "UPDATE meta SET value=?2 WHERE key=?1",
            params![key(&action), value],
        )
        .unwrap();
        refuse_unchanged(&conn, ErrorCode::HistoryAccessUnavailable, || {
            controls::cancel(&queued.job_id, action.clone())
        });
    }
    conn.execute(
        "UPDATE meta SET value=?2 WHERE key=?1",
        params![key(&action), original],
    )
    .unwrap();
    unchanged_replay(&conn, || controls::cancel(&queued.job_id, action));
}

#[test]
fn public_control_rejects_noncanonical_or_missing_request_and_inconsistent_header() {
    let home = IsolatedAppHome::acquire("public-control-request-shape");
    let _reset = ResetRuntime;
    let queued = queued(&home);
    let conn = catalog::open_catalog().unwrap();
    let original = request(&queued);
    for request_id in [
        String::new(),
        "not-a-uuid".into(),
        uuid::Uuid::nil().to_string(),
        format!(" {}", original.request_id),
        "ABCDEF01-2345-4678-9ABC-DEF012345678".into(),
        "x".repeat(100_000),
    ] {
        let mut invalid = original.clone();
        invalid.request_id = request_id;
        refuse_unchanged(&conn, ErrorCode::JobActionConflict, || {
            controls::cancel(&queued.job_id, invalid)
        });
    }
    let mut invalid = original.clone();
    invalid.expected_state_version = 0;
    refuse_unchanged(&conn, ErrorCode::JobActionConflict, || {
        controls::cancel(&queued.job_id, invalid)
    });
    conn.execute(
        "UPDATE jobs SET state_version=state_version+1 WHERE job_id=?1",
        [&queued.job_id],
    )
    .unwrap();
    // The pre-existing signed writer authenticates the SQL/header pair before
    // the action reader runs. Preserve that earlier precise refusal code.
    refuse_unchanged(&conn, ErrorCode::PlanStale, || {
        controls::cancel(&queued.job_id, original)
    });
    assert!(serde_json::from_value::<controls::JobActionRequest>(
        json!({"requestId":uuid::Uuid::new_v4(),"expectedStateVersion":1,"authorized":true})
    )
    .is_err());
}
