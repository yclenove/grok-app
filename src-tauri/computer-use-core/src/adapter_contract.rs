//! Named contract tests: per-action capability maps and pre-dispatch rejection.
use super::adapter::{ActionSupport, Capabilities, ComputerUseAdapter, SurfaceKind, TargetInfo};
use super::broker::{BrokerOptions, ComputerUseBroker};
use super::protocol::{ActionKind, ActionRequest, ActionTarget, OutcomeKind, PROTOCOL_VERSION};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

#[test]
fn capabilities_report_each_action_and_input_mode_separately() {
    let desktop = Capabilities::for_surface(SurfaceKind::Desktop, "windows");
    let webview = Capabilities::for_surface(SurfaceKind::WebView, "webview");
    let browser = Capabilities::for_surface(SurfaceKind::ManagedBrowser, "managed-browser");
    let tab = Capabilities::for_surface(SurfaceKind::ExistingTab, "existing-tab");

    assert_ne!(desktop.click, desktop.drag);
    assert_ne!(desktop.set_value.coordinate, desktop.click.coordinate);
    assert_eq!(webview.click, ActionSupport::NONE);
    assert_eq!(webview.drag, ActionSupport::NONE);
    assert!(
        browser.click.semantic && browser.click.coordinate,
        "managed browser must publish both click modes"
    );
    assert_eq!(tab.drag, ActionSupport::NONE);
    assert!(desktop.wait.semantic && !desktop.wait.coordinate);
    assert_eq!(webview.wait, ActionSupport::NONE);
    assert_ne!(desktop.backend_id, webview.backend_id);
    assert_ne!(browser.notes, tab.notes);
}

#[test]
fn target_info_carries_backend_stamp_display_and_scope() {
    let fake = super::fake::FakeAdapter::new();
    let targets = fake.list_targets().unwrap();
    let fixture = targets
        .iter()
        .find(|t| t.title == "GrokCuFixture")
        .expect("fixture target");
    assert_eq!(fixture.backend, "fake");
    assert!(fixture.lifecycle_stamp >= 1);
    assert!(!fixture.display_id.trim().is_empty());
    assert_eq!(fixture.coordinate_space, "image-pixels");
    assert!(fixture.scope_label.contains("fake"));
    assert_eq!(fixture.execution_mode, "exclusive");
}

struct DenyAdapter {
    acts: AtomicU64,
}

impl ComputerUseAdapter for DenyAdapter {
    fn backend_id(&self) -> &'static str {
        "deny-test"
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities::for_surface(SurfaceKind::WebView, "deny-test")
    }
    fn list_targets(&self) -> Result<Vec<TargetInfo>, String> {
        Ok(vec![TargetInfo {
            target_id: "deny:1".into(),
            title: "Denied".into(),
            app_name: "test".into(),
            kind: "window".into(),
            pid: Some(1),
            backend: "deny-test".into(),
            execution_mode: "exclusive".into(),
            replay_policy: "never".into(),
            lifecycle_stamp: 1,
            display_id: "test".into(),
            coordinate_space: "image-pixels".into(),
            scope_label: "unavailable webview".into(),
        }])
    }
    fn target_alive(&self, _target_id: &str) -> bool {
        true
    }
    fn observe(&self, _target_id: &str) -> Result<super::protocol::Observation, String> {
        Err("observe must not run after capability reject".into())
    }
    fn act(
        &self,
        _req: &super::adapter::DispatchRequest,
    ) -> Result<super::adapter::AdapterActResult, String> {
        self.acts.fetch_add(1, Ordering::SeqCst);
        Ok(super::adapter::AdapterActResult {
            applied: true,
            outcome: None,
            postcondition_ok: true,
            verifiable: true,
            detail: "should not dispatch".into(),
        })
    }
    fn abort(&self, _run_id: &str, _generation: u64) -> Result<(), String> {
        Ok(())
    }
    fn is_idle(&self, _run_id: &str) -> bool {
        true
    }
    fn start_periodic_preview(&self, _target_id: &str) {}
    fn stop_periodic_preview(&self) {}
    fn periodic_preview_active(&self) -> bool {
        false
    }
}

#[test]
fn unsupported_action_is_rejected_before_adapter_act() {
    let adapter = Arc::new(DenyAdapter {
        acts: AtomicU64::new(0),
    });
    let instance_id = uuid::Uuid::new_v4().to_string();
    let broker = ComputerUseBroker::new(
        adapter.clone(),
        BrokerOptions {
            instance_id: instance_id.clone(),
            lease_path: std::env::temp_dir()
                .join(format!("grok-cu-contract-{}.lease", instance_id)),
            feature_enabled: true,
            action_timeout: std::time::Duration::from_millis(100),
            ..BrokerOptions::default()
        },
    );
    broker.open_run("owner", "run").unwrap();
    let generation = broker.authorize_target("run", "deny:1").unwrap();
    let outcome = broker.act(ActionRequest {
        version: PROTOCOL_VERSION,
        action_id: "click-1".into(),
        run_id: "run".into(),
        target_id: "deny:1".into(),
        target_generation: generation,
        snapshot_id: "snap".into(),
        geometry_revision: 1,
        action: ActionKind::Click,
        target: ActionTarget::Element {
            element_ref: "n1".into(),
        },
        parameters: serde_json::json!({}),
    });
    assert_eq!(outcome.kind, OutcomeKind::Rejected);
    assert!(!outcome.executed);
    let reason = outcome.reason.unwrap_or_default();
    assert!(
        reason.contains("unsupported action") && reason.contains("not dispatched"),
        "{reason}"
    );
    assert_eq!(adapter.acts.load(Ordering::SeqCst), 0);
}

#[test]
fn fake_desktop_still_allows_semantic_click() {
    let fake = Arc::new(super::fake::FakeAdapter::new());
    let caps = fake.capabilities();
    assert!(caps.allows(
        ActionKind::Click,
        &ActionTarget::Element {
            element_ref: "n1".into()
        }
    ));
    assert!(!caps.allows(
        ActionKind::SetValue,
        &ActionTarget::Coord { x: 1.0, y: 1.0 }
    ));
}
