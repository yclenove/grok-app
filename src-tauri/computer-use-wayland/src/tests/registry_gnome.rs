//! Real private portal + policy transports; no installed GNOME acceptance.
#[path = "registry_gnome_owner.rs"]
mod owner_lifetime;
use super::adapter::FixturePolicy;
use super::logind::gnome_session::native::Source;
use super::registry::{closed, options, ready};
use super::*;
use crate::registry::SelectionOwner;
use grok_computer_use_core::adapter::ComputerUseAdapter;

async fn entered(fixture: &Fixture) {
    tokio::time::timeout(Duration::from_secs(2), async {
        while !fixture.shared.called("Start") {
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    })
    .await
    .unwrap();
}
fn select(
    registry: &PortalRegistry,
    fixture: &Fixture,
    source: Arc<Source>,
    run: &str,
) -> PortalSelection {
    let address = fixture.bus.address.clone();
    registry
        .select_owned(
            options(run),
            SelectionOwner {
                generation: 1,
                target_generation: 1,
                preparation: None,
                parent: None,
                policy: Some(Box::new(move |_base| {
                    Box::pin(async move { source.consent_watch().await })
                })),
            },
            move |o| PortalSession::on_test_bus(o, address),
        )
        .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn picker_activity_is_pending_only_and_first_active_event_closes_registry() {
    let source = Arc::new(Source::consent_source().await);
    let fixture = Fixture::new(Mode::Normal).await;
    let (release, gate) = tokio::sync::oneshot::channel();
    *fixture.shared.start_release.lock().unwrap() = Some(gate);
    let registry = PortalRegistry::new(Arc::new(FixturePolicy::default()));
    let selection = select(&registry, &fixture, source.clone(), "staged-picker");
    entered(&fixture).await;
    for _ in 0..3 {
        source.takeover().await;
    }
    tokio::time::sleep(Duration::from_millis(240)).await;
    assert_eq!(
        registry.state(&selection).unwrap(),
        PortalSelectionState::Pending
    );
    assert!(registry
        .list_targets_for_run("staged-picker")
        .unwrap()
        .is_empty());
    assert!(!registry.is_idle("staged-picker"));
    release.send(()).unwrap();
    let target = ready(&registry, &selection).await;
    assert!(registry.target_alive(&target));
    source.takeover().await;
    closed(&registry, &selection).await;
    fixture.assert_sessions_closed();
    fixture.shared.assert_peers_closed(2);
    assert!(!registry.target_alive(&target));
    registry.forget(&selection).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn host_permission_loss_during_picker_is_not_absorbed_with_physical_clicks() {
    let source = Arc::new(Source::consent_source().await);
    let fixture = Fixture::new(Mode::SilentStart).await;
    let host = Arc::new(FixturePolicy::default());
    let registry = PortalRegistry::new(host.clone());
    let selection = select(&registry, &fixture, source.clone(), "staged-host-loss");
    entered(&fixture).await;
    source.takeover().await;
    host.deny.store(true, std::sync::atomic::Ordering::SeqCst);
    closed(&registry, &selection).await;
    host.deny.store(false, std::sync::atomic::Ordering::SeqCst);
    assert!(registry
        .list_targets_for_run("staged-host-loss")
        .unwrap()
        .is_empty());
    fixture.assert_sessions_closed();
    registry.forget(&selection).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancel_during_original_policy_initialization_retains_occupancy_until_join() {
    let source = Arc::new(Source::consent_source().await);
    let fixture = Fixture::new(Mode::Normal).await;
    let registry = PortalRegistry::new(Arc::new(FixturePolicy::default()));
    let (notify, notified) = tokio::sync::oneshot::channel();
    let (release, gate) = tokio::sync::oneshot::channel();
    let (guard_tx, guard_rx) = tokio::sync::oneshot::channel();
    let address = fixture.bus.address.clone();
    let selection = registry
        .select_owned(
            options("staged-init-cancel"),
            SelectionOwner {
                generation: 1,
                target_generation: 1,
                preparation: None,
                parent: None,
                policy: Some(Box::new(move |_base| {
                    Box::pin(async move {
                        let prepared = source.consent_watch().await?;
                        let _ = guard_tx.send(prepared.0.input_policy());
                        let _ = notify.send(());
                        gate.await.map_err(|_| "fixture gate dropped")?;
                        Ok(prepared)
                    })
                })),
            },
            move |o| PortalSession::on_test_bus(o, address),
        )
        .unwrap();
    notified.await.unwrap();
    let guard = guard_rx.await.unwrap();
    registry.cancel(&selection).unwrap();
    assert!(!registry.is_idle("staged-init-cancel"));
    assert!(registry.forget(&selection).is_err());
    assert!(!fixture.shared.called("Start"));
    release.send(()).unwrap();
    closed(&registry, &selection).await;
    assert!(!guard.input_available());
    assert!(!fixture.shared.called("CreateSession"));
    registry.forget(&selection).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn denied_portal_joins_policy_monitor_without_publishing_a_target() {
    let source = Arc::new(Source::consent_source().await);
    let fixture = Fixture::new(Mode::Denied).await;
    let registry = PortalRegistry::new(Arc::new(FixturePolicy::default()));
    let selection = select(&registry, &fixture, source, "staged-denial");
    closed(&registry, &selection).await;
    assert!(registry
        .list_targets_for_run("staged-denial")
        .unwrap()
        .is_empty());
    fixture.assert_sessions_closed();
    registry.forget(&selection).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancel_during_activation_snapshot_joins_original_monitor_and_native_session() {
    let source = Arc::new(Source::consent_source().await);
    let fixture = Fixture::new(Mode::Normal).await;
    let (release, gate) = tokio::sync::oneshot::channel();
    *fixture.shared.start_release.lock().unwrap() = Some(gate);
    let registry = PortalRegistry::new(Arc::new(FixturePolicy::default()));
    let selection = select(&registry, &fixture, source.clone(), "staged-arm-cancel");
    entered(&fixture).await;
    let before = source.hold_replies();
    release.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        while !fixture.shared.called("ConnectToEIS") || source.calls() <= before {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        registry.state(&selection).unwrap(),
        PortalSelectionState::Pending
    );
    assert!(registry
        .list_targets_for_run("staged-arm-cancel")
        .unwrap()
        .is_empty());
    registry.cancel(&selection).unwrap();
    source.release_replies();
    closed(&registry, &selection).await;
    fixture.assert_sessions_closed();
    fixture.shared.assert_peers_closed(2);
    assert!(registry
        .list_targets_for_run("staged-arm-cancel")
        .unwrap()
        .is_empty());
    registry.forget(&selection).unwrap();
}
