//! Cancellation is measured separately from the uninterrupted publication run.
use super::*;
use crate::session_migration::{catalog, engine, job_control, jobs};
use std::collections::BTreeMap;
use std::time::Duration;

fn enqueue(count: usize, source: Option<&Path>) -> jobs::JobRecord {
    let (batch_id, _) = create_capacity_batch_from_root(count, source);
    let mappings = super::super::mappings(&batch_id, None).unwrap();
    let prepared = super::super::validated_plan_inputs(&batch_id).unwrap();
    let plan = engine::plan_from_validated_prepare(prepared, mappings).unwrap();
    engine::commit(&plan.plan_id, &plan.plan_hash, "cancel-resume").unwrap()
}

fn bounded(job: &jobs::JobRecord, count: usize) {
    assert_eq!(job.item_count as usize, count);
    assert!(
        job.items.len() <= 50,
        "IPC returned {} items",
        job.items.len()
    );
    assert!(json_bytes(job) < 64 * 1024);
    assert_eq!(job.next_item_cursor.is_some(), count > 50);
}

#[test]
fn cancel_and_retry_replies_keep_all_durable_items_but_page_the_response() {
    let _home = CapacityHome::new();
    let job = enqueue(61, None);
    let original_cursor = job.next_item_cursor.clone().unwrap();
    let replay = engine::commit(&job.plan_id, &job.plan_hash, "cancel-resume").unwrap();
    assert_eq!(replay.job_id, job.job_id);
    bounded(&replay, 61);
    let cancelled = engine::cancel_job(&job.job_id).unwrap();
    bounded(&cancelled, 61);
    assert_eq!(cancelled.state, JobState::Cancelling);
    job_control::run_pending().unwrap();
    let stopped = engine::job_get(&job.job_id).unwrap();
    assert_eq!(
        stopped.progress,
        Some(jobs::JobProgress {
            done: 61,
            retryable: 61
        })
    );
    // Job pages bind the immutable plan, not progress versions: a UI browsing
    // a later page must retain that cursor across cancellation and retry.
    let stopped_page =
        engine::job_items_page(&job.job_id, Some(&original_cursor), Some(50)).unwrap();
    assert_eq!(stopped_page.items.len(), 11);
    assert!(stopped_page
        .items
        .iter()
        .all(|item| item.phase == "cancelled"));
    assert_eq!(stopped_page.state_version, stopped.state_version);
    bounded(&engine::undo_import_job(&job.job_id).unwrap(), 61);
    let retry = engine::retry_failed(&job.job_id).unwrap();
    bounded(&retry, 61);
    assert_eq!(retry.state, JobState::Queued);
    assert_eq!(
        retry.progress,
        Some(jobs::JobProgress {
            done: 0,
            retryable: 0
        })
    );
    let rest = engine::job_items_page(&job.job_id, Some(&original_cursor), Some(50)).unwrap();
    assert_eq!(rest.items.len(), 11);
    assert!(rest.items.iter().all(|item| item.phase == "queued"));
    assert_eq!(rest.state_version, retry.state_version);
    let conn = catalog::open_catalog_readonly().unwrap();
    let durable = jobs::get_job(&conn, &job.job_id).unwrap().unwrap();
    assert_eq!(
        durable.items.len(),
        61,
        "paging cannot truncate persisted input"
    );
}

