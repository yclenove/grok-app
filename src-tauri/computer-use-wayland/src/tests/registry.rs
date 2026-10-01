use super::adapter::FixturePolicy;
use super::*;
use grok_computer_use_core::adapter::ComputerUseAdapter;
use grok_computer_use_core::protocol::JS_MAX_SAFE_INTEGER;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Condvar,
};

pub(super) fn options(run: &str) -> PortalOptions {
    let mut options = PortalOptions::new(run.into(), SourceKind::Monitor);
    options.parent_window = "wayland:owned-fixture-parent".into();
    options.timeout = Duration::from_secs(3);
    options
}

fn select(registry: &PortalRegistry, fixture: &Fixture, run: &str) -> PortalSelection {
    let address = fixture.bus.address.clone();
    registry
        .select_with(options(run), 7, 7, move |options| {
            assert_eq!(std::thread::current().name(), Some("cu-wayland-registry"));
            PortalSession::on_test_bus(options, address)
        })
        .unwrap()
}

pub(super) async fn ready(registry: &PortalRegistry, ticket: &PortalSelection) -> String {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match registry.state(ticket).unwrap() {
                PortalSelectionState::Ready { target_id } => return target_id,
                PortalSelectionState::Pending => {}
                other => panic!("selection failed before readiness: {other:?}"),
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("registry did not publish ready grant")
}

pub(super) async fn closed(registry: &PortalRegistry, ticket: &PortalSelection) {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match registry.state(ticket).unwrap() {
                PortalSelectionState::Closed { .. } => return,
                PortalSelectionState::Failed { error } => panic!("cleanup unproven: {error}"),
                _ => {}
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("registry did not join selection owner")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn maintenance_requires_policy_fence_and_original_owner_join() {
    let fixture = Fixture::new(Mode::SilentStart).await;
    let policy = Arc::new(FixturePolicy::default());
    let registry = PortalRegistry::new(policy.clone());
    let ticket = select(&registry, &fixture, "maintenance");
    tokio::time::timeout(Duration::from_secs(2), async {
        while !fixture.shared.called("Start") {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    assert!(registry.quiesce_for_maintenance().is_err());
    policy.deny.store(true, Ordering::SeqCst);
    assert!(!registry.quiesce_for_maintenance().unwrap());
    let address = fixture.bus.address.clone();
    assert!(registry
        .select_with(options("new-during-maintenance"), 8, 8, move |o| {
            PortalSession::on_test_bus(o, address)
        })
        .is_err());
    closed(&registry, &ticket).await;
    assert!(registry.quiesce_for_maintenance().unwrap());
    fixture.assert_sessions_closed();
    registry.forget(&ticket).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pending_consent_is_occupied_and_abort_preserves_exact_run_generation() {
    let fixture = Fixture::new(Mode::SilentStart).await;
    let registry = PortalRegistry::new(Arc::new(FixturePolicy::default()));
    let ticket = select(&registry, &fixture, "pending");
    tokio::time::timeout(Duration::from_secs(2), async {
        while !fixture.shared.called("Start") {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    assert!(!registry.is_idle("pending"));
    assert!(registry.list_targets_for_run("pending").unwrap().is_empty());
    assert!(registry.forget(&ticket).is_err());
    registry.abort("foreign", u64::MAX).unwrap();
    registry.abort("pending", 7).unwrap();
    assert_eq!(
        registry.state(&ticket).unwrap(),
        PortalSelectionState::Pending
    );
    let address = fixture.bus.address.clone();
    assert!(registry
        .select_with(options("pending"), 8, 8, move |o| {
            PortalSession::on_test_bus(o, address)
        })
        .is_err());
    registry.abort("pending", 8).unwrap();
    closed(&registry, &ticket).await;
    assert!(registry.is_idle("pending"));
    fixture.assert_sessions_closed();
    registry.forget(&ticket).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ready_stop_retains_original_reactor_and_join_before_forget() {
    let fixture = Fixture::new(Mode::SlowCleanup).await;
    let registry = PortalRegistry::new(Arc::new(FixturePolicy::default()));
    let ticket = select(&registry, &fixture, "slow");
    let target = ready(&registry, &ticket).await;
    assert!(!registry.capabilities().native_wayland);
    assert!(registry.is_idle("slow"));
    registry.abort("slow", 7).unwrap();
    assert!(registry.target_alive(&target));
    let start = std::time::Instant::now();
    registry.cancel(&ticket).unwrap();
    assert!(start.elapsed() < Duration::from_millis(250));
    assert!(!registry.target_alive(&target));
    assert!(!registry.is_idle("slow"));
    assert!(registry.forget(&ticket).is_err());
    assert!(registry.claim_target_for_run("slow", &target).is_err());
    closed(&registry, &ticket).await;
    assert!(start.elapsed() >= Duration::from_millis(700));
    assert!(registry.is_idle("slow"));
    fixture.assert_sessions_closed();
    fixture.shared.assert_peers_closed(2);
    registry.forget(&ticket).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn independent_runs_never_share_targets_claims_or_retirement() {
    let fixture = Fixture::new(Mode::Normal).await;
    let registry = PortalRegistry::new(Arc::new(FixturePolicy::default()));
    let a = select(&registry, &fixture, "a");
    let b = select(&registry, &fixture, "b");
    let ta = ready(&registry, &a).await;
    let tb = ready(&registry, &b).await;
    assert_ne!(ta, tb);
    assert!(registry.list_targets().is_err());
    assert!(registry.list_targets_for_run("foreign").unwrap().is_empty());
    assert_eq!(registry.list_targets_for_run("a").unwrap()[0].target_id, ta);
    assert_eq!(registry.list_targets_for_run("b").unwrap()[0].target_id, tb);
    assert!(registry.claim_target(&ta).is_err());
    assert!(registry.claim_target_for_run("b", &ta).is_err());
    registry.claim_target_for_run("a", &ta).unwrap();
    registry.claim_target_for_run("b", &tb).unwrap();
    registry.release_target_for_run("b", &ta);
    assert!(registry.target_alive(&ta));
    registry.cancel(&a).unwrap();
    closed(&registry, &a).await;
    assert!(registry.target_alive(&tb));
    assert!(registry.is_idle("b"));
    registry.release_target_for_run("b", &tb);
    closed(&registry, &b).await;
    fixture.assert_sessions_closed();
    fixture.shared.assert_peers_closed(4);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn old_ticket_and_foreign_registry_cannot_cancel_same_run_replacement() {
    let fixture = Fixture::new(Mode::Normal).await;
    let registry = PortalRegistry::new(Arc::new(FixturePolicy::default()));
    let old = select(&registry, &fixture, "reused");
    ready(&registry, &old).await;
    registry.cancel(&old).unwrap();
    closed(&registry, &old).await;
    registry.forget(&old).unwrap();
    let next = select(&registry, &fixture, "reused");
    let target = ready(&registry, &next).await;
    let other = PortalRegistry::new(Arc::new(FixturePolicy::default()));
    let foreign = select(&other, &fixture, "reused");
    ready(&other, &foreign).await;
    assert!(registry.cancel(&old).is_err());
    assert!(registry.forget(&old).is_err());
    assert!(registry.state(&old).is_err());
    assert!(registry.cancel(&foreign).is_err());
    assert!(registry.target_alive(&target));
    registry.cancel(&next).unwrap();
    other.cancel(&foreign).unwrap();
    closed(&registry, &next).await;
    closed(&other, &foreign).await;
    fixture.assert_sessions_closed();
    fixture.shared.assert_peers_closed(6);
}

struct PublicationGate {
    fixture: Arc<Shared>,
    entered: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
    release: (Mutex<bool>, Condvar),
    denied: AtomicBool,
}
impl PortalInputPolicy for PublicationGate {
    fn input_available(&self) -> bool {
        // Gate the actual native handoff, not earlier policy monitor ticks.
        if std::thread::current().name() == Some("cu-wayland-registry")
            && self.fixture.called("ConnectToEIS")
        {
            if let Some(send) = self.entered.lock().unwrap().take() {
                let _ = send.send(());
            }
            let (_held, deadline) = self
                .release
                .1
                .wait_timeout_while(
                    self.release.0.lock().unwrap(),
                    Duration::from_secs(5),
                    |released| !*released,
                )
                .unwrap();
            if deadline.timed_out() {
                return false;
            }
        }
        !self.denied.load(Ordering::SeqCst)
    }
    fn user_input_active(&self) -> bool {
        false
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancel_during_native_handoff_cannot_publish_late_grant() {
    publication_race(false).await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn policy_loss_during_native_handoff_cannot_publish_late_grant() {
    publication_race(true).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn session_ticket_cancel_during_handoff_never_publishes_ready() {
    use grok_computer_use_core::{broker::StopState, session_grants::SessionGrants};
    let fixture = Fixture::new(Mode::Normal).await;
    let (send, entered) = tokio::sync::oneshot::channel();
    let gate = Arc::new(PublicationGate {
        fixture: fixture.shared.clone(),
        entered: Mutex::new(Some(send)),
        release: (Mutex::new(false), Condvar::new()),
        denied: AtomicBool::new(false),
    });
    let registry = Arc::new(PortalRegistry::new(gate.clone()));
    let broker = super::authorization::broker(&registry);
    let grants = SessionGrants::default();
    let ticket = grants.begin(&broker, "chat", None).unwrap();
    let address = fixture.bus.address.clone();
    let selection = registry
        .select_guarded(
            options(&ticket.run_id),
            1,
            1,
            Some(ticket.clone()),
            move |o| PortalSession::on_test_bus(o, address),
        )
        .unwrap();
    tokio::time::timeout(Duration::from_secs(2), entered)
        .await
        .unwrap()
        .unwrap();
    let fence = grants.fence_revoke_checked(&broker, "chat", || {});
    assert!(ticket.is_preparation_cancelled());
    assert_eq!(
        registry.state(&selection).unwrap(),
        PortalSelectionState::Closing
    );
    assert!(!registry.is_idle(&ticket.run_id));
    assert!(registry.forget(&selection).is_err());
    *gate.release.0.lock().unwrap() = true;
    gate.release.1.notify_all();
    closed(&registry, &selection).await;
    assert!(registry
        .list_targets_for_run(&ticket.run_id)
        .unwrap()
        .is_empty());
    fixture.assert_sessions_closed();
    fixture.shared.assert_peers_closed(2);
    broker.finish_stop_cleanup(&fence.cleanup.unwrap()).unwrap();
    assert_eq!(
        broker.stop_state(&ticket.run_id).unwrap(),
        StopState::Stopped
    );
    registry.forget(&selection).unwrap();
}

async fn publication_race(deny: bool) {
    let fixture = Fixture::new(Mode::Normal).await;
    let (send, entered) = tokio::sync::oneshot::channel();
    let gate = Arc::new(PublicationGate {
        fixture: fixture.shared.clone(),
        entered: Mutex::new(Some(send)),
        release: (Mutex::new(false), Condvar::new()),
        denied: AtomicBool::new(false),
    });
    let registry = PortalRegistry::new(gate.clone());
    let ticket = select(&registry, &fixture, "handoff");
    tokio::time::timeout(Duration::from_secs(2), entered)
        .await
        .unwrap()
        .unwrap();
    assert!(!registry.is_idle("handoff"));
    assert!(registry.list_targets_for_run("handoff").unwrap().is_empty());
    if deny {
        gate.denied.store(true, Ordering::SeqCst);
    } else {
        registry.cancel(&ticket).unwrap();
    }
    assert!(registry.forget(&ticket).is_err());
    *gate.release.0.lock().unwrap() = true;
    gate.release.1.notify_all();
    closed(&registry, &ticket).await;
    assert!(registry.list_targets_for_run("handoff").unwrap().is_empty());
    fixture.assert_sessions_closed();
    fixture.shared.assert_peers_closed(2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dropping_registry_requests_stop_but_retained_owner_finishes_cleanup() {
    let fixture = Fixture::new(Mode::SlowCleanup).await;
    let registry = PortalRegistry::new(Arc::new(FixturePolicy::default()));
    let ticket = select(&registry, &fixture, "drop");
    ready(&registry, &ticket).await;
    let retained = registry.entry("drop").unwrap();
    drop(registry);
    assert!(!retained.idle());
    tokio::time::timeout(Duration::from_secs(5), async {
        while !retained.idle() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    fixture.assert_sessions_closed();
    fixture.shared.assert_peers_closed(2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn denied_selection_reports_reason_and_joins_original_owner() {
    let fixture = Fixture::new(Mode::Denied).await;
    let registry = PortalRegistry::new(Arc::new(FixturePolicy::default()));
    let ticket = select(&registry, &fixture, "denied");
    closed(&registry, &ticket).await;
    assert!(matches!(
        registry.state(&ticket).unwrap(),
        PortalSelectionState::Closed { reason: Some(_) }
    ));
    assert!(registry.is_idle("denied"));
    assert!(registry.list_targets_for_run("denied").unwrap().is_empty());
    fixture.assert_sessions_closed();
    registry.forget(&ticket).unwrap();
}

#[test]
fn invalid_or_unparented_selection_fails_before_native_start() {
    let policy = Arc::new(FixturePolicy::default());
    let registry = PortalRegistry::new(policy.clone());
    for (mut value, generation) in [
        (options(""), 1),
        (options("valid"), 0),
        (options("valid"), JS_MAX_SAFE_INTEGER + 1),
    ] {
        value.timeout = Duration::from_secs(1);
        assert!(registry
            .select_with(value, generation, 1, |_| panic!(
                "invalid selection reached native backend"
            ))
            .is_err());
    }
    let mut value = options("valid");
    value.parent_window.clear();
    assert!(registry
        .select_with(value, 1, 1, |_| panic!("missing parent"))
        .is_err());
    let mut value = options("valid");
    value.source = SourceKind::Window;
    assert!(registry
        .select_with(value, 1, 1, |_| panic!("window input unsupported"))
        .is_err());
    policy.deny.store(true, Ordering::SeqCst);
    assert!(registry
        .select_with(options("valid"), 1, 1, |_| panic!("policy denial"))
        .is_err());
    assert!(registry.all_entries().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn failed_handoff_retains_original_session_for_exact_cleanup() {
    let fixture = Fixture::new(Mode::Normal).await;
    let mut session = fixture.start();
    session.ready().await.unwrap();
    let mut slot = Some(session);
    assert!(PortalHostSession::from_session_slot(&mut slot, 0, 1).is_err());
    assert!(matches!(
        slot.as_ref().unwrap().state(),
        SessionState::Granted(_)
    ));
    let mut host = Some(PortalHostSession::from_session_slot(&mut slot, 1, 1).unwrap());
    assert!(slot.is_none());
    host.as_mut().unwrap().stop().await.unwrap();
    assert!(
        PortalAdapter::from_granted_slot(&mut host, Arc::new(FixturePolicy::default())).is_err()
    );
    assert!(host.is_some());
    host.as_mut().unwrap().stop().await.unwrap();
    fixture.assert_sessions_closed();
    fixture.shared.assert_peers_closed(2);
}
