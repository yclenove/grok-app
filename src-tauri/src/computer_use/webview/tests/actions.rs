//! Document authority, typed actions and semantic wait regression tests.

use super::*;

#[test]
fn navigation_fences_target_until_explicit_reauthorization() {
    let adapter = WebViewAdapter::new();
    let view = TestView::new(0);
    let old = bind(&adapter, view.clone());
    view.navigate("https://fixture.invalid/new").unwrap();
    assert!(!adapter.target_alive(&old.target_id));
    assert!(adapter.list_targets().unwrap().is_empty());
    assert!(adapter.observe(&old.target_id).is_err());
    let new = bind(&adapter, view);
    assert_ne!(new.target_id, old.target_id);
    assert!(adapter.target_alive(&new.target_id));
}

#[test]
fn rebind_same_document_rotates_binding_and_old_release_cannot_unbind_it() {
    let adapter = WebViewAdapter::new();
    let view = TestView::new(0);
    let old = bind(&adapter, view.clone());
    let new = bind(&adapter, view);
    assert_ne!(old.target_id, new.target_id);
    adapter.release_target(&old.target_id);
    assert!(adapter.target_alive(&new.target_id));
    adapter.release_target(&new.target_id);
    assert!(adapter.list_targets().unwrap().is_empty());
}

#[test]
fn trailing_target_identity_fields_are_rejected() {
    let adapter = WebViewAdapter::new();
    let target = bind(&adapter, TestView::new(0));
    assert!(!adapter.target_alive(&format!("{}|replacement", target.target_id)));
    assert!(parse_id(&format!("{}|replacement", target.target_id)).is_none());
}

#[test]
fn navigation_while_observation_is_inflight_discards_the_old_result() {
    let adapter = Arc::new(WebViewAdapter::new());
    let view = TestView::new(1);
    let target = bind(&adapter, view.clone());
    let observing = {
        let adapter = adapter.clone();
        std::thread::spawn(move || adapter.observe(&target.target_id))
    };
    view.entered.wait();
    view.navigate("https://fixture.invalid/new").unwrap();
    view.release.wait();
    assert!(observing.join().unwrap().unwrap_err().contains("changed"));
}

#[test]
fn rebind_while_observation_is_inflight_never_publishes_under_the_new_owner() {
    let adapter = Arc::new(WebViewAdapter::new());
    let old_view = TestView::new(1);
    let target = bind(&adapter, old_view.clone());
    let observing = {
        let adapter = adapter.clone();
        std::thread::spawn(move || adapter.observe(&target.target_id))
    };
    old_view.entered.wait();
    let replacement = bind(&adapter, TestView::new(0));
    old_view.release.wait();
    assert!(observing.join().unwrap().is_err());
    assert!(adapter.target_alive(&replacement.target_id));
}

#[test]
fn old_action_reply_cannot_modify_the_replacement_published_snapshot() {
    let adapter = Arc::new(WebViewAdapter::new());
    // act validates the model reference, then dispatches the side-effect script.
    let old_view = TestView::new(2);
    let target = bind(&adapter, old_view.clone());
    let acting = {
        let adapter = adapter.clone();
        std::thread::spawn(move || adapter.act(&click_request(target)))
    };
    old_view.entered.wait();
    let replacement = bind(&adapter, TestView::new(0));
    old_view.release.wait();
    assert!(acting.join().unwrap().unwrap_err().contains("unknown"));
    assert!(adapter.target_alive(&replacement.target_id));
    assert_eq!(
        adapter
            .bound_target(&replacement.target_id)
            .unwrap()
            .observation
            .lock()
            .snapshot
            .as_ref()
            .unwrap()
            .id,
        "mock-snapshot"
    );
}

#[test]
fn navigation_during_preflight_prevents_side_effect_dispatch() {
    let adapter = Arc::new(WebViewAdapter::new());
    let view = TestView::new(1);
    let target = bind(&adapter, view.clone());
    let acting = {
        let adapter = adapter.clone();
        std::thread::spawn(move || adapter.act(&click_request(target)))
    };
    view.entered.wait();
    view.navigate("https://fixture.invalid/new").unwrap();
    view.release.wait();
    assert!(acting
        .join()
        .unwrap()
        .unwrap_err()
        .contains("before dispatch"));
    assert_eq!(view.scripts.load(Ordering::SeqCst), 1);
}

