//! Real public Undo writer, durable cleanup failures, restart and exact replay.
use super::*;

#[path = "job_action_undo_recovery_tests.rs"]
mod recovery;

fn projects() -> Vec<u8> {
    std::fs::read(crate::paths::projects_file()).unwrap()
}

fn fail_completion(conn: &Connection) {
    conn.execute_batch(
        "CREATE TRIGGER lose_undo_cleanup_reply AFTER INSERT ON meta
        WHEN NEW.key LIKE 'job_action_cleanup_done:v1:%'
        BEGIN SELECT RAISE(ABORT,'synthetic cleanup completion failure'); END;",
    )
    .unwrap();
}

fn cleanup_key(action: &controls::JobActionRequest, done: bool) -> String {
    format!(
        "job_action_cleanup{}:v1:{}:0",
        if done { "_done" } else { "" },
        action.request_id
    )
}

#[test]
fn public_undo_create_replays_without_version_bump_or_another_content_rollback() {
    let home = IsolatedAppHome::acquire("public-undo-create");
    let _reset = ResetRuntime;
    let completed = finish(&plan(&home, true), "public-undo-create");
    let conn = catalog::open_catalog().unwrap();
    let action = request(&completed);
    let undone = controls::undo(&completed.job_id, action.clone()).unwrap();
    assert_eq!(undone.items[0].phase, "undone");
    assert!(undone.items[0].error.is_none(), "{undone:?}");
    assert_eq!(undone.state_version, completed.state_version + 1);
    assert!(crate::store::load_projects_snapshot().unwrap().is_empty());
    assert!(home.path.join("workspace").is_dir());
    engine::reset_runtime();
    std::fs::rename(home.path.join("source"), home.path.join("offline-source")).unwrap();
    for _ in 0..2 {
        let replay = unchanged_replay(&conn, || controls::undo(&completed.job_id, action.clone()));
        assert_eq!(
            serde_json::to_value(replay).unwrap(),
            serde_json::to_value(&undone).unwrap()
        );
    }
    refuse_unchanged(&conn, ErrorCode::JobActionConflict, || {
        controls::retry(&completed.job_id, action.clone())
    });
    let mut stale = action;
    stale.request_id = uuid::Uuid::new_v4().to_string();
    refuse_unchanged(&conn, ErrorCode::JobStateChanged, || {
        controls::undo(&completed.job_id, stale)
    });
}

#[test]
fn public_undo_native_effect_before_lost_completion_resumes_original_pending_intent() {
    let home = IsolatedAppHome::acquire("public-undo-native-loss");
    let _reset = ResetRuntime;
    let completed = finish(&plan(&home, true), "public-undo-native-loss");
    let conn = catalog::open_catalog().unwrap();
    let action = request(&completed);
    fail_completion(&conn);
    let error = controls::undo(&completed.job_id, action.clone()).unwrap_err();
    assert_eq!(error.code, ErrorCode::SnapshotUnavailable);
    let content_done = engine::job_get(&completed.job_id).unwrap();
    assert_eq!(content_done.items[0].phase, "undone");
    assert_eq!(content_done.state_version, completed.state_version + 1);
    assert!(crate::store::load_projects_snapshot().unwrap().is_empty());
    assert!(conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM meta WHERE key=?1)",
            [key(&action)],
            |r| r.get::<_, bool>(0)
        )
        .unwrap());
    assert!(conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM meta WHERE key=?1)",
            [cleanup_key(&action, false)],
            |r| r.get::<_, bool>(0)
        )
        .unwrap());
    assert!(!conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM meta WHERE key=?1)",
            [cleanup_key(&action, true)],
            |r| r.get::<_, bool>(0)
        )
        .unwrap());
    conn.execute_batch("DROP TRIGGER lose_undo_cleanup_reply")
        .unwrap();
    let entry = content_done.items[0].import_id.as_deref().unwrap();
    let hidden = catalog::get_entry(&conn, entry).unwrap().unwrap();
    engine::reset_runtime();
    std::fs::rename(home.path.join("source"), home.path.join("offline-source")).unwrap();
    let resumed = controls::undo(&completed.job_id, action.clone()).unwrap();
    assert_eq!(resumed.state_version, content_done.state_version);
    assert_eq!(
        serde_json::to_value(catalog::get_entry(&conn, entry).unwrap().unwrap()).unwrap(),
        serde_json::to_value(hidden).unwrap()
    );
    assert!(resumed.items[0].error.is_none());
    unchanged_replay(&conn, || controls::undo(&completed.job_id, action));
}

