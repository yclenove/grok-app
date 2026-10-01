use super::*;
use crate::fake::FakeAdapter;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

type CaptureCall = (String, u64, bool, bool, bool);

struct CallbackGate {
    entered: std::sync::mpsc::Sender<()>,
    resume: Mutex<std::sync::mpsc::Receiver<()>>,
}
impl CallbackGate {
    fn block(&self) {
        self.entered.send(()).unwrap();
        self.resume
            .lock()
            .recv_timeout(Duration::from_secs(5))
            .expect("test must release the owned callback");
    }
}
fn callback_gate() -> (
    Arc<CallbackGate>,
    std::sync::mpsc::Receiver<()>,
    std::sync::mpsc::Sender<()>,
) {
    let (entered, wait) = std::sync::mpsc::channel();
    let (resume, release) = std::sync::mpsc::channel();
    (
        Arc::new(CallbackGate {
            entered,
            resume: Mutex::new(release),
        }),
        wait,
        resume,
    )
}

struct Scoped {
    fake: FakeAdapter,
    claims: AtomicUsize,
    releases: AtomicUsize,
    idle: AtomicBool,
    fail_claim: AtomicBool,
    captures: Mutex<Vec<CaptureCall>>,
    claim_gate: Mutex<Option<Arc<CallbackGate>>>,
    release_gate: Mutex<Option<Arc<CallbackGate>>>,
    idle_gate: Mutex<Option<Arc<CallbackGate>>>,
}
impl Scoped {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            fake: FakeAdapter::new(),
            claims: AtomicUsize::new(0),
            releases: AtomicUsize::new(0),
            idle: AtomicBool::new(true),
            fail_claim: AtomicBool::new(false),
            captures: Mutex::new(Vec::new()),
            claim_gate: Mutex::new(None),
            release_gate: Mutex::new(None),
            idle_gate: Mutex::new(None),
        })
    }
    fn target(&self) -> String {
        self.fake.list_targets().unwrap()[0].target_id.clone()
    }
}
impl ComputerUseAdapter for Scoped {
    fn backend_id(&self) -> &'static str {
        "strict-scoped"
    }
    fn capabilities(&self) -> Capabilities {
        self.fake.capabilities()
    }
    fn list_targets(&self) -> Result<Vec<TargetInfo>, String> {
        Err("unscoped list refused".into())
    }
    fn list_targets_for_run(&self, run: &str) -> Result<Vec<TargetInfo>, String> {
        if run == "foreign" {
            Ok(Vec::new())
        } else {
            self.fake.list_targets()
        }
    }
    fn claim_target(&self, _: &str) -> Result<(), String> {
        Err("unscoped claim refused".into())
    }
    fn claim_target_for_run(&self, run: &str, target: &str) -> Result<(), String> {
        let gate = self.claim_gate.lock().take();
        if let Some(gate) = gate {
            gate.block();
        }
        if run == "foreign" || target != self.target() || self.fail_claim.load(Ordering::SeqCst) {
            return Err("scope refused".into());
        }
        self.claims.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    fn target_alive(&self, target: &str) -> bool {
        self.fake.target_alive(target)
    }
    fn observe(&self, _: &str) -> Result<Observation, String> {
        Err("unscoped observe refused".into())
    }
    fn capture_for_run(&self, _: &str, _: &str, _: CaptureOptions) -> Result<Observation, String> {
        Err("trusted generation missing".into())
    }
    fn capture_for_run_at_generation(
        &self,
        run: &str,
        target: &str,
        generation: u64,
        options: CaptureOptions,
    ) -> Result<Observation, String> {
        self.captures.lock().push((
            run.into(),
            generation,
            options.for_model,
            options.screenshot,
            options.cancellation.check().is_err(),
        ));
        options.cancellation.check()?;
        let mut image = self.fake.observe(target)?;
        image.run_id = run.into();
        image.target_generation = generation;
        Ok(image)
    }
    fn act(&self, req: &DispatchRequest) -> Result<AdapterActResult, String> {
        self.fake.act(req)
    }
    fn abort(&self, _: &str, _: u64) -> Result<(), String> {
        Ok(())
    }
    fn is_idle(&self, _: &str) -> bool {
        let idle = self.idle.load(Ordering::SeqCst);
        let gate = self.idle_gate.lock().take();
        if let Some(gate) = gate {
            gate.block();
        }
        idle
    }
    fn release_target_for_run(&self, _: &str, _: &str) {
        let gate = self.release_gate.lock().take();
        if let Some(gate) = gate {
            gate.block();
        }
        self.releases.fetch_add(1, Ordering::SeqCst);
    }
    fn start_periodic_preview(&self, _: &str) {}
    fn stop_periodic_preview(&self) {}
    fn periodic_preview_active(&self) -> bool {
        false
    }
}

