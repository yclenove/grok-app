//! Real private D-Bus/SCM_RIGHTS lifecycle, not installed Shell or App consent.
use super::*;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn original_shell_name_aba_closes_pending_picker_without_another_model_call() {
    for shield in [false, true] {
        let source = Arc::new(Source::consent_source().await);
        let fixture = Fixture::new(Mode::SilentStart).await;
        let registry = PortalRegistry::new(Arc::new(FixturePolicy::default()));
        let selection = select(&registry, &fixture, source.clone(), "owner-picker");
        entered(&fixture).await;
        source.cycle_shell_name(shield).await;
        closed(&registry, &selection).await;
        fixture.assert_sessions_closed();
        assert!(registry.is_idle("owner-picker"));
        assert!(registry
            .list_targets_for_run("owner-picker")
            .unwrap()
            .is_empty());
        registry.forget(&selection).unwrap();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn original_shell_name_aba_during_activation_cannot_accept_late_helper_reply() {
    for shield in [false, true] {
        let source = Arc::new(Source::consent_source().await);
        let fixture = Fixture::new(Mode::Normal).await;
        let (release, gate) = tokio::sync::oneshot::channel();
        *fixture.shared.start_release.lock().unwrap() = Some(gate);
        let registry = PortalRegistry::new(Arc::new(FixturePolicy::default()));
        let selection = select(&registry, &fixture, source.clone(), "owner-activation");
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
        source.cycle_shell_name(shield).await;
        // Keep the original reply pending until the monitor/registry has joined;
        // releasing a reply cannot be a precondition for cancelling its owner.
        closed(&registry, &selection).await;
        source.release_replies();
        fixture.assert_sessions_closed();
        fixture.shared.assert_peers_closed(2);
        assert!(registry.is_idle("owner-activation"));
        assert!(registry
            .list_targets_for_run("owner-activation")
            .unwrap()
            .is_empty());
        registry.forget(&selection).unwrap();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn original_shell_name_aba_retires_ready_target_and_fresh_selection_does_not_revive_it() {
    for shield in [false, true] {
        let source = Arc::new(Source::consent_source().await);
        let fixture = Fixture::new(Mode::Normal).await;
        let registry = PortalRegistry::new(Arc::new(FixturePolicy::default()));
        let old = select(&registry, &fixture, source.clone(), "owner-ready");
        let old_target = ready(&registry, &old).await;
        assert!(registry.target_alive(&old_target));
        source.cycle_shell_name(shield).await;
        closed(&registry, &old).await;
        fixture.assert_sessions_closed();
        fixture.shared.assert_peers_closed(2);
        assert!(!registry.target_alive(&old_target));
        assert!(registry
            .list_targets_for_run("owner-ready")
            .unwrap()
            .is_empty());
        registry.forget(&old).unwrap();
        // A deliberate new selection owns new subscriptions and FDs. Merely
        // reacquiring the same name above never restarted the old selection.
        // Retain the original fake Shell/bus until both monitors have joined;
        // moving the last Arc into preparation would destroy the transport as
        // soon as preparation returns and test a different failure instead.
        let fresh = select(&registry, &fixture, source.clone(), "owner-ready");
        let fresh_target = ready(&registry, &fresh).await;
        assert!(registry.target_alive(&fresh_target));
        assert!(!registry.target_alive(&old_target));
        registry.cancel(&fresh).unwrap();
        closed(&registry, &fresh).await;
        fixture.assert_sessions_closed();
        fixture.shared.assert_peers_closed(4);
        assert!(!registry.target_alive(&fresh_target));
        assert!(!registry.target_alive(&old_target));
        assert!(registry.is_idle("owner-ready"));
        registry.forget(&fresh).unwrap();
    }
}
