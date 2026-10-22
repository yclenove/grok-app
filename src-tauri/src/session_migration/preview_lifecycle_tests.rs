//! Deterministic overlap tests; all paths belong to a synthetic App Home.
use std::cell::RefCell;
use std::sync::mpsc;
use std::time::Duration;

use super::catalog;
use super::job_control;
use super::jobs::{self, CommitItem, JobRecord, JobState};
use super::prepared_builder::PreparedRevisionBuilder;
use super::test_home::IsolatedAppHome;

#[path = "preview_retention_tests.rs"]
mod preview_retention_tests;

type PublishHook = (&'static str, Box<dyn FnOnce()>);

thread_local! {
    static BEFORE_ENQUEUE_COMMIT: RefCell<Option<Box<dyn FnOnce()>>> = RefCell::new(None);
    static AFTER_ENQUEUE_COMMIT: RefCell<Option<Box<dyn FnOnce()>>> = RefCell::new(None);
    static AFTER_ENQUEUE_RETENTION: RefCell<Option<Box<dyn FnOnce()>>> = RefCell::new(None);
    static PUBLISH_POINT: RefCell<Option<PublishHook>> = RefCell::new(None);
}

pub(super) fn test_publish_point(point: &'static str, hook: impl FnOnce() + 'static) {
    PUBLISH_POINT.with(|slot| *slot.borrow_mut() = Some((point, Box::new(hook))));
}

pub(super) fn after_publish_point(point: &'static str) {
    let hook = PUBLISH_POINT.with(|slot| {
        let mut value = slot.borrow_mut();
        if value
            .as_ref()
            .is_some_and(|(expected, _)| *expected == point)
        {
            value.take().map(|(_, hook)| hook)
        } else {
            None
        }
    });
    if let Some(hook) = hook {
        hook();
    }
}

pub(super) fn before_enqueue_commit() {
    let hook = BEFORE_ENQUEUE_COMMIT.with(|hook| hook.borrow_mut().take());
    if let Some(hook) = hook {
        hook();
    }
}

pub(super) fn after_enqueue_commit() {
    if let Some(hook) = AFTER_ENQUEUE_COMMIT.with(|hook| hook.borrow_mut().take()) {
        hook();
    }
}

pub(super) fn test_after_enqueue_commit(hook: impl FnOnce() + 'static) {
    AFTER_ENQUEUE_COMMIT.with(|slot| *slot.borrow_mut() = Some(Box::new(hook)));
}

pub(super) fn test_before_enqueue_commit(hook: impl FnOnce() + 'static) {
    BEFORE_ENQUEUE_COMMIT.with(|slot| *slot.borrow_mut() = Some(Box::new(hook)));
}

pub(super) fn association_crash_report(home: &IsolatedAppHome) -> serde_json::Value {
    association_crash_report_at(home, "association-after-commit")
}

pub(super) fn association_crash_report_at(home: &IsolatedAppHome, mode: &str) -> serde_json::Value {
    assert!(
        mode.starts_with("association-publication-")
            || matches!(
                mode,
                "association-before-commit" | "association-after-commit"
            )
    );
    let (mut child, signal) = start_child(home, mode);
    let expected = if mode.starts_with("association-publication-") {
        77
    } else if mode == "association-before-commit" {
        73
    } else {
        74
    };
    assert_eq!(child.0.wait().unwrap().code(), Some(expected));
    serde_json::from_slice(&std::fs::read(signal).unwrap()).unwrap()
}

pub(super) fn after_enqueue_retention() {
    if let Some(hook) = AFTER_ENQUEUE_RETENTION.with(|hook| hook.borrow_mut().take()) {
        hook();
    }
}

fn input() -> CommitItem {
    input_with_candidate("concurrent")
}

fn input_with_candidate(candidate: &str) -> CommitItem {
    let adapter = super::adapters::generic::normalize_bytes(
        br#"[{"role":"user","content":"synthetic concurrency input"}]"#,
        "synthetic.json",
    )
    .unwrap();
    jobs::prepare_test_commit_item(CommitItem {
        candidate_id: candidate.into(),
        installation_id: "installation".into(),
        provider: "generic-file".into(),
        root_path: "fixture-root".into(),
        adapter,
        prepared: None,
        options: serde_json::json!({}),
        mapping_project_id: None,
        explicit_reimport: false,
        create_project: false,
        import_unassigned_anyway: false,
        explicit_target: None,
        identity_association: None,
        selection_set: Default::default(),
    })
}

fn job() -> JobRecord {
    JobRecord {
        job_id: uuid::Uuid::new_v4().to_string(),
        request_id: uuid::Uuid::new_v4().to_string(),
        plan_id: "synthetic-plan".into(),
        plan_hash: "synthetic-hash".into(),
        state: JobState::Queued,
        state_version: 1,
        items: Vec::new(),
        created_at: chrono::Utc::now().to_rfc3339(),
        item_count: 0,
        next_item_cursor: None,
        progress: None,
    }
}

#[test]
fn cleanup_does_not_delete_a_preview_while_builder_is_writing() {
    let _home = IsolatedAppHome::acquire("preview-building");
    let _catalog = catalog::open_catalog().unwrap();
    let builder = PreparedRevisionBuilder::new().unwrap();
    let path = builder.path().to_path_buf();
    let removed = jobs::cleanup_unreferenced_preview_dirs().unwrap();
    assert_eq!(
        removed, 0,
        "in-progress directory has not written a lease yet"
    );
    assert!(path.is_dir());
    drop(builder);
    assert!(
        !path.exists(),
        "the owner still discards its failed construction"
    );
}

#[test]
fn cleanup_during_enqueue_cannot_delete_a_soon_to_be_durable_input() {
    let _home = IsolatedAppHome::acquire("preview-enqueue-race");
    let conn = catalog::open_catalog().unwrap();
    let item = input();
    let path = item.prepared.as_ref().unwrap().path.clone();
    let requested = job();
    let job_id = requested.job_id.clone();
    let (ready_tx, ready_rx) = mpsc::channel();
    let (resume_tx, resume_rx) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        BEFORE_ENQUEUE_COMMIT.with(|hook| {
            *hook.borrow_mut() = Some(Box::new(move || {
                ready_tx.send(()).unwrap();
                resume_rx.recv_timeout(Duration::from_secs(10)).unwrap();
            }))
        });
        job_control::enqueue(requested, vec![item])
    });
    ready_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    let cleanup = jobs::cleanup_unreferenced_preview_dirs();
    let retained = path.is_dir();
    // Always release/join before asserting, so a red assertion cannot hang.
    resume_tx.send(()).unwrap();
    worker.join().unwrap().unwrap();
    assert!(
        retained,
        "cleanup removed an in-flight enqueue input: {cleanup:?}"
    );
    assert_eq!(cleanup.unwrap(), 0);
    assert_eq!(job_control::run_pending().unwrap(), 1);
    assert_eq!(
        jobs::get_job(&conn, &job_id).unwrap().unwrap().state,
        JobState::Completed
    );
    assert!(
        !path.exists(),
        "completed unreferenced preview is still reclaimable"
    );
}