#[test]
fn run_scoped_discovery_and_claim_reach_native_adapter() {
    let native = Scoped::new();
    let target = native.target();
    let owned = HostOwnedAdapter::new(native.clone());
    assert_eq!(
        owned.list_targets_for_run("run-a").unwrap()[0].target_id,
        target
    );
    assert!(owned.list_targets_for_run("foreign").unwrap().is_empty());
    assert!(owned.claim_target_for_run("foreign", &target).is_err());
    owned.claim_target_for_run("run-a", &target).unwrap();
    assert_eq!(native.claims.load(Ordering::SeqCst), 1);
}

#[test]
fn generation_and_capture_purpose_cross_product_wrapper_unchanged() {
    let native = Scoped::new();
    let target = native.target();
    let owned = HostOwnedAdapter::new(native.clone());
    owned.claim_target_for_run("run-a", &target).unwrap();
    let model = owned
        .capture_for_run_at_generation("run-a", &target, 17, CaptureOptions::model(true))
        .unwrap();
    assert_eq!(model.target_generation, 17);
    let mut preview = CaptureOptions::model(false);
    preview.for_model = false;
    owned
        .capture_for_run_at_generation("run-a", &target, 18, preview)
        .unwrap();
    let cancelled = CaptureOptions::model(true);
    cancelled.cancellation.cancel();
    assert!(owned
        .capture_for_run_at_generation("run-a", &target, 19, cancelled)
        .is_err());
    assert_eq!(
        *native.captures.lock(),
        vec![
            ("run-a".into(), 17, true, true, false),
            ("run-a".into(), 18, false, false, false),
            ("run-a".into(), 19, true, true, true)
        ]
    );
}

#[test]
fn foreign_and_unscoped_release_cannot_clear_run_owned_target() {
    let native = Scoped::new();
    let target = native.target();
    let owned = HostOwnedAdapter::new(native.clone());
    let other = HostOwnedAdapter::new(native.clone())
        .with_ownership_id("other")
        .share_owners(&owned);
    owned.claim_target_for_run("run-a", &target).unwrap();
    owned.release_target_for_run("foreign", &target);
    owned.release_target(&target);
    assert_eq!(native.releases.load(Ordering::SeqCst), 0);
    assert!(other.claim_target_for_run("run-b", &target).is_err());
    owned.release_target_for_run("run-a", &target);
    assert_eq!(native.releases.load(Ordering::SeqCst), 1);
    other.claim_target_for_run("run-b", &target).unwrap();
}

#[test]
fn retiring_native_owner_blocks_backend_switch_until_exact_idle() {
    let native = Scoped::new();
    let target = native.target();
    let owned = HostOwnedAdapter::new(native.clone());
    let other = HostOwnedAdapter::new(native.clone())
        .with_ownership_id("other")
        .share_owners(&owned);
    owned.claim_target_for_run("run-a", &target).unwrap();
    native.idle.store(false, Ordering::SeqCst);
    owned.release_target_for_run("run-a", &target);
    assert!(!owned.is_idle("run-a"));
    assert!(other.claim_target_for_run("run-b", &target).is_err());
    native.idle.store(true, Ordering::SeqCst);
    assert!(owned.is_idle("run-a"));
    other.claim_target_for_run("run-b", &target).unwrap();
}

