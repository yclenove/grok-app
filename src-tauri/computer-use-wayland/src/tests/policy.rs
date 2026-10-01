//! Policy loss must retire authority even without another model operation.
use super::adapter::{idle, FixturePolicy};
use super::registry::{closed, options, ready};
use super::*;
use grok_computer_use_core::adapter::ComputerUseAdapter;
use std::sync::atomic::Ordering;

async fn wait_for(mut predicate: impl FnMut() -> bool) -> bool {
    tokio::time::timeout(Duration::from_secs(1), async {
        while !predicate() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .is_ok()
}

async fn pending_loss(takeover: bool) {
    let fixture = Fixture::new(Mode::SilentStart).await;
    let policy = Arc::new(FixturePolicy::default());
    let registry = PortalRegistry::new(policy.clone());
    let mut value = options("policy-pending");
    value.timeout = Duration::from_secs(60);
    let address = fixture.bus.address.clone();
    let ticket = registry
        .select_with(value, 1, 1, move |o| PortalSession::on_test_bus(o, address))
        .unwrap();
    assert!(wait_for(|| fixture.shared.called("Start")).await);
    if takeover {
        policy.takeover.store(true, Ordering::SeqCst);
    } else {
        policy.deny.store(true, Ordering::SeqCst);
    }
    let retired = wait_for(|| {
        !matches!(
            registry.state(&ticket).unwrap(),
            PortalSelectionState::Pending
        )
    })
    .await;
    // Always join the original owner, including on the red baseline.
    registry.cancel(&ticket).unwrap();
    closed(&registry, &ticket).await;
    fixture.assert_sessions_closed();
    assert!(registry.is_idle("policy-pending"));
    assert!(
        retired,
        "policy loss left consent pending until its 60s deadline"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn policy_revocation_closes_pending_consent_without_next_operation() {
    pending_loss(false).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn policy_takeover_closes_pending_consent_without_next_operation() {
    pending_loss(true).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn policy_revocation_retires_idle_registry_and_never_revives_old_grant() {
    let fixture = Fixture::new(Mode::Normal).await;
    let policy = Arc::new(FixturePolicy::default());
    let registry = PortalRegistry::new(policy.clone());
    let address = fixture.bus.address.clone();
    let ticket = registry
        .select_with(options("policy-idle"), 1, 1, move |o| {
            PortalSession::on_test_bus(o, address)
        })
        .unwrap();
    let target = ready(&registry, &ticket).await;
    let retained = registry.entry("policy-idle").unwrap().adapter().unwrap();
    registry
        .claim_target_for_run("policy-idle", &target)
        .unwrap();
    policy.deny.store(true, Ordering::SeqCst);
    let retired = wait_for(|| !retained.target_alive(&target)).await;
    policy.deny.store(false, Ordering::SeqCst);
    let revived = retained.target_alive(&target);
    registry.cancel(&ticket).unwrap();
    closed(&registry, &ticket).await;
    fixture.assert_sessions_closed();
    fixture.shared.assert_peers_closed(2);
    assert!(
        retired,
        "idle policy revocation never reached the native owner"
    );
    assert!(!revived, "restoring permission resurrected the old target");
    assert!(registry
        .claim_target_for_run("policy-idle", &target)
        .is_err());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn policy_takeover_retires_standalone_adapter_without_registry_or_model_call() {
    let fixture = Fixture::new(Mode::Normal).await;
    let policy = Arc::new(FixturePolicy::default());
    let mut session = fixture.start();
    session.ready().await.unwrap();
    let host = PortalHostSession::new(session, 1, 1).unwrap();
    let adapter = PortalAdapter::from_granted(host, policy.clone()).unwrap();
    let run = "owned-native-fixture";
    let target = adapter.target_id().to_owned();
    adapter.claim_target_for_run(run, &target).unwrap();
    policy.takeover.store(true, Ordering::SeqCst);
    let retired = wait_for(|| !adapter.target_alive(&target)).await;
    policy.takeover.store(false, Ordering::SeqCst);
    let revived = adapter.target_alive(&target);
    adapter.abort(run, u64::MAX).unwrap();
    idle(&adapter, run).await;
    fixture.assert_sessions_closed();
    fixture.shared.assert_peers_closed(2);
    assert!(
        retired,
        "standalone adapter requires external Stop after takeover"
    );
    assert!(!revived, "takeover ending silently revived the old grant");
}

#[test]
fn policy_lease_latches_denial_and_takeover_but_fresh_consent_gets_new_lease() {
    let policy = Arc::new(FixturePolicy::default());
    for takeover in [false, true] {
        let lease = crate::policy::PolicyLease::new(policy.clone());
        assert!(lease.input_available());
        if takeover {
            policy.takeover.store(true, Ordering::SeqCst);
        } else {
            policy.deny.store(true, Ordering::SeqCst);
        }
        assert!(!lease.input_available());
        policy.deny.store(false, Ordering::SeqCst);
        policy.takeover.store(false, Ordering::SeqCst);
        assert!(!lease.input_available());
        assert!(lease.is_revoked());
        assert!(crate::policy::PolicyLease::new(policy.clone()).input_available());
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn policy_recovery_needs_exact_old_join_and_fresh_explicit_selection() {
    let fixture = Fixture::new(Mode::Normal).await;
    let policy = Arc::new(FixturePolicy::default());
    let registry = PortalRegistry::new(policy.clone());
    let select = || {
        let address = fixture.bus.address.clone();
        registry.select_with(options("policy-recovery"), 1, 1, move |o| {
            PortalSession::on_test_bus(o, address)
        })
    };
    let old = select().unwrap();
    let old_target = ready(&registry, &old).await;
    policy.takeover.store(true, Ordering::SeqCst);
    closed(&registry, &old).await;
    policy.takeover.store(false, Ordering::SeqCst);
    assert!(!registry.target_alive(&old_target));
    assert!(
        select().is_err(),
        "recovery discarded retained old selection"
    );
    registry.forget(&old).unwrap();
    let new = select().unwrap();
    let target = ready(&registry, &new).await;
    assert_ne!(target, old_target);
    assert!(registry
        .claim_target_for_run("policy-recovery", &old_target)
        .is_err());
    assert!(registry.cancel(&old).is_err());
    registry
        .claim_target_for_run("policy-recovery", &target)
        .unwrap();
    registry.cancel(&new).unwrap();
    closed(&registry, &new).await;
    fixture.assert_sessions_closed();
    fixture.shared.assert_peers_closed(4);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn policy_query_cannot_certify_idle_before_retained_owner_observes_revocation() {
    use std::sync::{atomic::AtomicBool, Condvar};
    struct HeldMonitor {
        deny: AtomicBool,
        entered: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
        release: (Mutex<bool>, Condvar),
    }
    impl PortalInputPolicy for HeldMonitor {
        fn input_available(&self) -> bool {
            if std::thread::current().name() == Some("cu-wayland-host") {
                if let Some(entered) = self.entered.lock().unwrap().take() {
                    let _ = entered.send(());
                }
                let (_held, timeout) = self
                    .release
                    .1
                    .wait_timeout_while(
                        self.release.0.lock().unwrap(),
                        Duration::from_secs(3),
                        |ready| !*ready,
                    )
                    .unwrap();
                if timeout.timed_out() {
                    return false;
                }
            }
            !self.deny.load(Ordering::SeqCst)
        }
        fn user_input_active(&self) -> bool {
            false
        }
    }
    let fixture = Fixture::new(Mode::Normal).await;
    let (send, entered) = tokio::sync::oneshot::channel();
    let policy = Arc::new(HeldMonitor {
        deny: AtomicBool::new(false),
        entered: Mutex::new(Some(send)),
        release: (Mutex::new(false), Condvar::new()),
    });
    let mut session = fixture.start();
    session.ready().await.unwrap();
    let host = PortalHostSession::new(session, 1, 1).unwrap();
    let adapter = PortalAdapter::from_granted(host, policy.clone()).unwrap();
    tokio::time::timeout(Duration::from_secs(1), entered)
        .await
        .unwrap()
        .unwrap();
    policy.deny.store(true, Ordering::SeqCst);
    assert!(!adapter.input_available());
    let prematurely_idle = adapter.is_idle("owned-native-fixture");
    *policy.release.0.lock().unwrap() = true;
    policy.release.1.notify_all();
    idle(&adapter, "owned-native-fixture").await;
    fixture.assert_sessions_closed();
    fixture.shared.assert_peers_closed(2);
    assert!(
        !prematurely_idle,
        "native owner was still held at revocation"
    );
}