#[test]
fn wrong_run_cannot_dispatch_any_script_against_a_live_binding() {
    let adapter = WebViewAdapter::new();
    let view = TestView::new(0);
    let target = bind(&adapter, view.clone());
    let mut request = click_request(target);
    request.run_id = "replacement-run".into();
    assert!(adapter.act(&request).unwrap_err().contains("run identity"));
    assert_eq!(view.scripts.load(Ordering::SeqCst), 0);
}

#[test]
fn closed_bound_view_cannot_observe_or_advertise_a_live_target() {
    let adapter = WebViewAdapter::new();
    let view = TestView::new(0);
    let target = bind(&adapter, view.clone());
    view.alive.store(false, Ordering::SeqCst);
    assert!(!adapter.target_alive(&target.target_id));
    assert!(adapter.list_targets().unwrap().is_empty());
    assert!(adapter.observe(&target.target_id).is_err());
}

#[test]
fn cancellation_epoch_exhaustion_retires_binding_instead_of_restoring_old_authority() {
    let adapter = WebViewAdapter::new();
    let view = TestView::new(0);
    let target = bind(&adapter, view.clone());
    let current = adapter.bound_target(&target.target_id).unwrap();
    current.execution_epoch.store(u64::MAX, Ordering::SeqCst);
    adapter.abort("run", 1).unwrap();
    assert_eq!(current.execution_epoch.load(Ordering::SeqCst), u64::MAX);
    assert!(!adapter.target_alive(&target.target_id));
    assert!(adapter.list_targets().unwrap().is_empty());
    assert!(adapter.observe(&target.target_id).is_err());
    assert_eq!(view.scripts.load(Ordering::SeqCst), 0);
    let replacement = bind(&adapter, view);
    assert!(adapter.target_alive(&replacement.target_id));
}

#[test]
fn href_cannot_bypass_element_identity_as_hidden_navigation() {
    let adapter = WebViewAdapter::new();
    let view = TestView::new(0);
    let target = bind(&adapter, view.clone());
    let mut request = click_request(target);
    request.parameters = serde_json::json!({"href": "file:///private.txt"});
    assert!(adapter.act(&request).unwrap_err().contains("parameter"));
    assert_eq!(view.scripts.load(Ordering::SeqCst), 0);
    assert_eq!(view.generation.load(Ordering::SeqCst), 1);
}

#[test]
fn fill_never_silently_clears_on_missing_ambiguous_or_wrong_type_input() {
    use crate::computer_use::protocol::ActionKind;
    let adapter = WebViewAdapter::new();
    let view = TestView::new(0);
    let target = bind(&adapter, view.clone());
    let mut request = click_request(target);
    request.action = ActionKind::SetValue;
    for parameters in [
        serde_json::json!({}),
        serde_json::json!({"text": null}),
        serde_json::json!({"text": "a", "value": "b"}),
        serde_json::json!({"text": "a", "selector": "#replacement"}),
        serde_json::json!(["text", "a"]),
    ] {
        request.parameters = parameters;
        assert!(adapter.act(&request).is_err());
    }
    assert_eq!(view.scripts.load(Ordering::SeqCst), 0);
    for parameters in [
        serde_json::json!({"text": ""}),
        serde_json::json!({"value": "你好"}),
    ] {
        request.parameters = parameters;
        assert!(validate_action_parameters(&request).is_ok());
    }
}

#[test]
fn returned_script_is_applied_not_independently_verified_and_does_not_echo_page_data() {
    let adapter = WebViewAdapter::new();
    let view = TestView::new(0);
    *view.reply.lock() = Some(serde_json::json!({"ok": true, "value": "PRIVATE_INPUT_SENTINEL"}));
    let target = bind(&adapter, view.clone());
    let outcome = adapter.act(&click_request(target)).unwrap();
    assert!(outcome.applied);
    assert!(!outcome.verifiable);
    assert!(!outcome.postcondition_ok);
    assert!(!outcome.detail.contains("PRIVATE_INPUT_SENTINEL"));
    assert_eq!(view.scripts.load(Ordering::SeqCst), 2);
}

#[test]
fn rejected_action_does_not_echo_page_controlled_error_data() {
    let adapter = WebViewAdapter::new();
    let view = TestView::new(0);
    *view.reply.lock() = Some(serde_json::json!({"ok": false, "error": "PRIVATE_INPUT_SENTINEL"}));
    let target = bind(&adapter, view);
    let error = adapter.act(&click_request(target)).unwrap_err();
    assert!(!error.contains("PRIVATE_INPUT_SENTINEL"));
}

