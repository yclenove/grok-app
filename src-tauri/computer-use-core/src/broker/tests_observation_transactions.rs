//! Deterministic admission races; no native input, sleeps or global test hooks.
use super::*;
use crate::adapter::{AdapterActResult, Capabilities};
use crate::fake::FakeAdapter;

struct CaptureGate {
    entered: mpsc::Sender<()>,
    release: mpsc::Receiver<()>,
}

struct GatedAdapter {
    fake: FakeAdapter,
    next: Mutex<Option<CaptureGate>>,
    next_action: Mutex<Option<CaptureGate>>,
    next_idle: Mutex<Option<CaptureGate>>,
    fail: AtomicBool,
    panic_capture: AtomicBool,
}

impl GatedAdapter {
    fn new() -> Self {
        Self {
            fake: FakeAdapter::new(),
            next: Mutex::new(None),
            next_action: Mutex::new(None),
            next_idle: Mutex::new(None),
            fail: AtomicBool::new(false),
            panic_capture: AtomicBool::new(false),
        }
    }

    fn gate_next(&self) -> (mpsc::Receiver<()>, mpsc::Sender<()>) {
        let (entered, waiting) = mpsc::channel();
        let (release, released) = mpsc::channel();
        *self.next.lock() = Some(CaptureGate {
            entered,
            release: released,
        });
        (waiting, release)
    }
}

impl ComputerUseAdapter for GatedAdapter {
    fn backend_id(&self) -> &'static str {
        "gated-fixture"
    }
    fn capabilities(&self) -> Capabilities {
        self.fake.capabilities()
    }
    fn list_targets(&self) -> Result<Vec<TargetInfo>, String> {
        self.fake.list_targets()
    }
    fn target_alive(&self, id: &str) -> bool {
        self.fake.target_alive(id)
    }
    fn observe(&self, id: &str) -> Result<Observation, String> {
        assert!(
            !self.panic_capture.load(Ordering::SeqCst),
            "fixture capture panic"
        );
        let result = self.fake.observe(id)?;
        let gate = self.next.lock().take();
        if let Some(gate) = gate {
            gate.entered.send(()).map_err(|e| e.to_string())?;
            gate.release
                .recv_timeout(Duration::from_secs(3))
                .map_err(|e| e.to_string())?;
        }
        if self.fail.load(Ordering::SeqCst) {
            return Err("fixture capture failed".into());
        }
        Ok(result)
    }
    fn act(&self, request: &DispatchRequest) -> Result<AdapterActResult, String> {
        let result = self.fake.act(request);
        let gate = self.next_action.lock().take();
        if let Some(gate) = gate {
            gate.entered.send(()).map_err(|e| e.to_string())?;
            gate.release
                .recv_timeout(Duration::from_secs(3))
                .map_err(|e| e.to_string())?;
        }
        result
    }
    fn abort(&self, run: &str, generation: u64) -> Result<(), String> {
        self.fake.abort(run, generation)
    }
    fn is_idle(&self, run: &str) -> bool {
        let gate = self.next_idle.lock().take();
        if let Some(gate) = gate {
            gate.entered.send(()).unwrap();
            if gate.release.recv_timeout(Duration::from_secs(3)).is_err() {
                return false;
            }
        }
        self.fake.is_idle(run)
    }
    fn start_periodic_preview(&self, id: &str) {
        self.fake.start_periodic_preview(id);
    }
    fn stop_periodic_preview(&self) {
        self.fake.stop_periodic_preview();
    }
    fn periodic_preview_active(&self) -> bool {
        self.fake.periodic_preview_active()
    }
}

#[path = "tests_resume_transactions.rs"]
mod resume_transactions;

