use super::*;
use crate::tests::adapter::{idle, FixturePolicy};
use grok_computer_use_core::{
    adapter::ComputerUseAdapter,
    broker::{BrokerOptions, ComputerUseBroker, StopState},
    protocol::{ActionRequest, PROTOCOL_VERSION},
};
use std::sync::atomic::Ordering;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires owned native PipeWire and libeis fixtures"]
async fn session_authorization_cancel_joins_native_grant_before_broker_cleanup() {
    use crate::tests::registry::{closed, options, ready};
    use grok_computer_use_core::session_grants::SessionGrants;
    let native = Native::start().await;
    let eis = crate::tests::eis::NativeEis::start("owned-fixture-region").await;
    let fixture = Fixture::new(Mode::Normal).await;
    *fixture.shared.native.lock().unwrap() = Some(native.grant.clone());
    *fixture.shared.eis_socket.lock().unwrap() = Some(eis.socket.clone());
    let registry = Arc::new(crate::PortalRegistry::new(Arc::new(
        FixturePolicy::default(),
    )));
    let broker = ComputerUseBroker::new(
        Arc::new(grok_computer_use_core::owned::HostOwnedAdapter::new(
            registry.clone(),
        )),
        BrokerOptions {
            feature_enabled: true,
            lease_path: native.root.join("authorization-broker.lease"),
            ..BrokerOptions::default()
        },
    );
    let grants = SessionGrants::default();
    let ticket = grants
        .begin_attempt(
            &broker,
            "native-authorization-chat",
            None,
            "native-selection",
            1,
        )
        .unwrap();
    let address = fixture.bus.address.clone();
    let selection = registry
        .select_guarded(
            options(&ticket.run_id),
            1,
            1,
            Some(ticket.clone()),
            move |o| PortalSession::on_native_test_bus(o, address),
        )
        .unwrap();
    let target = ready(&registry, &selection).await;
    native.link(None).await;
    let generation = broker.authorize_target(&ticket.run_id, &target).unwrap();
    // Matches App activation before async session attachment/commit.
    grants.publish(&broker, &ticket, false, || {}).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    let observation = loop {
        match broker.observe(&ticket.run_id) {
            Ok(image) => break image,
            Err(error) => {
                assert!(Instant::now() < deadline, "authorization capture: {error}");
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }
    };
    assert_eq!(observation.target_generation, generation);
    let fence = grants
        .fence_cancel_attempt_checked(
            &broker,
            "native-authorization-chat",
            "native-selection",
            1,
            || {},
        )
        .unwrap();
    assert!(fence.cancelled && fence.revoke.error.is_none());
    assert!(ticket.is_preparation_cancelled());
    assert!(!registry.target_alive(&target));
    assert!(registry
        .claim_target_for_run(&ticket.run_id, &target)
        .is_err());
    assert!(broker.observe(&ticket.run_id).is_err());
    assert!(grants
        .publish(&broker, &ticket, true, || panic!("late App commit"))
        .is_err());
    // Deliberately retain (do not dispatch) Broker cleanup until the ticket
    // signal has joined both original native owners and the granting reactor.
    closed(&registry, &selection).await;
    fixture.assert_sessions_closed();
    eis.wait_event("disconnect", 1).await;
    native.no_capture_node();
    assert!(inputs(&eis).is_empty());
    broker
        .finish_stop_cleanup(&fence.revoke.cleanup.unwrap())
        .unwrap();
    assert_eq!(
        broker.stop_state(&ticket.run_id).unwrap(),
        StopState::Stopped
    );
    registry.forget(&selection).unwrap();
    assert!(registry
        .select_guarded(options(&ticket.run_id), 1, 1, Some(ticket.clone()), |_| {
            panic!("retired ticket restarted native consent")
        })
        .is_err());
    fs::write(native.root.join("authorization-broker.json"), serde_json::to_vec_pretty(&json!({
        "hostPickerRun":ticket.run_id, "pickerTarget":target, "observation":observation,
        "inputs":inputs(&eis), "nativePeerRoot":eis.socket.parent().unwrap(),
        "cancelledBeforeCommit":true, "joinedBeforeBrokerCleanup":true, "lateCommitRejected":true,
        "nativeJoined":true, "brokerStopped":true, "appEnabled":false,
        "actualNativeGnome":false, "applicationEffectVerified":false
    })).unwrap()).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires owned native PipeWire and libeis fixtures"]
async fn broker_registry_preserves_two_native_owners_across_run_retirement() {
    use crate::tests::registry::{closed, options, ready};
    let registry = Arc::new(crate::PortalRegistry::new(Arc::new(
        FixturePolicy::default(),
    )));
    let mut retained = Vec::new();
    for run in ["registry-left", "registry-right"] {
        let native = Native::start().await;
        let eis = crate::tests::eis::NativeEis::start("owned-fixture-region").await;
        let fixture = Fixture::new(Mode::Normal).await;
        *fixture.shared.native.lock().unwrap() = Some(native.grant.clone());
        *fixture.shared.eis_socket.lock().unwrap() = Some(eis.socket.clone());
        let address = fixture.bus.address.clone();
        let ticket = registry
            .select_with(options(run), 1, 1, move |options| {
                PortalSession::on_native_test_bus(options, address)
            })
            .unwrap();
        let target = ready(&registry, &ticket).await;
        native.link(None).await;
        retained.push((run, native, eis, fixture, ticket, target));
    }
    let broker = ComputerUseBroker::new(
        Arc::new(grok_computer_use_core::owned::HostOwnedAdapter::new(
            registry.clone(),
        )),
        BrokerOptions {
            feature_enabled: true,
            lease_path: retained[0].1.root.join("registry-broker.lease"),
            ..BrokerOptions::default()
        },
    );
    for (run, _, _, _, _, _) in &retained {
        broker.open_run("app-registry-fixture", run).unwrap();
    }
    assert!(broker
        .list_targets_for_surface(grok_computer_use_core::adapter::SurfaceKind::Desktop)
        .is_err());
    for (index, (run, native, eis, fixture, ticket, target)) in retained.iter().enumerate() {
        let picker = broker
            .list_targets_for_surface_in_run(
                grok_computer_use_core::adapter::SurfaceKind::Desktop,
                run,
            )
            .unwrap();
        assert_eq!(picker.len(), 1);
        assert_eq!(&picker[0].target_id, target);
        let other = retained[1 - index].0;
        assert!(registry.claim_target_for_run(other, target).is_err());
        assert!(broker.authorized_target(run).is_err());
        let generation = broker.authorize_target(run, target).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        let observation = loop {
            match broker.observe(run) {
                Ok(image) => break image,
                Err(error) => {
                    assert!(
                        Instant::now() < deadline,
                        "registry capture did not become ready: {error}"
                    );
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            }
        };
        let png = base64::engine::general_purpose::STANDARD
            .decode(observation.image.png_base64.as_ref().unwrap())
            .unwrap();
        registry.abort(run, generation).unwrap();
        assert!(
            registry.target_alive(target),
            "equal live generation must not revoke an advanced grant"
        );
        let pixels = image::load_from_memory_with_format(&png, image::ImageFormat::Png)
            .unwrap()
            .to_rgba8();
        assert!(pixels.width() > 0 && pixels.height() > 0);
        let request = ActionRequest {
            version: PROTOCOL_VERSION,
            action_id: uuid::Uuid::new_v4().to_string(),
            run_id: (*run).into(),
            target_id: target.clone(),
            target_generation: generation,
            snapshot_id: observation.snapshot_id.clone(),
            geometry_revision: observation.geometry_revision,
            action: ActionKind::Click,
            target: ActionTarget::Coord { x: 0.75, y: 0.25 },
            parameters: json!({"button":"right"}),
        };
        let outcome = broker.act(request.clone());
        assert_eq!(outcome.kind, OutcomeKind::Applied, "{:?}", outcome.reason);
        eis.wait_event("button", 2).await;
        let first = inputs(eis);
        assert_eq!(first.len(), 3);
        let duplicate = broker.act(request);
        assert_eq!(duplicate.kind, OutcomeKind::Applied);
        assert_eq!(inputs(eis), first);
        if index == 0 {
            assert!(
                inputs(&retained[1].2).is_empty(),
                "left run reached right peer"
            );
        }
        broker.request_stop(run).unwrap();
        closed(&registry, ticket).await;
        assert!(registry.is_idle(run));
        assert_eq!(broker.stop_state(run).unwrap(), StopState::Stopped);
        assert!(broker.observe(run).is_err());
        fixture.assert_sessions_closed();
        eis.wait_event("disconnect", 1).await;
        native.no_capture_node();
        if index == 0 {
            assert!(registry.target_alive(&retained[1].5));
            let snapshot = query(&retained[1].1.root, &retained[1].1.remote);
            assert!(snapshot
                .as_array()
                .unwrap()
                .iter()
                .any(|v| property(v, "node.name")
                    .and_then(Json::as_str)
                    .is_some_and(|name| name.starts_with("grok-cu-capture-"))));
            assert!(!retained[1]
                .2
                .events()
                .iter()
                .any(|e| e["event"] == "disconnect"));
        }
        fs::write(
            native.root.join("registry-broker.json"),
            serde_json::to_vec_pretty(&json!({
                "hostPickerRun":run, "pickerTarget":target, "observation":observation,
                "outcome":outcome,"duplicate":duplicate,"inputs":inputs(eis),
                "nativePeerRoot":eis.socket.parent().unwrap(), "nativeJoined":true,
                "brokerStopped":true,"appEnabled":false,"actualNativeGnome":false,
                "applicationEffectVerified":false
            }))
            .unwrap(),
        )
        .unwrap();
        registry.forget(ticket).unwrap();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires owned native PipeWire and libeis fixtures"]
async fn adapter_failed_model_attempt_retires_old_snapshot_before_admission() {
    failed_model_attempt(false).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires owned native PipeWire and libeis fixtures"]
async fn product_wrapper_failed_model_attempt_retires_native_snapshot() {
    failed_model_attempt(true).await;
}

async fn failed_model_attempt(wrapped: bool) {
    let run = "adapter-failed-capture";
    let (_native, eis, _fixture, session) = setup(run).await;
    let host = crate::PortalHostSession::new(session, 1, 1).unwrap();
    let policy = Arc::new(FixturePolicy::default());
    let native = Arc::new(crate::PortalAdapter::from_granted(host, policy.clone()).unwrap());
    let target = native.target_id().to_owned();
    let adapter: Arc<dyn ComputerUseAdapter> = if wrapped {
        Arc::new(grok_computer_use_core::owned::HostOwnedAdapter::new(
            native.clone(),
        ))
    } else {
        native.clone()
    };
    adapter.claim_target_for_run(run, &target).unwrap();
    // Cancellation/no-image retire a snapshot; permission loss now retires the
    // whole grant. Exercise all three, with final revocation last rather than
    // implicitly resurrecting that grant for the next case.
    for reason in ["cancelled", "no-image", "policy"] {
        let image = adapter
            .capture_for_run_at_generation(run, &target, 3, CaptureOptions::model(true))
            .unwrap();
        let options = CaptureOptions::model(reason != "no-image");
        if reason == "cancelled" {
            options.cancellation.cancel();
        }
        if reason == "policy" {
            policy.deny.store(true, Ordering::SeqCst);
        }
        assert!(adapter
            .capture_for_run_at_generation(run, &target, 3, options)
            .is_err());
        policy.deny.store(false, Ordering::SeqCst);
        let mut req = request(&image, ActionKind::Click, json!({}));
        req.generation = 3;
        let result = adapter.act(&req);
        assert!(
            result.is_err() || result.unwrap().outcome == Some(OutcomeKind::Rejected),
            "old snapshot survived {reason}"
        );
        assert!(inputs(&eis).is_empty());
    }
    assert!(
        !native.target_alive(&target),
        "permission reset revived old grant"
    );
    adapter.abort(run, 4).unwrap();
    idle(&native, run).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires owned native PipeWire and libeis fixtures"]
async fn broker_owned_adapter_preserves_png_generation_preview_dedupe_and_stop() {
    broker_adapter_round_trip(false).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires owned native PipeWire and libeis fixtures"]
async fn broker_product_wrapper_preserves_native_run_generation_and_retirement() {
    broker_adapter_round_trip(true).await;
}

async fn broker_adapter_round_trip(wrapped: bool) {
    let run = "broker-native-owner";
    let (native, eis, fixture, session) = setup(run).await;
    let host = crate::PortalHostSession::new(session, 1, 1).unwrap();
    let adapter = Arc::new(
        crate::PortalAdapter::from_granted(host, Arc::new(FixturePolicy::default())).unwrap(),
    );
    let target = adapter.target_id().to_owned();
    let executor: Arc<dyn ComputerUseAdapter> = if wrapped {
        Arc::new(grok_computer_use_core::owned::HostOwnedAdapter::new(
            adapter.clone(),
        ))
    } else {
        adapter.clone()
    };
    let broker = ComputerUseBroker::new(
        executor,
        BrokerOptions {
            feature_enabled: true,
            lease_path: native.root.join("owned-broker.lease"),
            ..BrokerOptions::default()
        },
    );
    broker.open_run("app-fixture", run).unwrap();
    broker.open_run("other-app", "foreign").unwrap();
    // Exercise the Host picker API actually called by the App, rather than
    // relying only on the model-facing list_targets route. A granted portal
    // must never disclose its retained target to an unscoped/foreign picker.
    let desktop = grok_computer_use_core::adapter::SurfaceKind::Desktop;
    assert!(broker
        .list_targets_for_surface(desktop)
        .unwrap_err()
        .to_string()
        .contains("owning run"));
    assert!(broker
        .list_targets_for_surface_in_run(desktop, "foreign")
        .unwrap()
        .is_empty());
    let picker = broker
        .list_targets_for_surface_in_run(desktop, run)
        .unwrap();
    assert_eq!(picker.len(), 1);
    assert_eq!(picker[0].target_id, target);
    assert!(broker.authorized_target(run).is_err());
    assert!(broker.list_targets("foreign").unwrap().is_empty());
    assert_eq!(broker.list_targets(run).unwrap()[0].target_id, target);
    assert!(broker.authorize_target("foreign", &target).is_err());
    let generation = broker.authorize_target(run, &target).unwrap();
    assert!(generation > 1);
    let observation = broker.observe(run).unwrap();
    assert_eq!(observation.target_generation, generation);
    assert_eq!(
        observation.geometry_revision,
        adapter.current_geometry_revision_for(&target)
    );
    let preview = broker.observe_preview(run).unwrap();
    assert_ne!(observation.snapshot_id, preview.snapshot_id);
    let request = ActionRequest {
        version: PROTOCOL_VERSION,
        action_id: uuid::Uuid::new_v4().to_string(),
        run_id: run.into(),
        target_id: target.clone(),
        target_generation: generation,
        snapshot_id: observation.snapshot_id.clone(),
        geometry_revision: observation.geometry_revision,
        action: ActionKind::Click,
        target: ActionTarget::Coord { x: 0.75, y: 0.25 },
        parameters: json!({"button":"right"}),
    };
    let outcome = broker.act(request.clone());
    assert_eq!(outcome.kind, OutcomeKind::Applied, "{:?}", outcome.reason);
    assert!(outcome.executed);
    eis.wait_event("button", 2).await;
    let first_events = inputs(&eis);
    assert_eq!(first_events.len(), 3);
    let duplicate = broker.act(request.clone());
    assert_eq!(duplicate.kind, OutcomeKind::Applied);
    assert_eq!(
        inputs(&eis),
        first_events,
        "Broker must not replay an action ID"
    );
    let next_generation = broker.authorize_target(run, &target).unwrap();
    assert!(next_generation > generation);
    let next = broker.observe(run).unwrap();
    assert_eq!(next.target_generation, next_generation);
    let mut stale = request.clone();
    stale.action_id = uuid::Uuid::new_v4().to_string();
    assert_eq!(broker.act(stale).kind, OutcomeKind::Rejected);
    let mut key = request;
    key.action_id = uuid::Uuid::new_v4().to_string();
    key.target_generation = next_generation;
    key.snapshot_id = next.snapshot_id.clone();
    key.geometry_revision = next.geometry_revision;
    key.action = ActionKind::Key;
    key.parameters = json!({"key":"enter"});
    assert_eq!(broker.act(key).kind, OutcomeKind::Applied);
    eis.wait_event("key", 2).await;
    assert_eq!(inputs(&eis).len(), 5);
    broker.request_stop(run).unwrap();
    idle(&adapter, run).await;
    assert_eq!(broker.stop_state(run).unwrap(), StopState::Stopped);
    assert!(broker.observe(run).is_err());
    fixture.assert_sessions_closed();
    eis.wait_event("disconnect", 1).await;
    fs::write(
        native.root.join(if wrapped {
            "wrapped-broker.json"
        } else {
            "adapter-broker.json"
        }),
        serde_json::to_vec_pretty(&json!({
            "picker":picker.iter().map(|t|json!({"targetId":t.target_id,"kind":t.kind})).collect::<Vec<_>>(), "hostPickerRun":run,
            "observation":observation, "preview":preview, "next":next,
            "outcome":outcome, "duplicate":duplicate, "inputs":inputs(&eis),
            "nativePeerRoot":eis.socket.parent().expect("owned EI socket directory"),
            "nativeJoined":true, "brokerStopped":true, "appEnabled":false,
            "actualNativeGnome":false, "applicationEffectVerified":false
        }))
        .unwrap(),
    )
    .unwrap();
    eprintln!("PASS actual Broker -> {}PortalAdapter -> PNG/EI; run/generation/preview/dedupe/stop, not App E4",
        if wrapped { "HostOwnedAdapter -> " } else { "" });
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires owned native PipeWire and libeis fixtures"]
async fn adapter_generation_cancellation_and_native_source_loss_fail_closed() {
    let run = "adapter-admission";
    let (mut native, eis, fixture, session) = setup(run).await;
    let policy = Arc::new(FixturePolicy::default());
    let host = crate::PortalHostSession::new(session, 1, 1).unwrap();
    let adapter = crate::PortalAdapter::from_granted(host, policy.clone()).unwrap();
    let target = adapter.target_id().to_owned();
    assert!(adapter
        .capture_for_run_at_generation(run, &target, 2, CaptureOptions::model(true))
        .is_err());
    adapter.claim_target_for_run(run, &target).unwrap();
    // Policy loss now irrevocably retires a grant. Native denial/takeover
    // coverage lives in policy_loss_joins_native_owners_without_model_input;
    // this independent grant covers generation, cancellation and source loss.
    let image = adapter
        .capture_for_run_at_generation(run, &target, 9, CaptureOptions::model(true))
        .unwrap();
    assert_eq!(image.target_generation, 9);
    assert!(adapter
        .capture_for_run_at_generation(run, &target, 8, CaptureOptions::model(true))
        .is_err());
    let mut req = request(&image, ActionKind::Click, json!({}));
    req.generation = 10;
    assert!(adapter.act(&req).is_err());
    req.generation = 9;
    req.cancellation.cancel();
    assert!(adapter.act(&req).is_err());
    assert!(inputs(&eis).is_empty());
    adapter.abort(run, 9).unwrap();
    assert!(
        adapter.target_alive(&target),
        "late abort must not cancel current generation"
    );
    native.source.0.kill().unwrap();
    native.source.0.wait().unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while adapter.target_alive(&target) {
        assert!(Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    idle(&adapter, run).await;
    req.cancellation = ActionCancellation::default();
    assert!(adapter.act(&req).is_err());
    assert!(inputs(&eis).is_empty());
    fixture.assert_sessions_closed();
    eis.wait_event("disconnect", 1).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires owned native PipeWire and libeis fixtures"]
async fn policy_loss_joins_native_owners_without_model_input() {
    for takeover in [false, true] {
        let run = if takeover {
            "native-policy-takeover"
        } else {
            "native-policy-denial"
        };
        let (native, eis, fixture, session) = setup(run).await;
        let policy = Arc::new(FixturePolicy::default());
        let host = crate::PortalHostSession::new(session, 1, 1).unwrap();
        let adapter = crate::PortalAdapter::from_granted(host, policy.clone()).unwrap();
        let target = adapter.target_id().to_owned();
        adapter.claim_target_for_run(run, &target).unwrap();
        let before = adapter
            .capture_for_run_at_generation(run, &target, 1, CaptureOptions::model(true))
            .unwrap();
        if takeover {
            policy.takeover.store(true, Ordering::SeqCst);
        } else {
            policy.deny.store(true, Ordering::SeqCst);
        }
        // Observe the independent C peer, not an adapter query that could
        // itself cause cleanup. No capture/act/Stop is sent after policy loss.
        eis.wait_event("disconnect", 1).await;
        idle(&adapter, run).await;
        assert!(!adapter.target_alive(&target));
        fixture.assert_sessions_closed();
        native.no_capture_node();
        policy.deny.store(false, Ordering::SeqCst);
        policy.takeover.store(false, Ordering::SeqCst);
        assert!(adapter.claim_target_for_run(run, &target).is_err());
        assert!(adapter
            .capture_for_run_at_generation(run, &target, 2, CaptureOptions::model(true))
            .is_err());
        let mut old_action = request(&before, ActionKind::Click, json!({}));
        old_action.generation = 1;
        assert!(adapter.act(&old_action).is_err());
        assert!(inputs(&eis).is_empty());
        fs::write(
            native.root.join("policy-revocation.json"),
            serde_json::to_vec_pretty(&json!({
                "reason": if takeover { "user-takeover" } else { "permission-denied" },
                "observation": before, "targetId": target,
                "nativePeerRoot": eis.socket.parent().unwrap(),
                "inputEvents": inputs(&eis), "nativeJoined": true, "resurrectionRejected": true,
                "externalStopRequired": false, "actualNativeGnome": false,
                "osPolicySourceVerified": false, "applicationEffectVerified": false
            }))
            .unwrap(),
        )
        .unwrap();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires owned native PipeWire and libeis fixtures"]
async fn gnome_shield_signal_retires_native_capture_and_input_without_model_call() {
    use crate::tests::logind::gnome_session::NativePolicySource;
    let (source, watch) = NativePolicySource::start().await;
    let policy = watch.input_policy();
    let (_stop, stopped) = tokio::sync::oneshot::channel();
    let owner = tokio::spawn(watch.run_until(stopped));
    tokio::time::timeout(Duration::from_secs(2), async {
        while !policy.input_available() {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .unwrap();
    let run = "native-gnome-shield";
    let (native, eis, fixture, session) = setup(run).await;
    let host = crate::PortalHostSession::new(session, 1, 1).unwrap();
    let adapter = crate::PortalAdapter::from_granted(host, policy.clone()).unwrap();
    let target = adapter.target_id().to_owned();
    adapter.claim_target_for_run(run, &target).unwrap();
    let before = adapter
        .capture_for_run_at_generation(run, &target, 1, CaptureOptions::model(true))
        .unwrap();
    source.set_active(true).await;
    // First witness is the independently running C EIS server. No product
    // query, act, Stop, capture, or explicit registry cancellation drives this.
    eis.wait_event("disconnect", 1).await;
    let result = tokio::time::timeout(Duration::from_secs(2), owner)
        .await
        .unwrap()
        .unwrap();
    assert!(result.unwrap_err().contains("screen shield activated"));
    idle(&adapter, run).await;
    fixture.assert_sessions_closed();
    native.no_capture_node();
    source.set_active(false).await;
    assert!(!policy.input_available());
    assert!(!adapter.target_alive(&target));
    assert!(adapter.claim_target_for_run(run, &target).is_err());
    assert!(adapter
        .capture_for_run_at_generation(run, &target, 2, CaptureOptions::model(true))
        .is_err());
    let mut old = request(&before, ActionKind::Click, json!({}));
    old.generation = 1;
    assert!(adapter.act(&old).is_err());
    assert!(inputs(&eis).is_empty());
    fs::write(native.root.join("gnome-shield-revocation.json"), serde_json::to_vec_pretty(&json!({
        "reason":"gnome-screen-shield", "observation":before, "targetId":target,
        "nativePeerRoot":eis.socket.parent().unwrap(), "inputEvents":inputs(&eis),
        "nativeJoined":true, "resurrectionRejected":true, "externalStopRequired":false,
        "actualNativeGnome":false, "osPolicySourceVerified":false, "applicationEffectVerified":false,
        "privateSystemAndSessionBus":true, "sourceMonitorReturned":true
    })).unwrap()).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires owned native PipeWire and libeis fixtures"]
async fn gnome_physical_generation_retires_native_capture_and_input_without_model_call() {
    use crate::tests::logind::gnome_session::native::Source;
    let (source, watch) = Source::start().await;
    let policy = watch.input_policy();
    let (_stop, stopped) = tokio::sync::oneshot::channel();
    let owner = tokio::spawn(watch.run_until(stopped));
    tokio::time::timeout(Duration::from_secs(2), async {
        while !policy.input_available() {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .unwrap();
    let run = "native-gnome-physical";
    let (native, eis, fixture, session) = setup(run).await;
    let host = crate::PortalHostSession::new(session, 1, 1).unwrap();
    let adapter = crate::PortalAdapter::from_granted(host, policy.clone()).unwrap();
    let target = adapter.target_id().to_owned();
    adapter.claim_target_for_run(run, &target).unwrap();
    let before = adapter
        .capture_for_run_at_generation(run, &target, 1, CaptureOptions::model(true))
        .unwrap();
    source.takeover().await;
    // First witness is the independently running C EIS server. No product
    // query, act, Stop, capture, or explicit registry cancellation drives this.
    eis.wait_event("disconnect", 1).await;
    let result = tokio::time::timeout(Duration::from_secs(2), owner)
        .await
        .unwrap()
        .unwrap();
    assert!(result.unwrap_err().contains("new consent required"));
    idle(&adapter, run).await;
    fixture.assert_sessions_closed();
    native.no_capture_node();
    assert!(policy.user_input_active());
    assert!(!policy.input_available());
    assert!(!adapter.target_alive(&target));
    assert!(adapter.claim_target_for_run(run, &target).is_err());
    assert!(adapter
        .capture_for_run_at_generation(run, &target, 2, CaptureOptions::model(true))
        .is_err());
    let mut old = request(&before, ActionKind::Click, json!({}));
    old.generation = 1;
    assert!(adapter.act(&old).is_err());
    assert!(inputs(&eis).is_empty());
    fs::write(native.root.join("gnome-physical-revocation.json"), serde_json::to_vec_pretty(&json!({
        "reason":"gnome-helper-generation", "observation":before, "targetId":target,
        "nativePeerRoot":eis.socket.parent().unwrap(), "inputEvents":inputs(&eis),
        "nativeJoined":true, "resurrectionRejected":true, "externalStopRequired":false,
        "actualNativeGnome":false, "osPolicySourceVerified":false, "applicationEffectVerified":false,
        "privateSystemAndSessionBus":true, "sourceMonitorReturned":true
    })).unwrap()).unwrap();
}
