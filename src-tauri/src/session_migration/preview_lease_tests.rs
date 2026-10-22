use super::*;
use crate::session_migration::errors::ErrorCode;
use crate::session_migration::jobs::{
    cleanup_unreferenced_preview_dirs, expire_preview_lease_file, find_preview_dir,
    preview_directory_scans, read_preview_lease, reset_preview_directory_scans,
    verify_disk_preview_integrity, write_preview_lease,
};
use std::io::Write;
use std::path::Path;
use std::time::SystemTime;

struct Home {
    path: PathBuf,
    _lock: crate::session_migration::test_home::AppHomeEnvGuard,
}

impl Home {
    fn new() -> Self {
        let lock = crate::session_migration::test_home::AppHomeEnvGuard::acquire();
        let path = std::env::temp_dir().join(format!("preview-lease-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&path).unwrap();
        std::env::set_var("GROK_APP_HOME", &path);
        let _ = crate::paths::ensure_app_dirs();
        Self { path, _lock: lock }
    }

    fn dir(&self, name: &str) -> PathBuf {
        let dir = self.path.join(name);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        reset_runtime();
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn restore_mtime(path: &Path, when: SystemTime) {
    std::fs::OpenOptions::new()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(when)
        .unwrap();
}

fn first_candidate(provider: Provider, dir: &Path) -> SourceCandidate {
    let scan = scan_start(vec![(provider, dir.to_string_lossy().into())]).unwrap();
    scan.candidates
        .into_iter()
        .next()
        .expect("scan produced a candidate")
}

#[test]
fn preview_lookup_does_not_scan_the_staging_directory() {
    let home = Home::new();
    let dir = home.dir("files");
    std::fs::write(
        dir.join("chat.json"),
        r#"[{"role":"user","content":"direct lookup"}]"#,
    )
    .unwrap();
    let candidate = first_candidate(Provider::GenericFile, &dir);
    let (preview_id, _) =
        preview(&candidate.candidate_id, Some(&candidate.source_revision)).unwrap();

    reset_preview_directory_scans();
    assert!(find_preview_dir(&preview_id).unwrap().is_some());
    assert_eq!(preview_directory_scans(), 0);
}

#[test]
fn preview_lookup_rejects_invalid_ids_and_non_directory_targets() {
    let _home = Home::new();

    let invalid = find_preview_dir("not-a-preview-id").unwrap_err();
    assert_eq!(invalid.code, ErrorCode::PlanStale);
    assert!(invalid.message.contains("invalid"), "{}", invalid.message);

    let non_canonical = find_preview_dir("00000000-0000-4000-8000-0000000000AA").unwrap_err();
    assert_eq!(non_canonical.code, ErrorCode::PlanStale);
    assert!(
        non_canonical.message.contains("non-canonical"),
        "{}",
        non_canonical.message
    );

    let parent = crate::session_migration::catalog::staging_dir().join("previews");
    std::fs::create_dir_all(&parent).unwrap();
    let file_id = uuid::Uuid::new_v4().to_string();
    std::fs::write(parent.join(&file_id), b"not a preview directory").unwrap();
    let file_error = find_preview_dir(&file_id).unwrap_err();
    assert_eq!(file_error.code, ErrorCode::SnapshotUnavailable);

    let target = parent.join("symlink-target");
    std::fs::create_dir_all(&target).unwrap();
    let link_id = uuid::Uuid::new_v4().to_string();
    let link = parent.join(&link_id);
    #[cfg(unix)]
    let link_result = std::os::unix::fs::symlink(&target, &link);
    #[cfg(windows)]
    let link_result = std::os::windows::fs::symlink_dir(&target, &link);
    match link_result {
        Ok(()) => {
            let link_error = find_preview_dir(&link_id).unwrap_err();
            assert_eq!(link_error.code, ErrorCode::SnapshotUnavailable);
        }
        #[cfg(windows)]
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::Unsupported
            ) => {}
        Err(error) => panic!("failed to create test symlink: {error}"),
    }
}

#[test]
fn same_mtime_rewrite_is_source_changed() {
    let home = Home::new();
    let dir = home.dir("files");
    let path = dir.join("chat.json");
    let before = r#"[{"role":"user","content":"before-rewrite"}]"#;
    let after = r#"[{"role":"user","content":"after--rewrite"}]"#;
    assert_eq!(before.len(), after.len());
    std::fs::write(&path, before).unwrap();
    let mtime = std::fs::metadata(&path).unwrap().modified().unwrap();
    let cand = first_candidate(Provider::GenericFile, &dir);
    preview(&cand.candidate_id, Some(&cand.source_revision)).unwrap();
    std::fs::write(&path, after).unwrap();
    restore_mtime(&path, mtime);
    assert_eq!(std::fs::metadata(&path).unwrap().modified().unwrap(), mtime);
    let err = preview(&cand.candidate_id, Some(&cand.source_revision)).unwrap_err();
    assert_eq!(err.code, ErrorCode::SourceChanged);
}

#[test]
fn jsonl_half_line_append_is_detected_and_open_preview_stays_frozen() {
    let home = Home::new();
    let dir = home.dir("codex");
    let path = dir.join("rollout.jsonl");
    let body = concat!(
        r#"{"type":"session_meta","payload":{"id":"s1","cwd":"C:\\r"}}"#,
        "\n",
        r#"{"type":"event_msg","payload":{"type":"user_message","message":"hello-prefix"}}"#,
        "\n"
    );
    std::fs::write(&path, body).unwrap();
    let cand = first_candidate(Provider::Codex, &dir);
    let (pid, _) = preview(&cand.candidate_id, Some(&cand.source_revision)).unwrap();
    let (page, _) = preview_events_page(&pid, None, Some(20), None).unwrap();
    let frozen: Vec<String> = page.iter().map(|e| e.text.clone()).collect();
    assert!(frozen.iter().any(|t| t.contains("hello-prefix")));
    std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(
            b"{\"type\":\"event_msg\",\"payload\":{\"type\":\"user_message\",\"message\":\"half",
        )
        .unwrap();
    let still = preview_events_page(&pid, None, Some(20), None).unwrap().0;
    assert_eq!(
        still.iter().map(|e| e.text.as_str()).collect::<Vec<_>>(),
        frozen.iter().map(|s| s.as_str()).collect::<Vec<_>>()
    );
    let err = preview(&cand.candidate_id, Some(&cand.source_revision)).unwrap_err();
    assert_eq!(err.code, ErrorCode::SourceChanged);
}

#[test]
fn rewrite_during_open_preview_does_not_mutate_snapshot() {
    let home = Home::new();
    let dir = home.dir("files");
    let path = dir.join("chat.json");
    std::fs::write(&path, r#"[{"role":"user","content":"frozen-original"}]"#).unwrap();
    let cand = first_candidate(Provider::GenericFile, &dir);
    let (pid, _) = preview(&cand.candidate_id, Some(&cand.source_revision)).unwrap();
    std::fs::write(&path, r#"[{"role":"user","content":"live-rewrite"}]"#).unwrap();
    let (page, _) = preview_events_page(&pid, None, Some(10), None).unwrap();
    assert!(page.iter().any(|e| e.text.contains("frozen-original")));
    assert!(page.iter().all(|e| !e.text.contains("live-rewrite")));
}

#[test]
fn expired_preview_is_rejected_and_unreferenced_files_are_removed() {
    let home = Home::new();
    let dir = home.dir("files");
    std::fs::write(
        dir.join("chat.json"),
        r#"[{"role":"user","content":"ttl"}]"#,
    )
    .unwrap();
    let cand = first_candidate(Provider::GenericFile, &dir);
    let (pid, _) = preview(&cand.candidate_id, Some(&cand.source_revision)).unwrap();
    let snap = find_preview_dir(&pid).unwrap().expect("lease on disk");
    expire_preview(&pid);
    let err = preview_events_page(&pid, None, Some(10), None).unwrap_err();
    assert_eq!(err.code, ErrorCode::PlanStale);
    let _ = cleanup_preview_leases().unwrap();
    assert!(
        !snap.exists(),
        "expired unreferenced preview dir must be removed"
    );
}

#[test]
fn job_held_lease_survives_ttl_and_restart_cleanup() {
    use crate::session_migration::job_control::run_pending;
    use crate::session_migration::jobs::{get_job, JobState};
    let home = Home::new();
    let dir = home.dir("files");
    std::fs::write(
        dir.join("chat.json"),
        r#"[{"role":"user","content":"held-by-job"}]"#,
    )
    .unwrap();
    let cand = first_candidate(Provider::GenericFile, &dir);
    let (pid, _) = preview(&cand.candidate_id, Some(&cand.source_revision)).unwrap();
    let plan = plan(vec![pid.clone()], vec![]).unwrap();
    let job = commit(&plan.plan_id, &plan.plan_hash, "lease-req").unwrap();
    let snap = find_preview_dir(&pid).unwrap().expect("leased snapshot");
    expire_preview(&pid);
    assert_eq!(
        preview_events_page(&pid, None, Some(10), None)
            .unwrap_err()
            .code,
        ErrorCode::PlanStale
    );
    reset_runtime();
    assert_eq!(cleanup_unreferenced_preview_dirs().unwrap(), 0);
    assert!(snap.exists());
    let lease = read_preview_lease(&snap).unwrap().unwrap();
    assert!(lease.retained_by_job);
    assert_eq!(run_pending().unwrap(), 1);
    let conn = open_catalog().unwrap();
    let done = get_job(&conn, &job.job_id).unwrap().unwrap();
    assert_eq!(done.state, JobState::Completed);
    assert!(
        !snap.exists(),
        "a completed job must release its retained preview snapshot"
    );
}

#[test]
fn commit_uses_disk_lease_after_preview_leaves_memory() {
    use crate::session_migration::job_control::run_pending;
    use crate::session_migration::jobs::{get_job, JobState};
    let home = Home::new();
    let dir = home.dir("files");
    std::fs::write(
        dir.join("chat.json"),
        r#"[{"role":"user","content":"from-disk-lease"}]"#,
    )
    .unwrap();
    let cand = first_candidate(Provider::GenericFile, &dir);
    let (pid, _) = preview(&cand.candidate_id, Some(&cand.source_revision)).unwrap();
    let plan = plan(vec![pid.clone()], vec![]).unwrap();
    forget_preview(&pid);
    let job = commit(&plan.plan_id, &plan.plan_hash, "after-expire").unwrap();
    assert!(!job.job_id.is_empty());
    assert_eq!(run_pending().unwrap(), 1);
    let conn = open_catalog().unwrap();
    let done = get_job(&conn, &job.job_id).unwrap().unwrap();
    assert_eq!(done.state, JobState::Completed);
}

#[test]
fn deleted_source_is_not_an_empty_session() {
    let home = Home::new();
    let dir = home.dir("files");
    let path = dir.join("chat.json");
    std::fs::write(&path, r#"[{"role":"user","content":"soon-gone"}]"#).unwrap();
    let cand = first_candidate(Provider::GenericFile, &dir);
    std::fs::remove_file(&path).unwrap();
    let err = preview(&cand.candidate_id, Some(&cand.source_revision)).unwrap_err();
    assert!(
        matches!(
            err.code,
            ErrorCode::SourceNotFound | ErrorCode::SourceChanged | ErrorCode::SnapshotUnavailable
        ),
        "deleted source must not preview as an empty chat: {:?}",
        err
    );
}

#[test]
fn restart_keeps_unexpired_preview_without_ram() {
    let home = Home::new();
    let dir = home.dir("files");
    std::fs::write(
        dir.join("chat.json"),
        r#"[{"role":"user","content":"persist-me"}]"#,
    )
    .unwrap();
    let cand = first_candidate(Provider::GenericFile, &dir);
    let (pid, _) = preview(&cand.candidate_id, Some(&cand.source_revision)).unwrap();
    reset_runtime();
    let (page, _) = preview_events_page(&pid, None, Some(10), None).unwrap();
    assert!(page.iter().any(|e| e.text.contains("persist-me")));
}

#[test]
fn dropping_preview_with_unreadable_lease_preserves_recovery_input() {
    let home = Home::new();
    let dir = home.dir("files");
    std::fs::write(
        dir.join("chat.json"),
        r#"[{"role":"user","content":"keep-on-lease-error"}]"#,
    )
    .unwrap();
    let cand = first_candidate(Provider::GenericFile, &dir);
    let (pid, _) = preview(&cand.candidate_id, Some(&cand.source_revision)).unwrap();
    let path = find_preview_dir(&pid).unwrap().expect("lease on disk");
    std::fs::write(path.join("lease.json"), b"{truncated").unwrap();
    forget_preview(&pid);
    assert!(
        path.exists(),
        "lease read failure must not delete the preview"
    );
}

fn preview_then_forget(home: &Home) -> (String, PlanState) {
    let dir = home.dir("files");
    std::fs::write(
        dir.join("chat.json"),
        r#"[{"role":"user","content":"disk-lease"}]"#,
    )
    .unwrap();
    let cand = first_candidate(Provider::GenericFile, &dir);
    let (pid, _) = preview(&cand.candidate_id, Some(&cand.source_revision)).unwrap();
    let plan_state = plan(vec![pid.clone()], vec![]).unwrap();
    forget_preview(&pid);
    (pid, plan_state)
}

#[test]
fn disk_commit_rejects_expired_lease() {
    let home = Home::new();
    let (pid, plan_state) = preview_then_forget(&home);
    let snap = find_preview_dir(&pid).unwrap().expect("lease on disk");
    expire_preview_lease_file(&snap).unwrap();
    let err = commit(&plan_state.plan_id, &plan_state.plan_hash, "expired-disk").unwrap_err();
    assert_eq!(err.code, ErrorCode::PlanStale);
    assert!(err.message.contains("expired"), "{}", err.message);
}

#[test]
fn disk_commit_rejects_snapshot_digest_tamper() {
    let home = Home::new();
    let (pid, plan_state) = preview_then_forget(&home);
    let snap = find_preview_dir(&pid).unwrap().expect("lease on disk");
    let mut lease = read_preview_lease(&snap).unwrap().unwrap();
    lease.snapshot_digest = "deadbeef".into();
    write_preview_lease(&snap, &lease).unwrap();
    let err = commit(&plan_state.plan_id, &plan_state.plan_hash, "digest-tamper").unwrap_err();
    assert_eq!(err.code, ErrorCode::PlanStale);
    assert!(err.message.contains("digest"), "{}", err.message);
}

#[test]
fn disk_commit_rejects_file_hash_tamper() {
    let home = Home::new();
    let (pid, plan_state) = preview_then_forget(&home);
    let snap = find_preview_dir(&pid).unwrap().expect("lease on disk");
    std::fs::write(snap.join("events.jsonl"), b"{\"tampered\":true}\n").unwrap();
    let err = commit(&plan_state.plan_id, &plan_state.plan_hash, "hash-tamper").unwrap_err();
    assert_eq!(err.code, ErrorCode::PlanStale);
    assert!(err.message.contains("hash"), "{}", err.message);
}

#[test]
fn disk_lease_identity_mismatch_is_fail_closed() {
    let home = Home::new();
    let (pid, _) = preview_then_forget(&home);
    let snap = find_preview_dir(&pid).unwrap().expect("lease on disk");
    let mut lease = read_preview_lease(&snap).unwrap().unwrap();
    lease.preview_id = "00000000-0000-4000-8000-000000000099".into();
    std::fs::write(snap.join("lease.json"), serde_json::to_vec(&lease).unwrap()).unwrap();
    assert!(find_preview_dir(&pid).unwrap().is_none());
    let err = verify_disk_preview_integrity(&snap, &pid).unwrap_err();
    assert_eq!(err.code, ErrorCode::PlanStale);
    assert!(err.message.contains("identity"), "{}", err.message);
}

fn queued_expired_preview(home: &Home) -> (PathBuf, crate::session_migration::jobs::JobRecord) {
    let (pid, plan) = preview_then_forget(home);
    let job = commit(&plan.plan_id, &plan.plan_hash, "cleanup-recovery").unwrap();
    let path = find_preview_dir(&pid).unwrap().unwrap();
    expire_preview_lease_file(&path).unwrap();
    reset_runtime();
    (path, job)
}

fn finish_preserved_job(job: &crate::session_migration::jobs::JobRecord, path: &Path) {
    assert_eq!(
        crate::session_migration::job_control::run_pending().unwrap(),
        1
    );
    let conn = open_catalog().unwrap();
    let done = crate::session_migration::jobs::get_job(&conn, &job.job_id)
        .unwrap()
        .unwrap();
    assert_eq!(
        done.state,
        crate::session_migration::jobs::JobState::Completed
    );
    assert!(
        !path.exists(),
        "successful publication releases the snapshot"
    );
}

#[test]
fn cleanup_catalog_open_failure_preserves_queued_preview_until_recovery() {
    let home = Home::new();
    let (path, job) = queued_expired_preview(&home);
    let hashes = crate::session_migration::jobs::prepared_hashes(&path).unwrap();
    let conn = open_catalog().unwrap();
    conn.execute("UPDATE meta SET value='999' WHERE key='schema_version'", [])
        .unwrap();
    let result = cleanup_unreferenced_preview_dirs();
    assert!(
        path.exists(),
        "failed catalog read deleted recovery input: {result:?}"
    );
    assert_eq!(result.unwrap_err().code, ErrorCode::ImportSchemaTooNew);
    assert_eq!(
        crate::session_migration::jobs::prepared_hashes(&path).unwrap(),
        hashes
    );
    conn.execute(
        "UPDATE meta SET value=?1 WHERE key='schema_version'",
        [crate::session_migration::types::CATALOG_SCHEMA_VERSION.to_string()],
    )
    .unwrap();
    drop(conn);
    finish_preserved_job(&job, &path);
}

#[test]
fn cleanup_reference_query_failure_preserves_queued_preview_until_recovery() {
    let home = Home::new();
    let (path, job) = queued_expired_preview(&home);
    let conn = open_catalog().unwrap();
    conn.execute_batch("ALTER TABLE job_items RENAME TO unavailable_job_items;")
        .unwrap();
    let result = cleanup_unreferenced_preview_dirs();
    assert!(
        path.exists(),
        "failed reference query deleted recovery input: {result:?}"
    );
    assert_eq!(result.unwrap_err().code, ErrorCode::SnapshotUnavailable);
    conn.execute_batch("ALTER TABLE unavailable_job_items RENAME TO job_items;")
        .unwrap();
    drop(conn);
    finish_preserved_job(&job, &path);
}

#[test]
fn cleanup_missing_catalog_does_not_initialize_an_empty_replacement() {
    let home = Home::new();
    let (path, job) = queued_expired_preview(&home);
    let catalog = crate::session_migration::catalog::catalog_path();
    let backup = home.path.join("synthetic-catalog-backup.sqlite");
    std::fs::rename(&catalog, &backup).unwrap();
    let result = cleanup_unreferenced_preview_dirs();
    assert!(
        path.exists(),
        "missing catalog deleted recovery input: {result:?}"
    );
    assert_eq!(
        result.unwrap_err().code,
        ErrorCode::HistoryAccessUnavailable
    );
    assert!(
        !catalog.exists(),
        "cleanup must not create a replacement catalog"
    );
    std::fs::rename(backup, catalog).unwrap();
    finish_preserved_job(&job, &path);
}

#[test]
fn cleanup_invalid_durable_reference_preserves_queued_preview_until_recovery() {
    use rusqlite::types::Value;
    for invalid in [
        Value::Null,
        Value::Text("{truncated".into()),
        Value::Text("{}".into()),
        Value::Text(r#"{"previewPath":7}"#.into()),
        Value::Text(r#"{"previewPath":""}"#.into()),
        Value::Text(r#"{"previewPath":"relative-path"}"#.into()),
    ] {
        let home = Home::new();
        let (path, job) = queued_expired_preview(&home);
        let conn = open_catalog().unwrap();
        let original: String = conn
            .query_row(
                "SELECT result_json FROM job_items WHERE job_id=?1",
                [&job.job_id],
                |row| row.get(0),
            )
            .unwrap();
        conn.execute(
            "UPDATE job_items SET result_json=?1 WHERE job_id=?2",
            rusqlite::params![invalid, job.job_id],
        )
        .unwrap();
        let result = cleanup_unreferenced_preview_dirs();
        assert!(
            path.exists(),
            "invalid reference deleted recovery input: {result:?}"
        );
        assert_eq!(result.unwrap_err().code, ErrorCode::SnapshotUnavailable);
        conn.execute(
            "UPDATE job_items SET result_json=?1 WHERE job_id=?2",
            rusqlite::params![original, job.job_id],
        )
        .unwrap();
        drop(conn);
        finish_preserved_job(&job, &path);
    }
}

#[cfg(windows)]
#[test]
fn cleanup_delete_failure_is_reported_and_can_be_retried() {
    use std::os::windows::fs::OpenOptionsExt;
    let home = Home::new();
    let (pid, _) = preview_then_forget(&home);
    let path = find_preview_dir(&pid).unwrap().unwrap();
    expire_preview_lease_file(&path).unwrap();
    let lock_path = path.join("cleanup-lock");
    std::fs::write(&lock_path, b"synthetic locked file").unwrap();
    let locked = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(&lock_path)
        .unwrap();
    let result = cleanup_unreferenced_preview_dirs();
    drop(locked);
    assert_eq!(result.unwrap_err().code, ErrorCode::SnapshotUnavailable);
    assert!(path.exists());
    assert_eq!(cleanup_unreferenced_preview_dirs().unwrap(), 1);
    assert!(!path.exists());
}

fn home_contract_fixture() {
    let _fixture = Home::new();
}

crate::session_migration::test_home::fixture_home_contract_tests!(home_contract_fixture);