fn ready(budget: usize) -> (Arc<ComputerUseBroker>, Arc<GatedAdapter>) {
    let adapter = Arc::new(GatedAdapter::new());
    let mut options = gates::enabled_opts(100);
    options.observe_budget = budget;
    let broker = Arc::new(ComputerUseBroker::new(adapter.clone(), options));
    broker.open_run("session", "run").unwrap();
    broker
        .authorize_target("run", &adapter.fake.fixture_id())
        .unwrap();
    (broker, adapter)
}

fn pending_capture(
    broker: &Arc<ComputerUseBroker>,
    adapter: &GatedAdapter,
    preview: bool,
) -> (
    mpsc::Sender<()>,
    std::thread::JoinHandle<Result<Observation, BrokerError>>,
) {
    let (entered, release) = adapter.gate_next();
    let broker = broker.clone();
    let pending = std::thread::spawn(move || {
        if preview {
            broker.observe_preview("run")
        } else {
            broker.observe("run")
        }
    });
    entered.recv_timeout(Duration::from_secs(2)).unwrap();
    (release, pending)
}

#[test]
fn overlapping_model_or_preview_captures_are_rejected_before_adapter_dispatch() {
    for first_preview in [false, true] {
        let (broker, adapter) = ready(3);
        let (release, pending) = pending_capture(&broker, &adapter, first_preview);
        let model = broker.observe("run");
        let preview = broker.observe_preview("run");
        release.send(()).unwrap();
        let first = pending.join().unwrap().unwrap();
        assert!(
            matches!(model, Err(BrokerError::LeaseHeld { .. })),
            "model overlap: {model:?}"
        );
        assert!(
            matches!(preview, Err(BrokerError::LeaseHeld { .. })),
            "preview overlap: {preview:?}"
        );
        assert_eq!(adapter.fake.observe_calls(), 1);
        if !first_preview {
            assert_eq!(broker.act_defaults("run").unwrap().2, first.snapshot_id);
        }
    }
}

#[test]
fn capture_blocks_old_snapshot_action_and_reauthorization_without_side_effects() {
    let (broker, adapter) = ready(3);
    let previous = broker.observe("run").unwrap();
    let (release, pending) = pending_capture(&broker, &adapter, false);
    let action = broker.act(gates::click_req(
        "run",
        &previous.target_id,
        previous.target_generation,
        &previous.snapshot_id,
        previous.geometry_revision,
        "during-capture",
    ));
    let authorize = broker.authorize_target("run", &previous.target_id);
    release.send(()).unwrap();
    let captured = pending.join().unwrap();
    assert_eq!(action.kind, OutcomeKind::Rejected, "{action:?}");
    assert!(adapter.fake.executions().is_empty());
    assert!(
        matches!(authorize, Err(BrokerError::LeaseHeld { .. })),
        "{authorize:?}"
    );
    assert!(captured.is_ok(), "{captured:?}");
}

#[test]
fn stop_fences_pending_capture_without_claiming_it_already_quiesced() {
    let (broker, adapter) = ready(3);
    let (release, pending) = pending_capture(&broker, &adapter, false);
    let stopped = broker.request_stop("run").unwrap();
    release.send(()).unwrap();
    let captured = pending.join().unwrap();
    assert_eq!(stopped, StopState::StopRequested);
    assert!(captured.is_err(), "stopped capture leaked a frame");
    assert_eq!(
        broker.wait_stopped("run", Duration::from_secs(1)).unwrap(),
        StopState::Stopped
    );
}

#[test]
fn failed_model_capture_consumes_budget_but_ui_preview_does_not() {
    let (broker, adapter) = ready(1);
    adapter.fail.store(true, Ordering::SeqCst);
    assert!(broker.observe("run").is_err());
    adapter.fail.store(false, Ordering::SeqCst);
    assert!(broker.observe_preview("run").is_ok());
    let result = broker.observe("run");
    assert!(
        matches!(result, Err(BrokerError::Schema(ref text)) if text.contains("budget")),
        "{result:?}"
    );
    assert_eq!(adapter.fake.observe_calls(), 2);
}

