use super::*;
use std::sync::atomic::AtomicBool;
use std::sync::Barrier;

struct TestView {
    alive: AtomicBool,
    generation: AtomicU64,
    scripts: AtomicU64,
    reply: Mutex<Option<serde_json::Value>>,
    pending: Mutex<Option<ScriptOperation>>,
    timeout_at: AtomicU64,
    block_at: u64,
    entered: Arc<Barrier>,
    release: Arc<Barrier>,
}

impl TestView {
    fn new(block_at: u64) -> Arc<Self> {
        Arc::new(Self {
            alive: AtomicBool::new(true),
            generation: AtomicU64::new(1),
            scripts: AtomicU64::new(0),
            reply: Mutex::new(None),
            pending: Mutex::new(None),
            timeout_at: AtomicU64::new(0),
            block_at,
            entered: Arc::new(Barrier::new(2)),
            release: Arc::new(Barrier::new(2)),
        })
    }
}

impl LiveWebView for TestView {
    fn label(&self) -> String {
        "resource-browser-test".into()
    }
    fn tab_id(&self) -> String {
        "native-test".into()
    }
    fn url(&self) -> Result<String, String> {
        Ok("https://fixture.invalid/".into())
    }
    fn title(&self) -> Result<String, String> {
        Ok("owned fixture".into())
    }
    fn is_alive(&self) -> bool {
        self.alive.load(Ordering::SeqCst)
    }
    fn navigation_generation(&self) -> u64 {
        self.generation.load(Ordering::SeqCst)
    }
    fn navigate(&self, _url: &str) -> Result<u64, String> {
        Ok(self.generation.fetch_add(1, Ordering::SeqCst) + 1)
    }
    fn host_script(&self, js: &str) -> Result<String, String> {
        let call = self.scripts.fetch_add(1, Ordering::SeqCst) + 1;
        if call == self.block_at {
            self.entered.wait();
            self.release.wait();
        }
        Ok(self
            .reply
            .lock()
            .clone()
            .unwrap_or_else(|| {
                let payload = js.rsplit_once("\n(").unwrap().1.strip_suffix(')').unwrap();
                let command: serde_json::Value = serde_json::from_str(payload).unwrap();
                if command["kind"] == "observe" {
                    let snapshot = command["snapshot"].as_str().unwrap();
                    serde_json::json!({"ok": true, "snapshot": snapshot, "width": 800, "height": 600,
                        "text": "Fixture", "truncated": false,
                        "nodes": [{"elementRef": format!("{snapshot}:1"), "role": "button", "name": "Owned",
                            "actions": ["click"], "x": 10, "y": 10, "width": 100, "height": 30}]})
                } else {
                    serde_json::json!({"ok": true})
                }
            })
            .to_string())
    }
    fn host_script_owned(
        &self,
        js: &str,
        _generation: u64,
        operation: ScriptOperation,
    ) -> Result<String, String> {
        if let Err(error) = operation.check() {
            operation.finished();
            return Err(error);
        }
        let result = self.host_script(js);
        if self.scripts.load(Ordering::SeqCst) == self.timeout_at.load(Ordering::SeqCst) {
            *self.pending.lock() = Some(operation);
            return Err("WebView typed script outcome unknown".into());
        }
        operation.finished();
        result
    }
}

#[test]
fn native_timeout_remains_occupied_after_unbind_and_same_view_reauthorization() {
    let adapter = WebViewAdapter::new();
    let view = TestView::new(0);
    view.timeout_at.store(2, Ordering::SeqCst);
    let target = bind(&adapter, view.clone());
    assert!(adapter
        .act(&click_request(target.clone()))
        .unwrap_err()
        .contains("unknown"));
    assert!(!adapter.is_idle("run"));
    adapter.abort("run", 2).unwrap();
    adapter.release_target(&target.target_id);
    assert!(!adapter.is_idle("run"));
    let new = adapter
        .bind(view.clone(), "next", "next-run", &serde_json::json!({}))
        .unwrap();
    assert!(adapter
        .claim_target_for_run("next-run", &new.target_id)
        .unwrap_err()
        .contains("still pending"));
    assert!(adapter
        .observe(&new.target_id)
        .unwrap_err()
        .contains("still pending"));
    assert_eq!(view.scripts.load(Ordering::SeqCst), 2);
    let callback = view.pending.lock().take().unwrap();
    assert!(callback.check().is_err());
    callback.finished();
    assert!(adapter.is_idle("run"));
    adapter
        .claim_target_for_run("next-run", &new.target_id)
        .unwrap();
    adapter.observe(&new.target_id).unwrap();
    assert!(adapter.target_alive(&new.target_id));
}

#[test]
fn abort_during_preflight_prevents_the_side_effect_even_without_caller_polling() {
    let adapter = Arc::new(WebViewAdapter::new());
    let view = TestView::new(1);
    let target = bind(&adapter, view.clone());
    let acting = {
        let adapter = adapter.clone();
        std::thread::spawn(move || adapter.act(&click_request(target)))
    };
    view.entered.wait();
    assert!(!adapter.is_idle("run"));
    adapter.abort("run", 2).unwrap();
    assert!(!adapter.is_idle("run"));
    view.release.wait();
    assert!(acting.join().unwrap().is_err());
    assert_eq!(view.scripts.load(Ordering::SeqCst), 1);
    assert!(adapter.is_idle("run"));
}