#[test]
fn product_observation_never_extracts_fixture_input_values() {
    for id in ["name", "mark", "count", "scroller"] {
        assert!(!dom::KERNEL.contains(&format!("getElementById('{id}')")));
    }
}

#[test]
fn preview_preserves_model_snapshot_and_forged_preview_action_never_dispatches() {
    use crate::computer_use::adapter::CaptureOptions;
    use crate::computer_use::protocol::ActionTarget;
    let adapter = WebViewAdapter::new();
    let view = TestView::new(0);
    let target = bind(&adapter, view.clone());
    let model = adapter.observe(&target.target_id).unwrap();
    let preview = adapter
        .capture_for_run(
            "run",
            &target.target_id,
            CaptureOptions {
                managed_request: None,
                for_model: false,
                screenshot: false,
                cancellation: Default::default(),
            },
        )
        .unwrap();
    assert_ne!(model.snapshot_id, preview.snapshot_id);
    let mut request = click_request(target);
    request.snapshot_id = preview.snapshot_id;
    request.target = ActionTarget::Element {
        element_ref: preview.nodes[0].node_ref.clone(),
    };
    assert!(adapter.act(&request).unwrap_err().contains("stale"));
    assert_eq!(view.scripts.load(Ordering::SeqCst), 2);
    request.snapshot_id = model.snapshot_id;
    request.target = ActionTarget::Element {
        element_ref: model.nodes[0].node_ref.clone(),
    };
    assert!(adapter.act(&request).unwrap().applied);
    assert_eq!(view.scripts.load(Ordering::SeqCst), 4);
    assert!(adapter.act(&request).unwrap_err().contains("stale"));
    assert_eq!(view.scripts.load(Ordering::SeqCst), 4);
}

#[test]
fn missing_observation_and_unadvertised_action_fail_before_any_native_call() {
    use crate::computer_use::protocol::ActionKind;
    let adapter = WebViewAdapter::new();
    let view = TestView::new(0);
    let target = adapter
        .bind(view.clone(), "session", "run", &serde_json::json!({}))
        .unwrap();
    assert!(adapter.act(&click_request(target)).is_err());
    assert_eq!(view.scripts.load(Ordering::SeqCst), 0);
    let target = bind(&adapter, view.clone());
    let mut request = click_request(target);
    request.action = ActionKind::Scroll;
    request.parameters = serde_json::json!({"dy": 100});
    assert!(adapter.act(&request).is_err());
    assert_eq!(view.scripts.load(Ordering::SeqCst), 0);
    assert!(adapter.capabilities().scroll.semantic);
    assert!(!adapter.capabilities().scroll.coordinate);
}

#[test]
fn semantic_wait_polls_without_consuming_the_exact_snapshot() {
    let adapter = WebViewAdapter::new();
    let view = TestView::new(0);
    view.wait_match_at.store(3, Ordering::SeqCst);
    let target = bind(&adapter, view.clone());
    assert!(adapter.capabilities().wait.semantic);

    let outcome = adapter
        .act(&wait_request(target.clone(), "完成", 500))
        .unwrap();
    assert!(outcome.applied);
    assert!(outcome.verifiable);
    assert!(outcome.postcondition_ok);
    assert_eq!(view.scripts.load(Ordering::SeqCst), 3);
    assert!(adapter
        .bound_target(&target.target_id)
        .unwrap()
        .observation
        .lock()
        .snapshot
        .is_some());

    assert!(adapter.act(&click_request(target)).unwrap().applied);
    assert_eq!(view.scripts.load(Ordering::SeqCst), 5);
}

#[test]
fn semantic_type_text_is_verified_and_rejects_clipboard_or_invalid_text_before_dispatch() {
    let adapter = WebViewAdapter::new();
    let view = TestView::new(0);
    let target = bind(&adapter, view.clone());
    assert!(adapter.capabilities().type_text.semantic);

    let outcome = adapter
        .act(&type_text_request(target.clone(), "中文🙂"))
        .unwrap();
    assert!(outcome.applied);
    assert!(outcome.verifiable);
    assert!(outcome.postcondition_ok);
    assert_eq!(view.scripts.load(Ordering::SeqCst), 2);

    let adapter = WebViewAdapter::new();
    let view = TestView::new(0);
    let target = bind(&adapter, view.clone());
    for parameters in [
        serde_json::json!({}),
        serde_json::json!({"text":null}),
        serde_json::json!({"text":"x","via":"clipboard"}),
        serde_json::json!({"text":"x\0y"}),
        serde_json::json!({"text":"🙂".repeat(4001)}),
        serde_json::json!({"text":"x","selector":"input"}),
    ] {
        let mut request = type_text_request(target.clone(), "valid");
        request.parameters = parameters;
        assert!(adapter.act(&request).is_err());
    }
    assert_eq!(view.scripts.load(Ordering::SeqCst), 0);
}