#[test]
fn a_capture_does_not_hold_global_broker_state_or_another_runs_admission() {
    let (broker, adapter) = ready(3);
    broker.open_run("session-other", "run-other").unwrap();
    broker
        .authorize_target("run-other", &adapter.fake.fixture_id())
        .unwrap();
    let (release, pending) = pending_capture(&broker, &adapter, false);
    let other = broker.observe("run-other");
    release.send(()).unwrap();
    assert!(pending.join().unwrap().is_ok());
    assert!(
        other.is_ok(),
        "unrelated run capture was blocked: {other:?}"
    );
}

#[test]
fn busy_observations_do_not_spend_budget_and_capture_panic_releases_admission() {
    let (broker, adapter) = ready(2);
    let (release, pending) = pending_capture(&broker, &adapter, false);
    for _ in 0..4 {
        assert!(matches!(
            broker.observe("run"),
            Err(BrokerError::LeaseHeld { .. })
        ));
    }
    release.send(()).unwrap();
    pending.join().unwrap().unwrap();
    adapter.panic_capture.store(true, Ordering::SeqCst);
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| broker.observe("run"))).is_err()
    );
    adapter.panic_capture.store(false, Ordering::SeqCst);
    assert!(broker.observe_preview("run").is_ok());
    assert!(matches!(broker.observe("run"), Err(BrokerError::Schema(_))));
}

#[test]
fn lifecycle_changes_during_capture_never_publish_the_late_frame() {
    for change in ["pause", "feature-off", "restore"] {
        let (broker, adapter) = ready(3);
        let old = broker.observe("run").unwrap();
        let (release, pending) = pending_capture(&broker, &adapter, false);
        match change {
            "pause" => broker.pause("run").unwrap(),
            "feature-off" => broker.set_feature_enabled(false),
            _ => broker
                .restore_run(PersistedRun {
                    app_session_id: "session".into(),
                    run_id: "run".into(),
                    traces: vec![],
                    last_target_id: Some(old.target_id),
                    consumed_action_ids: vec![],
                })
                .unwrap(),
        }
        release.send(()).unwrap();
        assert!(
            pending.join().unwrap().is_err(),
            "{change} leaked a late frame"
        );
        assert!(!broker.inner.lock().runs["run"]
            .in_flight
            .load(Ordering::SeqCst));
    }
}

#[test]
fn native_action_keeps_admission_after_worker_exits_until_outcome_is_published() {
    let (broker, adapter) = ready(3);
    let obs = broker.observe("run").unwrap();
    let baseline_refs = Arc::strong_count(&adapter);
    let (entered, waiting) = mpsc::channel();
    let (release, released) = mpsc::channel();
    *adapter.next_action.lock() = Some(CaptureGate {
        entered,
        release: released,
    });
    let caller = broker.clone();
    let pending = std::thread::spawn(move || {
        caller.act(gates::click_req(
            "run",
            &obs.target_id,
            obs.target_generation,
            &obs.snapshot_id,
            obs.geometry_revision,
            "commit-gap",
        ))
    });
    waiting.recv_timeout(Duration::from_secs(2)).unwrap();
    // Block only the caller's trace/outcome commit. The worker already entered
    // the adapter and never needs this lock, so it can physically finish.
    let state = broker.inner.lock();
    release.send(()).unwrap();
    let until = Instant::now() + Duration::from_secs(2);
    while Arc::strong_count(&adapter) != baseline_refs && Instant::now() < until {
        std::thread::yield_now();
    }
    let worker_released_adapter = Arc::strong_count(&adapter) == baseline_refs;
    let still_admitted = state.runs["run"].in_flight.load(Ordering::SeqCst);
    let before_commit = state.runs["run"].seen["commit-gap"].kind;
    drop(state);
    let result = pending.join().unwrap();
    assert!(
        worker_released_adapter,
        "worker did not finish before the deadline"
    );
    assert_eq!(before_commit, OutcomeKind::Unknown);
    assert!(
        still_admitted,
        "worker released admission before result publication"
    );
    assert_eq!(result.kind, OutcomeKind::Verified);
    assert!(!broker.inner.lock().runs["run"]
        .in_flight
        .load(Ordering::SeqCst));
    assert_eq!(adapter.fake.executions(), ["commit-gap"]);
}