#[test]
fn wrong_run_observation_and_cancelled_capture_never_queue_native_work() {
    let adapter = WebViewAdapter::new();
    let view = TestView::new(0);
    let target = bind(&adapter, view.clone());
    assert!(adapter.observe_for_run("wrong", &target.target_id).is_err());
    let options = super::super::adapter::CaptureOptions::model(false);
    options.cancellation.cancel();
    assert!(adapter
        .capture_for_run("run", &target.target_id, options)
        .is_err());
    assert_eq!(view.scripts.load(Ordering::SeqCst), 0);
    assert!(adapter.is_idle("run"));
}

#[test]
fn unrelated_run_cannot_list_claim_or_release_the_bound_target() {
    let adapter = WebViewAdapter::new();
    let target = bind(&adapter, TestView::new(0));
    assert!(adapter.list_targets_for_run("wrong").unwrap().is_empty());
    assert_eq!(adapter.list_targets_for_run("run").unwrap().len(), 1);
    assert!(adapter
        .claim_target_for_run("wrong", &target.target_id)
        .is_err());
    adapter.release_target_for_run("wrong", &target.target_id);
    assert!(adapter.target_alive(&target.target_id));
    adapter
        .claim_target_for_run("run", &target.target_id)
        .unwrap();
    adapter.release_target_for_run("run", &target.target_id);
    assert!(!adapter.target_alive(&target.target_id));
}

#[test]
fn rebind_revokes_queued_old_script_without_falsely_settling_it() {
    let adapter = WebViewAdapter::new();
    let view = TestView::new(0);
    view.timeout_at.store(1, Ordering::SeqCst);
    let old = bind(&adapter, view.clone());
    assert!(adapter.observe(&old.target_id).is_err());
    let old_callback = view.pending.lock().take().unwrap();
    assert!(old_callback.check().is_ok());
    let new = adapter
        .bind(view, "next", "new-run", &serde_json::json!({}))
        .unwrap();
    assert!(old_callback.check().is_err());
    assert!(!adapter.is_idle("run"));
    adapter.abort("run", 2).unwrap();
    assert!(adapter.target_alive(&new.target_id));
    old_callback.finished();
    assert!(adapter.is_idle("run"));
}

#[test]
fn broker_stays_stop_requested_until_native_completion_even_after_release() {
    use super::super::test_support::CountingAdapter;
    use super::super::{BrokerOptions, ComputerUseBroker, StopState, SurfaceKind};
    let adapter = Arc::new(WebViewAdapter::new());
    let desktop = Arc::new(CountingAdapter::default());
    let broker = ComputerUseBroker::new(
        desktop.clone(),
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
    view.timeout_at.store(1, Ordering::SeqCst);
    let target = bind(&adapter, view.clone());
    broker.open_run("session", "run").unwrap();
    broker
        .authorize_on_surface("session", "run", SurfaceKind::WebView, &target.target_id)
        .unwrap();
    assert!(broker.observe("run").is_err());
    assert_eq!(
        broker.request_stop("run").unwrap(),
        StopState::StopRequested
    );
    assert!(adapter.list_targets().unwrap().is_empty());
    assert_eq!(broker.stop_state("run").unwrap(), StopState::StopRequested);
    let callback = view.pending.lock().take().unwrap();
    callback.finished();
    assert_eq!(broker.stop_state("run").unwrap(), StopState::Stopped);
    assert_eq!(desktop.aborts(), 0);
    assert_eq!(desktop.releases(), 0);
}

fn bind(adapter: &WebViewAdapter, view: Arc<TestView>) -> TargetInfo {
    let target = adapter
        .bind(view, "session", "run", &serde_json::json!({}))
        .unwrap();
    // Race fixtures seed a mock observation without advancing native call
    // counters. DOM behavior is exercised separately against the actual kernel.
    adapter
        .bound
        .lock()
        .as_ref()
        .unwrap()
        .observation
        .lock()
        .snapshot = Some(dom::Published {
        id: "mock-snapshot".into(),
        nodes: [("mock-snapshot:1".into(), vec!["click".into()])]
            .into_iter()
            .collect(),
    });
    target
}

fn click_request(target: TargetInfo) -> DispatchRequest {
    use super::super::adapter::ActionScope;
    use super::super::protocol::{ActionKind, ActionTarget};
    DispatchRequest {
        managed_request: None,
        cancellation: Default::default(),
        run_id: "run".into(),
        action_id: "old".into(),
        generation: 1,
        target_id: target.target_id,
        target_generation: target.lifecycle_stamp,
        snapshot_id: "mock-snapshot".into(),
        geometry_revision: target.lifecycle_stamp,
        action: ActionKind::Click,
        target: ActionTarget::Element {
            element_ref: "mock-snapshot:1".into(),
        },
        parameters: serde_json::json!({}),
        scope: ActionScope::Directed,
    }
}

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
            .bound
            .lock()
            .as_ref()
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
    let current = adapter.bound.lock().clone().unwrap();
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
    use super::super::protocol::ActionKind;
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
    use super::super::adapter::CaptureOptions;
    use super::super::protocol::ActionTarget;
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
    use super::super::protocol::ActionKind;
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
fn oversized_fill_and_scroll_parameters_are_rejected_before_native_dispatch() {
    use super::super::protocol::ActionKind;
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
