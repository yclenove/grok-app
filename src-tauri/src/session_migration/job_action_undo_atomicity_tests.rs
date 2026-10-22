//! Refused content writers and cleanup finishers preserve every raw catalog cell.
//! Fixtures publish through real registered sources; SQL only injects faults.
use super::*;

fn refuse_both(conn: &Connection, completed: &jobs::JobRecord) {
    assert!(jobs::item_store::uses_rows(conn, &completed.job_id).unwrap());
    let action = request(completed);
    for _ in 0..2 {
        refuse_unchanged(conn, ErrorCode::HistoryAccessUnavailable, || {
            jobs::undo_job(conn, &completed.job_id)
        });
        refuse_unchanged(conn, ErrorCode::HistoryAccessUnavailable, || {
            controls::undo(&completed.job_id, action.clone())
        });
        engine::reset_runtime();
    }
}

#[test]
fn rows_v1_missing_receipt_refuses_undo_without_changing_display_bytes() {
    let home = IsolatedAppHome::acquire("rows-undo-missing-receipt");
    let _reset = ResetRuntime;
    let completed = finish(
        &super::super::super::mixed_undo::mixed_plan(&home),
        "rows-undo-missing-receipt",
    );
    let conn = catalog::open_catalog().unwrap();
    assert_eq!(completed.items.len(), 2);
    assert!(completed.items.iter().all(|item| item.phase == "committed"));
    let deleted = conn
        .execute(
            "DELETE FROM receipts WHERE job_id=?1 AND candidate_id=?2",
            params![completed.job_id, completed.items[1].candidate_id],
        )
        .unwrap();
    assert!(deleted > 0);
    refuse_both(&conn, &completed);
}

#[test]
fn rows_v1_ownership_refusal_rolls_back_earlier_items_and_display_bytes() {
    for signed in [false, true] {
        let home = IsolatedAppHome::acquire("rows-undo-ownership");
        let _reset = ResetRuntime;
        let planned = super::super::super::mixed_undo::mixed_plan(&home);
        let completed = if signed {
            finish(&planned, "rows-undo-ownership")
        } else {
            let queued = engine::enqueue_legacy_plan(&planned, "rows-undo-ownership");
            assert_eq!(job_control::run_pending().unwrap(), 1);
            engine::job_get(&queued.job_id).unwrap()
        };
        assert_eq!(
            PublicationProtocol::for_job_id(&completed.job_id).unwrap(),
            if signed {
                PublicationProtocol::SignedOutcomeV1
            } else {
                PublicationProtocol::Legacy
            }
        );
        assert_eq!(completed.state, jobs::JobState::Completed);
        assert_eq!(completed.items.len(), 2);
        assert!(completed.items.iter().all(|item| item.phase == "committed"));
        let conn = catalog::open_catalog().unwrap();
        assert_eq!(
            conn.execute(
                "UPDATE receipts SET json=json_set(json,'$.mappingProjectId','foreign')
                 WHERE job_id=?1 AND candidate_id=?2 AND phase='committed'",
                params![completed.job_id, completed.items[1].candidate_id],
            )
            .unwrap(),
            1
        );
        // Legacy validates this receipt after changing the first target;
        // signed Undo captures all authenticated receipts before any write.
        refuse_both(&conn, &completed);
    }
}

#[test]
fn rows_v1_downgrade_header_ownership_refusal_preserves_display_bytes() {
    let home = IsolatedAppHome::acquire("rows-undo-downgrade");
    let _reset = ResetRuntime;
    let completed = finish(&plan(&home, true), "rows-undo-downgrade");
    let conn = catalog::open_catalog().unwrap();
    assert!(jobs::item_store::uses_rows(&conn, &completed.job_id).unwrap());
    let native_before = projects();
    let mut changed = completed.clone();
    changed.items[0].mapping_project_id = Some("foreign".into());
    conn.execute(
        "UPDATE jobs SET json=?2 WHERE job_id=?1",
        params![completed.job_id, serde_json::to_string(&changed).unwrap()],
    )
    .unwrap();
    let action = request(&completed);
    refuse_unchanged(&conn, ErrorCode::HistoryAccessUnavailable, || {
        jobs::undo_job(&conn, &completed.job_id)
    });
    refuse_unchanged(&conn, ErrorCode::HistoryAccessUnavailable, || {
        controls::undo(&completed.job_id, action)
    });
    assert_eq!(projects(), native_before);
}