#[test]
fn cleanup_locks_are_shared_nested_and_released() {
    let _home = IsolatedAppHome::acquire("preview-nested-lock");
    let first = super::preview_lifecycle::operation().unwrap();
    let second = super::preview_lifecycle::operation().unwrap();
    let (ready_tx, ready_rx) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        let guard = super::preview_lifecycle::operation().unwrap();
        ready_tx.send(guard).unwrap();
    });
    let other_thread = ready_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    worker.join().unwrap();
    assert!(super::preview_lifecycle::try_cleanup().unwrap().is_none());
    drop(first);
    drop(second);
    assert!(super::preview_lifecycle::try_cleanup().unwrap().is_none());
    drop(other_thread);
    let cleanup = super::preview_lifecycle::try_cleanup().unwrap().unwrap();
    assert_eq!(
        super::preview_lifecycle::operation().unwrap_err().code,
        super::errors::ErrorCode::SourceBusy
    );
    drop(cleanup);
    drop(super::preview_lifecycle::operation().unwrap());
}

#[test]
fn cleanup_winning_before_enqueue_rejects_without_a_durable_job() {
    let _home = IsolatedAppHome::acquire("preview-cleanup-first");
    let conn = catalog::open_catalog().unwrap();
    let item = input();
    let path = item.prepared.as_ref().unwrap().path.clone();
    let requested = job();
    let job_id = requested.job_id.clone();
    jobs::expire_preview_lease_file(&path).unwrap();
    assert_eq!(jobs::cleanup_unreferenced_preview_dirs().unwrap(), 1);
    assert!(job_control::enqueue(requested, vec![item]).is_err());
    assert!(jobs::get_job(&conn, &job_id).unwrap().is_none());
}

