use std::path::PathBuf;
use std::sync::{Arc, Barrier};
use std::time::{Duration, Instant};

use super::errors::ErrorCode;
use super::prepare::{PrepareBatchState, PrepareBatchStatus};
use super::types::Provider;

#[path = "prepare_plan_tests.rs"]
mod plan_tests;

#[path = "prepare_mapping_tests.rs"]
mod mapping_tests;

#[path = "prepare_lazy_preview_tests.rs"]
mod lazy_preview_tests;

#[path = "prepare_member_tests.rs"]
mod member_tests;

struct Fixture {
    root: PathBuf,
    _lock: crate::session_migration::test_home::AppHomeEnvGuard,
}

impl Fixture {
    fn new(count: usize) -> Self {
        let lock = crate::session_migration::test_home::AppHomeEnvGuard::acquire();
        let root = std::env::temp_dir().join(format!("session-prepare-{}", uuid::Uuid::new_v4()));
        let fixture = Self { root, _lock: lock };
        let source = fixture.root.join("source");
        std::fs::create_dir_all(&source).unwrap();
        for index in 0..count {
            std::fs::write(
                source.join(format!("{index:05}.json")),
                format!(r#"[{{"role":"user","content":"event-{index}"}}]"#),
            )
            .unwrap();
        }
        std::env::set_var("GROK_APP_HOME", fixture.root.join("app"));
        crate::paths::ensure_app_dirs().unwrap();
        fixture
    }

    fn source(&self) -> String {
        self.root.join("source").to_string_lossy().into_owned()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        super::prepare::reset_runtime();
        super::engine::reset_runtime();
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn wait_terminal(batch_id: &str) -> PrepareBatchStatus {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let status = super::prepare::status(batch_id).unwrap();
        if !matches!(
            status.state,
            PrepareBatchState::Queued
                | PrepareBatchState::Preparing
                | PrepareBatchState::Cancelling
        ) && !super::prepare::running_for_test(batch_id)
        {
            // The terminal transaction is visible before the worker drops its SQLite
            // connection. Recovery fixtures must wait for RunningGuard to release
            // it before resetting controls or replacing WAL/SHM files on Windows.
            return status;
        }
        assert!(
            Instant::now() < deadline,
            "prepare batch timed out: {status:?}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn start_batch(fixture: &Fixture) -> PrepareBatchStatus {
    let scan = super::scan::scan_start(vec![(Provider::GenericFile, fixture.source())]).unwrap();
    let selection = super::scan_selection::freeze_current(&scan.scan_id, scan.epoch).unwrap();
    super::prepare::start(
        &scan.scan_id,
        scan.epoch,
        &selection.selection_id,
        Vec::new(),
    )
    .unwrap()
}

#[test]
fn cleanup_opens_each_prepare_batch_once_for_many_previews() {
    let fixture = Fixture::new(3);
    let batch = start_batch(&fixture);
    let done = wait_terminal(&batch.batch_id);
    assert_eq!(done.state, PrepareBatchState::Completed);
    let (preview_ids, _) = super::prepare::plan_inputs(&batch.batch_id).unwrap();
    assert_eq!(preview_ids.len(), 3);
    let paths = preview_ids
        .iter()
        .map(|preview_id| {
            let path = super::jobs::find_preview_dir(preview_id)
                .unwrap()
                .expect("prepared preview is on disk");
            super::jobs::expire_preview_lease_file(&path).unwrap();
            path
        })
        .collect::<Vec<_>>();

    super::prepare::reset_open_existing_count();
    assert_eq!(super::jobs::cleanup_unreferenced_preview_dirs().unwrap(), 0);
    assert_eq!(super::prepare::open_existing_count(), 1);
    assert!(paths.iter().all(|path| path.is_dir()));
}

#[test]
fn live_disk_lease_lookup_does_not_open_prepare_batch_db() {
    let fixture = Fixture::new(1);
    let batch = start_batch(&fixture);
    let done = wait_terminal(&batch.batch_id);
    assert_eq!(done.state, PrepareBatchState::Completed);
    let (preview_ids, _) = super::prepare::plan_inputs(&batch.batch_id).unwrap();
    let preview_id = preview_ids.into_iter().next().unwrap();

    super::prepare::reset_runtime();
    super::engine::reset_runtime();
    super::prepare::reset_open_existing_count();
    let (events, next) =
        super::engine::preview_events_page(&preview_id, None, Some(10), None).unwrap();
    assert!(!events.is_empty());
    assert!(next.is_none());
    assert_eq!(
        super::prepare::open_existing_count(),
        0,
        "a live disk lease must not reopen the prepare batch"
    );
}

#[test]
fn off_page_candidate_is_prepared_planned_and_committed_from_host_snapshot() {
    let fixture = Fixture::new(51);
    let scan = super::scan::scan_start(vec![(Provider::GenericFile, fixture.source())]).unwrap();
    let selection = super::scan_selection::freeze_current(&scan.scan_id, scan.epoch).unwrap();
    assert_eq!(selection.selected_count, 51);
    let selected_first = super::scan_selection::page(
        &scan.scan_id,
        scan.epoch,
        &selection.selection_id,
        None,
        Some(50),
    )
    .unwrap();
    let selected_second = super::scan_selection::page(
        &scan.scan_id,
        scan.epoch,
        &selection.selection_id,
        selected_first.next_cursor.as_deref(),
        Some(50),
    )
    .unwrap();
    let off_page_id = selected_second.candidates[0].candidate_id.clone();

    let batch = super::prepare::start(
        &scan.scan_id,
        scan.epoch,
        &selection.selection_id,
        Vec::new(),
    )
    .unwrap();
    let done = wait_terminal(&batch.batch_id);
    assert_eq!(done.state, PrepareBatchState::Completed);
    assert_eq!(done.prepared, 51);
    assert_eq!(done.active, 0);

    let first = super::prepare::items_page(&batch.batch_id, None, Some(50)).unwrap();
    assert_eq!(first.items.len(), 50);
    let second =
        super::prepare::items_page(&batch.batch_id, first.next_cursor.as_deref(), Some(50))
            .unwrap();
    assert_eq!(second.items.len(), 1);
    assert_eq!(second.items[0].candidate_id, off_page_id);
    assert_eq!(
        second.items[0].state,
        super::prepare::PrepareItemState::Prepared
    );

    let mappings = super::prepare::mappings(&batch.batch_id, None).unwrap();
    let prepared = super::prepare::validated_plan_inputs(&batch.batch_id).unwrap();
    assert_eq!(prepared.preview_ids.len(), 51);
    assert_eq!(prepared.selection_set.entries.len(), 51);
    let plan = super::engine::plan_from_validated_prepare(prepared, mappings).unwrap();
    assert_eq!(plan.preview_ids.len(), 51);
    let job = super::engine::commit(
        &plan.plan_id,
        &plan.plan_hash,
        &format!("prepare-page-{}", batch.batch_id),
    )
    .unwrap();
    assert_eq!(super::job_control::run_pending().unwrap(), 1);
    let first_job_page = super::engine::job_get(&job.job_id).unwrap();
    assert_eq!(first_job_page.state, super::jobs::JobState::Completed);
    assert_eq!(first_job_page.item_count, 51);
    assert_eq!(first_job_page.items.len(), 50);
    let second_job_page = super::engine::job_items_page(
        &job.job_id,
        first_job_page.next_item_cursor.as_deref(),
        Some(50),
    )
    .unwrap();
    assert_eq!(second_job_page.items.len(), 1);
    let committed = first_job_page
        .items
        .iter()
        .chain(second_job_page.items.iter())
        .collect::<Vec<_>>();
    let committed_ids = committed
        .iter()
        .map(|item| item.candidate_id.as_str())
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(committed.len(), 51);
    assert_eq!(committed_ids.len(), 51);
    assert!(committed.iter().all(|item| item.import_id.is_some()));
    assert!(committed
        .iter()
        .any(|item| item.candidate_id == off_page_id));
}

#[test]
fn cancel_stops_claiming_queued_candidates() {
    let fixture = Fixture::new(80);
    let scan = super::scan::scan_start(vec![(Provider::GenericFile, fixture.source())]).unwrap();
    let selection = super::scan_selection::freeze_current(&scan.scan_id, scan.epoch).unwrap();
    let batch = super::prepare::start(
        &scan.scan_id,
        scan.epoch,
        &selection.selection_id,
        Vec::new(),
    )
    .unwrap();
    let cancelled = super::prepare::cancel(&batch.batch_id).unwrap();
    assert!(matches!(
        cancelled.state,
        PrepareBatchState::Cancelling | PrepareBatchState::Cancelled
    ));
    let done = wait_terminal(&batch.batch_id);
    assert_eq!(done.state, PrepareBatchState::Cancelled);
    assert_eq!(done.active, 0);
    assert_eq!(done.prepared + done.failed + done.skipped, done.total);
    assert!(done.skipped > 0);
}

#[test]
fn recovery_failure_releases_the_running_latch() {
    let fixture = Fixture::new(1);
    let batch = start_batch(&fixture);
    let done = wait_terminal(&batch.batch_id);
    assert_eq!(done.state, PrepareBatchState::Completed);
    super::prepare::reset_runtime();

    let conn = super::prepare::open_existing(&batch.batch_id).unwrap();
    conn.execute("UPDATE items SET state='preparing'", [])
        .unwrap();
    conn.execute(
        "UPDATE meta SET state='preparing',cancel_requested=0 WHERE id=1",
        [],
    )
    .unwrap();
    conn.execute_batch(
        "CREATE TRIGGER reject_recovery BEFORE UPDATE OF state ON items
         WHEN OLD.state='preparing'
         BEGIN SELECT RAISE(ABORT, 'forced recovery failure'); END;",
    )
    .unwrap();
    drop(conn);

    assert!(super::prepare::ensure_running(&batch.batch_id).is_err());
    assert!(!super::prepare::running_for_test(&batch.batch_id));
}

#[test]
fn concurrent_claims_are_atomic_and_distinct() {
    let fixture = Fixture::new(8);
    let batch = start_batch(&fixture);
    let done = wait_terminal(&batch.batch_id);
    assert_eq!(done.state, PrepareBatchState::Completed);
    super::prepare::reset_runtime();
    let conn = super::prepare::open_existing(&batch.batch_id).unwrap();
    conn.execute(
        "UPDATE items SET state='queued',preview_id=NULL,selection_json=NULL,error_json=NULL",
        [],
    )
    .unwrap();
    conn.execute(
        "UPDATE meta SET state='queued',cancel_requested=0 WHERE id=1",
        [],
    )
    .unwrap();
    drop(conn);

    let barrier = Arc::new(Barrier::new(9));
    let mut workers = Vec::new();
    for _ in 0..8 {
        let barrier = Arc::clone(&barrier);
        let batch_id = batch.batch_id.clone();
        workers.push(std::thread::spawn(move || {
            barrier.wait();
            super::prepare::claim_next_for_test(&batch_id)
        }));
    }
    barrier.wait();
    let mut claimed = workers
        .into_iter()
        .map(|worker| worker.join().unwrap().unwrap().unwrap().0)
        .collect::<Vec<_>>();
    claimed.sort_unstable();
    claimed.dedup();
    assert_eq!(claimed.len(), 8);
}

#[test]
fn manual_and_batch_previews_share_the_global_concurrency_limit() {
    let fixture = Fixture::new(40);
    let scan = super::scan::scan_start(vec![(Provider::GenericFile, fixture.source())]).unwrap();
    let manual = scan
        .candidates
        .iter()
        .take(10)
        .map(|candidate| {
            (
                candidate.candidate_id.clone(),
                candidate.source_revision.clone(),
            )
        })
        .collect::<Vec<_>>();
    let selection = super::scan_selection::freeze_current(&scan.scan_id, scan.epoch).unwrap();
    let start = Arc::new(Barrier::new(manual.len() + 1));
    let mut workers = Vec::new();
    super::preview_gate::reset_metrics_for_test();
    for (candidate_id, source_revision) in manual {
        let start = Arc::clone(&start);
        workers.push(std::thread::spawn(move || {
            start.wait();
            super::engine::preview(&candidate_id, Some(&source_revision))
        }));
    }
    let batch = super::prepare::start(
        &scan.scan_id,
        scan.epoch,
        &selection.selection_id,
        Vec::new(),
    )
    .unwrap();
    start.wait();
    for worker in workers {
        worker.join().unwrap().unwrap();
    }
    let done = wait_terminal(&batch.batch_id);
    assert_eq!(done.state, PrepareBatchState::Completed);

    let (max_active, manual_acquires, batch_acquires) = super::preview_gate::metrics_for_test();
    assert_eq!(manual_acquires, 10);
    assert_eq!(batch_acquires, 40);
    assert!(max_active > 0);
    assert!(max_active <= super::budget::READ_CONCURRENCY);
}

#[test]
fn retrying_a_planned_batch_does_not_mutate_items() {
    let fixture = Fixture::new(2);
    let batch = start_batch(&fixture);
    let done = wait_terminal(&batch.batch_id);
    assert_eq!(done.state, PrepareBatchState::Completed);
    super::prepare::mark_planned(&batch.batch_id).unwrap();
    super::prepare::reset_runtime();

    let before = super::prepare::items_page(&batch.batch_id, None, Some(10)).unwrap();
    assert!(super::prepare::retry(&batch.batch_id).is_err());
    let after = super::prepare::items_page(&batch.batch_id, None, Some(10)).unwrap();
    assert_eq!(
        super::prepare::status_raw(&batch.batch_id).unwrap().state,
        PrepareBatchState::Planned
    );
    assert_eq!(
        before
            .items
            .iter()
            .map(|item| item.state)
            .collect::<Vec<_>>(),
        after
            .items
            .iter()
            .map(|item| item.state)
            .collect::<Vec<_>>()
    );
}

#[test]
fn mark_planned_rejects_non_terminal_state_without_mutation() {
    let fixture = Fixture::new(1);
    let batch = start_batch(&fixture);
    let done = wait_terminal(&batch.batch_id);
    assert_eq!(done.state, PrepareBatchState::Completed);
    super::prepare::reset_runtime();

    let conn = super::prepare::open_existing(&batch.batch_id).unwrap();
    conn.execute("UPDATE meta SET state='failed' WHERE id=1", [])
        .unwrap();
    drop(conn);
    let before = super::prepare::status_raw(&batch.batch_id).unwrap();

    let error = super::prepare::mark_planned(&batch.batch_id).unwrap_err();
    let after = super::prepare::status_raw(&batch.batch_id).unwrap();
    assert_eq!(error.code, super::errors::ErrorCode::PlanStale);
    assert_eq!(after.state, PrepareBatchState::Failed);
    assert_eq!(after.state_version, before.state_version);
}

#[test]
fn retry_requeues_only_failed_items_and_completes() {
    let fixture = Fixture::new(2);
    let batch = start_batch(&fixture);
    let done = wait_terminal(&batch.batch_id);
    assert_eq!(done.state, PrepareBatchState::Completed);
    super::prepare::reset_runtime();

    let before = super::prepare::items_page(&batch.batch_id, None, Some(10)).unwrap();
    let failed_attempt = before.items[0].attempt;
    let retained_attempt = before.items[1].attempt;
    let retained_preview = before.items[1].preview_id.clone();
    let injected = super::errors::ImportError::new(
        super::errors::ErrorCode::SourceBusy,
        "transient test failure",
    );
    let conn = super::prepare::open_existing(&batch.batch_id).unwrap();
    conn.execute(
        "UPDATE items SET state='failed',preview_id=NULL,selection_json=NULL,error_json=?1
         WHERE seq=1",
        [serde_json::to_string(&injected).unwrap()],
    )
    .unwrap();
    conn.execute(
        "UPDATE meta SET state='completed-with-issues',cancel_requested=0 WHERE id=1",
        [],
    )
    .unwrap();
    drop(conn);

    let partial = super::prepare::status_raw(&batch.batch_id).unwrap();
    assert_eq!(partial.state, PrepareBatchState::CompletedWithIssues);
    assert_eq!(partial.prepared, 1);
    assert_eq!(partial.failed, 1);
    super::prepare::retry(&batch.batch_id).unwrap();

    let retried = wait_terminal(&batch.batch_id);
    assert_eq!(retried.state, PrepareBatchState::Completed);
    assert_eq!(retried.prepared, 2);
    assert_eq!(retried.failed, 0);
    let after = super::prepare::items_page(&batch.batch_id, None, Some(10)).unwrap();
    assert_eq!(
        after.items[0].state,
        super::prepare::PrepareItemState::Prepared
    );
    assert_eq!(after.items[0].attempt, failed_attempt + 1);
    assert!(after.items[0].error.is_none());
    assert_eq!(after.items[1].attempt, retained_attempt);
    assert_eq!(after.items[1].preview_id, retained_preview);
}

#[test]
fn cancelling_a_completed_batch_is_idempotent() {
    let fixture = Fixture::new(2);
    let batch = start_batch(&fixture);
    let done = wait_terminal(&batch.batch_id);
    assert_eq!(done.state, PrepareBatchState::Completed);
    super::prepare::reset_runtime();

    let cancelled = super::prepare::cancel(&batch.batch_id).unwrap();
    assert_eq!(cancelled.state, PrepareBatchState::Completed);
    assert_eq!(cancelled.prepared, done.prepared);
    assert_eq!(cancelled.skipped, done.skipped);
}

#[test]
fn completed_batch_survives_restart_through_mapping_plan_and_commit() {
    let fixture = Fixture::new(1);
    let batch = start_batch(&fixture);
    let done = wait_terminal(&batch.batch_id);
    assert_eq!(done.state, PrepareBatchState::Completed);
    super::prepare::reset_runtime();
    super::engine::reset_runtime();

    let mappings_after_reset = super::prepare::mappings(&batch.batch_id, None).unwrap();
    assert!(!mappings_after_reset.is_empty());

    let recovered = super::engine::recovery().unwrap();
    assert_eq!(
        recovered
            .prepare
            .as_ref()
            .map(|status| status.batch_id.as_str()),
        Some(batch.batch_id.as_str())
    );
    let mappings = super::prepare::mappings(&batch.batch_id, None).unwrap();
    let prepared = super::prepare::validated_plan_inputs(&batch.batch_id).unwrap();
    let plan = super::engine::plan_from_validated_prepare(prepared, mappings).unwrap();
    super::engine::reset_runtime();

    let job = super::engine::commit(
        &plan.plan_id,
        &plan.plan_hash,
        &format!("prepare-restart-{}", batch.batch_id),
    )
    .unwrap();
    assert_eq!(super::job_control::run_pending().unwrap(), 1);
    let completed = super::engine::job_get(&job.job_id).unwrap();
    assert_eq!(completed.state, super::jobs::JobState::Completed);
    assert_eq!(completed.item_count, 1);
    assert_eq!(completed.items.len(), 1);
    assert!(completed.items[0].import_id.is_some());
}

#[test]
fn batch_mapping_rejects_tampered_candidate_identity() {
    let fixture = Fixture::new(1);
    let batch = start_batch(&fixture);
    let done = wait_terminal(&batch.batch_id);
    assert_eq!(done.state, PrepareBatchState::Completed);

    let conn = super::prepare::open_existing(&batch.batch_id).unwrap();
    let raw: String = conn
        .query_row("SELECT input_json FROM items WHERE seq=1", [], |row| {
            row.get(0)
        })
        .unwrap();
    let mut reference: super::scan::CandidateRef = serde_json::from_str(&raw).unwrap();
    reference.candidate.candidate_id = "tampered-candidate".into();
    conn.execute(
        "UPDATE items SET input_json=?1 WHERE seq=1",
        rusqlite::params![serde_json::to_string(&reference).unwrap()],
    )
    .unwrap();
    drop(conn);

    let error = super::prepare::mappings(&batch.batch_id, None).unwrap_err();
    assert_eq!(error.code, ErrorCode::PlanStale);
    assert!(error.message.contains("mapping identity"));
}

#[test]
fn validated_prepare_plan_rejects_lease_file_and_selection_tampering() {
    let fixture = Fixture::new(1);
    let batch = start_batch(&fixture);
    let done = wait_terminal(&batch.batch_id);
    assert_eq!(done.state, PrepareBatchState::Completed);
    let item = super::prepare::items_page(&batch.batch_id, None, Some(2))
        .unwrap()
        .items
        .into_iter()
        .next()
        .unwrap();
    let preview_id = item.preview_id.clone().unwrap();
    let path = super::jobs::find_preview_dir(&preview_id)
        .unwrap()
        .expect("prepared preview");

    let original_lease = super::jobs::read_preview_lease(&path).unwrap().unwrap();
    let mut lease_tampered = original_lease.clone();
    lease_tampered.candidate_id = "tampered-candidate".into();
    super::jobs::write_preview_lease(&path, &lease_tampered).unwrap();
    let error = super::prepare::validated_plan_inputs(&batch.batch_id).unwrap_err();
    assert_eq!(error.code, ErrorCode::PlanStale);
    super::jobs::write_preview_lease(&path, &original_lease).unwrap();

    let manifest_path = path.join("manifest.json");
    let original_manifest = std::fs::read(&manifest_path).unwrap();
    let mut tampered_manifest = original_manifest.clone();
    tampered_manifest.push(b' ');
    std::fs::write(&manifest_path, tampered_manifest).unwrap();
    let error = super::prepare::validated_plan_inputs(&batch.batch_id).unwrap_err();
    assert_eq!(error.code, ErrorCode::PlanStale);
    std::fs::write(&manifest_path, original_manifest).unwrap();

    let conn = super::prepare::open_existing(&batch.batch_id).unwrap();
    let raw: String = conn
        .query_row("SELECT selection_json FROM items WHERE seq=1", [], |row| {
            row.get(0)
        })
        .unwrap();
    let mut selection: super::selection::PreviewSelection = serde_json::from_str(&raw).unwrap();
    selection.leaf_id = "tampered-leaf".into();
    conn.execute(
        "UPDATE items SET selection_json=?1 WHERE seq=1",
        rusqlite::params![serde_json::to_string(&selection).unwrap()],
    )
    .unwrap();
    drop(conn);
    let error = super::prepare::validated_plan_inputs(&batch.batch_id).unwrap_err();
    assert_eq!(error.code, ErrorCode::HistoryAccessUnavailable);
}

#[test]
fn expired_batch_and_owned_sidecars_are_removed_during_recovery() {
    assert_expired_batch_cleanup(false);
}

#[test]
fn expired_planned_batch_is_cleaned_without_a_pending_work_expiry_alert() {
    assert_expired_batch_cleanup(true);
}

fn assert_expired_batch_cleanup(planned: bool) {
    let fixture = Fixture::new(1);
    let batch = start_batch(&fixture);
    let done = wait_terminal(&batch.batch_id);
    assert_eq!(done.state, PrepareBatchState::Completed);
    if planned {
        super::prepare::mark_planned(&batch.batch_id).unwrap();
    }
    super::prepare::reset_runtime();
    let root = super::prepare::batch_root().unwrap();
    let path = root.join(format!("{}.sqlite", batch.batch_id));
    let conn = super::prepare::open_existing(&batch.batch_id).unwrap();
    conn.execute("UPDATE meta SET retention_expires_unix=0 WHERE id=1", [])
        .unwrap();
    drop(conn);
    let wal = PathBuf::from(format!("{}-wal", path.display()));
    let shm = PathBuf::from(format!("{}-shm", path.display()));
    std::fs::write(&wal, b"stale wal").unwrap();
    std::fs::write(&shm, b"stale shm").unwrap();

    let recovery = super::prepare::latest_recoverable().unwrap();
    assert!(recovery.status.is_none());
    assert_eq!(recovery.expired, !planned);
    assert!(recovery.failed.is_none());
    assert!(!path.exists());
    assert!(!wal.exists());
    assert!(!shm.exists());
}

#[test]
fn corrupt_batch_is_preserved_and_reported() {
    let _fixture = Fixture::new(0);
    let batch_id = uuid::Uuid::new_v4().to_string();
    let path = super::prepare::batch_root()
        .unwrap()
        .join(format!("{batch_id}.sqlite"));
    std::fs::write(&path, b"not a sqlite database").unwrap();

    let recovery = super::prepare::latest_recoverable().unwrap();
    assert!(recovery.status.is_none());
    assert!(!recovery.expired);
    assert!(recovery
        .failed
        .as_deref()
        .is_some_and(|message| message.starts_with("SNAPSHOT_UNAVAILABLE:")));
    assert!(path.exists());
}

#[test]
fn orphan_prepare_sidecars_are_removed_without_touching_other_files() {
    let _fixture = Fixture::new(0);
    let root = super::prepare::batch_root().unwrap();
    let batch_id = uuid::Uuid::new_v4();
    let orphan = root.join(format!("{batch_id}.sqlite-wal"));
    let unrelated = root.join("notes.sqlite-wal");
    std::fs::write(&orphan, b"orphan").unwrap();
    std::fs::write(&unrelated, b"keep").unwrap();

    let recovery = super::prepare::latest_recoverable().unwrap();
    assert!(recovery.status.is_none());
    assert!(!orphan.exists());
    assert!(unrelated.exists());
}

#[test]
fn status_reads_committed_snapshot_while_a_writer_holds_the_batch() {
    let fixture = Fixture::new(1);
    let batch = start_batch(&fixture);
    let done = wait_terminal(&batch.batch_id);
    assert_eq!(done.state, PrepareBatchState::Completed);

    let mut writer = super::prepare::open_existing(&batch.batch_id).unwrap();
    let tx = writer
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .unwrap();
    tx.execute(
        "UPDATE meta SET state='failed',state_version=state_version+1 WHERE id=1",
        [],
    )
    .unwrap();
    let uncommitted: String = tx
        .query_row("SELECT state FROM meta WHERE id=1", [], |row| row.get(0))
        .unwrap();
    assert_eq!(uncommitted, "failed");

    let started = Instant::now();
    let status = super::prepare::status(&batch.batch_id).unwrap();
    assert!(
        started.elapsed() < Duration::from_millis(super::budget::DB_BUSY_MS),
        "status waited on the writer lock: {:?}",
        started.elapsed()
    );
    assert_eq!(status.batch_id, done.batch_id);
    assert_eq!(status.state, PrepareBatchState::Completed);
    assert_eq!(status.state_version, done.state_version);
    assert_eq!(status.prepared, done.prepared);
    assert_eq!(status.failed, done.failed);

    let still_uncommitted: String = tx
        .query_row("SELECT state FROM meta WHERE id=1", [], |row| row.get(0))
        .unwrap();
    assert_eq!(still_uncommitted, "failed");
    tx.rollback().unwrap();
}

#[test]
fn status_reports_retryable_busy_when_a_lock_blocks_the_read() {
    let fixture = Fixture::new(1);
    let batch = start_batch(&fixture);
    let done = wait_terminal(&batch.batch_id);
    assert_eq!(done.state, PrepareBatchState::Completed);

    let writer = super::prepare::open_existing(&batch.batch_id).unwrap();
    writer
        .execute_batch(
            "PRAGMA locking_mode=EXCLUSIVE;
             BEGIN IMMEDIATE;
             UPDATE meta SET state_version=state_version+1 WHERE id=1;",
        )
        .unwrap();

    let started = Instant::now();
    let error = super::prepare::status(&batch.batch_id).unwrap_err();
    let elapsed = started.elapsed();
    assert_eq!(
        error.code,
        ErrorCode::SourceBusy,
        "{error} after {elapsed:?}"
    );
    assert_eq!(error.retry, "retry");
    assert!(error.message.contains("database is locked"), "{error}");
    // Each attempt runs up to three statements (two pragmas, one SELECT) and
    // SQLite may wait out the busy timeout on each of them (macOS does).
    let bound = Duration::from_millis(
        super::budget::DB_BUSY_MS * 3 * (u64::from(super::budget::DB_BUSY_RETRIES) + 1) + 500,
    );
    assert!(
        elapsed < bound,
        "status retried a blocking lock without a bound: {elapsed:?}"
    );
    drop(writer);

    let after = super::prepare::status(&batch.batch_id).unwrap();
    assert_eq!(after.state, PrepareBatchState::Completed);
    assert_eq!(after.state_version, done.state_version);
}

fn home_contract_fixture() {
    let _fixture = Fixture::new(0);
}

crate::session_migration::test_home::fixture_home_contract_tests!(home_contract_fixture);
