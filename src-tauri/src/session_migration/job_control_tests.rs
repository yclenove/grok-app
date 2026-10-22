use super::catalog::{
    get_entry, imports_dir, is_tombstoned, list_entries, open_catalog, revision_dir,
    revision_inventory,
};
use super::errors::ErrorCode;
use super::job_control::{cancel, enqueue, retry, run_pending};
use super::jobs::{
    delete_imported, get_job, latest_receipt, publish_item, recover_incomplete, save_job, undo_job,
    CommitItem, JobItem, JobRecord, JobState,
};

struct Home {
    path: std::path::PathBuf,
    old: Option<std::ffi::OsString>,
    _lock: std::sync::MutexGuard<'static, ()>,
}

impl Home {
    fn new() -> Self {
        let lock = crate::paths::APP_HOME_ENV_LOCK
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let path = std::env::temp_dir().join(format!("import-jobs-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&path).unwrap();
        let old = std::env::var_os("GROK_APP_HOME");
        std::env::set_var("GROK_APP_HOME", &path);
        let _ = crate::paths::ensure_app_dirs();
        Self {
            path,
            old,
            _lock: lock,
        }
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        if let Some(value) = &self.old {
            std::env::set_var("GROK_APP_HOME", value);
        } else {
            std::env::remove_var("GROK_APP_HOME");
        }
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn input(text: &str) -> CommitItem {
    let bytes = serde_json::to_vec(&serde_json::json!([{"role":"user","content":text}])).unwrap();
    let adapter = super::adapters::generic::normalize_bytes(&bytes, "synthetic.json").unwrap();
    super::jobs::prepare_test_commit_item(CommitItem {
        candidate_id: "c1".into(),
        installation_id: "installation".into(),
        provider: "generic-file".into(),
        root_path: "offline-copy".into(),
        adapter,
        prepared: None,
        options: serde_json::json!({"rawRetention":"normalized-only"}),
        mapping_project_id: None,
        explicit_reimport: false,
        create_project: false,
        import_unassigned_anyway: false,
        explicit_target: None,
        identity_association: None,
        selection_set: Default::default(),
    })
}

fn input_named(candidate: &str, text: &str) -> CommitItem {
    let mut item = input(text);
    item.candidate_id = candidate.into();
    item.adapter.graph.native_session_id = candidate.into();
    super::jobs::prepare_test_commit_item(item)
}

fn job() -> JobRecord {
    JobRecord {
        job_id: uuid::Uuid::new_v4().to_string(),
        request_id: uuid::Uuid::new_v4().to_string(),
        plan_hash: "fixed-plan".into(),
        plan_id: "plan".into(),
        state: JobState::Queued,
        state_version: 1,
        items: vec![],
        created_at: chrono::Utc::now().to_rfc3339(),
        item_count: 0,
        next_item_cursor: None,
        progress: None,
    }
}

fn durable_json(
    conn: &rusqlite::Connection,
    job_id: &str,
    candidate_id: &str,
) -> serde_json::Value {
    let raw: String = conn
        .query_row(
            "SELECT result_json FROM job_items WHERE job_id=?1 AND candidate_id=?2",
            rusqlite::params![job_id, candidate_id],
            |row| row.get(0),
        )
        .unwrap();
    serde_json::from_str(&raw).unwrap()
}

fn replace_durable_json(
    conn: &rusqlite::Connection,
    job_id: &str,
    candidate_id: &str,
    value: &serde_json::Value,
) {
    conn.execute(
        "UPDATE job_items SET result_json=?3 WHERE job_id=?1 AND candidate_id=?2",
        rusqlite::params![job_id, candidate_id, serde_json::to_string(value).unwrap()],
    )
    .unwrap();
}

#[test]
fn durable_selection_is_required_strict_and_revalidated_on_reload() {
    let _home = Home::new();
    let conn = open_catalog().unwrap();
    let queued = enqueue(job(), vec![input("durable")]).unwrap();
    let original = durable_json(&conn, &queued.job_id, "c1");

    let mut missing = original.clone();
    missing.as_object_mut().unwrap().remove("selectionSet");
    replace_durable_json(&conn, &queued.job_id, "c1", &missing);
    assert!(super::job_control::load_input(&conn, &queued.job_id, "c1").is_err());

    let mut unknown = original.clone();
    unknown
        .as_object_mut()
        .unwrap()
        .insert("legacySelection".into(), serde_json::json!({}));
    replace_durable_json(&conn, &queued.job_id, "c1", &unknown);
    assert!(super::job_control::load_input(&conn, &queued.job_id, "c1").is_err());

    let mut bad_hash = original.clone();
    bad_hash["selectionHash"] = serde_json::json!("tampered");
    replace_durable_json(&conn, &queued.job_id, "c1", &bad_hash);
    assert_eq!(
        super::job_control::load_input(&conn, &queued.job_id, "c1")
            .unwrap_err()
            .code,
        ErrorCode::PlanStale
    );

    let mut duplicate = original.clone();
    let duplicate_id = duplicate["selectionSet"]["entries"][0]["selection"]["eventIds"][0].clone();
    duplicate["selectionSet"]["entries"][0]["selection"]["eventIds"]
        .as_array_mut()
        .unwrap()
        .push(duplicate_id);
    replace_durable_json(&conn, &queued.job_id, "c1", &duplicate);
    assert!(super::job_control::load_input(&conn, &queued.job_id, "c1").is_err());

    let mut empty = original.clone();
    empty["selectionSet"]["entries"] = serde_json::json!([]);
    replace_durable_json(&conn, &queued.job_id, "c1", &empty);
    assert!(super::job_control::load_input(&conn, &queued.job_id, "c1").is_err());

    let mut two = original.clone();
    let mut second = two["selectionSet"]["entries"][0].clone();
    second["previewId"] = serde_json::json!(uuid::Uuid::new_v4().to_string());
    two["selectionSet"]["entries"]
        .as_array_mut()
        .unwrap()
        .push(second);
    replace_durable_json(&conn, &queued.job_id, "c1", &two);
    assert!(super::job_control::load_input(&conn, &queued.job_id, "c1").is_err());

    let mut wrong_identity = original.clone();
    wrong_identity["candidateId"] = serde_json::json!("other-candidate");
    replace_durable_json(&conn, &queued.job_id, "c1", &wrong_identity);
    assert_eq!(
        super::job_control::load_input(&conn, &queued.job_id, "c1")
            .unwrap_err()
            .code,
        ErrorCode::PlanStale
    );

    replace_durable_json(&conn, &queued.job_id, "c1", &original);
    assert!(super::job_control::load_input(&conn, &queued.job_id, "c1").is_ok());
}

#[test]
fn job_page_hydrates_title_for_legacy_item_projection() {
    let _home = Home::new();
    let queued = enqueue(job(), vec![input("legacy title")]).unwrap();
    assert!(queued.items[0].title.is_some());

    // Simulate a pre-title job JSON while retaining its durable input row.
    let conn = open_catalog().unwrap();
    let mut legacy = get_job(&conn, &queued.job_id).unwrap().unwrap();
    legacy.items[0].title = None;
    let tx = conn.unchecked_transaction().unwrap();
    save_job(&tx, &legacy).unwrap();
    tx.commit().unwrap();

    let restored = crate::session_migration::engine::job_get(&queued.job_id).unwrap();
    assert!(restored.items[0].title.is_some());
}

#[test]
fn enqueue_returns_before_publication_and_recovers_without_runtime_previews() {
    let _home = Home::new();
    let conn = open_catalog().unwrap();
    let queued = enqueue(job(), vec![input("固定预览，重启后不读源")]).unwrap();
    assert_eq!(queued.state, JobState::Queued);
    assert!(list_entries(&conn).unwrap().is_empty());
    assert_eq!(run_pending().unwrap(), 1);
    let done = get_job(&conn, &queued.job_id).unwrap().unwrap();
    assert_eq!(done.state, JobState::Completed);
    let events = super::continuation::load_events(
        done.items[0].import_id.as_ref().unwrap(),
        done.items[0].revision_id.as_ref().unwrap(),
    )
    .unwrap();
    assert_eq!(events[0].searchable_text(), "固定预览，重启后不读源");
    assert_eq!(run_pending().unwrap(), 0);
}

#[test]
fn request_id_cannot_silently_accept_a_different_plan() {
    let _home = Home::new();
    let first = enqueue(job(), vec![input("one")]).unwrap();
    let same = enqueue(first.clone(), vec![input("one")]).unwrap();
    assert_eq!(same.job_id, first.job_id);
    let mut changed = first;
    changed.plan_hash = "other".into();
    let err = enqueue(changed, vec![input("two")]).unwrap_err();
    assert_eq!(err.code, super::errors::ErrorCode::PlanStale);
}

#[test]
fn cancelled_job_keeps_committed_items_and_retry_uses_same_inputs() {
    let _home = Home::new();
    let conn = open_catalog().unwrap();
    let first_input = input("first");
    let mut second_input = input("second");
    second_input.candidate_id = "c2".into();
    second_input.adapter.graph.native_session_id = "second".into();
    second_input = super::jobs::prepare_test_commit_item(second_input);
    let queued = enqueue(job(), vec![first_input.clone(), second_input]).unwrap();
    // Simulate the process stopping after one committed item, before job state.
    // Replay the durable preview lease, not a second in-memory copy.
    let durable = super::job_control::load_input(&conn, &queued.job_id, "c1").unwrap();
    let first = publish_item(&conn, &queued.job_id, &durable, &|| false).unwrap();
    cancel(&queued.job_id).unwrap();
    run_pending().unwrap();
    let stopped = get_job(&conn, &queued.job_id).unwrap().unwrap();
    assert_eq!(stopped.state, JobState::Cancelled);
    assert_eq!(stopped.items[0].revision_id, first.revision_id);
    assert_eq!(stopped.items[1].phase, "cancelled");
    assert_eq!(list_entries(&conn).unwrap().len(), 1);
    retry(&queued.job_id).unwrap();
    run_pending().unwrap();
    let done = get_job(&conn, &queued.job_id).unwrap().unwrap();
    assert_eq!(done.state, JobState::Completed);
    assert_eq!(done.items[0].revision_id, first.revision_id);
    assert_eq!(list_entries(&conn).unwrap().len(), 2);
}

#[test]
fn cancel_after_prepared_receipt_resumes_the_same_import() {
    let _home = Home::new();
    let conn = open_catalog().unwrap();
    let queued = enqueue(
        job(),
        vec![
            input_named("inflight", "cancel after the receipt"),
            input_named("waiting", "still queued when cancel lands"),
        ],
    )
    .unwrap();
    let job_id = queued.job_id.clone();
    super::preview_lifecycle_tests::test_publish_point("prepared", {
        let job_id = job_id.clone();
        move || {
            cancel(&job_id).unwrap();
        }
    });
    assert_eq!(run_pending().unwrap(), 1);
    let stopped = get_job(&conn, &job_id).unwrap().unwrap();
    assert_eq!(stopped.state, JobState::Cancelled);
    let inflight = stopped
        .items
        .iter()
        .find(|item| item.candidate_id == "inflight")
        .unwrap();
    assert_eq!(inflight.phase, "cancelled");
    assert_eq!(inflight.outcome.as_deref(), Some("cancelled"));
    assert!(inflight.error.is_none());
    let import_id = inflight
        .import_id
        .clone()
        .expect("prepared receipt already allocated an import id");
    let waiting = stopped
        .items
        .iter()
        .find(|item| item.candidate_id == "waiting")
        .unwrap();
    assert_eq!(waiting.phase, "cancelled");
    assert!(waiting.import_id.is_none());
    assert!(list_entries(&conn).unwrap().is_empty());

    let requeued = retry(&job_id).unwrap();
    assert_eq!(requeued.state, JobState::Queued);
    assert!(requeued.items.iter().all(|item| {
        item.phase == "queued"
            && item.import_id.is_none()
            && item.revision_id.is_none()
            && item.outcome.is_none()
            && item.error.is_none()
    }));
    assert_eq!(run_pending().unwrap(), 1);
    let done = get_job(&conn, &job_id).unwrap().unwrap();
    assert_eq!(done.state, JobState::Completed);
    let resumed = done
        .items
        .iter()
        .find(|item| item.candidate_id == "inflight")
        .unwrap();
    assert_eq!(resumed.phase, "committed");
    assert_eq!(resumed.outcome.as_deref(), Some("complete"));
    assert_eq!(resumed.import_id.as_deref(), Some(import_id.as_str()));
    assert!(resumed.error.is_none());
    let other = done
        .items
        .iter()
        .find(|item| item.candidate_id == "waiting")
        .unwrap();
    assert_eq!(other.phase, "committed");
    assert_eq!(other.outcome.as_deref(), Some("complete"));
    assert_ne!(other.import_id.as_deref(), Some(import_id.as_str()));
    let receipt = latest_receipt(&conn, &job_id, "inflight")
        .unwrap()
        .expect("resumed receipt");
    assert_eq!(receipt.phase, "committed");
    assert_eq!(receipt.import_id.as_deref(), Some(import_id.as_str()));
    let entries = list_entries(&conn).unwrap();
    assert_eq!(entries.len(), 2);
    assert!(entries.iter().any(|entry| entry.import_id == import_id));
}

#[test]
fn source_busy_cancel_check_is_retried_until_the_item_commits() {
    let _home = Home::new();
    let conn = open_catalog().unwrap();
    let queued = enqueue(job(), vec![input("busy cancel check")]).unwrap();
    let fault = super::job_control::CancelCheckFaultGuard::arm(
        super::errors::ImportError::new(ErrorCode::SourceBusy, "synthetic catalog lock"),
        1,
    );
    assert_eq!(run_pending().unwrap(), 1);
    fault.assert_consumed();
    let done = get_job(&conn, &queued.job_id).unwrap().unwrap();
    assert_eq!(done.state, JobState::Completed);
    assert_eq!(done.items[0].phase, "committed");
    assert_eq!(done.items[0].outcome.as_deref(), Some("complete"));
    assert!(done.items[0].error.is_none());
    assert_eq!(list_entries(&conn).unwrap().len(), 1);
}

#[test]
fn source_busy_cancel_checks_stop_after_the_attempt_cap() {
    let _home = Home::new();
    let conn = open_catalog().unwrap();
    let queued = enqueue(job(), vec![input("busy until the cap")]).unwrap();
    let fault = super::job_control::CancelCheckFaultGuard::arm(
        super::errors::ImportError::new(ErrorCode::SourceBusy, "synthetic catalog lock"),
        super::job_control::SOURCE_BUSY_ATTEMPTS,
    );
    assert_eq!(run_pending().unwrap(), 1);
    fault.assert_consumed();
    let failed = get_job(&conn, &queued.job_id).unwrap().unwrap();
    assert_eq!(failed.state, JobState::CompletedWithIssues);
    assert_eq!(failed.items[0].phase, "failed");
    assert_eq!(failed.items[0].outcome.as_deref(), Some("failed"));
    assert!(failed.items[0]
        .error
        .as_deref()
        .unwrap()
        .starts_with("SOURCE_BUSY:"));
    assert!(list_entries(&conn).unwrap().is_empty());
}

#[test]
fn history_fence_cancel_check_is_not_retried() {
    let _home = Home::new();
    let conn = open_catalog().unwrap();
    let queued = enqueue(job(), vec![input("fence stays terminal")]).unwrap();
    let fault = super::job_control::CancelCheckFaultGuard::arm(
        super::errors::ImportError::new(
            ErrorCode::HistoryAccessUnavailable,
            "another item's input or the signed acceptance changed inside this publication",
        ),
        1,
    );
    assert_eq!(run_pending().unwrap(), 1);
    fault.assert_consumed();
    let failed = get_job(&conn, &queued.job_id).unwrap().unwrap();
    assert_eq!(failed.state, JobState::CompletedWithIssues);
    assert_eq!(failed.items[0].phase, "failed");
    assert!(failed.items[0]
        .error
        .as_deref()
        .unwrap()
        .starts_with("HISTORY_ACCESS_UNAVAILABLE:"));
    assert!(list_entries(&conn).unwrap().is_empty());
    let receipts: i64 = conn
        .query_row("SELECT COUNT(*) FROM receipts", [], |row| row.get(0))
        .unwrap();
    assert_eq!(receipts, 0);
}

#[test]
fn cancel_before_worker_does_not_start_queued_items() {
    let _home = Home::new();
    let conn = open_catalog().unwrap();
    let mut second = input("second");
    second.candidate_id = "c2".into();
    second.adapter.graph.native_session_id = "second".into();
    second = super::jobs::prepare_test_commit_item(second);
    let queued = enqueue(job(), vec![input("first"), second]).unwrap();
    cancel(&queued.job_id).unwrap();
    run_pending().unwrap();
    let stopped = get_job(&conn, &queued.job_id).unwrap().unwrap();
    assert_eq!(stopped.state, JobState::Cancelled);
    assert!(
        stopped.items.iter().all(|i| i.phase == "cancelled"),
        "queued items must be cancelled without publishing: {:?}",
        stopped.items
    );
    assert!(list_entries(&conn).unwrap().is_empty());
}

#[test]
fn require_disk_space_rejects_too_small_volume() {
    let err = super::jobs::require_disk_space(0, 4096).unwrap_err();
    assert_eq!(err.code, super::errors::ErrorCode::DiskSpaceLow);
    assert!(super::jobs::require_disk_space(10 * 1024 * 1024, 100).is_err());
    assert!(super::jobs::require_disk_space(600 * 1024 * 1024, 100).is_ok());
}

#[test]
fn estimated_bytes_include_normalized_index_not_just_adapter_json() {
    let _home = Home::new();
    let item = input("estimate-me");
    let mut adapter = item.adapter.clone();
    let prepared = super::jobs::prepare_revision(&mut adapter, &item.options, None, None).unwrap();
    let mut with_preview = item;
    with_preview.prepared = Some(prepared.clone());
    let estimated = super::jobs::estimated_publish_bytes(&with_preview);
    let events = std::fs::metadata(prepared.path.join("events.jsonl"))
        .unwrap()
        .len();
    let index = std::fs::metadata(prepared.path.join("event-index.sqlite"))
        .map(|m| m.len())
        .unwrap_or(0);
    assert!(
        estimated >= events + index,
        "estimate {estimated} must cover events {events} + index {index}"
    );
}

#[test]
fn job_get_pages_ten_thousand_items() {
    let _home = Home::new();
    let mut stored = job();
    stored.items = (0..10_000)
        .map(|i| JobItem {
            candidate_id: format!("c{i}"),
            title: None,
            phase: "queued".into(),
            import_id: None,
            revision_id: None,
            outcome: None,
            error: None,
            mapping_project_id: None,
        })
        .collect();
    stored.item_count = stored.items.len() as u32;
    let mut conn = open_catalog().unwrap();
    let tx = conn.transaction().unwrap();
    save_job(&tx, &stored).unwrap();
    tx.commit().unwrap();
    crate::session_migration::jobs::take_job_item_deserializes();
    let page = crate::session_migration::engine::job_get(&stored.job_id).unwrap();
    assert_eq!(page.item_count, 10_000);
    assert_eq!(page.items.len(), 50);
    let parsed = crate::session_migration::jobs::take_job_item_deserializes();
    assert!(
        parsed <= 51,
        "paging 10k items must not deserialize the whole job blob, parsed {parsed}"
    );
    let next = page.next_item_cursor.clone().expect("more items");
    let page2 = crate::session_migration::engine::job_items_page(&stored.job_id, Some(&next), None)
        .unwrap();
    assert_eq!(page2.items.len(), 50);
    assert_ne!(page.items[0].candidate_id, page2.items[0].candidate_id);
    let past =
        crate::session_migration::engine::job_items_page(&stored.job_id, Some("10000"), None);
    assert!(past.is_err(), "bare numeric job cursors must fail closed");
}

#[test]
fn publish_item_does_not_create_git_metadata() {
    let _home = Home::new();
    let conn = open_catalog().unwrap();
    let queued = enqueue(job(), vec![input("no-git")]).unwrap();
    run_pending().unwrap();
    let done = get_job(&conn, &queued.job_id).unwrap().unwrap();
    let import_id = done.items[0].import_id.as_ref().unwrap();
    let rev = done.items[0].revision_id.as_ref().unwrap();
    let dest = revision_dir(import_id, rev).unwrap();
    assert!(dest.is_dir());
    assert!(!dest.join(".git").exists());
    assert!(!dest.join("HEAD").exists());
}

#[test]
fn every_publication_receipt_boundary_recovers_the_preallocated_revision() {
    let _home = Home::new();
    let conn = open_catalog().unwrap();
    for (index, phase) in ["prepared", "staged", "published", "linked", "committed"]
        .iter()
        .enumerate()
    {
        let mut item = input("same authorized snapshot");
        item.candidate_id = format!("fault-{index}");
        item.adapter.graph.native_session_id = format!("fault-{index}");
        item = super::jobs::prepare_test_commit_item(item);
        let queued = enqueue(job(), vec![item.clone()]).unwrap();
        // SQL trigger models failure at the durable receipt boundary. No source
        // data or production database is touched by this test.
        conn.execute_batch(&format!("CREATE TEMP TRIGGER crash_receipt BEFORE INSERT ON receipts WHEN NEW.phase='{phase}' BEGIN SELECT RAISE(ABORT,'injected fault'); END;")).unwrap();
        assert!(publish_item(&conn, &queued.job_id, &item, &|| false).is_err());
        let before = latest_receipt(&conn, &queued.job_id, &item.candidate_id).unwrap();
        assert_eq!(list_entries(&conn).unwrap().len(), index);
        conn.execute_batch("DROP TRIGGER crash_receipt").unwrap();
        run_pending().unwrap();
        let done = get_job(&conn, &queued.job_id).unwrap().unwrap();
        assert_eq!(done.state, JobState::Completed, "{phase}: {done:?}");
        if let Some(before) = before {
            assert_eq!(done.items[0].import_id, before.import_id, "{phase}");
            assert_eq!(done.items[0].revision_id, before.revision_id, "{phase}");
        }
        assert_eq!(list_entries(&conn).unwrap().len(), index + 1);
    }
}

#[test]
fn rename_without_published_receipt_reuses_preallocated_ids() {
    let _home = Home::new();
    let conn = open_catalog().unwrap();
    let item = input("rename-gap");
    let queued = enqueue(job(), vec![item.clone()]).unwrap();
    conn.execute_batch(
        "CREATE TEMP TRIGGER crash_published BEFORE INSERT ON receipts WHEN NEW.phase='published' BEGIN SELECT RAISE(ABORT,'injected fault'); END;",
    )
    .unwrap();
    assert!(publish_item(&conn, &queued.job_id, &item, &|| false).is_err());
    let staged = latest_receipt(&conn, &queued.job_id, &item.candidate_id)
        .unwrap()
        .expect("staged receipt");
    assert_eq!(staged.phase, "staged");
    let dest = revision_dir(
        staged.import_id.as_ref().unwrap(),
        staged.revision_id.as_ref().unwrap(),
    )
    .unwrap();
    assert!(
        dest.is_dir(),
        "rename must have finished before the receipt"
    );
    conn.execute_batch("DROP TRIGGER crash_published").unwrap();
    let again = publish_item(&conn, &queued.job_id, &item, &|| false).unwrap();
    assert_eq!(again.import_id, staged.import_id);
    assert_eq!(again.revision_id, staged.revision_id);
    assert_eq!(again.phase, "committed");
    assert_eq!(list_entries(&conn).unwrap().len(), 1);
}

#[test]
fn refresh_interrupt_current_revision_is_complete_old_or_complete_new() {
    let _home = Home::new();
    let conn = open_catalog().unwrap();
    let mut v1 = input("v1-complete");
    v1.candidate_id = "sess".into();
    v1.adapter.graph.native_session_id = "sess".into();
    v1 = super::jobs::prepare_test_commit_item(v1);
    let first = enqueue(job(), vec![v1]).unwrap();
    run_pending().unwrap();
    let first = get_job(&conn, &first.job_id).unwrap().unwrap();
    let import_id = first.items[0].import_id.clone().unwrap();
    let old_rev = first.items[0].revision_id.clone().unwrap();
    assert_eq!(
        get_entry(&conn, &import_id)
            .unwrap()
            .unwrap()
            .current_revision
            .as_deref(),
        Some(old_rev.as_str())
    );
    let old_dir = revision_dir(&import_id, &old_rev).unwrap();
    assert!(old_dir.join("events.jsonl").is_file());
    assert!(old_dir.join("selection.json").is_file());

    let mut v2 = input("v2-complete");
    v2.candidate_id = "sess-v2".into();
    v2.adapter.graph.native_session_id = "sess".into();
    v2 = super::jobs::prepare_test_commit_item(v2);
    let queued = enqueue(job(), vec![v2.clone()]).unwrap();
    conn.execute_batch(
        "CREATE TEMP TRIGGER crash_published BEFORE INSERT ON receipts WHEN NEW.phase='published' BEGIN SELECT RAISE(ABORT,'injected fault'); END;",
    )
    .unwrap();
    assert!(publish_item(&conn, &queued.job_id, &v2, &|| false).is_err());
    let interrupted = get_entry(&conn, &import_id).unwrap().unwrap();
    assert_eq!(
        interrupted.current_revision.as_deref(),
        Some(old_rev.as_str()),
        "catalog must keep the complete old revision while the new publish is incomplete"
    );
    assert_eq!(list_entries(&conn).unwrap().len(), 1);
    let rec = latest_receipt(&conn, &queued.job_id, "sess-v2")
        .unwrap()
        .expect("staged receipt");
    assert_ne!(rec.phase, "committed");
    conn.execute_batch("DROP TRIGGER crash_published").unwrap();
    run_pending().unwrap();
    let done = get_job(&conn, &queued.job_id).unwrap().unwrap();
    assert_eq!(done.state, JobState::Completed);
    let new_rev = done.items[0].revision_id.clone().unwrap();
    assert_eq!(done.items[0].import_id.as_deref(), Some(import_id.as_str()));
    assert_ne!(new_rev, old_rev);
    let recovered = get_entry(&conn, &import_id).unwrap().unwrap();
    assert_eq!(
        recovered.current_revision.as_deref(),
        Some(new_rev.as_str())
    );
    let new_dir = revision_dir(&import_id, &new_rev).unwrap();
    let old_text = std::fs::read_to_string(old_dir.join("events.jsonl")).unwrap();
    let new_text = std::fs::read_to_string(new_dir.join("events.jsonl")).unwrap();
    assert!(old_text.contains("v1-complete"), "{old_text}");
    assert!(new_text.contains("v2-complete"), "{new_text}");
    assert!(
        !old_text.contains("v2-complete"),
        "old revision must not splice new events"
    );
    assert!(
        !new_text.contains("v1-complete"),
        "new revision must not splice old events"
    );
    assert_eq!(list_entries(&conn).unwrap().len(), 1);
}

#[test]
fn linked_mapping_stays_frozen_when_retry_sends_a_different_project() {
    let _home = Home::new();
    for id in ["proj-a", "proj-b"] {
        let path = _home.path.join(id);
        std::fs::create_dir(&path).unwrap();
        crate::store::ensure_import_project(id, path.to_str().unwrap()).unwrap();
    }
    let conn = open_catalog().unwrap();
    let mut item = input("link-freeze");
    item.mapping_project_id = Some("proj-a".into());
    let queued = enqueue(job(), vec![item.clone()]).unwrap();
    conn.execute_batch(
        "CREATE TEMP TRIGGER crash_linked BEFORE INSERT ON receipts WHEN NEW.phase='linked' BEGIN SELECT RAISE(ABORT,'injected fault'); END;",
    )
    .unwrap();
    let interrupted = publish_item(&conn, &queued.job_id, &item, &|| false).unwrap_err();
    assert!(
        interrupted.message.contains("injected fault"),
        "{interrupted:?}"
    );
    conn.execute_batch("DROP TRIGGER crash_linked").unwrap();
    item.mapping_project_id = Some("proj-b".into());
    let done = publish_item(&conn, &queued.job_id, &item, &|| false).unwrap();
    assert_eq!(done.mapping_project_id.as_deref(), Some("proj-a"));
    let rec = latest_receipt(&conn, &queued.job_id, &item.candidate_id)
        .unwrap()
        .unwrap();
    assert_eq!(rec.mapping_project_id.as_deref(), Some("proj-a"));
    assert_eq!(rec.phase, "committed");
}

#[test]
fn missing_mapping_target_blocks_publication_but_keeps_the_frozen_request_recoverable() {
    let home = Home::new();
    let conn = open_catalog().unwrap();
    let mut item = input("missing-project-recovery");
    item.mapping_project_id = Some("original-target".into());
    let queued = enqueue(job(), vec![item.clone()]).unwrap();
    let blocked = publish_item(&conn, &queued.job_id, &item, &|| false).unwrap_err();
    assert_eq!(blocked.code, ErrorCode::MappingConflict);
    assert!(list_entries(&conn).unwrap().is_empty());
    let receipt = latest_receipt(&conn, &queued.job_id, &item.candidate_id)
        .unwrap()
        .unwrap();
    assert_eq!(receipt.phase, "published");
    assert_eq!(
        receipt.mapping_project_id.as_deref(),
        Some("original-target")
    );
    assert!(revision_dir(
        receipt.import_id.as_deref().unwrap(),
        receipt.revision_id.as_deref().unwrap()
    )
    .unwrap()
    .is_dir());
    let target = home.path.join("restored-project");
    std::fs::create_dir(&target).unwrap();
    crate::store::ensure_import_project("original-target", target.to_str().unwrap()).unwrap();
    let done = publish_item(&conn, &queued.job_id, &item, &|| false).unwrap();
    assert_eq!(done.phase, "committed");
    assert_eq!(done.mapping_project_id.as_deref(), Some("original-target"));
    assert_eq!(list_entries(&conn).unwrap().len(), 1);
}

#[test]
fn mid_publish_stop_is_not_complete_and_is_recoverable() {
    let _home = Home::new();
    let conn = open_catalog().unwrap();
    let item = input("r01-crash-boundary");
    let queued = enqueue(job(), vec![item.clone()]).unwrap();
    conn.execute_batch(
        "CREATE TEMP TRIGGER crash_receipt BEFORE INSERT ON receipts WHEN NEW.phase='published' BEGIN SELECT RAISE(ABORT,'injected fault'); END;",
    )
    .unwrap();
    assert!(publish_item(&conn, &queued.job_id, &item, &|| false).is_err());
    let seen = crate::session_migration::engine::job_get(&queued.job_id).unwrap();
    assert_ne!(
        seen.state,
        JobState::Completed,
        "a stopped publish must not look like a finished import: {seen:?}"
    );
    assert!(
        list_entries(&conn).unwrap().is_empty(),
        "catalog must not invent a completed import after a receipt crash"
    );
    let rec = latest_receipt(&conn, &queued.job_id, &item.candidate_id)
        .unwrap()
        .expect("recoverable receipt");
    assert_ne!(rec.phase, "committed");
    conn.execute_batch("DROP TRIGGER crash_receipt").unwrap();
    run_pending().unwrap();
    let done = crate::session_migration::engine::job_get(&queued.job_id).unwrap();
    assert_eq!(done.state, JobState::Completed);
    assert_eq!(done.items[0].phase, "committed");
    assert_eq!(done.items[0].import_id, rec.import_id);
    assert_eq!(done.items[0].revision_id, rec.revision_id);
    assert_eq!(list_entries(&conn).unwrap().len(), 1);
}

#[test]
fn job_get_restores_completed_job_without_a_second_publish() {
    let _home = Home::new();
    let conn = open_catalog().unwrap();
    let queued = enqueue(job(), vec![input("r02-panel-reopen")]).unwrap();
    run_pending().unwrap();
    let receipts_before: i64 = conn
        .query_row("SELECT COUNT(*) FROM receipts", [], |r| r.get(0))
        .unwrap();
    let first = crate::session_migration::engine::job_get(&queued.job_id).unwrap();
    assert_eq!(first.state, JobState::Completed);
    assert_eq!(first.items[0].phase, "committed");
    let import_id = first.items[0].import_id.clone();
    let revision_id = first.items[0].revision_id.clone();
    assert_eq!(
        run_pending().unwrap(),
        0,
        "job_get must not enqueue a second publish"
    );
    let second = crate::session_migration::engine::job_get(&queued.job_id).unwrap();
    assert_eq!(second.state, JobState::Completed);
    assert_eq!(second.items[0].import_id, import_id);
    assert_eq!(second.items[0].revision_id, revision_id);
    let receipts_after: i64 = conn
        .query_row("SELECT COUNT(*) FROM receipts", [], |r| r.get(0))
        .unwrap();
    assert_eq!(
        receipts_before, receipts_after,
        "reopening a finished job must not write another receipt"
    );
    assert_eq!(list_entries(&conn).unwrap().len(), 1);
}

#[test]
fn lost_completion_event_does_not_duplicate_on_job_get() {
    let _home = Home::new();
    let conn = open_catalog().unwrap();
    let queued = enqueue(job(), vec![input("notify-lost")]).unwrap();
    run_pending().unwrap();
    let done = crate::session_migration::engine::job_get(&queued.job_id).unwrap();
    assert_eq!(done.state, JobState::Completed);
    assert_eq!(done.items[0].phase, "committed");
    let receipt_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM receipts", [], |row| row.get(0))
        .unwrap();

    // Simulate a crash after catalog commit but before the job completion state
    // became durable. The completed job's large preview has already been freed.
    let mut stale = done.clone();
    stale.state = JobState::Running;
    stale.items[0].phase = "running".into();
    stale.items[0].import_id = None;
    stale.items[0].revision_id = None;
    stale.items[0].outcome = None;
    save_job(&conn, &stale).unwrap();

    run_pending().unwrap();
    let recovered = crate::session_migration::engine::job_get(&queued.job_id).unwrap();
    assert_eq!(recovered.state, JobState::Completed);
    assert_eq!(recovered.items[0].phase, "committed");
    assert_eq!(recovered.items[0].import_id, done.items[0].import_id);
    assert_eq!(recovered.items[0].revision_id, done.items[0].revision_id);
    let receipt_count_after: i64 = conn
        .query_row("SELECT COUNT(*) FROM receipts", [], |row| row.get(0))
        .unwrap();
    assert_eq!(receipt_count_after, receipt_count);
    assert_eq!(list_entries(&conn).unwrap().len(), 1);
}

#[test]
fn committed_recovery_rejects_receipt_that_does_not_match_durable_input() {
    let _home = Home::new();
    let conn = open_catalog().unwrap();
    let queued = enqueue(job(), vec![input("receipt-mismatch")]).unwrap();
    run_pending().unwrap();
    let mut stale = get_job(&conn, &queued.job_id).unwrap().unwrap();
    stale.state = JobState::Running;
    stale.items[0].phase = "running".into();
    stale.items[0].outcome = None;
    save_job(&conn, &stale).unwrap();

    let raw: String = conn
        .query_row(
            "SELECT json FROM receipts WHERE job_id=?1 ORDER BY rowid DESC LIMIT 1",
            [&queued.job_id],
            |row| row.get(0),
        )
        .unwrap();
    let mut receipt: serde_json::Value = serde_json::from_str(&raw).unwrap();
    receipt["rootPath"] = serde_json::json!("different-root");
    conn.execute(
        "UPDATE receipts SET json=?2 WHERE rowid=(SELECT MAX(rowid) FROM receipts WHERE job_id=?1)",
        rusqlite::params![queued.job_id, serde_json::to_string(&receipt).unwrap()],
    )
    .unwrap();

    run_pending().unwrap();
    let failed = get_job(&conn, &queued.job_id).unwrap().unwrap();
    assert_eq!(failed.state, JobState::CompletedWithIssues);
    assert_eq!(failed.items[0].phase, "failed");
    assert_eq!(list_entries(&conn).unwrap().len(), 1);
}

#[test]
fn concurrent_publish_of_the_same_native_session_is_one_entry() {
    let _home = Home::new();
    let item = input("same-native");
    let a = enqueue(job(), vec![item.clone()]).unwrap();
    let mut other = item.clone();
    other.candidate_id = "c-other".into();
    other = super::jobs::prepare_test_commit_item(other);
    let b = enqueue(job(), vec![other.clone()]).unwrap();
    std::thread::scope(|scope| {
        scope.spawn(|| {
            let conn = open_catalog().unwrap();
            publish_item(&conn, &a.job_id, &item, &|| false).unwrap();
        });
        scope.spawn(|| {
            let conn = open_catalog().unwrap();
            let _ = publish_item(&conn, &b.job_id, &other, &|| false);
        });
    });
    let conn = open_catalog().unwrap();
    let entries = list_entries(&conn).unwrap();
    assert_eq!(
        entries.len(),
        1,
        "concurrent publish must not create two entries: {entries:?}"
    );
}

#[test]
fn undo_refresh_restores_previous_revision_without_deleting_either_snapshot() {
    let _home = Home::new();
    let conn = open_catalog().unwrap();
    let first = enqueue(job(), vec![input("v1")]).unwrap();
    run_pending().unwrap();
    let first = get_job(&conn, &first.job_id).unwrap().unwrap();
    let second = enqueue(job(), vec![input("v2")]).unwrap();
    run_pending().unwrap();
    let second = get_job(&conn, &second.job_id).unwrap().unwrap();
    assert_eq!(first.items[0].import_id, second.items[0].import_id);
    assert_ne!(first.items[0].revision_id, second.items[0].revision_id);
    let undone = undo_job(&conn, &second.job_id).unwrap();
    assert_eq!(undone.items[0].phase, "undone");
    let id = first.items[0].import_id.as_ref().unwrap();
    let entry = get_entry(&conn, id).unwrap().unwrap();
    assert!(!entry.hidden);
    assert_eq!(entry.current_revision, first.items[0].revision_id);
    assert!(
        revision_dir(id, second.items[0].revision_id.as_ref().unwrap())
            .unwrap()
            .exists()
    );
    assert_eq!(
        undo_job(&conn, &second.job_id).unwrap().items[0].phase,
        "undone"
    );
}

#[test]
fn publisher_checks_cancellation_during_copy_without_exposing_partial_entry() {
    let _home = Home::new();
    let conn = open_catalog().unwrap();
    let mut item = input(&"x".repeat(2 * 1024 * 1024));
    item = super::jobs::prepare_test_commit_item(item);
    let count = std::cell::Cell::new(0);
    let cancelled = || {
        count.set(count.get() + 1);
        count.get() > 10
    };
    let interrupted = publish_item(&conn, "copy-cancel", &item, &cancelled).unwrap();
    assert_eq!(interrupted.phase, "cancelled");
    assert!(list_entries(&conn).unwrap().is_empty());
    let rec = latest_receipt(&conn, "copy-cancel", &item.candidate_id)
        .unwrap()
        .unwrap();
    let done = publish_item(&conn, "copy-cancel", &item, &|| false).unwrap();
    assert_eq!(done.revision_id, rec.revision_id);
}

#[test]
fn corrupted_archive_is_not_silently_shortened_for_continuation() {
    let _home = Home::new();
    let conn = open_catalog().unwrap();
    let queued = enqueue(job(), vec![input("history")]).unwrap();
    run_pending().unwrap();
    let done = get_job(&conn, &queued.job_id).unwrap().unwrap();
    let id = done.items[0].import_id.as_ref().unwrap();
    let revision = done.items[0].revision_id.as_ref().unwrap();
    let path = revision_dir(id, revision).unwrap().join("events.jsonl");
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new().append(true).open(path).unwrap();
    writeln!(file, "{{broken archive}}").unwrap();
    drop(file);
    assert!(super::continuation::load_events(id, revision).is_err());
}

include!("job_control_tests_b.rs");

#[path = "job_progress_tests.rs"]
mod progress_tests;

#[path = "native_coexistence_tests.rs"]
mod native_coexistence_tests;

#[path = "native_soak_tests.rs"]
mod native_soak_tests;