#[test]
fn concurrent_lease_updates_preserve_retention_expiry_and_batch() {
    let _home = IsolatedAppHome::acquire("preview-lease-update");
    let _conn = catalog::open_catalog().unwrap();
    let item = input();
    let path = item.prepared.as_ref().unwrap().path.clone();
    jobs::expire_preview_lease_file(&path).unwrap();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
    let workers: Vec<_> = (0..3)
        .map(|operation| {
            let path = path.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                for _ in 0..12 {
                    match operation {
                        0 => jobs::mark_preview_retained(&path),
                        1 => jobs::renew_preview_lease(&path, 300),
                        _ => jobs::adopt_preview_for_batch(&path, "synthetic-batch"),
                    }
                    .unwrap();
                }
            })
        })
        .collect();
    for worker in workers {
        worker.join().unwrap();
    }
    let lease = jobs::read_preview_lease(&path).unwrap().unwrap();
    assert!(lease.retained_by_job);
    assert!(lease.expires_unix > chrono::Utc::now().timestamp());
    assert_eq!(lease.prepare_batch_id.as_deref(), Some("synthetic-batch"));
    assert_eq!(
        jobs::prepared_hashes(&path).unwrap(),
        item.prepared.as_ref().unwrap().file_hashes
    );
    assert!(!std::fs::read_dir(&path).unwrap().any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".lease-")
    }));
}

#[test]
fn missing_lease_updates_and_mismatched_cleanup_fail_closed() {
    let _home = IsolatedAppHome::acquire("preview-lease-invalid");
    let _conn = catalog::open_catalog().unwrap();
    let item = input();
    let path = item.prepared.as_ref().unwrap().path.clone();
    let mut lease = jobs::read_preview_lease(&path).unwrap().unwrap();
    std::fs::remove_file(path.join("lease.json")).unwrap();
    for result in [
        jobs::renew_preview_lease(&path, 30),
        jobs::mark_preview_retained(&path),
    ] {
        assert_eq!(
            result.unwrap_err().code,
            super::errors::ErrorCode::PlanStale
        );
    }
    lease.preview_id = uuid::Uuid::new_v4().to_string();
    lease.expires_unix = 0;
    std::fs::write(path.join("lease.json"), serde_json::to_vec(&lease).unwrap()).unwrap();
    assert!(jobs::cleanup_unreferenced_preview_dirs().is_err());
    assert!(path.is_dir());
}

#[test]
fn last_cached_owner_cannot_delete_an_active_readers_snapshot() {
    let _home = IsolatedAppHome::acquire("preview-reader-drop");
    let _conn = catalog::open_catalog().unwrap();
    let item = input();
    let path = item.prepared.as_ref().unwrap().path.clone();
    jobs::expire_preview_lease_file(&path).unwrap();
    let guard = super::preview_lifecycle::operation().unwrap();
    drop(item);
    assert!(path.is_dir());
    assert!(jobs::prepared_hashes(&path).is_ok());
    drop(guard);
    assert_eq!(jobs::cleanup_unreferenced_preview_dirs().unwrap(), 1);
}

