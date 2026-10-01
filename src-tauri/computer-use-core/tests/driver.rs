#![cfg(feature = "test-support")]
use grok_computer_use_core::{
    driver::{wire::Completion, PrivateWorker, WorkerOptions},
    execution::ActionCancellation,
};
use serde_json::json;
use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

struct Fixture {
    worker: Arc<PrivateWorker>,
    dir: PathBuf,
}

impl Fixture {
    fn start() -> Self {
        let dir = std::env::temp_dir().join(format!("grok-cu-worker-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        let manifest = dir.join("capabilities.json");
        std::fs::write(
            &manifest,
            "{\"version\":3,\"allow\":{\"tools\":[\"list_windows\"]}}",
        )
        .unwrap();
        let worker = PrivateWorker::start(WorkerOptions {
            binary: PathBuf::from(env!("CARGO_BIN_EXE_computer-use-worker-fixture")),
            manifest,
            host_bundle_id: "com.grok.computer-use-test".into(),
            startup_timeout: Duration::from_secs(5),
        })
        .unwrap();
        Self {
            worker: Arc::new(worker),
            dir,
        }
    }

    fn stopped(&self) {
        let start = Instant::now();
        while !self.worker.is_stopped() && start.elapsed() < Duration::from_secs(3) {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            self.worker.is_stopped(),
            "owned worker process did not exit"
        );
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.worker.request_stop().unwrap();
        self.stopped();
        std::fs::remove_dir_all(&self.dir).unwrap();
    }
}

#[test]
fn private_pipe_reuses_process_and_matches_session_responses() {
    let fixture = Fixture::start();
    let session = fixture
        .worker
        .bind("test", &fixture.dir.join("capabilities.json"))
        .unwrap();
    assert_eq!(session, "bound-fixture");
    for index in 0..5 {
        let value = fixture
            .worker
            .call(
                "echo",
                json!({"index": index}),
                "run-driver",
                Some(&session),
                &ActionCancellation::default(),
                Duration::from_secs(2),
            )
            .unwrap();
        assert_eq!(value["index"], index);
    }
    assert!(!fixture.worker.is_stopped());
}

#[test]
fn timeout_terminates_and_reaps_the_actual_child() {
    let fixture = Fixture::start();
    let result = fixture
        .worker
        .call(
            "hang",
            json!({}),
            "run-driver",
            None,
            &ActionCancellation::default(),
            Duration::from_millis(60),
        )
        .unwrap_err();
    assert_eq!(result.completion, Completion::Unknown);
    fixture.stopped();
    let retry = fixture
        .worker
        .call(
            "echo",
            json!({}),
            "run-driver",
            None,
            &ActionCancellation::default(),
            Duration::from_secs(1),
        )
        .unwrap_err();
    assert_eq!(retry.completion, Completion::NotStarted);
}

#[test]
fn stop_does_not_wait_for_blocked_pipe_io_and_queued_call_never_starts() {
    let fixture = Fixture::start();
    let started = fixture.dir.join("started");
    let worker = fixture.worker.clone();
    let marker = started.clone();
    let active = std::thread::spawn(move || {
        worker.call(
            "hang",
            json!({"started": marker}),
            "run-driver",
            None,
            &ActionCancellation::default(),
            Duration::from_secs(30),
        )
    });
    let deadline = Instant::now();
    while !started.exists() && deadline.elapsed() < Duration::from_secs(3) {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(started.exists());
    let worker = fixture.worker.clone();
    let queued = std::thread::spawn(move || {
        worker.call(
            "echo",
            json!({}),
            "run-driver",
            None,
            &ActionCancellation::default(),
            Duration::from_secs(10),
        )
    });
    let stop = Instant::now();
    fixture.worker.request_stop().unwrap();
    assert!(stop.elapsed() < Duration::from_secs(1));
    assert_eq!(
        active.join().unwrap().unwrap_err().completion,
        Completion::Unknown
    );
    assert_eq!(
        queued.join().unwrap().unwrap_err().completion,
        Completion::NotStarted
    );
    fixture.stopped();
}

#[test]
fn cancellation_reaches_worker_without_waiting_for_tool_timeout() {
    let fixture = Fixture::start();
    let cancellation = ActionCancellation::default();
    let worker = fixture.worker.clone();
    let signal = cancellation.clone();
    let started = fixture.dir.join("started");
    let marker = started.clone();
    let active = std::thread::spawn(move || {
        worker.call(
            "hang",
            json!({"started": marker}),
            "run-driver",
            None,
            &signal,
            Duration::from_secs(30),
        )
    });
    let deadline = Instant::now();
    while !started.exists() && deadline.elapsed() < Duration::from_secs(3) {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(started.exists());
    cancellation.cancel();
    assert_eq!(
        active.join().unwrap().unwrap_err().completion,
        Completion::Unknown
    );
    fixture.stopped();
}

#[test]
fn malformed_or_oversized_responses_revoke_worker_generation() {
    for name in ["wrong_generation", "oversize", "stale_request_id"] {
        let fixture = Fixture::start();
        let error = fixture
            .worker
            .call(
                name,
                json!({}),
                "run-driver",
                None,
                &ActionCancellation::default(),
                Duration::from_secs(3),
            )
            .unwrap_err();
        assert_eq!(error.completion, Completion::Unknown);
        fixture.stopped();
    }
}

#[test]
fn handshake_exposes_protocol_build_capabilities_generation_and_max_response() {
    let fixture = Fixture::start();
    let hs = fixture.worker.handshake();
    assert_eq!(
        hs.protocol_version,
        grok_computer_use_core::driver::wire::WIRE_VERSION
    );
    assert!(!hs.build_identity.trim().is_empty());
    assert_eq!(
        hs.max_response_size,
        grok_computer_use_core::driver::wire::MAX_RESPONSE
    );
    assert!(!hs.generation.trim().is_empty());
    assert_eq!(
        hs.capabilities.get("echo").and_then(|v| v.as_bool()),
        Some(true)
    );
    assert_eq!(hs.generation, fixture.worker.generation());
}

#[test]
fn call_carries_deadline_run_generation_and_cancellation() {
    let fixture = Fixture::start();
    let before = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    let meta = fixture
        .worker
        .call(
            "echo_meta",
            json!({}),
            "run-driver",
            Some("bound-fixture"),
            &ActionCancellation::default(),
            Duration::from_secs(2),
        )
        .unwrap();
    assert_eq!(meta["run_id"], "run-driver");
    assert_eq!(meta["generation"], fixture.worker.generation());
    assert!(meta["request_id"].as_u64().unwrap() >= 1);
    let deadline = meta["deadline_ms"].as_u64().unwrap();
    assert!(deadline >= before + 1000);
    assert_eq!(meta["cancellation_armed"], true);
    assert_eq!(
        meta["protocol_version"],
        grok_computer_use_core::driver::wire::WIRE_VERSION
    );
}

#[test]
fn empty_run_id_is_rejected_before_the_child_runs_a_tool() {
    let fixture = Fixture::start();
    let err = fixture
        .worker
        .call(
            "echo",
            json!({"index": 1}),
            "",
            None,
            &ActionCancellation::default(),
            Duration::from_secs(2),
        )
        .unwrap_err();
    assert_eq!(err.code, "invalid_request");
}

#[test]
fn replacement_worker_uses_new_generation_and_old_process_cannot_complete_it() {
    let first = Fixture::start();
    let old_generation = first.worker.generation().to_string();
    first.worker.request_stop().unwrap();
    first.stopped();
    let late = first
        .worker
        .call(
            "echo",
            json!({"index": 7}),
            "run-driver",
            None,
            &ActionCancellation::default(),
            Duration::from_secs(1),
        )
        .unwrap_err();
    assert_eq!(late.completion, Completion::NotStarted);
    drop(first);

    let second = Fixture::start();
    assert_ne!(second.worker.generation(), old_generation);
    let value = second
        .worker
        .call(
            "echo",
            json!({"index": 7}),
            "run-driver",
            None,
            &ActionCancellation::default(),
            Duration::from_secs(2),
        )
        .unwrap();
    assert_eq!(value["index"], 7);
    let stale = second
        .worker
        .call(
            "wrong_generation",
            json!({}),
            "run-driver",
            None,
            &ActionCancellation::default(),
            Duration::from_secs(3),
        )
        .unwrap_err();
    assert_eq!(stale.completion, Completion::Unknown);
    assert_eq!(stale.code, "invalid_response");
}

fn pid_running(pid: u32) -> bool {
    grok_computer_use_core::driver::pid_is_running(pid)
}

#[test]
fn stop_reaps_grandchild_in_the_owned_process_tree() {
    let fixture = Fixture::start();
    let pid_path = fixture.dir.join("grandchild.pid");
    let value = fixture
        .worker
        .call(
            "spawn_hanging_child",
            json!({"pid_path": pid_path}),
            "run-driver",
            None,
            &ActionCancellation::default(),
            Duration::from_secs(3),
        )
        .unwrap();
    let pid = value["child_pid"].as_u64().unwrap() as u32;
    let deadline = Instant::now();
    while !pid_path.exists() && deadline.elapsed() < Duration::from_secs(3) {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(pid_path.exists());
    assert!(pid_running(pid), "grandchild must be alive before stop");
    let stop = Instant::now();
    fixture.worker.request_stop().unwrap();
    assert!(stop.elapsed() < Duration::from_secs(1));
    fixture.stopped();
    let dead = Instant::now();
    while pid_running(pid) && dead.elapsed() < Duration::from_secs(3) {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        !pid_running(pid),
        "grandchild {pid} survived owned process-tree stop"
    );
}

#[test]
fn stderr_flood_is_bounded_redacted_and_does_not_block_stop() {
    let fixture = Fixture::start();
    fixture
        .worker
        .call(
            "flood_stderr",
            json!({
                "marker": "VISIBLE_MARKER",
                "secret": "cu-test-token-AABBCC"
            }),
            "run-driver",
            None,
            &ActionCancellation::default(),
            Duration::from_secs(5),
        )
        .unwrap();
    let drain = Instant::now();
    let log = loop {
        let log = fixture.worker.stderr_log();
        if log.contains("VISIBLE_MARKER")
            || log.contains("[redacted]")
            || drain.elapsed() > Duration::from_secs(2)
        {
            break log;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    assert!(
        log.len() <= grok_computer_use_core::driver::wire::MAX_STDERR,
        "stderr log exceeded bound: {}",
        log.len()
    );
    assert!(
        !log.contains("cu-test-token-AABBCC"),
        "secret leaked into worker stderr log"
    );
    assert!(
        log.contains("VISIBLE_MARKER") || log.contains("[redacted]"),
        "bounded stderr log lost diagnostic marker: {log:?}"
    );
    let stop = Instant::now();
    fixture.worker.request_stop().unwrap();
    assert!(stop.elapsed() < Duration::from_secs(1));
    fixture.stopped();
}

#[test]
fn product_path_dispatches_through_attached_private_worker() {
    use grok_computer_use_core::{
        adapter::{ActionScope, ComputerUseAdapter, DispatchRequest},
        fake::FakeAdapter,
        owned::HostOwnedAdapter,
        protocol::{ActionKind, ActionTarget},
    };
    let fixture = Fixture::start();
    let fake = Arc::new(FakeAdapter::new());
    let owned = HostOwnedAdapter::new(fake.clone());
    owned.attach_worker(fixture.worker.clone());
    assert!(owned.worker_attached());
    assert!(!owned.worker_generation().unwrap().trim().is_empty());
    let target = fake.fixture_id();
    let result = owned
        .act(&DispatchRequest {
            managed_request: None,
            cancellation: ActionCancellation::default(),
            run_id: "run-driver".into(),
            action_id: "owned-click".into(),
            generation: 1,
            target_id: target,
            target_generation: 1,
            snapshot_id: "snap".into(),
            geometry_revision: 1,
            action: ActionKind::Click,
            target: ActionTarget::Coord { x: 8.0, y: 8.0 },
            parameters: json!({}),
            scope: ActionScope::Directed,
        })
        .unwrap();
    assert!(result.applied);
    assert_eq!(fake.executions(), ["owned-click"]);
    let marker = fixture.dir.join("grok-cu-worker-act-owned-click.txt");
    assert!(marker.starts_with(&fixture.dir));
    let pid_text = std::fs::read_to_string(&marker).expect("worker act must write a side effect");
    let pid: u32 = pid_text.trim().parse().expect("worker pid");
    assert!(pid > 0, "worker act must run in the worker process");
    assert_ne!(
        pid,
        std::process::id(),
        "Host cannot manufacture worker proof"
    );
    let _ = std::fs::remove_file(&marker);
    let caps = owned.capabilities();
    assert!(
        caps.notes
            .iter()
            .any(|n| n.contains("private worker generation=")),
        "{:?}",
        caps.notes
    );
}

fn observe_control(fixture: &Fixture, name: &str, args: serde_json::Value) -> serde_json::Value {
    fixture
        .worker
        .call(
            name,
            args,
            "test-observe-control",
            None,
            &ActionCancellation::default(),
            Duration::from_secs(2),
        )
        .unwrap()
}

#[test]
fn product_capture_worker_receives_real_run_and_preserves_image_policy() {
    use grok_computer_use_core::{
        adapter::{CaptureOptions, ComputerUseAdapter},
        fake::FakeAdapter,
        owned::HostOwnedAdapter,
    };
    let fixture = Fixture::start();
    let fake = Arc::new(FakeAdapter::new());
    let target = fake.fixture_id();
    let owned = HostOwnedAdapter::new(fake.clone());
    owned.attach_worker(fixture.worker.clone());
    let model = owned
        .capture_for_run("model-owner", &target, CaptureOptions::model(false))
        .unwrap();
    assert!(model.image.png_base64.is_none());
    let preview = CaptureOptions {
        for_model: false,
        ..CaptureOptions::model(false)
    };
    owned
        .capture_for_run("preview-owner", &target, preview)
        .unwrap();
    owned.observe_for_run("run-owner", &target).unwrap();
    let history = observe_control(&fixture, "observe_history", json!({}));
    let calls = history.as_array().unwrap();
    assert_eq!(calls.len(), 3);
    for (call, run) in calls
        .iter()
        .zip(["model-owner", "preview-owner", "run-owner"])
    {
        assert_eq!(call["runId"], run);
        assert_eq!(call["targetId"], target);
        assert_ne!(call["pid"].as_u64().unwrap(), std::process::id() as u64);
    }
    assert_eq!(fake.observe_calls(), 3);
}

#[test]
fn product_capture_precancelled_does_not_contact_worker_or_native_adapter() {
    use grok_computer_use_core::{
        adapter::{CaptureOptions, ComputerUseAdapter},
        fake::FakeAdapter,
        owned::HostOwnedAdapter,
    };
    let fixture = Fixture::start();
    let fake = Arc::new(FakeAdapter::new());
    let owned = HostOwnedAdapter::new(fake.clone());
    owned.attach_worker(fixture.worker.clone());
    let options = CaptureOptions::model(true);
    options.cancellation.cancel();
    assert!(owned
        .capture_for_run("cancelled-owner", &fake.fixture_id(), options)
        .is_err());
    assert_eq!(
        observe_control(&fixture, "observe_history", json!({})),
        json!([])
    );
    assert_eq!(fake.observe_calls(), 0);
}

#[test]
fn product_capture_worker_refusal_or_wrong_identity_never_reaches_native_adapter() {
    use grok_computer_use_core::{
        adapter::{CaptureOptions, ComputerUseAdapter},
        fake::FakeAdapter,
        owned::HostOwnedAdapter,
    };
    for (behavior, expected) in [
        (json!({"executed":false}), "did not execute"),
        (json!({"targetId":"foreign-window"}), "identity mismatch"),
    ] {
        let fixture = Fixture::start();
        let fake = Arc::new(FakeAdapter::new());
        let owned = HostOwnedAdapter::new(fake.clone());
        owned.attach_worker(fixture.worker.clone());
        observe_control(&fixture, "configure_observe", behavior);
        let error = owned
            .capture_for_run("owner", &fake.fixture_id(), CaptureOptions::model(true))
            .unwrap_err();
        assert!(error.contains(expected), "{error}");
        assert_eq!(fake.observe_calls(), 0);
        assert_eq!(
            observe_control(&fixture, "observe_history", json!({}))
                .as_array()
                .unwrap()
                .len(),
            1
        );
    }
}

#[test]
fn product_capture_cancellation_during_worker_admission_never_observes_native_target() {
    use grok_computer_use_core::{
        adapter::{CaptureOptions, ComputerUseAdapter},
        fake::FakeAdapter,
        owned::HostOwnedAdapter,
    };
    let fixture = Fixture::start();
    let fake = Arc::new(FakeAdapter::new());
    let target = fake.fixture_id();
    let owned = Arc::new(HostOwnedAdapter::new(fake.clone()));
    owned.attach_worker(fixture.worker.clone());
    let started = fixture.dir.join("observe-started");
    observe_control(
        &fixture,
        "configure_observe",
        json!({"hang":true,"started":started}),
    );
    let options = CaptureOptions::model(true);
    let cancellation = options.cancellation.clone();
    let task = std::thread::spawn(move || {
        owned.capture_for_run("cancel-during-admission", &target, options)
    });
    let deadline = Instant::now() + Duration::from_secs(3);
    while !started.exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
    let actually_started = started.exists();
    cancellation.cancel();
    let result = task.join().unwrap();
    assert!(
        actually_started,
        "worker never reached the actual observe boundary"
    );
    assert!(
        result.is_err(),
        "cancelled admission must not publish an observation"
    );
    assert_eq!(fake.observe_calls(), 0);
    fixture.stopped();
}