#[test]
fn failed_native_claim_does_not_publish_local_ownership() {
    let native = Scoped::new();
    let target = native.target();
    let owned = HostOwnedAdapter::new(native.clone());
    let other = HostOwnedAdapter::new(native.clone())
        .with_ownership_id("other")
        .share_owners(&owned);
    native.fail_claim.store(true, Ordering::SeqCst);
    assert!(owned.claim_target_for_run("run-a", &target).is_err());
    native.fail_claim.store(false, Ordering::SeqCst);
    other.claim_target_for_run("run-b", &target).unwrap();
}

#[test]
fn same_backend_different_wrappers_do_not_share_one_claim_identity() {
    let native = Scoped::new();
    let target = native.target();
    let owned = HostOwnedAdapter::new(native.clone());
    let other = HostOwnedAdapter::new(native).share_owners(&owned);
    owned.claim_target_for_run("run-a", &target).unwrap();
    assert!(other.claim_target_for_run("run-a", &target).is_err());
    other.release_target_for_run("run-a", &target);
    assert!(other.claim_target_for_run("run-a", &target).is_err());
}

#[test]
fn supported_multiple_runs_remain_isolated_and_all_must_release_before_backend_switch() {
    let native = Scoped::new();
    let target = native.target();
    let owned = HostOwnedAdapter::new(native.clone());
    let other = HostOwnedAdapter::new(native.clone())
        .with_ownership_id("other")
        .share_owners(&owned);
    owned.claim_target_for_run("run-a", &target).unwrap();
    owned.claim_target_for_run("run-b", &target).unwrap();
    assert_eq!(native.claims.load(Ordering::SeqCst), 2);
    owned.release_target_for_run("run-a", &target);
    assert!(other.claim_target_for_run("run-c", &target).is_err());
    owned
        .capture_for_run_at_generation("run-b", &target, 4, CaptureOptions::model(true))
        .unwrap();
    owned.release_target_for_run("run-b", &target);
    other.claim_target_for_run("run-c", &target).unwrap();
}

#[test]
fn release_during_pending_claim_is_nonblocking_and_late_success_is_retired() {
    let native = Scoped::new();
    let target = native.target();
    let owned = Arc::new(HostOwnedAdapter::new(native.clone()));
    let other = HostOwnedAdapter::new(native.clone())
        .with_ownership_id("other")
        .share_owners(&owned);
    let (gate, entered, resume) = callback_gate();
    *native.claim_gate.lock() = Some(gate);
    let owner = owned.clone();
    let id = target.clone();
    let pending = std::thread::spawn(move || owner.claim_target_for_run("run-a", &id));
    entered.recv_timeout(Duration::from_secs(3)).unwrap();
    let started = std::time::Instant::now();
    assert!(!owned.is_idle("run-a"));
    owned.release_target_for_run("run-a", &target);
    assert!(other.claim_target_for_run("run-b", &target).is_err());
    assert!(started.elapsed() < Duration::from_millis(250));
    resume.send(()).unwrap();
    assert!(pending.join().unwrap().is_err());
    assert_eq!(native.releases.load(Ordering::SeqCst), 1);
    assert!(owned.is_idle("run-a"));
    other.claim_target_for_run("run-b", &target).unwrap();
}

#[test]
fn unfinished_release_callback_is_not_idle_and_holds_no_registry_mutex() {
    let native = Scoped::new();
    let target = native.target();
    let owned = Arc::new(HostOwnedAdapter::new(native.clone()));
    let other = HostOwnedAdapter::new(native.clone())
        .with_ownership_id("other")
        .share_owners(&owned);
    owned.claim_target_for_run("run-a", &target).unwrap();
    let (gate, entered, resume) = callback_gate();
    *native.release_gate.lock() = Some(gate);
    let owner = owned.clone();
    let id = target.clone();
    let pending = std::thread::spawn(move || owner.release_target_for_run("run-a", &id));
    entered.recv_timeout(Duration::from_secs(3)).unwrap();
    let started = std::time::Instant::now();
    assert!(!owned.is_idle("run-a"));
    assert!(other.claim_target_for_run("run-b", &target).is_err());
    assert!(started.elapsed() < Duration::from_millis(250));
    resume.send(()).unwrap();
    pending.join().unwrap();
    assert!(owned.is_idle("run-a"));
    other.claim_target_for_run("run-b", &target).unwrap();
}

