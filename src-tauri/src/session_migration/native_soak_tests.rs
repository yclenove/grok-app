//! Opt-in timed backend workload, not a UI/ACP or cross-platform acceptance test.
//! One process retains caches and durable state between cycles. The external
//! runner owns the fresh temporary root and samples the exact child process.
use super::*;
use crate::session_migration::{catalog, engine, jobs};
use crate::store;
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const BATCH: usize = 64;
const EVENTS: usize = 16;
const NEEDLE: &str = "soakneedle";

#[path = "native_soak_support.rs"]
mod support;
use support::{Counters, Metrics, SoakHome};

fn fixture(candidate: &str) -> CommitItem {
    let messages: Vec<_> = (0..EVENTS)
        .map(|index| {
            serde_json::json!({
                "role": if index % 2 == 0 { "user" } else { "assistant" },
                "content": format!("{NEEDLE} 合成记录 {candidate} #{index}"),
            })
        })
        .collect();
    let bytes = serde_json::to_vec(&messages).unwrap();
    let mut adapter =
        crate::session_migration::adapters::generic::normalize_bytes(&bytes, "synthetic-soak.json")
            .unwrap();
    adapter.graph.native_session_id = candidate.to_owned();
    jobs::prepare_test_commit_item(CommitItem {
        candidate_id: candidate.to_owned(),
        installation_id: "synthetic-soak".into(),
        provider: "generic-file".into(),
        root_path: "synthetic-memory-only".into(),
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

fn receipt_map(id: &str) -> BTreeMap<String, String> {
    let conn = catalog::open_catalog_readonly().unwrap();
    let mut query = conn.prepare(
        "SELECT receipt_id,json FROM receipts WHERE job_id=?1 AND phase='committed' ORDER BY receipt_id",
    ).unwrap();
    query
        .query_map([id], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

struct NativeWriter {
    session: store::SessionMeta,
    turns: usize,
    total_turns: usize,
    completed: Vec<(String, usize)>,
}

impl NativeWriter {
    fn new() -> Self {
        Self {
            session: store::create_session(None, Some("Synthetic soak".into()), false).unwrap(),
            turns: 0,
            total_turns: 0,
            completed: vec![],
        }
    }

    fn tick(&mut self) {
        // Bounded individual conversation length, without deleting any history.
        if self.turns == 128 {
            self.completed.push((self.session.id.clone(), self.turns));
            self.session =
                store::create_session(None, Some("Synthetic soak".into()), false).unwrap();
            self.turns = 0;
        }
        let message = |role: &str, content: &str| store::ChatMessageStored {
            id: format!("{role}-{}", self.turns),
            role: role.to_owned(),
            content: content.to_owned(),
            thought: None,
            created_at: chrono::Utc::now(),
            is_error: false,
            attachments: None,
            marker: None,
        };
        let user = message("user", "synthetic request");
        let mut answer = message("assistant", "synthetic streaming");
        store::append_message(&self.session.id, user.clone()).unwrap();
        store::append_message(&self.session.id, answer.clone()).unwrap();
        answer.content = "synthetic completed".into();
        store::save_messages(&self.session.id, &[user, answer]).unwrap();
        self.turns += 1;
        self.total_turns += 1;
        self.session.title = format!("Synthetic soak turn {}", self.turns);
        self.session.updated_at = chrono::Utc::now();
        store::update_runtime_session_meta(&self.session).unwrap();
    }

    fn verify(&self) {
        let mut sessions = self.completed.clone();
        sessions.push((self.session.id.clone(), self.turns));
        let index = store::load_sessions_index();
        assert_eq!(
            index.len(),
            sessions.len(),
            "imports must not become native rows"
        );
        for (id, turns) in sessions {
            let stored = store::load_messages(&id);
            assert_eq!(stored.len(), turns * 2);
            for turn in 0..turns {
                assert_eq!(stored[turn * 2].id, format!("user-{turn}"));
                assert_eq!(stored[turn * 2 + 1].id, format!("assistant-{turn}"));
                assert_eq!(stored[turn * 2 + 1].content, "synthetic completed");
            }
            assert_eq!(index.iter().filter(|row| row.id == id).count(), 1);
        }
    }
}

fn read_pinned(id: &str, revision: &str, leaf: &str, counters: &mut Counters) {
    let (first, cursor, total) =
        engine::events_page(id, Some(revision), None, Some(7), Some(leaf)).unwrap();
    assert_eq!(total as usize, EVENTS);
    assert_eq!(first.len(), 7);
    let (second, next, total) =
        engine::events_page(id, Some(revision), cursor.as_deref(), Some(7), Some(leaf)).unwrap();
    assert_eq!(total as usize, EVENTS);
    assert_eq!(second.len(), 7);
    let (last, end, _) =
        engine::events_page(id, Some(revision), next.as_deref(), Some(7), Some(leaf)).unwrap();
    assert_eq!(last.len(), 2);
    assert!(end.is_none());
    let ids: std::collections::BTreeSet<_> = first
        .iter()
        .chain(&second)
        .chain(&last)
        .map(|event| event["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids.len(), EVENTS);
    counters.reader_pages += 3;
    let (hits, cursor) =
        engine::search_imported_with_leaf(id, Some(revision), Some(leaf), NEEDLE, None, Some(5))
            .unwrap();
    assert_eq!(hits.len(), 5);
    assert!(cursor.is_some());
    let (next_hits, _) = engine::search_imported_with_leaf(
        id,
        Some(revision),
        Some(leaf),
        NEEDLE,
        cursor.as_deref(),
        Some(5),
    )
    .unwrap();
    assert_eq!(next_hits.len(), 5);
    counters.reader_searches += 2;
}

struct Workload<'a> {
    metrics: &'a mut Metrics,
    native: NativeWriter,
    pinned_id: String,
    pinned_revision: String,
    pinned_leaf: String,
}

impl Workload<'_> {
    fn run(&mut self, queued: &JobRecord, should_cancel: bool) -> BTreeMap<String, String> {
        let id = &queued.job_id;
        let mut preserved = BTreeMap::new();
        let mut cancel_at = None;
        let started = Instant::now();
        // A retried job already has committed progress. Only newly observed
        // progress in this worker invocation can establish overlap.
        let mut previous = engine::job_get(id).unwrap().progress.unwrap().done;
        self.metrics.counters.job_polls += 1;
        let mut overlapping_steps = 0;
        let stopped_at = std::thread::scope(|scope| {
            let worker = scope.spawn(|| {
                let result = run_pending();
                (result, Instant::now())
            });
            while !worker.is_finished() {
                self.metrics.check_stop();
                if started.elapsed() >= Duration::from_secs(120) {
                    self.metrics.worker_deadline(started.elapsed());
                    panic!("worker deadline exceeded");
                }
                let poll_start = Instant::now();
                let status = engine::job_get(id).unwrap();
                self.metrics.counters.job_polls += 1;
                self.metrics.observe("jobPoll", poll_start.elapsed());
                assert_eq!(status.item_count as usize, BATCH);
                assert!(status.items.len() <= 50);
                let page = engine::job_items_page(id, status.next_item_cursor.as_deref(), Some(50))
                    .unwrap();
                assert_eq!(page.items.len(), BATCH - 50);
                self.metrics.counters.job_pages += 1;
                let done = status.progress.as_ref().unwrap().done;
                if done > previous && done < BATCH as u32 {
                    overlapping_steps += 1;
                    previous = done;
                }
                if should_cancel && cancel_at.is_none() && done > 0 && done < BATCH as u32 {
                    preserved = receipt_map(id);
                    let requested = Instant::now();
                    let response = engine::cancel_job(id).unwrap();
                    self.metrics.observe("cancelAck", requested.elapsed());
                    self.metrics.counters.cancels += 1;
                    assert!(matches!(
                        response.state,
                        JobState::Cancelling | JobState::Cancelled
                    ));
                    cancel_at = Some(requested);
                }
                let native_start = Instant::now();
                self.native.tick();
                self.metrics.observe("nativeWrite", native_start.elapsed());
                self.metrics.counters.native_turns = self.native.total_turns as u64;
                let reader_start = Instant::now();
                read_pinned(
                    &self.pinned_id,
                    &self.pinned_revision,
                    &self.pinned_leaf,
                    &mut self.metrics.counters,
                );
                self.metrics.observe("readerRound", reader_start.elapsed());
                self.metrics.heartbeat(Some(&status), false);
                std::thread::sleep(Duration::from_millis(20));
            }
            let (result, stopped_at) = worker.join().unwrap();
            assert_eq!(result.unwrap(), 1);
            stopped_at
        });
        assert!(
            overlapping_steps > 0,
            "no observed in-flight progress overlap"
        );
        self.metrics.counters.overlap_steps += overlapping_steps;
        if should_cancel {
            self.metrics.observe(
                "workerStop",
                stopped_at.duration_since(cancel_at.expect("cancel window not exercised")),
            );
        }
        self.metrics.observe("workerRound", started.elapsed());
        preserved
    }
}

#[test]
#[ignore = "opt-in synthetic backend load: use scripts/session-import-soak.ps1"]
fn timed_active_workload() {
    let home = SoakHome::from_environment();
    let seconds = std::env::var("GROK_IMPORT_SOAK_SECONDS")
        .unwrap()
        .parse::<u64>()
        .unwrap();
    assert!((10..=86400).contains(&seconds));
    let mut metrics = Metrics::new(&home.root, seconds);
    let pinned = fixture("pinned-reader");
    let pinned_leaf = pinned.selection_set.single_entry().unwrap().leaf_id.clone();
    let pin = enqueue(job(), vec![pinned]).unwrap();
    assert_eq!(run_pending().unwrap(), 1);
    let pin = engine::job_get(&pin.job_id).unwrap();
    assert_eq!(pin.state, JobState::Completed);
    let pinned_id = pin.items[0].import_id.clone().unwrap();
    let pinned_revision = pin.items[0].revision_id.clone().unwrap();
    let original = std::fs::read(
        revision_dir(&pinned_id, &pinned_revision)
            .unwrap()
            .join("events.jsonl"),
    )
    .unwrap();
    let mut load = Workload {
        metrics: &mut metrics,
        native: NativeWriter::new(),
        pinned_id,
        pinned_revision,
        pinned_leaf,
    };
    while load.metrics.started.elapsed() < Duration::from_secs(seconds) {
        load.metrics.check_stop();
        let cycle = load.metrics.counters.cycles;
        let prepare_start = Instant::now();
        let inputs = (0..BATCH)
            .map(|index| fixture(&format!("cycle-{cycle}-{index}")))
            .collect();
        let queued = enqueue(job(), inputs).unwrap();
        load.metrics
            .observe("fixtureEnqueue", prepare_start.elapsed());
        let should_cancel = cycle.is_multiple_of(2);
        let preserved = load.run(&queued, should_cancel);
        if should_cancel {
            let stopped = engine::job_get(&queued.job_id).unwrap();
            assert_eq!(stopped.state, JobState::Cancelled);
            assert!(stopped.progress.unwrap().retryable > 0);
            let retry_start = Instant::now();
            let retry = engine::retry_failed(&queued.job_id).unwrap();
            load.metrics.observe("retryAck", retry_start.elapsed());
            load.metrics.counters.retries += 1;
            assert_eq!(retry.state, JobState::Queued);
            load.run(&retry, false);
        }
        let done = engine::job_get(&queued.job_id).unwrap();
        assert_eq!(done.state, JobState::Completed);
        assert_eq!(
            done.progress,
            Some(jobs::JobProgress {
                done: BATCH as u32,
                retryable: 0
            })
        );
        let receipts = receipt_map(&queued.job_id);
        assert_eq!(receipts.len(), BATCH);
        for (id, value) in preserved {
            assert_eq!(receipts.get(&id), Some(&value));
        }
        let conn = catalog::open_catalog_readonly().unwrap();
        let count = (cycle + 1) * BATCH as u64 + 1;
        for table in ["entries", "revisions"] {
            let total: u64 = conn
                .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                    row.get(0)
                })
                .unwrap();
            assert_eq!(total, count, "{table} diverged");
        }
        drop(conn);
        load.native.verify();
        load.metrics.counters.cycles += 1;
        load.metrics.counters.committed_imports += BATCH as u64;
        load.metrics.heartbeat(Some(&done), true);
    }
    assert!(load.metrics.counters.cycles > 0);
    assert!(load.metrics.counters.cancels > 0);
    assert!(load.metrics.counters.retries > 0);
    let after = std::fs::read(
        revision_dir(&load.pinned_id, &load.pinned_revision)
            .unwrap()
            .join("events.jsonl"),
    )
    .unwrap();
    assert_eq!(original, after, "pinned reader revision changed");
    load.native.verify();
    load.metrics.finish();
}

#[test]
fn fixture_leaf_drives_real_reader_pagination_and_search() {
    let _home = Home::new();
    let item = fixture("reader-fixture");
    let leaf = item.selection_set.single_entry().unwrap().leaf_id.clone();
    let queued = enqueue(job(), vec![item]).unwrap();
    assert_eq!(run_pending().unwrap(), 1);
    let done = engine::job_get(&queued.job_id).unwrap();
    let mut counts = Counters::default();
    read_pinned(
        done.items[0].import_id.as_deref().unwrap(),
        done.items[0].revision_id.as_deref().unwrap(),
        &leaf,
        &mut counts,
    );
    assert_eq!(counts.reader_pages, 3);
    assert_eq!(counts.reader_searches, 2);
}