#[test]
fn abandoned_atomic_lease_temporary_is_not_published() {
    let _home = IsolatedAppHome::acquire("preview-abandoned-lease");
    let conn = catalog::open_catalog().unwrap();
    let item = input();
    let path = item.prepared.as_ref().unwrap().path.clone();
    let temporary = format!(".lease-{}.tmp", uuid::Uuid::new_v4());
    std::fs::write(path.join(&temporary), b"synthetic interrupted lease write").unwrap();
    let requested = job();
    let job_id = requested.job_id.clone();
    job_control::enqueue(requested, vec![item]).unwrap();
    assert_eq!(job_control::run_pending().unwrap(), 1);
    let completed = jobs::get_job(&conn, &job_id).unwrap().unwrap();
    assert_eq!(completed.state, JobState::Completed);
    let published = catalog::revision_dir(
        completed.items[0].import_id.as_deref().unwrap(),
        completed.items[0].revision_id.as_deref().unwrap(),
    )
    .unwrap();
    assert!(!published.join(&temporary).exists());
    assert!(!published.join("lease.json").exists());
}

#[cfg(windows)]
#[test]
fn lease_replace_failure_does_not_commit_a_job_or_truncate_the_lease() {
    use std::os::windows::fs::OpenOptionsExt;
    let _home = IsolatedAppHome::acquire("preview-lease-replace-failure");
    let conn = catalog::open_catalog().unwrap();
    let item = input();
    let path = item.prepared.as_ref().unwrap().path.clone();
    let lease_path = path.join("lease.json");
    let original = std::fs::read(&lease_path).unwrap();
    let locked = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(&lease_path)
        .unwrap();
    let requested = job();
    let job_id = requested.job_id.clone();
    let result = job_control::enqueue(requested, vec![item]);
    drop(locked);
    assert_eq!(
        result.unwrap_err().code,
        super::errors::ErrorCode::SnapshotUnavailable
    );
    assert!(jobs::get_job(&conn, &job_id).unwrap().is_none());
    assert_eq!(std::fs::read(lease_path).unwrap(), original);
    assert!(path.is_dir());
    assert!(!std::fs::read_dir(path).unwrap().any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".lease-")
    }));
}

struct TestChild(std::process::Child);

