//! Controlled startup scheduling; armed health deadlines remain unchanged.
use super::*;
use std::sync::mpsc;

struct Api {
    entered: Mutex<Option<tokio::sync::oneshot::Sender<Arc<GuardEvents>>>>,
    release: Mutex<mpsc::Receiver<()>>,
}
struct Pump(Arc<GuardEvents>);
impl SafetyApi for Api {
    fn open(
        &self,
        _: HotkeyConfig,
        events: Arc<GuardEvents>,
    ) -> std::result::Result<Box<dyn SafetyPump>, String> {
        events.startup_progress(
            "hook_install",
            "SetWindowsHookExW pending; Win32 result unobserved",
        );
        if let Some(tx) = self.entered.lock().unwrap().take() {
            let _ = tx.send(events.clone());
        }
        self.release
            .lock()
            .unwrap()
            .recv_timeout(Duration::from_secs(2))
            .map_err(|e| e.to_string())?;
        Ok(Box::new(Pump(events)))
    }
}
impl SafetyPump for Pump {
    fn poll(&mut self, _: bool) -> std::result::Result<(), String> {
        self.0.clock().confirm(Instant::now());
        self.0.hotkey_heartbeat();
        Ok(())
    }
}
fn api() -> (
    Arc<Api>,
    tokio::sync::oneshot::Receiver<Arc<GuardEvents>>,
    mpsc::Sender<()>,
) {
    let (entered, wait) = tokio::sync::oneshot::channel();
    let (release, receiver) = mpsc::channel();
    (
        Arc::new(Api {
            entered: Mutex::new(Some(entered)),
            release: Mutex::new(receiver),
        }),
        wait,
        release,
    )
}
fn start(api: Arc<Api>) -> tokio::task::JoinHandle<Arc<SafetyGuard>> {
    tokio::spawn(SafetyGuard::start(
        api,
        Default::default(),
        Arc::new(Unstopped),
    ))
}
struct Unstopped;
impl StopLatch for Unstopped {
    fn is_stopped(&self) -> bool {
        false
    }
    fn request_stop(&self) {}
}

#[tokio::test]
async fn scheduled_setup_before_deadline_stays_disabled_until_both_proofs_arrive() {
    let (api, entered, release) = api();
    let task = start(api);
    let events = entered.await.unwrap();
    tokio::time::pause();
    tokio::time::advance(STARTUP_TIMEOUT / 2).await;
    assert!(!task.is_finished());
    assert!(!events.available());
    tokio::time::resume();
    release.send(()).unwrap();
    let guard = task.await.unwrap();
    assert!(guard.available());
    assert!(guard.events.reason.lock().unwrap().is_none());
}

#[tokio::test]
async fn stuck_startup_retains_pending_call_and_late_completion_never_revives_health() {
    let (api, entered, release) = api();
    let task = start(api);
    let events = entered.await.unwrap();
    tokio::time::pause();
    tokio::time::advance(STARTUP_TIMEOUT + Duration::from_millis(1)).await;
    let guard = task.await.unwrap();
    let mut issues = Vec::new();
    guard.doctor_issues(&mut issues);
    assert!(issues.iter().any(|i| i.contains("3000ms")
        && i.contains("SetWindowsHookExW pending")
        && i.contains("Win32 result unobserved")));
    release.send(()).unwrap();
    events.clock().confirm(Instant::now());
    events.heartbeat();
    events.hotkey_heartbeat();
    assert!(!guard.available());
    assert!(!guard.activity_clock().available());
}

#[derive(Default)]
struct ManualTime(Mutex<Option<Instant>>);
impl MonotonicTime for ManualTime {
    fn now(&self) -> Instant {
        *self.0.lock().unwrap().get_or_insert_with(Instant::now)
    }
}
impl ManualTime {
    fn advance(&self, elapsed: Duration) {
        let at = self.now() + elapsed;
        *self.0.lock().unwrap() = Some(at);
    }
}

#[test]
fn startup_probe_taking_226ms_succeeds_without_relaxing_armed_health_deadline() {
    let time = Arc::new(ManualTime::default());
    let guard = SafetyGuard::new(Arc::new(Unstopped), time.clone());
    let first_keyboard = time.now();
    guard
        .events
        .startup_step("hook_proofs", STARTUP_STEP_TIMEOUT);
    time.advance(Duration::from_millis(226));
    guard.events.check_health();
    assert!(!guard.available());
    assert!(!guard.events.failed.load(Ordering::Acquire));
    // The first mouse arrives after a 226ms native dispatch. An old keyboard
    // proof must not arm, nor permanently fail a listener still in setup.
    guard
        .events
        .confirm_hook_proofs(Some(first_keyboard), Some(time.now()))
        .unwrap();
    guard.events.heartbeat();
    guard.events.hotkey_heartbeat();
    assert!(!guard.available());
    guard
        .events
        .confirm_hook_proofs(Some(time.now()), Some(time.now()))
        .unwrap();
    guard.events.heartbeat();
    guard.events.hotkey_heartbeat();
    assert!(guard.available());
    time.advance(watchdog::HEARTBEAT_TIMEOUT + Duration::from_millis(1));
    assert!(guard
        .events
        .confirm_hook_proofs(Some(first_keyboard), Some(time.now()))
        .is_err());
    guard.events.check_health();
    assert!(!guard.available());
    assert!(guard.is_stopped());
}