#[test]
fn rows_v1_later_revision_conflict_and_undone_rows_roll_back_on_journal_refusal() {
    use std::io::Write;

    let home = IsolatedAppHome::acquire("rows-undo-later-revision");
    let _reset = ResetRuntime;
    let completed = finish(
        &super::super::super::mixed_undo::mixed_plan(&home),
        "rows-undo-later-revision",
    );
    let mut source = std::fs::OpenOptions::new()
        .append(true)
        .open(home.path.join("source/sessions/rollout.jsonl"))
        .unwrap();
    writeln!(
        source,
        "{}",
        json!({"type":"event_msg","payload":{"type":"user_message","message":"Later revision"}})
    )
    .unwrap();
    drop(source);
    engine::reset_runtime();
    let scanned = scan::scan_start(vec![(
        Provider::Codex,
        home.path.join("source").to_string_lossy().into(),
    )])
    .unwrap();
    let candidate = scanned
        .candidates
        .iter()
        .find(|candidate| candidate.native_session_id == "signed-normal-source")
        .unwrap();
    let preview = engine::preview(&candidate.candidate_id, Some(&candidate.source_revision))
        .unwrap()
        .0;
    let refreshed = finish(
        &engine::plan(vec![preview], vec![]).unwrap(),
        "rows-undo-new-revision",
    );
    let later = &refreshed.items[0];
    let original = completed
        .items
        .iter()
        .find(|item| item.import_id == later.import_id)
        .unwrap();
    assert_ne!(original.revision_id, later.revision_id);
    let conn = catalog::open_catalog().unwrap();
    assert!(jobs::item_store::uses_rows(&conn, &completed.job_id).unwrap());
    let action = request(&completed);
    for (body, code) in [
        (
            "SELECT RAISE(ABORT,'synthetic Undo journal refusal');",
            ErrorCode::SnapshotUnavailable,
        ),
        (
            "UPDATE job_item_states SET json=json_set(json,'$.title','foreign');",
            ErrorCode::HistoryAccessUnavailable,
        ),
    ] {
        conn.execute_batch(&format!(
            "CREATE TRIGGER refuse_rows_undo AFTER INSERT ON meta
             WHEN NEW.key LIKE 'job_action:v1:%' BEGIN {body} END;"
        ))
        .unwrap();
        // Both conflict and undone display phases have been saved by this
        // point. A refused journal must roll them back with the header/targets.
        refuse_unchanged(&conn, code, || {
            controls::undo(&completed.job_id, action.clone())
        });
        conn.execute_batch("DROP TRIGGER refuse_rows_undo").unwrap();
    }
    let undone = controls::undo(&completed.job_id, action.clone()).unwrap();
    assert_eq!(undone.state_version, completed.state_version + 1);
    for item in &undone.items {
        if item.import_id == later.import_id {
            assert_eq!(item.phase, "conflict");
            assert_eq!(item.error.as_deref(), Some("later revision kept"));
            assert_eq!(
                catalog::get_entry(&conn, item.import_id.as_deref().unwrap())
                    .unwrap()
                    .unwrap()
                    .current_revision,
                later.revision_id
            );
        } else {
            assert_eq!(item.phase, "undone");
        }
    }
    unchanged_replay(&conn, || controls::undo(&completed.job_id, action));
}

#[test]
fn rows_v1_cleanup_notes_and_completion_roll_back_together_on_resume_and_recovery() {
    let home = IsolatedAppHome::acquire("rows-undo-cleanup-atomicity");
    let _reset = ResetRuntime;
    let completed = finish(&plan(&home, true), "rows-undo-cleanup-atomicity");
    let conn = catalog::open_catalog().unwrap();
    assert!(jobs::item_store::uses_rows(&conn, &completed.job_id).unwrap());
    crate::store::rename_project(
        completed.items[0].mapping_project_id.as_deref().unwrap(),
        "USER_KEPT_PROJECT",
    )
    .unwrap();
    let native_before = projects();
    let action = request(&completed);
    fail_completion(&conn);
    assert_eq!(
        controls::undo(&completed.job_id, action.clone())
            .unwrap_err()
            .code,
        ErrorCode::SnapshotUnavailable
    );
    // Content Undo is already committed. The refused cleanup transaction must
    // preserve that exact state, including its still-empty display error note.
    let content_done = engine::job_get(&completed.job_id).unwrap();
    assert_eq!(content_done.items[0].phase, "undone");
    assert!(content_done.items[0].error.is_none());
    assert_eq!(content_done.state_version, completed.state_version + 1);
    let before = snapshot(&conn);
    refuse_unchanged(&conn, ErrorCode::SnapshotUnavailable, || {
        controls::undo(&completed.job_id, action.clone())
    });
    engine::reset_runtime();
    assert_eq!(
        controls::undo::recover(&conn).unwrap_err().code,
        ErrorCode::SnapshotUnavailable
    );
    assert_eq!(snapshot(&conn), before);
    assert_eq!(projects(), native_before);
    conn.execute_batch("DROP TRIGGER lose_undo_cleanup_reply")
        .unwrap();
    for point in [
        "AFTER INSERT ON meta WHEN NEW.key LIKE 'job_action_cleanup_done:v1:%'",
        "AFTER DELETE ON meta WHEN OLD.key LIKE 'job_action_cleanup_pending:v1:%'",
    ] {
        conn.execute_batch(&format!(
            "CREATE TRIGGER corrupt_cleanup_display {point}
             BEGIN UPDATE job_item_states SET json=json_set(json,'$.title','foreign'); END;"
        ))
        .unwrap();
        refuse_unchanged(&conn, ErrorCode::HistoryAccessUnavailable, || {
            controls::undo(&completed.job_id, action.clone())
        });
        conn.execute_batch("DROP TRIGGER corrupt_cleanup_display")
            .unwrap();
    }
    let resumed = controls::undo(&completed.job_id, action.clone()).unwrap();
    assert_eq!(resumed.state_version, content_done.state_version + 1);
    assert!(resumed.items[0]
        .error
        .as_deref()
        .unwrap()
        .contains("project was modified by the user"));
    assert_eq!(projects(), native_before);
    unchanged_replay(&conn, || controls::undo(&completed.job_id, action));
}
