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
    assert!(issues.iter().any(|i| i.contains("250ms")
        && i.contains("SetWindowsHookExW pending")
        && i.contains("Win32 result unobserved")));
    release.send(()).unwrap();
    events.clock().confirm(Instant::now());
    events.heartbeat();
    events.hotkey_heartbeat();
    assert!(!guard.available());
    assert!(!guard.activity_clock().available());
}

#[test]
fn listener_failure_revokes_health_and_latches_stop_before_entering_diagnostic_sink() {
    use tracing::{
        span::{Attributes, Id, Record},
        Event, Metadata, Subscriber,
    };
    struct Inspect(Arc<SafetyGuard>, Arc<AtomicBool>);
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
            // These guarantees must already hold if the sink now blocks forever.
            assert!(!self.0.available());
            assert!(!self.0.activity_clock().available());
            assert!(self.0.is_stopped());
            assert_eq!(
                self.0.events.reason.lock().unwrap().as_deref(),
                Some("fault injection")
            );
            self.1.store(true, Ordering::Release);
        }
    }
    let api = FakeSafetyApi::default();
    let guard = api.start(Default::default(), Arc::new(Unstopped));
    assert!(guard.available());
    let inspected = Arc::new(AtomicBool::new(false));
    tracing::subscriber::with_default(Inspect(guard.clone(), inspected.clone()), || {
        guard.disable("fault injection");
    });
    assert!(inspected.load(Ordering::Acquire));
}