#[test]
fn public_undo_pending_cleanup_refuses_corruption_and_preserves_later_user_project_changes() {
    let home = IsolatedAppHome::acquire("public-undo-pending-user-edit");
    let _reset = ResetRuntime;
    let completed = finish(&plan(&home, true), "public-undo-pending-user-edit");
    let project = completed.items[0].mapping_project_id.as_deref().unwrap();
    crate::store::rename_project(project, "USER_KEPT_PROJECT").unwrap();
    let conn = catalog::open_catalog().unwrap();
    let action = request(&completed);
    fail_completion(&conn);
    assert_eq!(
        controls::undo(&completed.job_id, action.clone())
            .unwrap_err()
            .code,
        ErrorCode::SnapshotUnavailable
    );
    let original_projects = projects();
    let original: String = conn
        .query_row(
            "SELECT value FROM meta WHERE key=?1",
            [cleanup_key(&action, false)],
            |row| row.get(0),
        )
        .unwrap();
    for sql in [
        "DELETE FROM meta WHERE key=?1",
        "UPDATE meta SET value=CAST(zeroblob(1048576) AS TEXT) WHERE key=?1",
        "UPDATE meta SET value=CAST(x'ff' AS TEXT) WHERE key=?1",
    ] {
        conn.execute(sql, [cleanup_key(&action, false)]).unwrap();
        refuse_unchanged(&conn, ErrorCode::HistoryAccessUnavailable, || {
            controls::undo(&completed.job_id, action.clone())
        });
        assert_eq!(projects(), original_projects);
        conn.execute("INSERT INTO meta(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value", params![cleanup_key(&action, false), original]).unwrap();
    }
    let receipt = jobs::latest_receipt(&conn, &completed.job_id, &completed.items[0].candidate_id)
        .unwrap()
        .unwrap();
    let receipt_raw: String = conn
        .query_row(
            "SELECT json FROM receipts WHERE receipt_id=?1",
            [&receipt.receipt_id],
            |r| r.get(0),
        )
        .unwrap();
    conn.execute("UPDATE receipts SET json=json_set(json,'$.mappingProjectId','foreign') WHERE receipt_id=?1", [&receipt.receipt_id]).unwrap();
    refuse_unchanged(&conn, ErrorCode::HistoryAccessUnavailable, || {
        controls::undo(&completed.job_id, action.clone())
    });
    assert_eq!(projects(), original_projects);
    conn.execute(
        "UPDATE receipts SET json=?2 WHERE receipt_id=?1",
        params![receipt.receipt_id, receipt_raw],
    )
    .unwrap();
    conn.execute_batch("DROP TRIGGER lose_undo_cleanup_reply")
        .unwrap();
    engine::reset_runtime();
    let resumed = controls::undo(&completed.job_id, action.clone()).unwrap();
    assert!(resumed.items[0]
        .error
        .as_deref()
        .unwrap()
        .contains("project was modified by the user"));
    assert_eq!(projects(), original_projects);
    unchanged_replay(&conn, || controls::undo(&completed.job_id, action));
    assert!(home.path.join("workspace").is_dir());
}

#[test]
fn public_undo_late_request_journal_fault_rolls_back_all_mixed_content() {
    let home = IsolatedAppHome::acquire("public-undo-late-journal");
    let _reset = ResetRuntime;
    let completed = finish(
        &super::super::mixed_undo::mixed_plan(&home),
        "public-undo-late-journal",
    );
    let conn = catalog::open_catalog().unwrap();
    let action = request(&completed);
    conn.execute_batch(
        "CREATE TRIGGER refuse_undo_journal AFTER INSERT ON meta
        WHEN NEW.key LIKE 'job_action:v1:%'
        BEGIN SELECT RAISE(ABORT,'synthetic late Undo journal failure'); END;",
    )
    .unwrap();
    refuse_unchanged(&conn, ErrorCode::SnapshotUnavailable, || {
        controls::undo(&completed.job_id, action.clone())
    });
    conn.execute_batch("DROP TRIGGER refuse_undo_journal")
        .unwrap();
    let undone = controls::undo(&completed.job_id, action.clone()).unwrap();
    assert!(undone.items.iter().all(|item| item.phase == "undone"));
    unchanged_replay(&conn, || controls::undo(&completed.job_id, action));
}

#[test]
fn public_legacy_undo_keeps_unproven_native_project_and_durable_note_on_replay() {
    let home = IsolatedAppHome::acquire("public-undo-legacy");
    let _reset = ResetRuntime;
    let planned = plan(&home, true);
    let queued = engine::enqueue_legacy_plan(&planned, "public-undo-legacy");
    assert_eq!(job_control::run_pending().unwrap(), 1);
    let completed = engine::job_get(&queued.job_id).unwrap();
    assert_eq!(completed.state, jobs::JobState::Completed);
    let conn = catalog::open_catalog().unwrap();
    let before = projects();
    let action = request(&completed);
    let undone = controls::undo(&completed.job_id, action.clone()).unwrap();
    assert_eq!(undone.items[0].phase, "undone");
    assert!(undone.items[0]
        .error
        .as_deref()
        .unwrap()
        .contains("provenance unavailable"));
    assert_eq!(projects(), before);
    let replay = unchanged_replay(&conn, || controls::undo(&completed.job_id, action));
    assert_eq!(replay.state_version, undone.state_version);
    assert_eq!(
        PublicationProtocol::for_job_id(&queued.job_id).unwrap(),
        PublicationProtocol::Legacy
    );
}