#[test]
fn unknown_blocks_new_action_until_observe() {
    let (broker, adapter) = ready(3);
    let obs = broker.observe("run").unwrap();
    let request = gates::click_req(
        "run",
        &obs.target_id,
        obs.target_generation,
        &obs.snapshot_id,
        obs.geometry_revision,
        "slow",
    );
    let (entered, waiting) = mpsc::channel();
    let (release, released) = mpsc::channel();
    *adapter.next_action.lock() = Some(CaptureGate {
        entered,
        release: released,
    });
    let caller = broker.clone();
    let original = request.clone();
    let pending = std::thread::spawn(move || caller.act(original));
    // The independent adapter signal, not a 30 ms scheduling assumption,
    // proves the side effect happened while physical completion is gated.
    waiting.recv_timeout(Duration::from_secs(2)).unwrap();
    let first = pending.join().unwrap();
    let mut next = request.clone();
    next.action_id = "next".into();
    let second = broker.act(next.clone());
    assert_eq!(first.kind, OutcomeKind::Unknown);
    assert_eq!(second.kind, OutcomeKind::Rejected);
    assert!(!second.executed);
    assert_eq!(adapter.fake.executions(), ["slow"]);
    assert!(matches!(
        broker.observe("run"),
        Err(BrokerError::LeaseHeld { .. })
    ));

    release.send(()).unwrap();
    let until = Instant::now() + Duration::from_secs(2);
    while broker.inner.lock().runs["run"]
        .in_flight
        .load(Ordering::SeqCst)
        && Instant::now() < until
    {
        std::thread::yield_now();
    }
    assert!(!broker.inner.lock().runs["run"]
        .in_flight
        .load(Ordering::SeqCst));
    let after_completion = broker.act(next.clone());
    assert_eq!(after_completion.kind, OutcomeKind::Rejected);
    assert!(after_completion.reason.unwrap().contains("unknown result"));
    let fresh = broker.observe("run").unwrap();
    next.snapshot_id = fresh.snapshot_id;
    next.geometry_revision = fresh.geometry_revision;
    assert_eq!(broker.act(next).kind, OutcomeKind::Verified);
    assert_eq!(broker.act(request).kind, OutcomeKind::Unknown);
    assert_eq!(adapter.fake.executions(), ["slow", "next"]);
}

#[test]
fn wait_keeps_admission_and_stop_rejects_its_late_match() {
    let (broker, adapter) = ready(3);
    let args = super::test_support::wait_args(&broker, "run", "Count", 2000);
    let snapshot = broker.model_snapshot_id("run").unwrap();
    let (entered, release) = adapter.gate_next();
    let caller = broker.clone();
    let pending =
        std::thread::spawn(move || crate::tools_wait::wait_for_node(&caller, "run", args));
    entered.recv_timeout(Duration::from_secs(2)).unwrap();
    let busy = broker.observe_preview("run");
    assert!(matches!(busy, Err(BrokerError::LeaseHeld { .. })));
    assert_eq!(broker.model_snapshot_id("run").unwrap(), snapshot);
    assert_eq!(
        broker.request_stop("run").unwrap(),
        StopState::StopRequested
    );
    release.send(()).unwrap();
    let result = pending.join().unwrap();
    assert_eq!(result["isError"], true, "{result}");
    assert!(adapter.fake.executions().is_empty());
    assert_eq!(
        broker.wait_stopped("run", Duration::from_secs(1)).unwrap(),
        StopState::Stopped
    );
}
