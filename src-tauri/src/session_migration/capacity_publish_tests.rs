//! Complete the capacity fixture through the same durable worker used by IPC.
//! One candidate is one session with one event, not a long single conversation.

use super::*;
use crate::session_migration::{catalog, engine, job_control, jobs};
use std::collections::HashSet;
use std::time::Duration;

#[path = "capacity_native_isolation_tests.rs"]
mod native_isolation;

pub(super) fn verify(
    job: &jobs::JobRecord,
    count: usize,
    queued_disk_bytes: u64,
    home_root: &Path,
) {
    let native = native_isolation::NativeIsolation::seed(home_root);
    let profile = crate::session_migration::codec_profile::Session::default();
    let poll_scope = profile.enter("poll");
    let publish_started = Instant::now();
    let (completed_jobs, sampled_disk_bytes, max_poll_ms, polls) = std::thread::scope(|scope| {
        let worker = scope.spawn(|| {
            let _worker_scope = profile.enter("worker");
            job_control::run_pending()
        });
        let mut sampled_disk_bytes = queued_disk_bytes;
        let mut max_poll_ms = 0;
        let mut polls = 0;
        let mut last_report = Instant::now();
        while !worker.is_finished() {
            std::thread::sleep(Duration::from_secs(1));
            let (page, poll_ms) = elapsed_ms(|| engine::job_get(&job.job_id).unwrap());
            max_poll_ms = max_poll_ms.max(poll_ms);
            polls += 1;
            assert_eq!(page.item_count as usize, count);
            assert!(page.items.len() <= 50);
            assert!(json_bytes(&page) < 64 * 1024);
            if last_report.elapsed() >= Duration::from_secs(15) {
                let conn = catalog::open_catalog_readonly().unwrap();
                let published: usize = conn
                    .query_row("SELECT COUNT(*) FROM entries", [], |row| row.get(0))
                    .unwrap();
                sampled_disk_bytes =
                    sampled_disk_bytes.max(jobs::dir_size(&catalog::imports_dir()));
                println!(
                    "WINDOW4_PUBLISH_PROGRESS candidateCount={count} published={published} elapsedMs={} pollMs={poll_ms}",
                    publish_started.elapsed().as_millis()
                );
                last_report = Instant::now();
            }
        }
        (
            worker.join().unwrap().unwrap(),
            sampled_disk_bytes,
            max_poll_ms,
            polls,
        )
    });
    let publish_ms = publish_started.elapsed().as_millis();
    drop(poll_scope);
    native.assert_unchanged();
    assert_eq!(completed_jobs, 1);
    let finished_disk_bytes = jobs::dir_size(&catalog::imports_dir());
    report_stage("publish-complete", count, publish_ms);
    profile.report("publish");

    // Drop all preparation/engine caches before reading durable results.
    super::super::reset_runtime();
    engine::reset_runtime();
    let verification_scope = profile.enter("verify");
    let verification_started = Instant::now();
    let (first_page, terminal_page_ms) = elapsed_ms(|| engine::job_get(&job.job_id).unwrap());
    // One cold page read authenticates the whole signed job: the job header is
    // decoded once and every input is hashed from the scanned row itself.
    // The only BLOB handle left is the signed header's scope fact.
    assert_eq!(profile.calls("verify", "job-header-decode"), 1);
    assert_eq!(profile.calls("verify", "acceptance-blob-open"), 1);
    assert_eq!(
        profile.calls("verify", "acceptance-blob-hash") as usize,
        count + 1
    );
    if first_page.state != JobState::Completed {
        report_unfinished_items(&job.job_id);
    }
    assert_eq!(first_page.state, JobState::Completed);
    assert_eq!(first_page.item_count as usize, count);
    let mut page = first_page;
    let conn = catalog::open_catalog().unwrap();
    let mut import_ids = HashSet::new();
    let mut revision_ids = HashSet::new();
    let mut candidate_ids = HashSet::new();
    let mut reader_events = 0;
    let mut source_tokens = 0;
    loop {
        assert_eq!(page.state, JobState::Completed);
        assert!(page.items.len() <= 50);
        assert!(json_bytes(&page) < 64 * 1024);
        for item in &page.items {
            assert_eq!(item.phase, "committed", "{item:?}");
            assert_eq!(item.outcome.as_deref(), Some("complete"));
            assert!(item.error.is_none());
            let import_id = item.import_id.as_deref().unwrap();
            let revision_id = item.revision_id.as_deref().unwrap();
            assert!(candidate_ids.insert(item.candidate_id.clone()));
            assert!(import_ids.insert(import_id.to_owned()));
            assert!(revision_ids.insert(revision_id.to_owned()));
            let entry = catalog::get_entry(&conn, import_id).unwrap().unwrap();
            assert_eq!(entry.current_revision.as_deref(), Some(revision_id));
            let index: usize = item
                .candidate_id
                .strip_prefix("capacity-candidate-")
                .unwrap()
                .parse()
                .unwrap();
            assert!(index < count);
            let mut expected = sample_adapter(index);
            expected.recount();
            let inventory = catalog::revision_inventory(&conn, import_id, revision_id).unwrap();
            assert_eq!(
                inventory, expected.inventory,
                "published inventory for {}",
                item.candidate_id
            );
            source_tokens += inventory.source_tokens;
            let receipt = jobs::latest_receipt(&conn, &job.job_id, &item.candidate_id)
                .unwrap()
                .unwrap();
            assert_eq!(receipt.phase, "committed");
            assert_eq!(receipt.import_id.as_deref(), Some(import_id));
            assert_eq!(receipt.revision_id.as_deref(), Some(revision_id));
            let dir = catalog::revision_dir(import_id, revision_id).unwrap();
            for name in [
                "manifest.json",
                "events.jsonl",
                "assets.json",
                "event-index.sqlite",
            ] {
                assert!(dir.join(name).is_file(), "missing published {name}");
            }
            if index == 0 {
                let manifest: Value =
                    serde_json::from_slice(&std::fs::read(dir.join("manifest.json")).unwrap())
                        .unwrap();
                assert!(!manifest["inventory"]
                    .as_object()
                    .unwrap()
                    .contains_key("sourceTokens"));
            }
            // Includes receipt/hash authorization and the production reader.
            let (events, cursor, total) =
                engine::events_page(import_id, Some(revision_id), None, Some(50), Some("e1"))
                    .unwrap();
            assert_eq!(total, 1);
            assert!(cursor.is_none());
            assert_eq!(events.len(), 1);
            assert_eq!(events[0]["id"], "e1");
            assert!(events[0].to_string().contains("bounded capacity event"));
            reader_events += events.len();
        }
        let Some(cursor) = page.next_item_cursor else {
            break;
        };
        page = engine::job_items_page(&job.job_id, Some(&cursor), Some(50)).unwrap();
    }
    assert_eq!(import_ids.len(), count);
    assert_eq!(revision_ids.len(), count);
    assert_eq!(candidate_ids.len(), count);
    assert_eq!(reader_events, count);
    let counts: (usize, usize, usize) = conn
        .query_row(
            "SELECT (SELECT COUNT(*) FROM entries), (SELECT COUNT(*) FROM revisions),
             (SELECT COUNT(*) FROM receipts WHERE phase='committed')",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(counts, (count, count, count));
    assert_eq!(
        source_tokens,
        (0..count)
            .map(|index| index as u64 * 1_000_000)
            .sum::<u64>()
    );
    drop(conn);
    let (_, catalog_page_ms) = elapsed_ms(|| {
        let (entries, _, total) = engine::list_imported_page(None, Some(50)).unwrap();
        assert_eq!(total as usize, count);
        assert_eq!(entries.len(), count.min(50));
    });
    assert!(
        terminal_page_ms < 500,
        "terminal page took {terminal_page_ms}ms"
    );
    assert!(
        catalog_page_ms < 500,
        "catalog page took {catalog_page_ms}ms"
    );
    // Reopening completed jobs must not publish duplicate revisions or entries.
    assert_eq!(job_control::run_pending().unwrap(), 0);
    native.assert_unchanged();
    let verification_ms = verification_started.elapsed().as_millis();
    drop(verification_scope);
    profile.report("publish-and-verification");
    println!(
        "WINDOW4_FULL_PUBLISH {}",
        serde_json::json!({
            "sessionCount": count,
            "eventsPerSession": 1,
            "publishedEntries": counts.0,
            "publishedRevisions": counts.1,
            "committedReceipts": counts.2,
            "readerEventsVerified": reader_events,
            "sourceTokensKeptOutOfNativeUsage": source_tokens,
            "nativeIsolationVerified": true,
            "publishMs": publish_ms,
            "verificationMs": verification_ms,
            "terminalPageMs": terminal_page_ms,
            "catalogPageMs": catalog_page_ms,
            "concurrentPolls": polls,
            "maxConcurrentJobPollMs": max_poll_ms,
            "sampledMaxLogicalDiskBytes": sampled_disk_bytes.max(finished_disk_bytes),
            "finishedLogicalDiskBytes": finished_disk_bytes,
            "processId": std::process::id(),
        })
    );
}

#[test]
fn full_publication_fixture_checks_worker_receipts_files_and_reader() {
    full_publication_fixture(8);
}

#[test]
#[ignore = "bounded native authentication profile; retains full publication and reader assertions"]
fn bounded_publication_authentication_profile() {
    full_publication_fixture(64);
}

fn full_publication_fixture(count: usize) {
    let home = CapacityHome::new();
    let (batch_id, _) = create_capacity_batch(count);
    let mappings = super::super::mappings(&batch_id, None).unwrap();
    let prepared = super::super::validated_plan_inputs(&batch_id).unwrap();
    let plan = engine::plan_from_validated_prepare(prepared, mappings).unwrap();
    let job = engine::commit(&plan.plan_id, &plan.plan_hash, "full-publish-smoke").unwrap();
    verify(
        &job,
        count,
        jobs::dir_size(&catalog::imports_dir()),
        &home.root,
    );
}

fn committed_job(count: usize, request: &str) -> (jobs::JobRecord, String) {
    let (batch_id, _) = create_capacity_batch(count);
    let mappings = super::super::mappings(&batch_id, None).unwrap();
    let prepared = super::super::validated_plan_inputs(&batch_id).unwrap();
    let plan = engine::plan_from_validated_prepare(prepared, mappings).unwrap();
    let job = engine::commit(&plan.plan_id, &plan.plan_hash, request).unwrap();
    let member_key = format!(
        "publication_acceptance:v1:{}:member:{}",
        job.job_id,
        crate::session_migration::hash::sha256_bytes(job.items[0].candidate_id.as_bytes())
    );
    (job, member_key)
}

fn job_state(conn: &rusqlite::Connection, job_id: &str) -> String {
    conn.query_row(
        "SELECT json_extract(json,'$.state') FROM jobs WHERE job_id=?1",
        [job_id],
        |row| row.get(0),
    )
    .unwrap()
}

/// A trigger fired inside the worker's own write damages an already committed
/// item's signed member. The write fence refuses that transaction, so the
/// damage is rolled back with it and never finalized.
#[test]
fn in_transaction_damage_to_a_committed_member_is_rolled_back() {
    let _home = CapacityHome::new();
    let count = 3;
    let (job, member_key) = committed_job(count, "fence-rollback");
    let conn = catalog::open_catalog().unwrap();
    let original: String = conn
        .query_row(
            "SELECT value FROM meta WHERE key=?1",
            [&member_key],
            |row| row.get(0),
        )
        .unwrap();
    // Test-only persistent trigger: the worker uses its own connection.
    conn.execute_batch(&format!(
        "CREATE TRIGGER damage_after_last AFTER UPDATE OF phase ON job_items
         WHEN NEW.job_id='{job_id}' AND NEW.phase='committed'
          AND (SELECT COUNT(*) FROM job_items WHERE job_id=NEW.job_id AND phase='committed') = {count}
         BEGIN UPDATE meta SET value='{{}}' WHERE key='{member_key}'; END;",
        job_id = job.job_id
    ))
    .unwrap();
    let _ = job_control::run_pending();
    conn.execute_batch("DROP TRIGGER damage_after_last")
        .unwrap();
    let value: String = conn
        .query_row(
            "SELECT value FROM meta WHERE key=?1",
            [&member_key],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(value, original, "the damaging write was rolled back");
    assert_ne!(job_state(&conn, &job.job_id), "completed");
}

/// Damage written by another connection between transactions is not seen by
/// the fence; the writer that would make the job terminal re-proves the
/// complete acceptance and refuses.
#[test]
fn finalizing_rechecks_acceptance_damaged_by_another_connection() {
    let _home = CapacityHome::new();
    let (job, member_key) = committed_job(2, "finalize-recheck");
    job_control::run_pending().unwrap();
    let conn = catalog::open_catalog().unwrap();
    assert_eq!(job_state(&conn, &job.job_id), "completed");
    // Reopen the terminal decision, then damage a committed member out of band.
    job_control::change_job(&conn, &job.job_id, |j| {
        j.state = jobs::JobState::Running;
        Ok(())
    })
    .unwrap();
    let other = catalog::open_catalog().unwrap();
    other
        .execute("UPDATE meta SET value='{}' WHERE key=?1", [&member_key])
        .unwrap();
    let crypto =
        crate::session_migration::publication_crypto::JobCrypto::prepare(&conn, &job.job_id)
            .unwrap();
    assert!(job_control::finalize_job(&conn, &job.job_id, &crypto).is_err());
    assert_eq!(job_state(&conn, &job.job_id), "running");
}

/// Name the items that kept a job from completing cleanly (at most 20), so a
/// long capacity run that ends with issues is diagnosable after cleanup.
fn report_unfinished_items(job_id: &str) {
    let mut cursor: Option<String> = None;
    let mut reported = 0;
    loop {
        let page = engine::job_items_page(job_id, cursor.as_deref(), Some(50)).unwrap();
        for item in &page.items {
            if item.phase != "committed" || item.outcome.as_deref() != Some("complete") {
                println!(
                    "WINDOW4_UNFINISHED_ITEM candidate={} phase={} outcome={:?} error={:?}",
                    item.candidate_id, item.phase, item.outcome, item.error
                );
                reported += 1;
                if reported == 20 {
                    return;
                }
            }
        }
        match page.next_item_cursor {
            Some(next) => cursor = Some(next),
            None => return,
        }
    }
}