impl Drop for TestChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn start_child(home: &IsolatedAppHome, mode: &str) -> (TestChild, std::path::PathBuf) {
    use std::process::{Command, Stdio};
    let signal = home.path.join(format!("lifecycle-child-{mode}"));
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--exact",
            "session_migration::preview_lifecycle_tests::lifecycle_child_entry",
            "--test-threads=1",
            "--nocapture",
        ])
        .env("GROK_APP_HOME", &home.path)
        .env("GROK_TEST_PREVIEW_LIFECYCLE", mode)
        .env("GROK_TEST_PREVIEW_SIGNAL", &signal)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = TestChild(command.spawn().unwrap());
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while !signal.exists() {
        assert!(
            child.0.try_wait().unwrap().is_none(),
            "child exited before holding lock"
        );
        assert!(
            std::time::Instant::now() < deadline,
            "child failed to acquire lock"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    (child, signal)
}

#[test]
fn lifecycle_child_entry() {
    use std::io::Read;
    let Ok(mode) = std::env::var("GROK_TEST_PREVIEW_LIFECYCLE") else {
        return;
    };
    let signal = std::path::PathBuf::from(std::env::var_os("GROK_TEST_PREVIEW_SIGNAL").unwrap());
    if mode.starts_with("association-publication-")
        || matches!(
            mode.as_str(),
            "association-before-commit" | "association-after-commit"
        )
    {
        crate::session_migration::engine::association_crash_child(signal);
    } else if mode == "builder" {
        let builder = PreparedRevisionBuilder::new().unwrap();
        let temporary = signal.with_extension("tmp");
        std::fs::write(&temporary, builder.path().to_string_lossy().as_bytes()).unwrap();
        std::fs::rename(temporary, signal).unwrap();
        std::io::stdin().read_exact(&mut [0]).unwrap();
        drop(builder);
    } else if matches!(
        mode.as_str(),
        "retention-before-commit"
            | "retention-after-commit"
            | "retention-before-multi"
            | "retention-after-multi"
            | "retention-before-mid-multi"
            | "retention-before-owned"
    ) {
        let item = if mode.ends_with("owned") {
            let fixture: serde_json::Value = serde_json::from_slice(
                &std::fs::read(catalog::imports_dir().join("synthetic-retention-input.json"))
                    .unwrap(),
            )
            .unwrap();
            let requested: JobRecord = serde_json::from_value(fixture["job"].clone()).unwrap();
            job_control::decode_input(fixture["input"].as_str().unwrap(), &requested).unwrap()
        } else {
            input()
        };
        let path = item.prepared.as_ref().unwrap().path.clone();
        let original = jobs::read_preview_lease(&path).unwrap().unwrap();
        let requested = job();
        let second = mode
            .ends_with("multi")
            .then(|| input_with_candidate("second"));
        let mut items = vec![item];
        if let Some(second) = second {
            items.push(second);
        }
        let inputs: Vec<_> = items
            .iter()
            .map(|item| job_control::encode_input(item).unwrap())
            .collect();
        let paths: Vec<_> = items
            .iter()
            .map(|item| item.prepared.as_ref().unwrap().path.clone())
            .collect();
        let report = serde_json::json!({
            "path": path,
            "jobId": requested.job_id,
            "originalLease": original,
            "job": requested,
            "inputs": inputs,
            "paths": paths,
        });
        if mode.starts_with("retention-before-") {
            let crash_hook = |hook: &RefCell<Option<Box<dyn FnOnce()>>>| {
                *hook.borrow_mut() = Some(Box::new(move || {
                    std::fs::write(signal, serde_json::to_vec(&report).unwrap()).unwrap();
                    // Exit only this synthetic child without destructors, after
                    // the lease write and before COMMIT. No accepted row is faked.
                    std::process::exit(73);
                }));
            };
            if mode.contains("mid") {
                AFTER_ENQUEUE_RETENTION.with(crash_hook);
            } else {
                BEFORE_ENQUEUE_COMMIT.with(crash_hook);
            }
            job_control::enqueue(requested, items).unwrap();
            panic!("the before-commit crash hook was not reached");
        } else {
            AFTER_ENQUEUE_COMMIT.with(|hook| {
                *hook.borrow_mut() = Some(Box::new(move || {
                    std::fs::write(signal, serde_json::to_vec(&report).unwrap()).unwrap();
                    // SQL COMMIT succeeded, but neither receipt retirement nor
                    // the caller's ACK happened. Use only genuine enqueue rows.
                    std::process::exit(74);
                }));
            });
            job_control::enqueue(requested, items).unwrap();
            panic!("the after-commit crash hook was not reached");
        }
    } else if mode == "retention" {
        job_control::test_preview_acceptance_owner(|| {
            std::fs::write(signal, b"ready").unwrap();
            std::io::stdin().read_exact(&mut [0]).unwrap();
        })
        .unwrap();
    } else {
        assert_eq!(mode, "cleanup");
        let guard = super::preview_lifecycle::try_cleanup().unwrap().unwrap();
        std::fs::write(signal, b"ready").unwrap();
        std::io::stdin().read_exact(&mut [0]).unwrap();
        drop(guard);
    }
}

#[test]
fn other_process_builder_is_protected_and_crash_releases_its_lock() {
    let home = IsolatedAppHome::acquire("preview-cross-process-builder");
    let _conn = catalog::open_catalog().unwrap();
    let (mut child, signal) = start_child(&home, "builder");
    let path = std::path::PathBuf::from(std::fs::read_to_string(signal).unwrap());
    assert_eq!(jobs::cleanup_unreferenced_preview_dirs().unwrap(), 0);
    assert!(path.is_dir());
    // A second process can still read/build while the first holds a shared lock.
    drop(super::preview_lifecycle::operation().unwrap());
    child.0.kill().unwrap();
    child.0.wait().unwrap();
    assert_eq!(jobs::cleanup_unreferenced_preview_dirs().unwrap(), 1);
    assert!(!path.exists());
}

#[test]
fn other_process_cleanup_blocks_operations_until_it_exits() {
    let home = IsolatedAppHome::acquire("preview-cross-process-cleanup");
    let (mut child, _) = start_child(&home, "cleanup");
    assert!(super::preview_lifecycle::try_cleanup().unwrap().is_none());
    assert_eq!(
        super::preview_lifecycle::operation().unwrap_err().code,
        super::errors::ErrorCode::SourceBusy
    );
    child.0.kill().unwrap();
    child.0.wait().unwrap();
    drop(super::preview_lifecycle::operation().unwrap());
}