#[test]
fn rejected_claim_with_unproven_cleanup_stays_reserved_until_native_idle() {
    let native = Scoped::new();
    let target = native.target();
    let owned = HostOwnedAdapter::new(native.clone());
    let other = HostOwnedAdapter::new(native.clone())
        .with_ownership_id("other")
        .share_owners(&owned);
    native.fail_claim.store(true, Ordering::SeqCst);
    native.idle.store(false, Ordering::SeqCst);
    assert!(owned.claim_target_for_run("run-a", &target).is_err());
    assert_eq!(native.releases.load(Ordering::SeqCst), 1);
    native.fail_claim.store(false, Ordering::SeqCst);
    assert!(other.claim_target_for_run("run-b", &target).is_err());
    native.idle.store(true, Ordering::SeqCst);
    assert!(owned.is_idle("run-a"));
    other.claim_target_for_run("run-b", &target).unwrap();
}

#[test]
fn stale_idle_query_cannot_retire_cleanup_started_after_its_native_read() {
    let native = Scoped::new();
    let target = native.target();
    let owned = Arc::new(HostOwnedAdapter::new(native.clone()));
    let other = HostOwnedAdapter::new(native.clone()).share_owners(&owned);
    owned.claim_target_for_run("run-a", &target).unwrap();
    let (gate, entered, resume) = callback_gate();
    *native.idle_gate.lock() = Some(gate);
    let owner = owned.clone();
    let pending = std::thread::spawn(move || owner.is_idle("run-a"));
    entered.recv_timeout(Duration::from_secs(3)).unwrap();
    native.idle.store(false, Ordering::SeqCst);
    owned.release_target_for_run("run-a", &target);
    assert!(other.claim_target_for_run("run-b", &target).is_err());
    resume.send(()).unwrap();
    assert!(
        !pending.join().unwrap(),
        "old idle read retired a newer cleanup"
    );
    assert!(other.claim_target_for_run("run-b", &target).is_err());
    native.idle.store(true, Ordering::SeqCst);
    assert!(owned.is_idle("run-a"));
    other.claim_target_for_run("run-b", &target).unwrap();
}

#[test]
fn stale_idle_query_cannot_retire_reclaimed_same_run_target_after_aba() {
    let native = Scoped::new();
    let target = native.target();
    let owned = Arc::new(HostOwnedAdapter::new(native.clone()));
    let other = HostOwnedAdapter::new(native.clone()).share_owners(&owned);
    owned.claim_target_for_run("run-a", &target).unwrap();
    native.idle.store(false, Ordering::SeqCst);
    owned.release_target_for_run("run-a", &target);
    native.idle.store(true, Ordering::SeqCst);
    let (gate, entered, resume) = callback_gate();
    *native.idle_gate.lock() = Some(gate);
    let owner = owned.clone();
    let pending = std::thread::spawn(move || owner.is_idle("run-a"));
    entered.recv_timeout(Duration::from_secs(3)).unwrap();
    // Retire the old reservation, then recreate the same run, target and phase.
    assert!(owned.is_idle("run-a"));
    owned.claim_target_for_run("run-a", &target).unwrap();
    native.idle.store(false, Ordering::SeqCst);
    owned.release_target_for_run("run-a", &target);
    resume.send(()).unwrap();
    assert!(
        !pending.join().unwrap(),
        "old idle read escaped the reservation ABA fence"
    );
    assert!(other.claim_target_for_run("run-b", &target).is_err());
    native.idle.store(true, Ordering::SeqCst);
    assert!(owned.is_idle("run-a"));
    other.claim_target_for_run("run-b", &target).unwrap();
}
