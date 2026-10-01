//! Existing Host wire protocol -> real PNG -> actual private libeis receipts.
//! No claim that these owned daemons constitute an App or GNOME acceptance run.
use super::*;
#[path = "native_adapter.rs"]
mod adapter;
#[path = "native_gnome_consent.rs"]
mod gnome_consent;
#[path = "native_gnome_owner.rs"]
mod gnome_owner;
#[path = "native_parent.rs"]
mod parent;
use base64::Engine;
use grok_computer_use_core::{
    adapter::{ActionScope, CaptureOptions, DispatchRequest},
    execution::ActionCancellation,
    protocol::{ActionKind, ActionTarget, Observation, OutcomeKind},
};
use serde_json::json;

fn request(observation: &Observation, action: ActionKind, parameters: Json) -> DispatchRequest {
    DispatchRequest {
        managed_request: None,
        cancellation: ActionCancellation::default(),
        run_id: observation.run_id.clone(),
        action_id: uuid::Uuid::new_v4().to_string(),
        generation: 11,
        target_id: observation.target_id.clone(),
        target_generation: observation.target_generation,
        snapshot_id: observation.snapshot_id.clone(),
        geometry_revision: observation.geometry_revision,
        action,
        target: ActionTarget::Coord { x: 0.75, y: 0.25 },
        parameters,
        scope: ActionScope::Directed,
    }
}
fn capture(host: &mut crate::PortalHostSession) -> Observation {
    let run = host.run_id().to_owned();
    let target = host.target_id().to_owned();
    host.capture(&run, &target, &CaptureOptions::model(true), 28, 20)
        .unwrap()
}
async fn setup(run: &str) -> (Native, super::super::eis::NativeEis, Fixture, PortalSession) {
    setup_mapping(run, Mode::Normal, "owned-fixture-region").await
}
async fn setup_mapping(
    run: &str,
    mode: Mode,
    mapping: &str,
) -> (Native, super::super::eis::NativeEis, Fixture, PortalSession) {
    let native = Native::start().await;
    let eis = super::super::eis::NativeEis::start(mapping).await;
    let fixture = Fixture::new(mode).await;
    *fixture.shared.native.lock().unwrap() = Some(native.grant.clone());
    *fixture.shared.eis_socket.lock().unwrap() = Some(eis.socket.clone());
    let mut session = PortalSession::on_native_test_bus(
        PortalOptions::new(run.into(), SourceKind::Monitor),
        fixture.bus.address.clone(),
    )
    .unwrap();
    session.ready().await.unwrap();
    super::super::eis::capabilities(&session).await;
    native.link(None).await;
    next_frame(&session, 0).await;
    (native, eis, fixture, session)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires owned native PipeWire and libeis fixtures"]
async fn host_keyboard_without_pointer_mapping_keeps_snapshot_and_keyboard_authority() {
    for (mode, mapping) in [
        (Mode::NoMapping, "owned-fixture-region"),
        (Mode::Normal, "unpaired-region"),
    ] {
        let (mut native, eis, fixture, session) =
            setup_mapping("host-keyboard-no-pointer", mode, mapping).await;
        native.source_command(b's').await;
        tokio::time::sleep(Duration::from_millis(250)).await;
        native.assert_source_streaming();
        let capabilities = super::super::eis::capabilities(&session).await;
        assert!(capabilities.keyboard && capabilities.absolute_region.is_none());
        let mut host = crate::PortalHostSession::new(session, 11, 7).unwrap();
        // Lack of pointer mapping must never be replaced with a guessed region.
        for (action, parameters) in [
            (ActionKind::Click, json!({})),
            (ActionKind::Scroll, json!({"delta":120})),
        ] {
            let observed = capture(&mut host);
            let rejected = host
                .dispatch(&request(&observed, action, parameters))
                .await
                .unwrap();
            assert_eq!(rejected.outcome, Some(OutcomeKind::Rejected));
            assert!(!rejected.applied);
            assert!(
                rejected.detail.contains("no uniquely paired"),
                "{rejected:?}"
            );
        }
        assert!(inputs(&eis).is_empty());
        let original = capture(&mut host);
        let mut preview_options = CaptureOptions::model(true);
        preview_options.for_model = false;
        let preview = host
            .capture(
                &original.run_id,
                &original.target_id,
                &preview_options,
                28,
                20,
            )
            .unwrap();
        let invalid = request(&preview, ActionKind::Key, json!({"key":"enter"}));
        assert!(!host.dispatch(&invalid).await.unwrap().applied);
        let key = request(&original, ActionKind::Key, json!({"key":"enter"}));
        let applied = host.dispatch(&key).await.unwrap();
        assert!(
            applied.applied,
            "independent keyboard capability rejected: {applied:?}"
        );
        assert!(!applied.verifiable && !applied.postcondition_ok);
        let events = eis.wait_event("key", 2).await;
        let keys: Vec<_> = events
            .iter()
            .filter(|e| e["event"] == "key")
            .map(|e| (e["code"].as_u64().unwrap(), e["pressed"].as_bool().unwrap()))
            .collect();
        assert_eq!(keys, [(28, true), (28, false)]);
        assert_eq!(inputs(&eis).len(), 2, "keyboard must not move the pointer");
        assert!(
            !host.dispatch(&key).await.unwrap().applied,
            "one-use ticket replay"
        );
        let next = capture(&mut host);
        let cancelled = request(&next, ActionKind::Key, json!({"key":"enter"}));
        cancelled.cancellation.cancel();
        assert!(!host.dispatch(&cancelled).await.unwrap().applied);
        assert_eq!(inputs(&eis).len(), 2);
        host.stop().await.unwrap();
        fixture.assert_sessions_closed();
        eis.wait_event("disconnect", 1).await;
        fs::write(native.root.join("keyboard-without-pointer.json"), serde_json::to_vec_pretty(&json!({"mapping":mapping,"portalMappingAbsent":mode==Mode::NoMapping,"actualNativeGnome":false,"originalOwnersJoined":true,"inputs":inputs(&eis),"keyboardApplied":true,"pointerDenied":true,"previewDenied":true,"replayDenied":true,"cancellationDenied":true})).unwrap()).unwrap();
    }
}
fn inputs(eis: &super::super::eis::NativeEis) -> Vec<Json> {
    eis.events()
        .into_iter()
        .filter(|e| {
            matches!(
                e["event"].as_str(),
                Some(
                    "absolute"
                        | "relative"
                        | "key"
                        | "button"
                        | "scroll"
                        | "discrete"
                        | "scroll-cancel"
                )
            )
        })
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires owned native PipeWire and libeis fixtures"]
async fn host_png_click_key_scroll_preview_and_replay_use_exact_native_authority() {
    let (mut native, eis, _fixture, session) = setup("host-native-png").await;
    native.source_command(b's').await;
    tokio::time::sleep(Duration::from_millis(250)).await;
    native.assert_source_streaming();
    let mut host = crate::PortalHostSession::new(session, 11, 7).unwrap();
    let first = capture(&mut host);
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(first.image.png_base64.as_ref().unwrap())
        .unwrap();
    let decoded = image::load_from_memory_with_format(&bytes, image::ImageFormat::Png)
        .unwrap()
        .to_rgba8();
    assert_eq!(decoded.dimensions(), (28, 20));
    for (x, y, p) in decoded.enumerate_pixels() {
        assert_eq!(&p.0[1..], &[y as u8 + 2, x as u8 + 2, 255]);
    }
    let mut options = CaptureOptions::model(true);
    options.for_model = false;
    let preview = host
        .capture(&first.run_id, &first.target_id, &options, 28, 20)
        .unwrap();
    assert_eq!(preview.captured_at, first.captured_at);
    assert_eq!(preview.image.png_base64, first.image.png_base64);
    assert_ne!(preview.snapshot_id, first.snapshot_id);
    let rejected = host
        .dispatch(&request(&preview, ActionKind::Click, json!({})))
        .await
        .unwrap();
    assert_eq!(rejected.outcome, Some(OutcomeKind::Rejected));
    assert!(inputs(&eis).is_empty());
    let click = request(
        &first,
        ActionKind::Click,
        json!({"button":"right", "count":2}),
    );
    let result = host.dispatch(&click).await.unwrap();
    assert!(result.applied);
    assert!(!result.verifiable && !result.postcondition_ok && result.outcome.is_none());
    let events = eis.wait_event("button", 4).await;
    let motion = events.iter().find(|e| e["event"] == "absolute").unwrap();
    assert!(
        (motion["x"].as_f64().unwrap() - f64::from((100.0 + 0.5 * 800.0 / 28.0) as f32)).abs()
            < 1e-8
    );
    assert_eq!(motion["y"].as_f64().unwrap(), 215.0);
    let buttons: Vec<_> = events.iter().filter(|e| e["event"] == "button").collect();
    for (event, pressed) in buttons.iter().zip([true, false, true, false]) {
        assert_eq!(event["code"], 0x111);
        assert_eq!(event["pressed"], pressed);
    }
    assert_eq!(
        host.dispatch(&click).await.unwrap().outcome,
        Some(OutcomeKind::Rejected)
    );
    let second = capture(&mut host);
    assert_eq!(second.captured_at, first.captured_at);
    assert_eq!(second.image.png_base64, first.image.png_base64);
    let result = host
        .dispatch(&request(&second, ActionKind::Key, json!({"key":"enter"})))
        .await
        .unwrap();
    assert!(result.applied);
    let events = eis.wait_event("key", 2).await;
    let keys: Vec<_> = events.iter().filter(|e| e["event"] == "key").collect();
    assert_eq!(keys.len(), 2);
    assert_eq!(keys[0]["code"], 28);
    assert_eq!(keys[0]["pressed"], true);
    assert_eq!(keys[1]["code"], 28);
    assert_eq!(keys[1]["pressed"], false);
    let third = capture(&mut host);
    assert!(
        host.dispatch(&request(&third, ActionKind::Scroll, json!({"delta":120})))
            .await
            .unwrap()
            .applied
    );
    let events = eis.wait_event("scroll-cancel", 1).await;
    let scroll = events.iter().find(|e| e["event"] == "scroll").unwrap();
    assert_eq!(scroll["y"], 120.0);
    let before = inputs(&eis);
    let zero = capture(&mut host);
    let result = host
        .dispatch(&request(&zero, ActionKind::Scroll, json!({"delta":0})))
        .await
        .unwrap();
    assert!(!result.applied && result.outcome.is_none());
    assert_eq!(
        inputs(&eis),
        before,
        "zero scroll must not even move the pointer"
    );
    let negative = capture(&mut host);
    assert!(
        host.dispatch(&request(
            &negative,
            ActionKind::Scroll,
            json!({"delta":-120})
        ))
        .await
        .unwrap()
        .applied
    );
    let events = eis.wait_event("scroll-cancel", 2).await;
    let scrolls: Vec<_> = events
        .iter()
        .filter(|e| e["event"] == "scroll")
        .map(|e| e["y"].as_f64().unwrap())
        .collect();
    assert_eq!(scrolls, vec![120.0, -120.0]);
    fs::write(
        native.root.join("host-observations.json"),
        serde_json::to_vec_pretty(&json!({
            "model":first,"preview":preview,"key":second,"scroll":third,"zeroScroll":zero,"negativeScroll":negative,
            "nativeEvents":events,"applicationEffectVerified":false
        }))
        .unwrap(),
    )
    .unwrap();
    host.stop().await.unwrap();
    assert!(matches!(host.state(), SessionState::Closed(_)));
    println!("PASS Host PNG bytes + original static timestamp; preview non-authority; right double click, enter tap, signed scroll and zero no-op; one-ticket no replay; native submission NOT app effect");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires owned native PipeWire and libeis fixtures"]
async fn host_rejects_forged_identity_scope_geometry_and_superseded_or_failed_capture() {
    let (_native, eis, _fixture, session) = setup("host-hostile-binding").await;
    let mut host = crate::PortalHostSession::new(session, 11, 7).unwrap();
    let obs = capture(&mut host);
    let valid = request(&obs, ActionKind::Click, json!({}));
    let mut cases = Vec::new();
    let mut q = valid.clone();
    q.run_id = "foreign-run".into();
    cases.push(q);
    let mut q = valid.clone();
    q.target_id = "foreign-target".into();
    cases.push(q);
    let mut q = valid.clone();
    q.generation += 1;
    cases.push(q);
    let mut q = valid.clone();
    q.target_generation += 1;
    cases.push(q);
    let mut q = valid.clone();
    q.geometry_revision += 1;
    cases.push(q);
    let mut q = valid.clone();
    q.snapshot_id = "foreign-snapshot".into();
    cases.push(q);
    let mut q = valid.clone();
    q.scope = ActionScope::Desktop;
    cases.push(q);
    let mut q = valid.clone();
    q.cancellation = ActionCancellation::default();
    q.cancellation.cancel();
    cases.push(q);
    let mut q = valid.clone();
    q.parameters = json!({"yolo":true});
    cases.push(q);
    let mut q = valid.clone();
    q.parameters = json!({"count":3});
    cases.push(q);
    let mut q = valid.clone();
    q.target = ActionTarget::Coord { x: 28.0, y: 0.0 };
    cases.push(q);
    let mut q = valid.clone();
    q.target = ActionTarget::Coord {
        x: f64::NAN,
        y: 0.0,
    };
    cases.push(q);
    let mut q = valid.clone();
    q.target = ActionTarget::Element {
        element_ref: "forged".into(),
    };
    cases.push(q);
    let mut q = valid.clone();
    q.action = ActionKind::TypeText;
    q.parameters = json!({"text":"中文"});
    cases.push(q);
    for q in &cases {
        let result = host.dispatch(q).await.unwrap();
        assert_eq!(result.outcome, Some(OutcomeKind::Rejected), "{q:?}");
        assert!(!result.applied);
    }
    assert!(inputs(&eis).is_empty());
    let newer = capture(&mut host);
    assert_eq!(
        host.dispatch(&valid).await.unwrap().outcome,
        Some(OutcomeKind::Rejected)
    );
    assert!(
        host.dispatch(&request(&newer, ActionKind::Click, json!({})))
            .await
            .unwrap()
            .applied
    );
    eis.wait_event("button", 2).await;
    let before = inputs(&eis);
    for failure in 0..3 {
        let live = capture(&mut host);
        let mut options = CaptureOptions::model(true);
        if failure == 0 {
            options.cancellation.cancel();
        }
        if failure == 1 {
            options.screenshot = false;
        }
        let width = if failure == 2 { 0 } else { 28 };
        assert!(host
            .capture(&live.run_id, &live.target_id, &options, width, 20)
            .is_err());
        assert_eq!(
            host.dispatch(&request(&live, ActionKind::Click, json!({})))
                .await
                .unwrap()
                .outcome,
            Some(OutcomeKind::Rejected)
        );
    }
    assert_eq!(inputs(&eis), before);
    let live = capture(&mut host);
    host.stop().await.unwrap();
    assert_eq!(
        host.dispatch(&request(&live, ActionKind::Click, json!({})))
            .await
            .unwrap()
            .outcome,
        Some(OutcomeKind::Rejected)
    );
    println!("PASS Host hostile binding {} variants emitted no native input; replacement supersession; failed/cancelled/text-only capture retired old authority; stopped target rejects",cases.len());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires owned native PipeWire and libeis fixtures"]
async fn compound_preflight_rejects_late_invalid_capability_and_unrelated_held_state() {
    let (_native, mut eis, _fixture, mut session) = setup("compound-preflight").await;
    let caps = super::super::eis::capabilities(&session).await;
    // Removing the keyboard before preflight must reject BEFORE pointer motion.
    eis.command(b'k');
    let until = Instant::now() + Duration::from_secs(3);
    loop {
        if matches!(session.input_state(),InputState::Ready(ref c) if !c.keyboard && c.generation>caps.generation)
        {
            break;
        }
        assert!(Instant::now() < until);
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let observed = session.observe().unwrap().unwrap();
    assert!(session
        .input_sequence_observed(
            observed,
            vec![
                InputAction::Absolute { x: 1.0, y: 1.0 },
                InputAction::Key {
                    code: 30,
                    pressed: true
                },
                InputAction::Key {
                    code: 30,
                    pressed: false
                }
            ],
            ActionCancellation::default()
        )
        .await
        .is_err());
    assert!(inputs(&eis).is_empty());
    let generation = match session.input_state() {
        InputState::Ready(c) => c.generation,
        _ => panic!("ready"),
    };
    session
        .input(
            generation,
            InputAction::Button {
                code: 0x110,
                pressed: true,
            },
        )
        .await
        .unwrap();
    eis.wait_event("button", 1).await;
    let before = inputs(&eis);
    assert!(session
        .input_sequence_observed(
            session.observe().unwrap().unwrap(),
            vec![
                InputAction::Absolute { x: 1.0, y: 1.0 },
                InputAction::Button {
                    code: 0x111,
                    pressed: true
                },
                InputAction::Button {
                    code: 0x111,
                    pressed: false
                }
            ],
            ActionCancellation::default()
        )
        .await
        .unwrap_err()
        .contains("neutral"));
    assert_eq!(inputs(&eis), before);
    session
        .input(generation, InputAction::ReleaseAll)
        .await
        .unwrap();
    eis.wait_event("button", 2).await;
    let before = inputs(&eis);
    assert!(session
        .input_sequence_observed(
            session.observe().unwrap().unwrap(),
            vec![
                InputAction::Button {
                    code: 0x111,
                    pressed: true
                },
                InputAction::Absolute { x: 800.0, y: 0.0 },
                InputAction::Button {
                    code: 0x111,
                    pressed: false
                }
            ],
            ActionCancellation::default()
        )
        .await
        .is_err());
    assert_eq!(inputs(&eis), before);
    session.stop().await.unwrap();
    println!("PASS full compound preflight before first event: late missing keyboard, invalid absolute coordinate, unowned held-state interference");
}
