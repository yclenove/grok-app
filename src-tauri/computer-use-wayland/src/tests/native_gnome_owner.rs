//! Native C/PipeWire peers on private buses, not an installed GNOME acceptance.
use super::*;
use crate::registry::SelectionOwner;
use crate::tests::{
    adapter::FixturePolicy,
    logind::gnome_session::native::Source,
    registry::{closed, options, ready},
};
use grok_computer_use_core::adapter::ComputerUseAdapter;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires owned native PipeWire and libeis fixtures"]
async fn shell_and_shield_name_aba_retire_native_capture_and_input_without_model_call() {
    for shield in [false, true] {
        let source = Arc::new(Source::consent_source().await);
        let native = Native::start().await;
        let eis = crate::tests::eis::NativeEis::start("owned-fixture-region").await;
        let fixture = Fixture::new(Mode::Normal).await;
        *fixture.shared.native.lock().unwrap() = Some(native.grant.clone());
        *fixture.shared.eis_socket.lock().unwrap() = Some(eis.socket.clone());
        let registry = crate::PortalRegistry::new(Arc::new(FixturePolicy::default()));
        let address = fixture.bus.address.clone();
        let queued = source.clone();
        let run = "native-gnome-owner";
        let selection = registry
            .select_owned(
                options(run),
                SelectionOwner {
                    generation: 1,
                    target_generation: 1,
                    preparation: None,
                    parent: None,
                    policy: Some(Box::new(move |_host| {
                        Box::pin(async move { queued.consent_watch().await })
                    })),
                },
                move |o| PortalSession::on_native_test_bus(o, address),
            )
            .unwrap();
        let target = ready(&registry, &selection).await;
        native.link(None).await;
        registry.claim_target_for_run(run, &target).unwrap();
        let before = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Ok(image) = registry.capture_for_run_at_generation(
                    run,
                    &target,
                    1,
                    CaptureOptions::model(true),
                ) {
                    break image;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        source.cycle_shell_name(shield).await;
        // Read the independent native C peer first. No next model call or
        // registry cancellation is allowed to trigger this disconnection.
        eis.wait_event("disconnect", 1).await;
        closed(&registry, &selection).await;
        fixture.assert_sessions_closed();
        native.no_capture_node();
        assert!(!registry.target_alive(&target));
        assert!(registry.claim_target_for_run(run, &target).is_err());
        assert!(registry
            .capture_for_run_at_generation(run, &target, 2, CaptureOptions::model(true))
            .is_err());
        let mut old = request(&before, ActionKind::Click, json!({}));
        old.generation = 1;
        assert!(registry.act(&old).is_err());
        assert!(inputs(&eis).is_empty());
        registry.forget(&selection).unwrap();
        fs::write(native.root.join("gnome-owner-revocation.json"), serde_json::to_vec_pretty(&json!({
            "reason": if shield {"shield-name-release-reacquire"} else {"shell-name-release-reacquire"},
            "observation": before, "targetId": target, "nativePeerRoot": eis.socket.parent().unwrap(),
            "inputEvents": inputs(&eis), "nativeJoined": true, "registryJoined": true,
            "resurrectionRejected": true, "externalStopRequired": false,
            "actualNativeGnome": false, "osPolicySourceVerified": false, "applicationEffectVerified": false
        })).unwrap()).unwrap();
    }
}
