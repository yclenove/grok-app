//! Real signed producers and concrete late SQL faults, never positive seals.
use super::*;
use rusqlite::params;

fn input(conn: &rusqlite::Connection, job: &jobs::JobRecord) -> String {
    conn.query_row(
        "SELECT result_json FROM job_items WHERE job_id=?1 AND candidate_id=?2",
        params![job.job_id, job.items[0].candidate_id],
        |row| row.get(0),
    )
    .unwrap()
}

fn rows(conn: &rusqlite::Connection) -> (u64, u64) {
    conn.query_row(
        "SELECT (SELECT COUNT(*) FROM receipts),(SELECT COUNT(*) FROM revisions)",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )
    .unwrap()
}

#[test]
fn normal_signed_late_dedup_fault_rolls_back_ack_and_result_then_retries_offline() {
    let home = IsolatedAppHome::acquire("signed-late-dedup-fault");
    let _reset = ResetRuntime;
    let first = finish(&plan(&home, false), "late-dedup-first");
    let next = plan(&home, false);
    let queued = engine::commit(&next.plan_id, &next.plan_hash, "late-dedup-second").unwrap();
    let conn = catalog::open_catalog().unwrap();
    let original_input = input(&conn, &queued);
    let original_rows = rows(&conn);
    let entries = serde_json::to_value(catalog::list_entries(&conn).unwrap()).unwrap();
    let result_key = format!(
        "publication_outcome:v1:{}:{}:deduplicated",
        queued.job_id,
        crate::session_migration::hash::sha256_bytes(queued.items[0].candidate_id.as_bytes()),
    );
    // AFTER INSERT is later than ACK/display/result writes. RAISE(ABORT) must
    // roll the entire final transaction back, not leave an ACK without a seal.
    conn.execute_batch(&format!(
        "CREATE TRIGGER fail_late_dedup AFTER INSERT ON meta
         WHEN NEW.key='{result_key}'
         BEGIN SELECT RAISE(ABORT,'synthetic late dedup failure'); END;"
    ))
    .unwrap();
    assert_eq!(job_control::run_pending().unwrap(), 1);
    let failed = engine::job_get(&queued.job_id).unwrap();
    assert_eq!(
        failed.state,
        jobs::JobState::CompletedWithIssues,
        "{failed:?}"
    );
    assert_eq!(failed.items[0].phase, "failed");
    assert!(failed.items[0].import_id.is_none() && failed.items[0].revision_id.is_none());
    assert!(failed.items[0]
        .error
        .as_deref()
        .unwrap()
        .contains("synthetic late dedup failure"));
    assert_eq!(rows(&conn), original_rows);
    assert_eq!(input(&conn, &failed), original_input);
    assert_eq!(
        serde_json::to_value(catalog::list_entries(&conn).unwrap()).unwrap(),
        entries
    );
    assert!(!conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM meta WHERE key=?1)",
            [&result_key],
            |row| row.get::<_, bool>(0)
        )
        .unwrap());
    let value: serde_json::Value = serde_json::from_str(&original_input).unwrap();
    assert!(std::path::Path::new(value["previewPath"].as_str().unwrap()).is_dir());
    conn.execute_batch("DROP TRIGGER fail_late_dedup").unwrap();
    engine::reset_runtime();
    std::fs::rename(home.path.join("source"), home.path.join("source-offline")).unwrap();
    engine::retry_failed(&queued.job_id).unwrap();
    assert_eq!(job_control::run_pending().unwrap(), 1);
    let recovered = engine::job_get(&queued.job_id).unwrap();
    assert_eq!(recovered.state, jobs::JobState::Completed, "{recovered:?}");
    assert_eq!(recovered.items[0].outcome.as_deref(), Some("deduplicated"));
    assert_eq!(recovered.items[0].import_id, first.items[0].import_id);
    assert_eq!(recovered.items[0].revision_id, first.items[0].revision_id);
    assert_eq!(rows(&conn), original_rows);
    assert_eq!(input(&conn, &recovered), original_input);
}

#[test]
fn normal_signed_completed_ack_cannot_be_replaced_with_a_legacy_replay_header() {
    let home = IsolatedAppHome::acquire("signed-replay-header-refusal");
    let _reset = ResetRuntime;
    let completed = finish(&plan(&home, false), "signed-replay-header");
    let conn = catalog::open_catalog().unwrap();
    let raw = input(&conn, &completed);
    let crypto = JobCrypto::prepare(&conn, &completed.job_id).unwrap();
    let mut stale = completed.clone();
    stale.state = jobs::JobState::Running;
    stale.items[0].phase = "running".into();
    stale.items[0].import_id = None;
    stale.items[0].revision_id = None;
    stale.items[0].outcome = None;
    jobs::save_job(&conn, &stale).unwrap();
    let header: String = conn
        .query_row(
            "SELECT json FROM jobs WHERE job_id=?1",
            [&completed.job_id],
            |row| row.get(0),
        )
        .unwrap();
    let original_rows = rows(&conn);
    let entries = serde_json::to_value(catalog::list_entries(&conn).unwrap()).unwrap();
    let changes = conn.total_changes();
    let error = job_control::inspect_recovery_input(&raw, |original| {
        crate::session_migration::publication_outcome::recovery::recover(
            &conn,
            &completed.job_id,
            0,
            &original,
            &crypto,
        )
    })
    .unwrap()
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::PlanStale);
    assert_eq!(conn.total_changes(), changes);
    assert_eq!(rows(&conn), original_rows);
    assert_eq!(
        serde_json::to_value(catalog::list_entries(&conn).unwrap()).unwrap(),
        entries
    );
    assert_eq!(
        conn.query_row(
            "SELECT json FROM jobs WHERE job_id=?1",
            [&completed.job_id],
            |row| row.get::<_, String>(0)
        )
        .unwrap(),
        header
    );
}

#[test]
fn normal_signed_undo_keeps_created_project_referenced_by_saved_source_workspace() {
    let home = IsolatedAppHome::acquire("signed-undo-workspace-reference");
    let _reset = ResetRuntime;
    let completed = finish(&plan(&home, true), "signed-undo-workspace-reference");
    let conn = catalog::open_catalog().unwrap();
    let project = completed.items[0].mapping_project_id.as_ref().unwrap();
    let target = home.path.join("workspace");
    let mut workspace =
        crate::session_migration::workspace::source_workspace_from_folder(target.to_str().unwrap());
    workspace.mapping_choices = vec![crate::session_migration::workspace::WorkspaceRootChoice {
        root_id: workspace.roots[0].root_id.clone(),
        target_project_id: Some(project.clone()),
        explicit_target: None,
        primary: true,
    }];
    catalog::upsert_workspace(&conn, &workspace).unwrap();
    let projects_before = std::fs::read(crate::paths::projects_file()).unwrap();
    let undone = engine::undo_import_job(&completed.job_id).unwrap();
    assert_eq!(undone.items[0].phase, "undone", "{undone:?}");
    assert!(undone.items[0]
        .error
        .as_deref()
        .unwrap()
        .contains("saved imported workspace"));
    assert_eq!(
        std::fs::read(crate::paths::projects_file()).unwrap(),
        projects_before
    );
    assert!(target.is_dir());
    assert!(
        catalog::get_entry(&conn, completed.items[0].import_id.as_deref().unwrap())
            .unwrap()
            .unwrap()
            .hidden
    );
}