#[test]
fn broker_reports_webview_type_text_as_verified() {
    use crate::computer_use::protocol::{
        ActionRequest, ActionTarget, OutcomeKind, PROTOCOL_VERSION,
    };
    use crate::computer_use::test_support::CountingAdapter;
    use crate::computer_use::{BrokerOptions, ComputerUseBroker, SurfaceKind};

    let adapter = Arc::new(WebViewAdapter::new());
    let broker = ComputerUseBroker::new(
        Arc::new(CountingAdapter::default()),
        BrokerOptions {
            feature_enabled: true,
            lease_path: std::env::temp_dir().join(format!("cu-wv-{}.lease", uuid::Uuid::new_v4())),
            ..Default::default()
        },
    );
    broker
        .register_surface_adapter(SurfaceKind::WebView, adapter.clone())
        .unwrap();
    let view = TestView::new(0);
    let target = bind(&adapter, view.clone());
    broker.open_run("session", "run").unwrap();
    let generation = broker
        .authorize_on_surface("session", "run", SurfaceKind::WebView, &target.target_id)
        .unwrap();
    let observed = broker.observe("run").unwrap();
    let typed = broker.act(ActionRequest {
        version: PROTOCOL_VERSION,
        action_id: "type-text-through-broker".into(),
        run_id: "run".into(),
        target_id: target.target_id,
        target_generation: observed.target_generation,
        snapshot_id: observed.snapshot_id,
        geometry_revision: observed.geometry_revision,
        action: ActionKind::TypeText,
        target: ActionTarget::Element {
            element_ref: observed.nodes[0].node_ref.clone(),
        },
        parameters: serde_json::json!({"text":"中文🙂"}),
    });

    assert_eq!(typed.kind, OutcomeKind::Verified, "{typed:?}");
    assert!(typed.executed);
    assert_eq!(typed.generation, generation);
    assert_eq!(view.scripts.load(Ordering::SeqCst), 3);
}

#[test]
fn broker_reports_webview_wait_as_verified_and_keeps_snapshot_for_the_next_action() {
    use crate::computer_use::protocol::{
        ActionRequest, ActionTarget, OutcomeKind, PROTOCOL_VERSION,
    };
    use crate::computer_use::test_support::CountingAdapter;
    use crate::computer_use::{BrokerOptions, ComputerUseBroker, SurfaceKind};

    let adapter = Arc::new(WebViewAdapter::new());
    let broker = ComputerUseBroker::new(
        Arc::new(CountingAdapter::default()),
        BrokerOptions {
            feature_enabled: true,
            lease_path: std::env::temp_dir().join(format!("cu-wv-{}.lease", uuid::Uuid::new_v4())),
            ..Default::default()
        },
    );
    broker
        .register_surface_adapter(SurfaceKind::WebView, adapter.clone())
        .unwrap();
    let view = TestView::new(0);
    view.wait_match_at.store(1, Ordering::SeqCst);
    let target = bind(&adapter, view.clone());
    broker.open_run("session", "run").unwrap();
    let generation = broker
        .authorize_on_surface("session", "run", SurfaceKind::WebView, &target.target_id)
        .unwrap();
    let observed = broker.observe("run").unwrap();
    let element_ref = observed.nodes[0].node_ref.clone();
    let identity = |action_id: &str, action: ActionKind, parameters| ActionRequest {
        version: PROTOCOL_VERSION,
        action_id: action_id.into(),
        run_id: "run".into(),
        target_id: target.target_id.clone(),
        target_generation: observed.target_generation,
        snapshot_id: observed.snapshot_id.clone(),
        geometry_revision: observed.geometry_revision,
        action,
        target: ActionTarget::Element {
            element_ref: element_ref.clone(),
        },
        parameters,
    };
    let waited = broker.act(identity(
        "wait-through-broker",
        ActionKind::Wait,
        serde_json::json!({"nameEquals":"完成","timeoutMs":100}),
    ));
    assert_eq!(waited.kind, OutcomeKind::Verified, "{waited:?}");
    assert!(waited.executed);

    let clicked = broker.act(identity(
        "click-after-wait",
        ActionKind::Click,
        serde_json::json!({}),
    ));
    assert_eq!(clicked.kind, OutcomeKind::Applied);
    assert!(clicked.executed);
    assert_eq!(clicked.generation, generation);
    assert_eq!(view.scripts.load(Ordering::SeqCst), 4);
}