#[test]
fn stuck_native_stage_fails_at_its_bound_and_late_proofs_cannot_rearm() {
    let time = Arc::new(ManualTime::default());
    let guard = SafetyGuard::new(Arc::new(Unstopped), time.clone());
    guard
        .events
        .startup_step("hook_install", STARTUP_STEP_TIMEOUT);
    guard.events.startup_progress(
        "hook_install",
        "SetWindowsHookExW pending; Win32 result unobserved",
    );
    time.advance(STARTUP_STEP_TIMEOUT - Duration::from_millis(1));
    guard.events.check_health();
    assert!(!guard.events.failed.load(Ordering::Acquire));
    time.advance(Duration::from_millis(1));
    guard.events.check_health();
    let reason = guard.events.reason.lock().unwrap().clone().unwrap();
    assert!(reason.contains("hook_install timed out after 1000ms"));
    assert!(reason.contains("Win32 result unobserved"));
    guard.events.clock().confirm(time.now());
    guard.events.heartbeat();
    guard.events.hotkey_heartbeat();
    assert!(!guard.available());
}

#[test]
fn startup_stages_cannot_extend_the_overall_deadline() {
    let time = Arc::new(ManualTime::default());
    let guard = SafetyGuard::new(Arc::new(Unstopped), time.clone());
    for stage in [
        "listener_thread",
        "hook_desktop",
        "hook_install",
        "hook_proofs",
    ] {
        guard.events.startup_step(stage, STARTUP_STEP_TIMEOUT);
        time.advance(Duration::from_millis(750));
        guard.events.check_health();
    }
    assert!(guard
        .events
        .reason
        .lock()
        .unwrap()
        .as_ref()
        .unwrap()
        .contains("3000ms"));
    assert!(!guard.available());
}

#[test]
fn late_startup_proofs_cannot_beat_the_next_watchdog_poll_or_reset_a_stage() {
    let time = Arc::new(ManualTime::default());
    let guard = SafetyGuard::new(Arc::new(Unstopped), time.clone());
    guard
        .events
        .startup_step("hook_proofs", STARTUP_STEP_TIMEOUT);
    time.advance(STARTUP_STEP_TIMEOUT);
    guard
        .events
        .startup_step("late_stage", STARTUP_STEP_TIMEOUT);
    guard
        .events
        .confirm_hook_proofs(Some(time.now()), Some(time.now()))
        .unwrap();
    guard.events.heartbeat();
    guard.events.hotkey_heartbeat();
    assert!(!guard.available());
    assert!(guard
        .events
        .reason
        .lock()
        .unwrap()
        .as_ref()
        .unwrap()
        .contains("hook_proofs timed out after 1000ms"));
}

#[test]
fn listener_failure_revokes_health_and_latches_stop_before_entering_diagnostic_sink() {
    use std::cell::RefCell;
    use tracing::{
        span::{Attributes, Id, Record},
        Event, Metadata, Subscriber,
    };
    type Probe = (Arc<SafetyGuard>, Arc<AtomicBool>);
    std::thread_local! {
        static PROBE: RefCell<Option<Probe>> = const { RefCell::new(None) };
    }
    // A process-wide sink instead of a scoped one: scoped dispatchers race with
    // the callsite interest cache when other tests emit the same event in parallel.
    // Only the thread that installed a probe is inspected.
    struct Inspect;
    impl Subscriber for Inspect {
        fn enabled(&self, metadata: &Metadata<'_>) -> bool {
            metadata.target() == "gcu_windows_timing"
        }
        fn new_span(&self, _: &Attributes<'_>) -> Id {
            Id::from_u64(1)
        }
        fn record(&self, _: &Id, _: &Record<'_>) {}
        fn record_follows_from(&self, _: &Id, _: &Id) {}
        fn enter(&self, _: &Id) {}
        fn exit(&self, _: &Id) {}
        fn event(&self, _: &Event<'_>) {
            PROBE.with(|probe| {
                let Some((guard, inspected)) = probe.borrow().clone() else {
                    return;
                };
                // These guarantees must already hold if the sink now blocks forever.
                assert!(!guard.available());
                assert!(!guard.activity_clock().available());
                assert!(guard.is_stopped());
                assert_eq!(
                    guard.events.reason.lock().unwrap().as_deref(),
                    Some("fault injection")
                );
                inspected.store(true, Ordering::Release);
            });
        }
    }
    // Err means another test already installed a global sink; this process has none.
    tracing::subscriber::set_global_default(Inspect).expect("single global diagnostic sink");
    let api = FakeSafetyApi::default();
    let guard = api.start(Default::default(), Arc::new(Unstopped));
    assert!(guard.available());
    let inspected = Arc::new(AtomicBool::new(false));
    PROBE.with(|probe| *probe.borrow_mut() = Some((guard.clone(), inspected.clone())));
    guard.disable("fault injection");
    PROBE.with(|probe| *probe.borrow_mut() = None);
    assert!(inspected.load(Ordering::Acquire));
}
