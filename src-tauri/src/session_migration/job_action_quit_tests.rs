//! Crash both sides of public control transactions and Undo's native cleanup.
use super::*;

fn request(job: &jobs::JobRecord) -> controls::JobActionRequest {
    controls::JobActionRequest {
        expected_state_version: job.state_version,
        request_id: uuid::Uuid::new_v4().to_string(),
    }
}

fn recorded(conn: &Connection, action: &controls::JobActionRequest) -> bool {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM meta WHERE key=?1)",
        [format!("job_action:v1:{}", action.request_id)],
        |row| row.get(0),
    )
    .unwrap()
}

fn invoke(work: &str, job: &str, action: &controls::JobActionRequest) -> jobs::JobRecord {
    match work {
        "cancel" => controls::cancel(job, action.clone()).unwrap(),
        "retry" => controls::retry(job, action.clone()).unwrap(),
        _ => panic!("unexpected control"),
    }
}

#[test]
fn cancel_and_retry_quit_transactions_restore_truthful_state_and_replay_once() {
    for work in ["cancel", "retry"] {
        for after in [false, true] {
            let home = IsolatedAppHome::acquire("public-control-quit");
            let _reset = ResetRuntime;
            let planned = plan(&home, false);
            let queued =
                engine::commit(&planned.plan_id, &planned.plan_hash, "control-quit").unwrap();
            let before = if work == "retry" {
                controls::cancel(&queued.job_id, request(&queued)).unwrap();
                assert_eq!(job_control::run_pending().unwrap(), 1);
                engine::job_get(&queued.job_id).unwrap()
            } else {
                queued
            };
            let action = request(&before);
            let point = format!("{}_{}_commit", if after { "after" } else { "before" }, work);
            child(
                &home,
                json!({"work":work,"point":point,"job":before.job_id,"action":action}),
                EXIT_AT_BOUNDARY,
            );
            engine::reset_runtime();
            std::fs::rename(home.path.join("source"), home.path.join("offline-source")).unwrap();
            let conn = catalog::open_catalog().unwrap();
            assert_eq!(recorded(&conn, &action), after);
            let restored = engine::job_get(&before.job_id).unwrap();
            if after {
                assert_eq!(restored.state_version, before.state_version + 1);
                assert_eq!(
                    restored.state,
                    if work == "retry" {
                        jobs::JobState::Queued
                    } else {
                        jobs::JobState::Cancelling
                    }
                );
            } else {
                assert_eq!(
                    serde_json::to_value(&restored).unwrap(),
                    serde_json::to_value(&before).unwrap()
                );
            }
            let accepted = invoke(work, &before.job_id, &action);
            assert_eq!(accepted.state_version, before.state_version + 1);
            child(&home, json!({"work":"recover","completed":1}), 0);
            let recovered = engine::job_get(&before.job_id).unwrap();
            let receipt_bytes = receipts(&conn, &before.job_id);
            let replay = invoke(work, &before.job_id, &action);
            assert_eq!(replay.state_version, recovered.state_version);
            assert_eq!(receipts(&conn, &before.job_id), receipt_bytes);
            if work == "cancel" {
                assert_eq!(recovered.state, jobs::JobState::Cancelled);
                assert_eq!(recovered.items[0].phase, "cancelled");
                assert!(receipt_bytes.is_empty());
                assert!(catalog::list_entries(&conn).unwrap().is_empty());
                controls::retry(&before.job_id, request(&recovered)).unwrap();
                child(&home, json!({"work":"recover","completed":1}), 0);
            }
            let completed = engine::job_get(&before.job_id).unwrap();
            assert_eq!(completed.state, jobs::JobState::Completed, "{completed:?}");
            let counts: (u64, u64, u64) = conn
                .query_row(
                    "SELECT (SELECT COUNT(*) FROM entries),(SELECT COUNT(*) FROM revisions),
                 (SELECT COUNT(*) FROM receipts WHERE phase='committed')",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .unwrap();
            assert_eq!(counts, (1, 1, 1));
            let stable = receipts(&conn, &before.job_id);
            child(&home, json!({"work":"recover","completed":0}), 0);
            assert_eq!(receipts(&conn, &before.job_id), stable);
        }
    }
}

#[test]
fn undo_quit_before_content_commit_and_during_cleanup_never_resurrects() {
    for point in &POINTS[14..19] {
        let home = IsolatedAppHome::acquire("public-undo-quit");
        let _reset = ResetRuntime;
        let completed = finish(&plan(&home, true), "undo-quit");
        let action = request(&completed);
        let conn = catalog::open_catalog().unwrap();
        let original_receipts = receipts(&conn, &completed.job_id);
        let import_id = completed.items[0].import_id.as_deref().unwrap();
        child(
            &home,
            json!({"work":"undo","point":point,"job":completed.job_id,"action":action}),
            EXIT_AT_BOUNDARY,
        );
        engine::reset_runtime();
        std::fs::rename(home.path.join("source"), home.path.join("offline-source")).unwrap();
        let accepted = *point != "before_undo_commit";
        assert_eq!(recorded(&conn, &action), accepted);
        assert_eq!(
            catalog::get_entry(&conn, import_id)
                .unwrap()
                .unwrap()
                .hidden,
            accepted
        );
        child(&home, json!({"work":"recover","completed":0}), 0);
        if !accepted {
            assert_eq!(
                engine::job_get(&completed.job_id).unwrap().items[0].phase,
                "committed"
            );
            assert!(
                !catalog::get_entry(&conn, import_id)
                    .unwrap()
                    .unwrap()
                    .hidden
            );
            assert_eq!(crate::store::load_projects_snapshot().unwrap().len(), 1);
        }
        let undone = controls::undo(&completed.job_id, action.clone()).unwrap();
        assert_eq!(undone.items[0].phase, "undone", "{point}: {undone:?}");
        assert!(undone.items[0].error.is_none(), "{point}: {undone:?}");
        assert_eq!(undone.state_version, completed.state_version + 1);
        assert!(crate::store::load_projects_snapshot().unwrap().is_empty());
        let entry = catalog::get_entry(&conn, import_id).unwrap().unwrap();
        assert!(entry.hidden);
        assert!(catalog::is_tombstoned(
            &conn,
            &entry.installation_id,
            &entry.provider,
            &entry.native_session_id
        )
        .unwrap());
        assert_eq!(receipts(&conn, &completed.job_id), original_receipts);
        let pending: u64 = conn
            .query_row(
                "SELECT COUNT(*) FROM meta WHERE key GLOB 'job_action_cleanup_pending:v1:*'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(pending, 0);
        assert!(catalog::revision_dir(
            import_id,
            completed.items[0].revision_id.as_deref().unwrap()
        )
        .unwrap()
        .is_dir());
        child(&home, json!({"work":"recover","completed":0}), 0);
        let replay = controls::undo(&completed.job_id, action).unwrap();
        assert_eq!(
            serde_json::to_value(&replay).unwrap(),
            serde_json::to_value(&undone).unwrap()
        );
        assert!(
            catalog::get_entry(&conn, import_id)
                .unwrap()
                .unwrap()
                .hidden
        );
        assert!(home.path.join("workspace").is_dir());
    }
}