#[test]
fn wait_timeout_keeps_snapshot_and_rejects_invalid_parameters_before_dispatch() {
    let adapter = WebViewAdapter::new();
    let view = TestView::new(0);
    let target = bind(&adapter, view.clone());
    let error = adapter
        .act(&wait_request(target.clone(), "never", 10))
        .unwrap_err();
    assert!(error.contains("timed out"));
    assert!(adapter
        .bound_target(&target.target_id)
        .unwrap()
        .observation
        .lock()
        .snapshot
        .is_some());
    let before = view.scripts.load(Ordering::SeqCst);

    for parameters in [
        serde_json::json!({}),
        serde_json::json!({"nameEquals":"   "}),
        serde_json::json!({"nameEquals":"x","timeoutMs":0}),
        serde_json::json!({"nameEquals":"x","timeoutMs":10001}),
        serde_json::json!({"nameEquals":"🙂".repeat(257)}),
        serde_json::json!({"nameEquals":"x","selector":"button"}),
    ] {
        let mut request = wait_request(target.clone(), "valid", 20);
        request.parameters = parameters;
        assert!(adapter.act(&request).is_err());
    }
    assert_eq!(view.scripts.load(Ordering::SeqCst), before);
}

#[test]
fn abort_and_navigation_each_retire_an_inflight_wait_before_success() {
    let adapter = Arc::new(WebViewAdapter::new());
    let view = TestView::new(1);
    view.wait_match_at.store(1, Ordering::SeqCst);
    let target = bind(&adapter, view.clone());
    let waiting = {
        let adapter = adapter.clone();
        let request = wait_request(target, "完成", 1000);
        std::thread::spawn(move || adapter.act(&request))
    };
    view.entered.wait();
    adapter.abort("run", 2).unwrap();
    view.release.wait();
    assert!(waiting.join().unwrap().is_err());
    assert_eq!(view.scripts.load(Ordering::SeqCst), 1);

    let adapter = Arc::new(WebViewAdapter::new());
    let view = TestView::new(1);
    view.wait_match_at.store(1, Ordering::SeqCst);
    let target = bind(&adapter, view.clone());
    let waiting = {
        let adapter = adapter.clone();
        let request = wait_request(target, "完成", 1000);
        std::thread::spawn(move || adapter.act(&request))
    };
    view.entered.wait();
    view.navigate("https://fixture.invalid/replaced").unwrap();
    view.release.wait();
    assert!(waiting.join().unwrap().is_err());
    assert_eq!(view.scripts.load(Ordering::SeqCst), 1);
}

#[test]
fn navigation_after_a_matched_reply_wins_before_the_final_document_commit() {
    let adapter = Arc::new(WebViewAdapter::new());
    let view = TestView::new(0);
    view.wait_match_at.store(1, Ordering::SeqCst);
    view.block_authority.store(true, Ordering::SeqCst);
    let target = bind(&adapter, view.clone());
    let waiting = {
        let adapter = adapter.clone();
        let request = wait_request(target, "完成", 1000);
        std::thread::spawn(move || adapter.act(&request))
    };

    // The DOM reply already matched, but final native document authority has
    // not yet been acquired. Navigation must retire that exact result.
    view.entered.wait();
    view.navigate("https://fixture.invalid/replaced").unwrap();
    view.release.wait();
    let error = waiting.join().unwrap().unwrap_err();
    assert!(error.contains("retired before verification"), "{error}");
    assert_eq!(view.scripts.load(Ordering::SeqCst), 1);
}

#[test]
fn oversized_fill_and_scroll_parameters_are_rejected_before_native_dispatch() {
    use crate::computer_use::protocol::ActionKind;
    let adapter = WebViewAdapter::new();
    let view = TestView::new(0);
    let target = bind(&adapter, view.clone());
    let mut request = click_request(target);
    request.action = ActionKind::SetValue;
    request.parameters = serde_json::json!({"text": "😀".repeat(8001)});
    assert!(adapter.act(&request).is_err());
    request.action = ActionKind::Scroll;
    for n in [4097i64, -4097, i64::MIN, i64::MAX] {
        request.parameters = serde_json::json!({"dy": n});
        assert!(adapter.act(&request).is_err());
    }
    assert_eq!(view.scripts.load(Ordering::SeqCst), 0);
}
