//! A fresh native process recovers only the original committed cleanup. The
//! child inherits a synthetic home; it cannot run standalone on real data.
use super::*;

#[test]
#[ignore = "only the synthetic parent supplies the fixture home and expected job"]
fn public_undo_cleanup_recovery_child_process() {
    let expected_home =
        std::env::var_os("GROK_IMPORT_UNDO_RECOVERY_FIXTURE").expect("synthetic parent only");
    assert_eq!(
        std::env::var_os("GROK_APP_HOME").as_ref(),
        Some(&expected_home)
    );
    let job = std::env::var("GROK_IMPORT_UNDO_RECOVERY_JOB").unwrap();
    assert_eq!(job_control::run_pending().unwrap(), 0);
    let recovered = engine::job_get(&job).unwrap();
    assert_eq!(recovered.items[0].phase, "undone");
    assert!(recovered.items[0].error.is_none());
    assert!(crate::store::load_projects_snapshot().unwrap().is_empty());
    let conn = catalog::open_catalog().unwrap();
    assert_eq!(
        conn.query_row(
            "SELECT COUNT(*) FROM meta WHERE key GLOB 'job_action_cleanup_pending:v1:*'",
            [],
            |r| r.get::<_, u64>(0)
        )
        .unwrap(),
        0
    );
}

#[test]
fn public_undo_cleanup_resumes_in_fresh_process_without_frontend_request_memory() {
    let home = IsolatedAppHome::acquire("public-undo-fresh-process");
    let _reset = ResetRuntime;
    let completed = finish(&plan(&home, true), "public-undo-fresh-process");
    let conn = catalog::open_catalog().unwrap();
    let action = request(&completed);
    fail_completion(&conn);
    assert_eq!(
        controls::undo(&completed.job_id, action.clone())
            .unwrap_err()
            .code,
        ErrorCode::SnapshotUnavailable
    );
    let before = engine::job_get(&completed.job_id).unwrap();
    conn.execute_batch("DROP TRIGGER lose_undo_cleanup_reply")
        .unwrap();
    engine::reset_runtime();
    std::fs::rename(home.path.join("source"), home.path.join("offline-source")).unwrap();
    let qualified = concat!(
        module_path!(),
        "::public_undo_cleanup_recovery_child_process"
    );
    let child_test = qualified.split_once("::").unwrap().1;
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            child_test,
            "--exact",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("GROK_IMPORT_UNDO_RECOVERY_FIXTURE", &home.path)
        .env("GROK_IMPORT_UNDO_RECOVERY_JOB", &completed.job_id)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let pid = child.id();
    let output = child.wait_with_output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "synthetic recovery child PID {pid}: {stdout} {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(stdout.contains("1 passed; 0 failed; 0 ignored"), "{stdout}");
    println!(
        "synthetic Undo recovery child PID {pid} exited {}: {stdout}",
        output.status
    );
    let recovered = engine::job_get(&completed.job_id).unwrap();
    assert_eq!(recovered.state_version, before.state_version);
    unchanged_replay(&conn, || controls::undo(&completed.job_id, action));
    let stable = snapshot(&conn);
    assert_eq!(job_control::run_pending().unwrap(), 0);
    assert_eq!(snapshot(&conn), stable);
}

#[test]
fn public_undo_bad_pending_index_refuses_while_unrelated_signed_job_still_publishes() {
    let home = IsolatedAppHome::acquire("public-undo-recovery-isolation");
    let _reset = ResetRuntime;
    let completed = finish(&plan(&home, true), "public-undo-recovery-isolation");
    let conn = catalog::open_catalog().unwrap();
    let action = request(&completed);
    fail_completion(&conn);
    assert_eq!(
        controls::undo(&completed.job_id, action.clone())
            .unwrap_err()
            .code,
        ErrorCode::SnapshotUnavailable
    );
    conn.execute_batch("DROP TRIGGER lose_undo_cleanup_reply")
        .unwrap();
    let pending_key = format!("job_action_cleanup_pending:v1:{}", action.request_id);
    let original: String = conn
        .query_row("SELECT value FROM meta WHERE key=?1", [&pending_key], |r| {
            r.get(0)
        })
        .unwrap();
    conn.execute("UPDATE meta SET value='{}' WHERE key=?1", [&pending_key])
        .unwrap();
    let records = [
        json!({"type":"session_meta","payload":{"id":"healthy-control-recovery","cwd":home.path.join("workspace")}}),
        json!({"type":"event_msg","payload":{"type":"user_message","message":"Healthy recovery control"}}),
    ];
    std::fs::write(
        home.path.join("source/sessions/healthy.jsonl"),
        records
            .iter()
            .map(|row| format!("{row}\n"))
            .collect::<String>(),
    )
    .unwrap();
    let scan = scan::scan_start(vec![(
        Provider::Codex,
        home.path.join("source").to_string_lossy().into(),
    )])
    .unwrap();
    let preview = scan
        .candidates
        .iter()
        .find(|candidate| candidate.native_session_id == "healthy-control-recovery")
        .map(|candidate| {
            engine::preview(&candidate.candidate_id, Some(&candidate.source_revision))
                .unwrap()
                .0
        })
        .unwrap();
    let healthy_plan = engine::plan(vec![preview], vec![]).unwrap();
    let healthy = engine::commit(
        &healthy_plan.plan_id,
        &healthy_plan.plan_hash,
        "healthy-control-recovery",
    )
    .unwrap();
    let before = serde_json::to_value(engine::job_get(&completed.job_id).unwrap()).unwrap();
    assert_eq!(
        job_control::run_pending().unwrap_err().code,
        ErrorCode::HistoryAccessUnavailable
    );
    let published = engine::job_get(&healthy.job_id).unwrap();
    assert_eq!(published.state, jobs::JobState::Completed, "{published:?}");
    assert_eq!(published.items[0].phase, "committed");
    assert_eq!(
        serde_json::to_value(engine::job_get(&completed.job_id).unwrap()).unwrap(),
        before
    );
    assert_eq!(
        conn.query_row("SELECT value FROM meta WHERE key=?1", [&pending_key], |r| r
            .get::<_, String>(0))
            .unwrap(),
        "{}"
    );
    conn.execute(
        "UPDATE meta SET value=?2 WHERE key=?1",
        params![pending_key, original],
    )
    .unwrap();
    assert_eq!(job_control::run_pending().unwrap(), 0);
    unchanged_replay(&conn, || controls::undo(&completed.job_id, action));
}