#[test]
fn cancellation_request_does_not_rewrite_durable_item_rows() {
    let _home = CapacityHome::new();
    let job = enqueue(61, None);
    let conn = catalog::open_catalog().unwrap();
    let inputs = |conn: &rusqlite::Connection| {
        conn.prepare(
            "SELECT candidate_id,phase,result_json FROM job_items
             WHERE job_id=?1 ORDER BY candidate_id",
        )
        .unwrap()
        .query_map([&job.job_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
    };
    let before = inputs(&conn);
    assert_eq!(before.len(), 61);
    conn.execute_batch(
        "CREATE TRIGGER reject_cancel_item_rewrite BEFORE UPDATE ON job_items
         BEGIN SELECT RAISE(ABORT,'cancel must not rewrite durable items'); END;",
    )
    .unwrap();

    let cancelled = engine::cancel_job(&job.job_id).unwrap();
    bounded(&cancelled, 61);
    assert_eq!(cancelled.state, JobState::Cancelling);
    assert_eq!(cancelled.state_version, job.state_version + 1);
    assert_eq!(inputs(&conn), before);
    assert!(receipts(&job.job_id).is_empty());
    assert!(conn.is_autocommit());
    // A repeated request preserves the existing state/version contract without
    // rewriting inputs; the worker owns the subsequent item-phase transition.
    let repeated = engine::cancel_job(&job.job_id).unwrap();
    assert_eq!(repeated.state, JobState::Cancelling);
    assert_eq!(repeated.state_version, cancelled.state_version + 1);
    assert_eq!(inputs(&conn), before);
    conn.execute_batch("DROP TRIGGER reject_cancel_item_rewrite")
        .unwrap();
    assert_eq!(job_control::run_pending().unwrap(), 1);
    let stopped = engine::job_get(&job.job_id).unwrap();
    assert_eq!(stopped.state, JobState::Cancelled);
    assert_eq!(stopped.item_count, 61);
    assert!(stopped.items.iter().all(|item| item.phase == "cancelled"));
}

#[test]
fn cancellation_header_write_failure_preserves_the_queued_job() {
    let _home = CapacityHome::new();
    let job = enqueue(3, None);
    let conn = catalog::open_catalog().unwrap();
    let before = jobs::get_job(&conn, &job.job_id).unwrap().unwrap();
    conn.execute_batch(
        "CREATE TRIGGER reject_cancel_header BEFORE UPDATE ON jobs
         BEGIN SELECT RAISE(ABORT,'synthetic cancel header failure'); END;",
    )
    .unwrap();
    let error = engine::cancel_job(&job.job_id).unwrap_err();
    assert_eq!(
        error.code,
        crate::session_migration::errors::ErrorCode::SnapshotUnavailable
    );
    assert!(error.message.contains("synthetic cancel header failure"));
    assert_eq!(
        serde_json::to_value(jobs::get_job(&conn, &job.job_id).unwrap().unwrap()).unwrap(),
        serde_json::to_value(before).unwrap()
    );
    assert_eq!(
        conn.query_row(
            "SELECT COUNT(*) FROM job_items WHERE job_id=?1 AND phase='queued'",
            [&job.job_id],
            |row| row.get::<_, u32>(0)
        )
        .unwrap(),
        3
    );
    assert!(receipts(&job.job_id).is_empty());
}

#[test]
fn cancellation_of_a_completed_job_keeps_its_results() {
    let _home = CapacityHome::new();
    let job = enqueue(3, None);
    assert_eq!(job_control::run_pending().unwrap(), 1);
    let conn = catalog::open_catalog().unwrap();
    let before = jobs::get_job(&conn, &job.job_id).unwrap().unwrap();
    assert_eq!(before.state, JobState::Completed);
    let before_receipts = receipts(&job.job_id);
    conn.execute_batch(
        "CREATE TRIGGER reject_terminal_item_rewrite BEFORE UPDATE ON job_items
         BEGIN SELECT RAISE(ABORT,'terminal cancel must not rewrite items'); END;",
    )
    .unwrap();
    let cancelled = engine::cancel_job(&job.job_id).unwrap();
    assert_eq!(cancelled.state, JobState::Completed);
    assert_eq!(cancelled.state_version, before.state_version + 1);
    assert_eq!(
        serde_json::to_value(cancelled.items).unwrap(),
        serde_json::to_value(before.items).unwrap()
    );
    assert_eq!(receipts(&job.job_id), before_receipts);
}

fn receipts(job_id: &str) -> BTreeMap<String, String> {
    let conn = catalog::open_catalog_readonly().unwrap();
    let mut query = conn.prepare(
        "SELECT receipt_id,json FROM receipts WHERE job_id=?1 AND phase='committed' ORDER BY receipt_id",
    ).unwrap();
    query
        .query_map([job_id], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

fn cancel_resume(count: usize) {
    let home = CapacityHome::new();
    let source = home.root.join("synthetic-source");
    std::fs::create_dir(&source).unwrap();
    std::fs::write(source.join("source-marker.txt"), "synthetic source only").unwrap();
    let job = enqueue(count, Some(&source));
    let target = (count / 5).max(1);
    let started = Instant::now();
    let mut sampled_disk_bytes = jobs::dir_size(&catalog::imports_dir());
    let (before_cancel, cancel_ms, stop_ms, max_poll_ms) = std::thread::scope(|scope| {
        let worker = scope.spawn(|| {
            let result = job_control::run_pending();
            (result, Instant::now())
        });
        let mut maximum = 0;
        let mut report = Instant::now();
        let before = loop {
            assert!(
                !worker.is_finished(),
                "worker completed before the cancellation window"
            );
            let (page, poll_ms) = elapsed_ms(|| engine::job_get(&job.job_id).unwrap());
            maximum = maximum.max(poll_ms);
            bounded(&page, count);
            let current = receipts(&job.job_id);
            if current.len() >= target {
                break current;
            }
            if report.elapsed() >= Duration::from_secs(15) {
                sampled_disk_bytes =
                    sampled_disk_bytes.max(jobs::dir_size(&catalog::imports_dir()));
                println!("WINDOW4_CANCEL_PROGRESS count={count} committed={} elapsedMs={} pollMs={poll_ms}",
                    current.len(), started.elapsed().as_millis());
                report = Instant::now();
            }
            std::thread::sleep(Duration::from_millis(if count < 100 { 20 } else { 1000 }));
        };
        let cancelled_at = Instant::now();
        let (response, cancel_ms) = elapsed_ms(|| engine::cancel_job(&job.job_id).unwrap());
        // Check the IPC shape after joining so a failed assertion cannot strand a worker.
        println!(
            "WINDOW4_CANCEL_ACK count={count} committed={} cancelMs={cancel_ms}",
            before.len()
        );
        while !worker.is_finished() {
            std::thread::sleep(Duration::from_millis(if count < 100 { 20 } else { 1000 }));
            let (page, poll_ms) = elapsed_ms(|| engine::job_get(&job.job_id).unwrap());
            maximum = maximum.max(poll_ms);
            bounded(&page, count);
            if report.elapsed() >= Duration::from_secs(15) {
                sampled_disk_bytes =
                    sampled_disk_bytes.max(jobs::dir_size(&catalog::imports_dir()));
                println!("WINDOW4_CANCELLING_PROGRESS count={count} elapsedMs={} pollMs={poll_ms} progress={:?}",
                    cancelled_at.elapsed().as_millis(), page.progress);
                report = Instant::now();
            }
        }
        let (completed, stopped_at) = worker.join().unwrap();
        let completed = completed.unwrap();
        let stop_ms = stopped_at.duration_since(cancelled_at).as_millis();
        assert_eq!(completed, 1);
        bounded(&response, count);
        assert!(matches!(
            response.state,
            JobState::Cancelling | JobState::Cancelled
        ));
        (before, cancel_ms, stop_ms, maximum)
    });
    let conn = catalog::open_catalog_readonly().unwrap();
    let stopped = jobs::get_job(&conn, &job.job_id).unwrap().unwrap();
    assert_eq!(stopped.state, JobState::Cancelled);
    assert_eq!(stopped.items.len(), count);
    assert!(stopped
        .items
        .iter()
        .all(|item| matches!(item.phase.as_str(), "committed" | "cancelled")));
    let committed = stopped
        .items
        .iter()
        .filter(|item| item.phase == "committed")
        .count();
    assert!(committed >= target && committed < count);
    let page = engine::job_get(&job.job_id).unwrap();
    assert_eq!(
        page.progress,
        Some(jobs::JobProgress {
            done: count as u32,
            retryable: (count - committed) as u32
        })
    );
    let catalog_count: usize = conn
        .query_row("SELECT COUNT(*) FROM entries", [], |row| row.get(0))
        .unwrap();
    assert_eq!(catalog_count, committed);
    let stopped_receipts = receipts(&job.job_id);
    assert_eq!(stopped_receipts.len(), committed);
    for (id, json) in &before_cancel {
        assert_eq!(stopped_receipts.get(id), Some(json));
    }
    for item in stopped
        .items
        .iter()
        .filter(|item| item.phase == "committed")
    {
        let (events, _, total) = engine::events_page(
            item.import_id.as_deref().unwrap(),
            item.revision_id.as_deref(),
            None,
            Some(50),
            Some("e1"),
        )
        .unwrap();
        assert_eq!(total, 1);
        assert_eq!(events.len(), 1);
    }
    drop(conn);
    println!(
        "WINDOW4_CANCEL_STOP {}",
        serde_json::json!({
            "sessionCount": count, "requestedAfterCommitted": before_cancel.len(),
            "committedAtStop": committed, "cancelReplyMs": cancel_ms, "workerStopMs": stop_ms,
        "maxConcurrentJobPollMs": max_poll_ms,
        "stoppedLogicalDiskBytes": jobs::dir_size(&catalog::imports_dir()),
        "sampledMaxLogicalDiskBytesBeforeRetry": sampled_disk_bytes,
        })
    );
    assert!(cancel_ms < 500, "cancellation reply took {cancel_ms}ms");

    // The original synthetic source goes offline; leased snapshots stay at their
    // separate app-owned location. This is a cache reset, not an OS restart.
    std::fs::rename(&source, home.root.join("synthetic-source-offline")).unwrap();
    assert!(!source.exists());
    super::super::reset_runtime();
    engine::reset_runtime();
    let (retry, retry_ms) = elapsed_ms(|| engine::retry_failed(&job.job_id).unwrap());
    bounded(&retry, count);
    assert_eq!(retry.state, JobState::Queued);
    println!(
        "WINDOW4_CANCEL_RESUME count={count} retryMs={retry_ms} remaining={}",
        count - committed
    );
    // This logs resumed publication only; never compare it to uninterrupted V42.
    super::publish::verify(&retry, count, jobs::dir_size(&catalog::imports_dir()));
    let final_receipts = receipts(&job.job_id);
    for (id, json) in stopped_receipts {
        assert_eq!(final_receipts.get(&id), Some(&json));
    }
    let retried_again = engine::retry_failed(&job.job_id).unwrap();
    bounded(&retried_again, count);
    assert_eq!(retried_again.state, JobState::Completed);
    assert_eq!(
        retried_again.progress,
        Some(jobs::JobProgress {
            done: count as u32,
            retryable: 0
        })
    );
    assert_eq!(job_control::run_pending().unwrap(), 0);
    assert_eq!(receipts(&job.job_id), final_receipts);
    assert!(!source.exists());
}

#[test]
fn small_batch_cancellation_preserves_commits_and_resumes_offline() {
    cancel_resume(16);
}

/// Child entry point for the process-level recovery test below. The publish
/// fault hook pauses inside the real `run_pending` worker; the parent kills
/// this child after the durable boundary marker is visible.
#[test]
#[ignore]
fn worker_child_runs_pending_for_fault() {
    job_control::run_pending().expect("worker child must reach the injected fault");
}

#[test]
fn worker_process_kill_and_restart_recovers_each_publish_boundary() {
    let exe = std::env::current_exe().expect("test exe");
    let points = [
        "after_rename",
        "after_receipt_published",
        "after_catalog_commit",
    ];

    for (cycle, point) in points.into_iter().enumerate() {
        let home = CapacityHome::new();
        let job = enqueue(1, None);
        let crash_dir = home.root.join("worker-crash");
        std::fs::create_dir_all(&crash_dir).unwrap();
        let marker = crash_dir.join(format!("hit-{point}"));
        let child_log = std::fs::File::create(crash_dir.join("child.log")).unwrap();
        let mut child = std::process::Command::new(&exe)
            .args([
                "--ignored",
                "--exact",
                "--test-threads=1",
                "--nocapture",
                "session_migration::prepare::capacity_tests::cancel::worker_child_runs_pending_for_fault",
            ])
            .env("GROK_APP_HOME", home.root.join("app"))
            .env("GROK_PUBLISH_CRASH_POINT", point)
            .env("GROK_PUBLISH_CRASH_DIR", &crash_dir)
            .stdout(std::process::Stdio::from(
                child_log.try_clone().expect("clone child log"),
            ))
            .stderr(std::process::Stdio::from(child_log))
            .spawn()
            .unwrap_or_else(|error| panic!("spawn worker child at {point}: {error}"));

        let started = std::time::Instant::now();
        while !marker.exists() {
            assert!(
                started.elapsed() < Duration::from_secs(30),
                "worker child never reached {point} on cycle {cycle}; child.log={}",
                std::fs::read_to_string(crash_dir.join("child.log")).unwrap_or_default()
            );
            if child.try_wait().ok().flatten().is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(
            marker.exists(),
            "worker child exited before reaching {point} on cycle {cycle}; child.log={}",
            std::fs::read_to_string(crash_dir.join("child.log")).unwrap_or_default()
        );
        let _ = child.kill();
        let _ = child.wait();

        // This is a new worker invocation in the same isolated app home. No
        // runtime/cache reset is used, so the durable recovery path is tested.
        let completed = job_control::run_pending().unwrap_or_else(|error| {
            panic!(
                "restart worker failed at {point} on cycle {cycle}: {error}; child.log={}",
                std::fs::read_to_string(crash_dir.join("child.log")).unwrap_or_default()
            )
        });
        assert_eq!(completed, 1, "restart should finish the interrupted job");
        let recovered = engine::job_get(&job.job_id).unwrap();
        assert_eq!(recovered.state, JobState::Completed);
        assert_eq!(recovered.items.len(), 1);
        assert_eq!(recovered.items[0].phase, "committed");
        let import_id = recovered.items[0].import_id.as_deref().unwrap();
        let revision_id = recovered.items[0].revision_id.as_deref().unwrap();
        let (events, _, total) =
            engine::events_page(import_id, Some(revision_id), None, Some(50), Some("e1")).unwrap();
        assert_eq!(total, 1);
        assert_eq!(events.len(), 1);
        let receipt_rows = receipts(&job.job_id);
        assert_eq!(receipt_rows.len(), 1);
        let conn = catalog::open_catalog_readonly().unwrap();
        let entries = catalog::list_entries(&conn).unwrap();
        assert_eq!(
            entries
                .iter()
                .filter(|entry| entry.import_id == import_id)
                .count(),
            1,
            "restart must not duplicate the catalog entry"
        );
        let revisions: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM revisions WHERE import_id=?1",
                rusqlite::params![import_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(revisions, 1, "restart must not duplicate the revision");
        drop(conn);
        drop(home);
    }
}

#[test]
#[ignore = "cancel/resume capacity; opt in with GROK_WINDOW4_CAPACITY=1 and 1000 or 10000 candidates"]
fn window4_cancel_resume_benchmark() {
    assert_eq!(std::env::var("GROK_WINDOW4_CAPACITY").as_deref(), Ok("1"));
    let count = std::env::var("GROK_WINDOW4_CANDIDATES")
        .unwrap()
        .parse()
        .unwrap();
    assert!(matches!(count, 1000 | 10000));
    cancel_resume(count);
}
