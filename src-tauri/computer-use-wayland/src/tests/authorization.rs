//! Actual SessionGrants fences drive the retained native selection owner.
use super::adapter::FixturePolicy;
use super::registry::{closed, options, ready};
use super::*;
use grok_computer_use_core::{
    adapter::ComputerUseAdapter,
    broker::{BrokerOptions, ComputerUseBroker, StopState},
    owned::HostOwnedAdapter,
    session_grants::{AuthorizationTicket, SessionGrants},
};

pub(super) fn broker(registry: &Arc<PortalRegistry>) -> ComputerUseBroker {
    ComputerUseBroker::new(
        Arc::new(HostOwnedAdapter::new(registry.clone())),
        BrokerOptions {
            feature_enabled: true,
            lease_path: std::env::temp_dir().join(uuid::Uuid::new_v4().to_string()),
            ..BrokerOptions::default()
        },
    )
}

fn select(
    registry: &PortalRegistry,
    fixture: &Fixture,
    ticket: &AuthorizationTicket,
) -> PortalSelection {
    let address = fixture.bus.address.clone();
    registry
        .select_guarded(
            options(&ticket.run_id),
            1,
            1,
            Some(ticket.clone()),
            move |o| PortalSession::on_test_bus(o, address),
        )
        .unwrap()
}

#[test]
fn cancel_before_selection_rejects_late_native_start_and_finishes_absent_run() {
    let registry = Arc::new(PortalRegistry::new(Arc::new(FixturePolicy::default())));
    let broker = broker(&registry);
    let grants = SessionGrants::default();
    let ticket = grants
        .begin_attempt(&broker, "chat", None, "attempt", 1)
        .unwrap();
    let fence = grants
        .fence_cancel_attempt_checked(&broker, "chat", "attempt", 1, || {})
        .unwrap();
    assert!(fence.cancelled);
    assert!(fence.revoke.error.is_none());
    assert!(registry.is_idle(&ticket.run_id));
    let cleanup = fence.revoke.cleanup.unwrap();
    broker.finish_stop_cleanup(&cleanup).unwrap();
    assert_eq!(
        broker.stop_state(&ticket.run_id).unwrap(),
        StopState::Stopped
    );
    assert!(registry
        .select_guarded(options(&ticket.run_id), 1, 1, Some(ticket), |_| {
            panic!("cancelled authorization must never start native consent")
        })
        .is_err());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancelled_pending_consent_joins_before_broker_cleanup_is_dispatched() {
    let fixture = Fixture::new(Mode::SilentStart).await;
    let registry = Arc::new(PortalRegistry::new(Arc::new(FixturePolicy::default())));
    let broker = broker(&registry);
    let grants = SessionGrants::default();
    let ticket = grants.begin(&broker, "chat", None).unwrap();
    let selection = select(&registry, &fixture, &ticket);
    tokio::time::timeout(Duration::from_secs(2), async {
        while !fixture.shared.called("Start") {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    assert!(!registry.is_idle(&ticket.run_id));
    let fence = grants.fence_fail_checked(&broker, &ticket, || {});
    assert!(fence.matched && fence.error.is_none());
    assert!(registry
        .list_targets_for_run(&ticket.run_id)
        .unwrap()
        .is_empty());
    // No adapter abort/release has been dispatched. The ticket itself wakes
    // the original granting reactor and retains occupancy until it joins.
    closed(&registry, &selection).await;
    fixture.assert_sessions_closed();
    assert!(registry.is_idle(&ticket.run_id));
    assert!(grants
        .publish(&broker, &ticket, true, || panic!("late publish"))
        .is_err());
    broker.finish_stop_cleanup(&fence.cleanup.unwrap()).unwrap();
    assert_eq!(
        broker.stop_state(&ticket.run_id).unwrap(),
        StopState::Stopped
    );
    registry.forget(&selection).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn committed_preparation_keeps_grant_until_explicit_broker_stop() {
    let fixture = Fixture::new(Mode::Normal).await;
    let registry = Arc::new(PortalRegistry::new(Arc::new(FixturePolicy::default())));
    let broker = broker(&registry);
    let grants = SessionGrants::default();
    let ticket = grants.begin(&broker, "chat", None).unwrap();
    let selection = select(&registry, &fixture, &ticket);
    let target = ready(&registry, &selection).await;
    broker.authorize_target(&ticket.run_id, &target).unwrap();
    grants.publish(&broker, &ticket, true, || {}).unwrap();
    assert!(!ticket.is_preparation_cancelled());
    assert!(ticket.check_preparation().is_err());
    assert!(registry.target_alive(&target));
    assert!(registry
        .select_guarded(options(&ticket.run_id), 1, 1, Some(ticket.clone()), |_| {
            panic!("committed ticket cannot restart native preparation")
        })
        .is_err());
    let fence = grants.fence_revoke_checked(&broker, "chat", || {});
    assert!(!ticket.is_preparation_cancelled());
    assert!(
        registry.target_alive(&target),
        "completed preparation is disarmed"
    );
    broker.finish_stop_cleanup(&fence.cleanup.unwrap()).unwrap();
    closed(&registry, &selection).await;
    assert_eq!(
        broker.stop_state(&ticket.run_id).unwrap(),
        StopState::Stopped
    );
    fixture.assert_sessions_closed();
    fixture.shared.assert_peers_closed(2);
    registry.forget(&selection).unwrap();
}

#[test]
fn authorization_ticket_cannot_select_a_different_run() {
    let registry = Arc::new(PortalRegistry::new(Arc::new(FixturePolicy::default())));
    let broker = broker(&registry);
    let grants = SessionGrants::default();
    let ticket = grants.begin(&broker, "chat", None).unwrap();
    assert!(registry
        .select_guarded(options("foreign-run"), 1, 1, Some(ticket.clone()), |_| {
            panic!("foreign run reached native preparation")
        })
        .is_err());
    assert!(ticket.check_preparation().is_ok());
    assert!(registry.is_idle(&ticket.run_id));
    assert!(registry.is_idle("foreign-run"));
}
