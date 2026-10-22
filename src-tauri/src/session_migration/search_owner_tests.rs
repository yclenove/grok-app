use super::*;
use crate::session_migration::types::{ContentBlock, EventBase, MessageRole, TimeOrigin};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::JoinHandle;
use std::time::Duration;

type BackupGate = Option<(Sender<()>, Receiver<()>)>;

thread_local! {
    static BACKUP_GATE: std::cell::RefCell<BackupGate> = const {
        std::cell::RefCell::new(None)
    };
}

pub(super) fn after_backup() {
    BACKUP_GATE.with(|gate| {
        if let Some((ready, release)) = gate.borrow_mut().take() {
            ready.send(()).unwrap();
            release.recv_timeout(Duration::from_secs(15)).unwrap();
        }
    });
}

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let fixture = Self(std::env::temp_dir().join(format!("idx-owner-{}", Uuid::new_v4())));
        let events = ["first", "second"].map(|id| ImportedEvent::Message {
            base: EventBase {
                id: id.into(),
                ordinal: u32::from(id == "second"),
                spans: vec![],
                timestamp: None,
                time_origin: TimeOrigin::Unknown,
            },
            role: MessageRole::User,
            blocks: vec![ContentBlock::text("synthetic immutable archive")],
            complete: true,
        });
        build_index(&fixture.0, &events).unwrap();
        fixture
    }

    fn snapshot(&self) -> BTreeMap<String, Vec<u8>> {
        std::fs::read_dir(&self.0)
            .unwrap()
            .map(|entry| {
                let entry = entry.unwrap();
                let bytes = if entry.file_name() == owner::LOCK_NAME {
                    // Windows byte locks reject even reading an empty sentinel.
                    // Compare its metadata, and keep archive/index comparisons exact.
                    let metadata = entry.metadata().unwrap();
                    format!(
                        "lock:{:?}:{:?}:{}:{}",
                        metadata.created(),
                        metadata.modified(),
                        metadata.len(),
                        metadata.permissions().readonly(),
                    )
                    .into_bytes()
                } else {
                    std::fs::read(entry.path()).unwrap()
                };
                (entry.file_name().to_string_lossy().into_owned(), bytes)
            })
            .collect()
    }

    fn assert_complete(&self) {
        let conn = open_index(&self.0).unwrap();
        let mut statement = conn
            .prepare("SELECT event_id FROM events ORDER BY ordinal")
            .unwrap();
        let ids: Vec<String> = statement
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(ids, ["first", "second"]);
        assert!(!self.0.join("event-index.sqlite.tmp").exists());
        assert!(!index_backup_path(&index_path(&self.0)).exists());
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub(crate) struct PausedBuilder {
    release: Option<Sender<()>>,
    join: Option<JoinHandle<Result<(), ImportError>>>,
}

impl PausedBuilder {
    pub(crate) fn start(dir: &Path) -> Self {
        let (ready_tx, ready_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let dir = dir.to_path_buf();
        let join = std::thread::spawn(move || {
            BACKUP_GATE.with(|gate| *gate.borrow_mut() = Some((ready_tx, release_rx)));
            rebuild_index(&dir, &|| false)
        });
        let owner = Self {
            release: Some(release_tx),
            join: Some(join),
        };
        ready_rx.recv_timeout(Duration::from_secs(10)).unwrap();
        owner
    }

    pub(crate) fn finish(mut self) -> Result<(), ImportError> {
        self.release.take().unwrap().send(()).unwrap();
        self.join.take().unwrap().join().unwrap()
    }
}

impl Drop for PausedBuilder {
    fn drop(&mut self) {
        if let Some(release) = self.release.take() {
            let _ = release.send(());
        }
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

#[test]
fn live_recovery_does_not_consume_active_builders_temp_file() {
    let fixture = Fixture::new();
    let owner = PausedBuilder::start(&fixture.0);
    let before = fixture.snapshot();
    let contender = ensure_index(&fixture.0);
    let after = fixture.snapshot();
    let published = owner.finish();
    assert!(
        published.is_ok(),
        "a live builder lost its temporary file: {published:?}"
    );
    assert!(
        before == after,
        "contending recovery mutated owner artifacts"
    );
    let error = contender.expect_err("active ownership must be retryable");
    assert_eq!(error.code, ErrorCode::SourceBusy);
    assert_eq!(error.retry, "retry");
    fixture.assert_complete();
}

#[test]
fn reader_open_during_index_swap_is_retryable_not_missing_history() {
    let fixture = Fixture::new();
    let owner = PausedBuilder::start(&fixture.0);
    let contender = open_index(&fixture.0);
    let published = owner.finish();
    assert!(
        published.is_ok(),
        "reader interfered with index publication"
    );
    let error = contender.expect_err("reader must respect active ownership");
    assert_eq!(error.code, ErrorCode::SourceBusy);
    assert_eq!(error.retry, "retry");
    fixture.assert_complete();
}

#[test]
fn another_revision_can_build_while_one_owner_is_paused() {
    let fixture = Fixture::new();
    let other = Fixture::new();
    let owner = PausedBuilder::start(&fixture.0);
    let independent = rebuild_index(&other.0, &|| false);
    let published = owner.finish();
    independent.unwrap();
    published.unwrap();
    fixture.assert_complete();
    other.assert_complete();
}

#[test]
fn cancellation_while_waiting_preserves_active_owner_artifacts() {
    let fixture = Fixture::new();
    let owner = PausedBuilder::start(&fixture.0);
    let before = fixture.snapshot();
    let calls = std::cell::Cell::new(0);
    let cancelled = rebuild_index(&fixture.0, &|| {
        calls.set(calls.get() + 1);
        calls.get() >= 4
    });
    let after = fixture.snapshot();
    owner.finish().unwrap();
    assert_eq!(cancelled.unwrap_err().code, ErrorCode::PlanStale);
    assert!(
        before == after,
        "cancelled contender mutated owner artifacts"
    );
    fixture.assert_complete();
}

#[test]
fn future_schema_rejection_does_not_create_an_owner_file() {
    let fixture = Fixture::new();
    std::fs::remove_file(fixture.0.join(owner::LOCK_NAME)).unwrap();
    let conn = Connection::open(index_path(&fixture.0)).unwrap();
    conn.execute_batch("PRAGMA user_version=99;").unwrap();
    drop(conn);
    let before = fixture.snapshot();
    for rebuild in [false, true] {
        let result = if rebuild {
            rebuild_index(&fixture.0, &|| false)
        } else {
            ensure_index(&fixture.0)
        };
        assert_eq!(result.unwrap_err().code, ErrorCode::ImportSchemaTooNew);
        assert!(before == fixture.snapshot());
    }
}

#[test]
fn locked_schema_rejection_does_not_create_an_owner_file() {
    let fixture = Fixture::new();
    std::fs::remove_file(fixture.0.join(owner::LOCK_NAME)).unwrap();
    let conn = Connection::open(index_path(&fixture.0)).unwrap();
    conn.execute_batch("BEGIN EXCLUSIVE;").unwrap();
    let before = fixture.snapshot();
    let result = ensure_index(&fixture.0);
    let after = fixture.snapshot();
    conn.execute_batch("ROLLBACK;").unwrap();
    let error = result.unwrap_err();
    assert_eq!(error.code, ErrorCode::HistoryAccessUnavailable);
    assert_eq!(error.retry, "retry");
    assert!(before == after);
}

#[test]
fn existing_readonly_owner_can_read_current_index() {
    let fixture = Fixture::new();
    let path = fixture.0.join(owner::LOCK_NAME);
    let original = std::fs::metadata(&path).unwrap().permissions();
    let mut readonly = original.clone();
    readonly.set_readonly(true);
    std::fs::set_permissions(&path, readonly).unwrap();
    let opened = open_index(&fixture.0);
    std::fs::set_permissions(&path, original).unwrap();
    drop(opened.expect("readable index must not need writable existing coordination state"));
    fixture.assert_complete();
}

#[test]
fn legacy_current_index_without_sentinel_keeps_exact_bytes() {
    let fixture = Fixture::new();
    let index = index_path(&fixture.0);
    let archive = fixture.0.join("events.jsonl");
    let index_bytes = std::fs::read(&index).unwrap();
    let archive_bytes = std::fs::read(&archive).unwrap();
    let lock = fixture.0.join(owner::LOCK_NAME);
    std::fs::remove_file(&lock).unwrap();

    ensure_index(&fixture.0).expect("a current legacy index must not be rebuilt");
    assert_eq!(std::fs::read(&index).unwrap(), index_bytes);
    assert_eq!(std::fs::read(&archive).unwrap(), archive_bytes);
    let created = std::fs::symlink_metadata(&lock).unwrap();
    assert!(created.is_file());
    assert!(!created.file_type().is_symlink());
    drop(open_index(&fixture.0).expect("legacy index remains readable"));
    assert_eq!(std::fs::read(&index).unwrap(), index_bytes);
    assert_eq!(std::fs::read(&archive).unwrap(), archive_bytes);
    assert!(!fixture.0.join("event-index.sqlite.tmp").exists());
    assert!(!index_backup_path(&index).exists());
    fixture.assert_complete();
}

#[test]
fn readonly_sentinel_allows_ensure_without_index_rewrite() {
    let fixture = Fixture::new();
    let index = index_path(&fixture.0);
    let archive = fixture.0.join("events.jsonl");
    let index_bytes = std::fs::read(&index).unwrap();
    let archive_bytes = std::fs::read(&archive).unwrap();
    let lock = fixture.0.join(owner::LOCK_NAME);
    let lock_bytes = std::fs::read(&lock).unwrap();
    let original = std::fs::metadata(&lock).unwrap().permissions();
    let mut readonly = original.clone();
    readonly.set_readonly(true);
    std::fs::set_permissions(&lock, readonly).unwrap();
    let ensured = ensure_index(&fixture.0);
    let opened = open_index(&fixture.0);
    let index_after = std::fs::read(&index).unwrap();
    let archive_after = std::fs::read(&archive).unwrap();
    let lock_after = std::fs::read(&lock).unwrap();
    let still_readonly = std::fs::metadata(&lock).unwrap().permissions().readonly();
    std::fs::set_permissions(&lock, original).unwrap();

    ensured.expect("an existing readonly sentinel must still admit the current index");
    drop(opened.expect("readonly sentinel remains readable after ensure"));
    assert_eq!(index_after, index_bytes);
    assert_eq!(archive_after, archive_bytes);
    assert_eq!(lock_after, lock_bytes);
    assert!(still_readonly);
    assert!(!fixture.0.join("event-index.sqlite.tmp").exists());
    assert!(!index_backup_path(&index).exists());
    fixture.assert_complete();
}

#[test]
fn nonregular_sentinel_refuses_ownership_and_keeps_index_bytes() {
    let fixture = Fixture::new();
    let index = index_path(&fixture.0);
    let archive = fixture.0.join("events.jsonl");
    let index_bytes = std::fs::read(&index).unwrap();
    let archive_bytes = std::fs::read(&archive).unwrap();
    let lock = fixture.0.join(owner::LOCK_NAME);
    std::fs::remove_file(&lock).unwrap();
    std::fs::create_dir(&lock).unwrap();

    for rebuild in [false, true] {
        let result = if rebuild {
            rebuild_index(&fixture.0, &|| false)
        } else {
            ensure_index(&fixture.0)
        };
        let error = result.expect_err("a directory sentinel must not become success");
        assert_eq!(error.code, ErrorCode::HistoryAccessUnavailable);
        assert_eq!(error.retry, "retry");
        assert!(error.message.contains("not a regular file"));
        assert_eq!(std::fs::read(&index).unwrap(), index_bytes);
        assert_eq!(std::fs::read(&archive).unwrap(), archive_bytes);
        assert!(std::fs::metadata(&lock).unwrap().is_dir());
        assert!(!fixture.0.join("event-index.sqlite.tmp").exists());
        assert!(!index_backup_path(&index).exists());
    }
    let opened = open_index(&fixture.0).expect_err("a reader must not bypass a directory sentinel");
    assert_eq!(opened.code, ErrorCode::HistoryAccessUnavailable);
    assert_eq!(opened.retry, "retry");
    assert_eq!(std::fs::read(&index).unwrap(), index_bytes);
    assert!(std::fs::metadata(&lock).unwrap().is_dir());
}

#[test]
fn corrupt_index_without_sentinel_is_not_replaced() {
    let fixture = Fixture::new();
    let index = index_path(&fixture.0);
    let archive = fixture.0.join("events.jsonl");
    let archive_bytes = std::fs::read(&archive).unwrap();
    std::fs::remove_file(fixture.0.join(owner::LOCK_NAME)).unwrap();
    std::fs::write(&index, b"not-a-sqlite").unwrap();

    for _ in 0..2 {
        let error = ensure_index(&fixture.0).expect_err("corrupt index must fail closed");
        assert_eq!(error.code, ErrorCode::HistoryAccessUnavailable);
        assert_eq!(error.retry, "retry");
        assert_eq!(std::fs::read(&index).unwrap(), b"not-a-sqlite");
        assert_eq!(std::fs::read(&archive).unwrap(), archive_bytes);
        assert!(!fixture.0.join("event-index.sqlite.tmp").exists());
        assert!(!index_backup_path(&index).exists());
    }
}

#[path = "search_owner_process_tests.rs"]
mod process_tests;
