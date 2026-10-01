use super::*;
use grok_computer_use_core::adapter::{CaptureOptions, ComputerUseAdapter};
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Default)]
pub(super) struct FixturePolicy {
    pub(super) deny: AtomicBool,
    pub(super) takeover: AtomicBool,
}
impl PortalInputPolicy for FixturePolicy {
    fn input_available(&self) -> bool {
        !self.deny.load(Ordering::SeqCst)
    }
    fn user_input_active(&self) -> bool {
        self.takeover.load(Ordering::SeqCst)
    }
}
pub(super) async fn idle(adapter: &PortalAdapter, run: &str) {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while !adapter.is_idle(run) {
        assert!(
            std::time::Instant::now() < deadline,
            "adapter did not retire: {:?}",
            adapter.cleanup_error()
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn timed_out_caller_does_not_release_busy_owner_or_block_stop_queries() {
    use std::sync::Condvar;
    struct HeldPolicy {
        entered: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
        release: (Mutex<bool>, Condvar),
    }
    impl PortalInputPolicy for HeldPolicy {
        fn input_available(&self) -> bool {
            if std::thread::current().name() == Some("cu-wayland-host") {
                if let Some(send) = self.entered.lock().unwrap().take() {
                    let _ = send.send(());
                }
                let (lock, wake) = &self.release;
                let (_held, timed) = wake
                    .wait_timeout_while(lock.lock().unwrap(), Duration::from_secs(10), |ready| {
                        !*ready
                    })
                    .unwrap();
                return !timed.timed_out();
            }
            true
        }
        fn user_input_active(&self) -> bool {
            false
        }
    }
    let fixture = Fixture::new(Mode::Normal).await;
    let mut session = fixture.start();
    session.ready().await.unwrap();
    let host = PortalHostSession::new(session, 1, 1).unwrap();
    let (send, entered) = tokio::sync::oneshot::channel();
    let policy = Arc::new(HeldPolicy {
        entered: Mutex::new(Some(send)),
        release: (Mutex::new(false), Condvar::new()),
    });
    let adapter = Arc::new(PortalAdapter::from_granted(host, policy.clone()).unwrap());
    let run = "owned-native-fixture";
    adapter
        .claim_target_for_run(run, adapter.target_id())
        .unwrap();
    let owner = adapter.clone();
    let capture = tokio::task::spawn_blocking(move || {
        owner.capture_for_run_at_generation(run, owner.target_id(), 3, CaptureOptions::model(true))
    });
    tokio::time::timeout(Duration::from_secs(2), entered)
        .await
        .unwrap()
        .unwrap();
    // The policy supervisor can enter the held callback before the queued
    // capture reaches admission. Establish occupancy explicitly, rather than
    // mistaking the monitor's preflight for command admission.
    tokio::time::timeout(Duration::from_secs(2), async {
        while adapter.is_idle(run) {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    let start = std::time::Instant::now();
    assert!(!adapter.is_idle(run));
    assert!(adapter.target_alive(adapter.target_id()));
    let busy = adapter
        .capture_for_run_at_generation(run, adapter.target_id(), 3, CaptureOptions::model(true))
        .unwrap_err();
    assert!(busy.contains("busy"));
    assert!(
        start.elapsed() < Duration::from_millis(250),
        "a callback held the shared status mutex"
    );
    let error = tokio::time::timeout(Duration::from_secs(7), capture)
        .await
        .unwrap()
        .unwrap()
        .unwrap_err();
    assert!(error.contains("completion unknown"));
    assert!(
        !adapter.is_idle(run),
        "business timeout is not owner completion"
    );
    assert!(!adapter.target_alive(adapter.target_id()));
    adapter.release_target_for_run(run, adapter.target_id());
    assert!(!adapter.is_idle(run));
    *policy.release.0.lock().unwrap() = true;
    policy.release.1.notify_all();
    idle(&adapter, run).await;
    fixture.assert_sessions_closed();
    fixture.shared.assert_peers_closed(2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn adapter_stop_is_nonblocking_but_idle_requires_exact_native_join() {
    let fixture = Fixture::new(Mode::SlowCleanup).await;
    let mut session = fixture.start();
    session.ready().await.unwrap();
    let host = PortalHostSession::new(session, 7, 7).unwrap();
    let adapter = PortalAdapter::from_granted(host, Arc::new(FixturePolicy::default())).unwrap();
    let run = "owned-native-fixture";
    let target = adapter.target_id().to_owned();
    assert!(adapter.list_targets().is_err());
    assert!(adapter.list_targets_for_run("foreign").unwrap().is_empty());
    assert_eq!(adapter.list_targets_for_run(run).unwrap().len(), 1);
    assert!(adapter.claim_target_for_run("foreign", &target).is_err());
    adapter.claim_target_for_run(run, &target).unwrap();
    adapter.abort("foreign", u64::MAX).unwrap();
    adapter.abort(run, 7).unwrap();
    assert!(adapter.target_alive(&target));
    let start = std::time::Instant::now();
    adapter.abort(run, 8).unwrap();
    assert!(start.elapsed() < Duration::from_millis(250));
    assert!(!adapter.target_alive(&target));
    assert!(!adapter.is_idle(run));
    assert!(adapter.claim_target_for_run(run, &target).is_err());
    adapter.release_target_for_run(run, &target);
    assert!(
        !adapter.is_idle(run),
        "release must not discard retirement owner"
    );
    idle(&adapter, run).await;
    assert!(start.elapsed() >= Duration::from_millis(700));
    assert!(adapter.cleanup_error().is_none());
    fixture.assert_sessions_closed();
    fixture.shared.assert_peers_closed(2);
}

#[tokio::test]
async fn adapter_refuses_current_thread_reactor_before_admitting_capture() {
    let fixture = Fixture::new(Mode::Normal).await;
    let mut session = fixture.start();
    session.ready().await.unwrap();
    let host = PortalHostSession::new(session, 1, 1).unwrap();
    let adapter = PortalAdapter::from_granted(host, Arc::new(FixturePolicy::default())).unwrap();
    let run = "owned-native-fixture";
    adapter
        .claim_target_for_run(run, adapter.target_id())
        .unwrap();
    let error = adapter
        .capture_for_run_at_generation(run, adapter.target_id(), 999, CaptureOptions::model(true))
        .unwrap_err();
    assert!(error.contains("current-thread reactor"));
    // 999 was never admitted. Fence 2 must still retire the original generation.
    adapter.abort(run, 2).unwrap();
    assert!(!adapter.target_alive(adapter.target_id()));
    idle(&adapter, run).await;
    fixture.assert_sessions_closed();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn adapter_drop_requests_native_retirement_without_claiming_completion() {
    let fixture = Fixture::new(Mode::SlowCleanup).await;
    let mut session = fixture.start();
    session.ready().await.unwrap();
    let mut state = session.subscribe();
    let host = PortalHostSession::new(session, 1, 1).unwrap();
    let adapter = PortalAdapter::from_granted(host, Arc::new(FixturePolicy::default())).unwrap();
    drop(adapter);
    assert!(!matches!(*state.borrow(), SessionState::Closed(_)));
    terminal(&mut state).await;
    fixture.assert_sessions_closed();
    fixture.shared.assert_peers_closed(2);
}
