use std::fs::{self, File, OpenOptions};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::lock_metrics::{ImportOsLockKind, LockMetrics};
use super::sample::FrontendSample;
use super::writer::{TelemetryWriter, MAX_BYTES};

struct TestHome(PathBuf);

impl TestHome {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("grok-telemetry-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for TestHome {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn sample_json() -> serde_json::Value {
    serde_json::json!({
        "version": 1, "sequence": 1, "elapsedMs": 30000, "intervalMs": 30000,
        "timerLagMs": 12.25, "frameCount": 600, "maxFrameGapMs": 42.5,
        "documentVisible": true, "previousWriteFailures": 0,
        "stream": { "receivedChunks": 25, "flushedChunks": 10, "pendingKeys": 2,
            "pendingTextCodeUnits": 60, "peakPendingKeys": 3, "peakPendingTextCodeUnits": 90 },
        "tools": { "receivedEvents": 7, "flushedEvents": 5, "pendingEvents": 2, "peakPendingEvents": 3 }
    })
}

#[test]
fn enablement_requires_all_explicit_mock_guards_without_touching_disk() {
    let home = TestHome::new();
    let text = home.0.to_str().unwrap();
    assert!(super::configured_home(None, Some("mock"), Some("1"), Some(text)).is_none());
    assert!(super::configured_home(Some("1"), None, Some("1"), Some(text)).is_none());
    assert!(super::configured_home(Some("1"), Some("real"), Some("1"), Some(text)).is_none());
    assert!(super::configured_home(Some("1"), Some("mock"), None, Some(text)).is_none());
    assert!(super::configured_home(Some("1"), Some("mock"), Some("1"), None).is_none());
    assert!(super::configured_home(Some("1"), Some("mock"), Some("1"), Some("relative")).is_none());
    assert_eq!(
        super::configured_home(Some("1"), Some("mock"), Some("1"), Some(text)),
        Some(home.0.clone())
    );
    assert_eq!(fs::read_dir(&home.0).unwrap().count(), 0);
}

#[test]
fn numeric_schema_rejects_unknown_fields_at_every_level() {
    for field in [None, Some("stream"), Some("tools")] {
        let mut value = sample_json();
        let object = match field {
            Some(field) => &mut value[field],
            None => &mut value,
        };
        object["content"] = serde_json::json!("not accepted");
        assert!(serde_json::from_value::<FrontendSample>(value).is_err());
    }
}

#[test]
fn disabled_write_exits_before_runtime_state_or_sink_initialization() {
    let home = TestHome::new();
    let sample = serde_json::from_value(sample_json()).unwrap();
    let result = tauri::async_runtime::block_on(super::write_sample(None, sample));
    assert_eq!(result.unwrap_err(), "telemetry_disabled");
    assert!(super::STATE.get().is_none());
    assert!(!super::WRITING.load(std::sync::atomic::Ordering::Relaxed));
    assert_eq!(fs::read_dir(&home.0).unwrap().count(), 0);
}

#[test]
fn numeric_schema_rejects_strings_negative_and_fractional_counters() {
    for invalid in [
        serde_json::json!("1"),
        serde_json::json!(-1),
        serde_json::json!(1.25),
    ] {
        let mut value = sample_json();
        value["sequence"] = invalid;
        assert!(serde_json::from_value::<FrontendSample>(value).is_err());
    }
}

#[test]
fn validates_finite_bounded_numbers_without_false_chunk_conservation() {
    let sample: FrontendSample = serde_json::from_value(sample_json()).unwrap();
    sample.validate().unwrap();
    for invalid in [
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
        -1.0,
        31_536_000_001.0,
    ] {
        let mut next = sample.clone();
        next.timer_lag_ms = invalid;
        assert!(next.validate().is_err());
    }
    let mut next = sample.clone();
    next.version = 2;
    assert!(next.validate().is_err());
    next = sample.clone();
    next.sequence = 9_007_199_254_740_992;
    assert!(next.validate().is_err());
    next = sample;
    next.stream.flushed_chunks = 999;
    next.tools.flushed_events = 999;
    next.validate().unwrap();
}

#[test]
fn writes_only_owned_new_file_and_never_overwrites_pid_reuse() {
    let home = TestHome::new();
    let mut writer = TelemetryWriter::open(&home.0, 101).unwrap();
    writer.append(b"{\"version\":1}\n").unwrap();
    assert!(TelemetryWriter::open(&home.0, 102).is_err());
    drop(writer);
    let path = home.0.join("diagnostics/native-soak-101.jsonl");
    let expected = fs::read(&path).unwrap();
    assert!(TelemetryWriter::open(&home.0, 101).is_err());
    assert_eq!(fs::read(path).unwrap(), expected);
}

#[test]
fn cap_is_shared_across_restarts_and_errors_without_truncating() {
    let home = TestHome::new();
    fs::create_dir(home.0.join("diagnostics")).unwrap();
    let old = home.0.join("diagnostics/native-soak-1.jsonl");
    File::create(&old).unwrap().set_len(MAX_BYTES - 2).unwrap();
    let mut writer = TelemetryWriter::open(&home.0, 2).unwrap();
    assert_eq!(writer.append(b"{}\n").unwrap_err(), "telemetry_disk_cap");
    assert_eq!(writer.append(b"{}\n").unwrap_err(), "telemetry_sink_failed");
    assert_eq!(fs::metadata(old).unwrap().len(), MAX_BYTES - 2);
    assert_eq!(
        fs::metadata(home.0.join("diagnostics/native-soak-2.jsonl"))
            .unwrap()
            .len(),
        0
    );
}

#[test]
fn writer_rejects_non_directory_and_parent_escape() {
    let home = TestHome::new();
    File::create(home.0.join("diagnostics")).unwrap();
    assert!(TelemetryWriter::open(&home.0, 1).is_err());
    assert!(TelemetryWriter::open(&home.0.join(".."), 1).is_err());
    assert!(TelemetryWriter::open(std::path::Path::new("relative"), 1).is_err());
}

#[test]
fn invalid_line_permanently_stops_sink_without_partial_append() {
    let home = TestHome::new();
    let mut writer = TelemetryWriter::open(&home.0, 1).unwrap();
    assert_eq!(
        writer.append(b"missing newline").unwrap_err(),
        "telemetry_line_limit"
    );
    assert_eq!(writer.append(b"{}\n").unwrap_err(), "telemetry_sink_failed");
    assert_eq!(
        fs::metadata(home.0.join("diagnostics/native-soak-1.jsonl"))
            .unwrap()
            .len(),
        0
    );
}

#[test]
fn measures_real_os_lock_contention_and_clears_active_wait() {
    let home = TestHome::new();
    let path = home.0.join("lock");
    let owner = OpenOptions::new()
        .create_new(true)
        .read(true)
        .write(true)
        .open(&path)
        .unwrap();
    fs2::FileExt::lock_exclusive(&owner).unwrap();
    let metrics = Arc::new(LockMetrics::default());
    let worker_metrics = Arc::clone(&metrics);
    let worker = std::thread::spawn(move || {
        let contender = OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .unwrap();
        worker_metrics
            .observe(ImportOsLockKind::Worker, || {
                fs2::FileExt::lock_exclusive(&contender)
            })
            .unwrap();
        fs2::FileExt::unlock(&contender).unwrap();
    });
    let deadline = Instant::now() + Duration::from_secs(2);
    while metrics.snapshot().worker.active_wait_count == 0 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(metrics.snapshot().worker.active_wait_count, 1);
    std::thread::sleep(Duration::from_millis(60));
    assert!(metrics.snapshot().worker.oldest_active_wait_us >= 40_000);
    fs2::FileExt::unlock(&owner).unwrap();
    worker.join().unwrap();
    let snapshot = metrics.snapshot();
    assert_eq!(snapshot.worker.acquire_count, 1);
    assert_eq!(snapshot.worker.failed_count, 0);
    assert_eq!(snapshot.worker.active_wait_count, 0);
    assert_eq!(snapshot.worker.oldest_active_wait_us, 0);
    assert!(snapshot.worker.total_acquire_us >= 40_000);
    assert_eq!(
        snapshot.worker.total_acquire_us,
        snapshot.worker.max_acquire_us
    );
    assert_eq!(snapshot.publish.acquire_count, 0);
}

#[test]
fn records_failed_os_acquisition_without_changing_result() {
    let home = TestHome::new();
    let path = home.0.join("lock");
    let owner = OpenOptions::new()
        .create_new(true)
        .read(true)
        .write(true)
        .open(&path)
        .unwrap();
    let contender = OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .unwrap();
    fs2::FileExt::lock_exclusive(&owner).unwrap();
    let metrics = LockMetrics::default();
    let result = metrics.observe(ImportOsLockKind::Publish, || {
        fs2::FileExt::try_lock_exclusive(&contender)
    });
    assert!(result.is_err());
    assert_eq!(metrics.snapshot().publish.failed_count, 1);
    assert_eq!(metrics.snapshot().publish.active_wait_count, 0);
    fs2::FileExt::unlock(&owner).unwrap();
}

#[test]
fn active_tracking_is_bounded_and_overflow_explicit() {
    let metrics = LockMetrics::default();
    let guards: Vec<_> = (0..130)
        .map(|_| metrics.begin(ImportOsLockKind::Publish))
        .collect();
    assert_eq!(metrics.snapshot().publish.active_wait_count, 130);
    assert_eq!(metrics.snapshot().publish.active_tracking_overflows, 2);
    assert_eq!(metrics.snapshot().publish.untracked_active_waits, 2);
    drop(guards);
    assert_eq!(metrics.snapshot().publish.active_wait_count, 0);
    assert_eq!(metrics.snapshot().publish.untracked_active_waits, 0);
    assert_eq!(metrics.snapshot().publish.failed_count, 130);
}

#[cfg(unix)]
#[test]
fn refuses_symlinked_home_diagnostics_and_budget_lock() {
    use std::os::unix::fs::symlink;
    let root = TestHome::new();
    let outside = TestHome::new();
    symlink(&outside.0, root.0.join("linked-home")).unwrap();
    assert!(TelemetryWriter::open(&root.0.join("linked-home"), 1).is_err());
    symlink(&outside.0, root.0.join("diagnostics")).unwrap();
    assert!(TelemetryWriter::open(&root.0, 1).is_err());
    fs::remove_file(root.0.join("diagnostics")).unwrap();
    fs::create_dir(root.0.join("diagnostics")).unwrap();
    let external = outside.0.join("external");
    fs::write(&external, b"untouched").unwrap();
    symlink(
        &external,
        root.0.join("diagnostics/native-soak-budget.lock"),
    )
    .unwrap();
    assert!(TelemetryWriter::open(&root.0, 1).is_err());
    assert_eq!(fs::read(external).unwrap(), b"untouched");
}
