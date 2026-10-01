//! Named S3.4 tests: typed actions, visible-image coords, no desktop fallback.
use super::gates::{click_req, coord_req, enabled_opts};
use super::*;
use crate::adapter::{ActionScope, DispatchRequest};
use crate::fake::FakeAdapter;
use crate::protocol::{ActionKind, ActionTarget, OutcomeKind, PROTOCOL_VERSION};
use std::sync::Arc;

fn ready(
    png: bool,
) -> (
    ComputerUseBroker,
    Arc<FakeAdapter>,
    String,
    u64,
    Observation,
) {
    let fake = Arc::new(FakeAdapter::new());
    fake.set_include_png(png);
    let broker = ComputerUseBroker::new(fake.clone(), enabled_opts(400));
    broker.open_run("owner", "run").unwrap();
    let target = fake.fixture_id();
    let gen = broker.authorize_target("run", &target).unwrap();
    let obs = broker.observe("run").unwrap();
    (broker, fake, target, gen, obs)
}

// Advertise a semantic source like the production managed-browser adapter.
// The normal FakeAdapter only advertises coordinate drag and masks this gate.
struct SemanticDragAdapter(Arc<FakeAdapter>);
impl ComputerUseAdapter for SemanticDragAdapter {
    fn backend_id(&self) -> &'static str {
        "semantic-drag-test"
    }
    fn capabilities(&self) -> crate::adapter::Capabilities {
        let mut caps = self.0.capabilities();
        caps.drag = crate::adapter::ActionSupport::BOTH;
        caps
    }
    fn list_targets(&self) -> Result<Vec<crate::adapter::TargetInfo>, String> {
        self.0.list_targets()
    }
    fn target_alive(&self, id: &str) -> bool {
        self.0.target_alive(id)
    }
    fn observe(&self, id: &str) -> Result<Observation, String> {
        self.0.observe(id)
    }
    fn act(&self, req: &DispatchRequest) -> Result<crate::adapter::AdapterActResult, String> {
        self.0.act(req)
    }
    fn abort(&self, id: &str, generation: u64) -> Result<(), String> {
        self.0.abort(id, generation)
    }
    fn is_idle(&self, id: &str) -> bool {
        self.0.is_idle(id)
    }
    fn start_periodic_preview(&self, id: &str) {
        self.0.start_periodic_preview(id)
    }
    fn stop_periodic_preview(&self) {
        self.0.stop_periodic_preview()
    }
    fn periodic_preview_active(&self) -> bool {
        self.0.periodic_preview_active()
    }
}

fn wait_req(
    target: &str,
    gen: u64,
    snap: &str,
    geo: u64,
    id: &str,
    name: &str,
    timeout_ms: u64,
) -> ActionRequest {
    ActionRequest {
        version: PROTOCOL_VERSION,
        action_id: id.into(),
        run_id: "run".into(),
        target_id: target.into(),
        target_generation: gen,
        snapshot_id: snap.into(),
        geometry_revision: geo,
        action: ActionKind::Wait,
        target: ActionTarget::Element {
            element_ref: "n1".into(),
        },
        parameters: serde_json::json!({"nameEquals": name, "timeoutMs": timeout_ms}),
    }
}

#[test]
fn wait_is_a_typed_action_and_does_not_dispatch_desktop_input() {
    let (broker, fake, target, gen, obs) = ready(true);
    let out = broker.act(wait_req(
        &target,
        gen,
        &obs.snapshot_id,
        obs.geometry_revision,
        "wait-hit",
        "Count",
        400,
    ));
    assert_eq!(out.kind, OutcomeKind::Verified, "{:?}", out.reason);
    assert!(out.executed);
    assert!(fake.executions().is_empty());
    assert_eq!(fake.desktop_fallback(), 0);
}

#[test]
fn wait_timeout_does_not_advance_snapshot_or_execute() {
    let (broker, fake, target, gen, obs) = ready(true);
    let before = obs.snapshot_id.clone();
    let out = broker.act(wait_req(
        &target,
        gen,
        &obs.snapshot_id,
        obs.geometry_revision,
        "wait-miss",
        "Never",
        80,
    ));
    assert_eq!(out.kind, OutcomeKind::Rejected);
    assert!(!out.executed);
    assert!(out.reason.unwrap_or_default().contains("timed out"));
    assert_eq!(
        broker.model_snapshot_id("run").as_deref(),
        Some(before.as_str())
    );
    assert!(fake.executions().is_empty());
    assert_eq!(fake.desktop_fallback(), 0);
}

#[test]
fn standalone_wait_uses_observed_identity_and_cannot_evade_attempt_budget() {
    let (mut broker, fake, _, _, observation) = ready(true);
    broker.observe_budget = 2; // Initial observe + one wait attempt.
    let binding = crate::ipc::SessionBinding {
        session_id: "owner".into(),
        run_id: "run".into(),
    };
    let mut args = super::test_support::wait_args(&broker, "run", "Never", 1);
    let stale = {
        let mut old = args.clone();
        old["snapshotId"] = serde_json::json!("not-observed");
        crate::tools::dispatch(&broker, &binding, "computer_wait", old)
    };
    assert_eq!(stale["isError"], true);
    assert_eq!(fake.observe_calls(), 1);
    let miss = crate::tools::dispatch(&broker, &binding, "computer_wait", args.clone());
    assert_eq!(miss["isError"], true);
    assert!(miss.to_string().contains("timed out"));
    let calls = fake.observe_calls();
    let replay = crate::tools::dispatch(&broker, &binding, "computer_wait", args.clone());
    assert_eq!(replay, miss);
    assert_eq!(fake.observe_calls(), calls);
    args["actionId"] = serde_json::json!("new-wait-after-budget");
    let exhausted = crate::tools::dispatch(&broker, &binding, "computer_wait", args);
    assert_eq!(exhausted["isError"], true);
    assert!(exhausted.to_string().contains("budget exhausted"));
    assert_eq!(fake.observe_calls(), calls);
    assert_eq!(
        broker.model_snapshot_id("run"),
        Some(observation.snapshot_id)
    );
    assert!(fake.executions().is_empty());
}

#[test]
fn standalone_wait_rejects_missing_foreign_and_null_identity_without_capture() {
    let (broker, fake, _, _, _) = ready(true);
    let binding = crate::ipc::SessionBinding {
        session_id: "owner".into(),
        run_id: "run".into(),
    };
    let valid = super::test_support::wait_args(&broker, "run", "Count", 80);
    for field in [
        "snapshotId",
        "actionId",
        "version",
        "targetGeneration",
        "geometryRevision",
    ] {
        let mut args = valid.clone();
        args.as_object_mut().unwrap().remove(field);
        assert_eq!(
            crate::tools::dispatch(&broker, &binding, "computer_wait", args)["isError"],
            true,
            "{field}"
        );
    }
    for (key, value) in [
        ("runId", serde_json::json!("other-run")),
        ("timeoutMs", serde_json::Value::Null),
        ("targetId", serde_json::json!("other-target")),
    ] {
        let mut args = valid.clone();
        args[key] = value;
        assert_eq!(
            crate::tools::dispatch(&broker, &binding, "computer_wait", args)["isError"],
            true,
            "{key}"
        );
    }
    assert_eq!(fake.observe_calls(), 1);
    assert!(fake.executions().is_empty());
}

#[test]
fn coordinate_action_without_current_visible_image_is_rejected() {
    let (broker, fake, target, gen, obs) = ready(false);
    let mut req = coord_req(
        "run",
        &target,
        gen,
        &obs.snapshot_id,
        obs.geometry_revision,
        "blind-coord",
    );
    req.target = ActionTarget::Coord { x: 40.0, y: 20.0 };
    let out = broker.act(req);
    assert_eq!(out.kind, OutcomeKind::Rejected);
    assert!(!out.executed);
    assert!(out
        .reason
        .unwrap_or_default()
        .contains("visual observation"));
    assert!(fake.executions().is_empty());
    assert_eq!(fake.desktop_fallback(), 0);
}

#[test]
fn semantic_drag_source_does_not_authorize_a_blind_pixel_destination() {
    let fake = Arc::new(FakeAdapter::new());
    fake.set_include_png(false);
    let broker = ComputerUseBroker::new(
        Arc::new(SemanticDragAdapter(fake.clone())),
        enabled_opts(400),
    );
    broker.open_run("owner", "run").unwrap();
    let target = fake.fixture_id();
    let gen = broker.authorize_target("run", &target).unwrap();
    let obs = broker.observe("run").unwrap();
    let mut req = click_req(
        "run",
        &target,
        gen,
        &obs.snapshot_id,
        obs.geometry_revision,
        "blind-drag",
    );
    req.action = ActionKind::Drag;
    req.parameters = serde_json::json!({"toX":1,"toY":1});
    let out = broker.act(req);
    assert_eq!(out.kind, OutcomeKind::Rejected);
    assert!(!out.executed);
    assert!(
        out.reason
            .as_deref()
            .unwrap_or_default()
            .contains("visual observation"),
        "{:?}",
        out.reason
    );
    assert!(fake.executions().is_empty());
}

#[test]
fn conflicting_drag_aliases_never_reach_the_adapter() {
    let (broker, fake, target, gen, obs) = ready(true);
    let mut req = coord_req(
        "run",
        &target,
        gen,
        &obs.snapshot_id,
        obs.geometry_revision,
        "ambiguous-drag",
    );
    req.action = ActionKind::Drag;
    req.parameters = serde_json::json!({"toX":1,"toY":1,"x1":999999,"y1":999999});
    let out = broker.act(req);
    assert_eq!(out.kind, OutcomeKind::Rejected);
    assert!(!out.executed);
    assert!(fake.executions().is_empty());
}

#[test]
fn coordinate_action_with_stale_geometry_is_rejected() {
    let (broker, fake, target, gen, obs) = ready(true);
    fake.bump_geometry();
    let mut req = coord_req(
        "run",
        &target,
        gen,
        &obs.snapshot_id,
        obs.geometry_revision,
        "stale-geo-coord",
    );
    req.target = ActionTarget::Coord { x: 40.0, y: 20.0 };
    let out = broker.act(req);
    assert_eq!(out.kind, OutcomeKind::Rejected);
    assert!(!out.executed);
    assert!(fake.executions().is_empty());
    assert_eq!(fake.desktop_fallback(), 0);
}

#[test]
fn semantic_action_is_preferred_when_coordinate_hits_an_element() {
    let (broker, fake, target, gen, obs) = ready(true);
    let out = broker.act(coord_req(
        "run",
        &target,
        gen,
        &obs.snapshot_id,
        obs.geometry_revision,
        "hit-n1",
    ));
    assert_ne!(out.kind, OutcomeKind::Rejected, "{:?}", out.reason);
    assert_eq!(fake.executions(), ["hit-n1"]);
    assert_eq!(
        fake.last_dispatch_target(),
        Some(ActionTarget::Element {
            element_ref: "n1".into()
        })
    );
    assert_eq!(fake.last_dispatch_scope(), Some(ActionScope::Directed));
    assert_eq!(fake.last_dispatch_target_id(), target);
}

#[test]
fn semantic_hit_uses_specific_action_and_smallest_unambiguous_control() {
    let node = |name: &str, size: f64, actions: &[&str]| crate::protocol::ObservationNode {
        node_ref: name.into(),
        x: Some(0.0),
        y: Some(0.0),
        width: Some(size),
        height: Some(size),
        actions: actions.iter().map(|s| s.to_string()).collect(),
        ..Default::default()
    };
    let nodes = [
        node("root", 500.0, &[]),
        node("editable", 40.0, &["set_value"]),
        node("panel", 100.0, &["click"]),
        node("button", 40.0, &["click"]),
    ];
    assert_eq!(
        super::actions::semantic_hit(&nodes, ActionKind::Click, 10.0, 10.0)
            .unwrap()
            .node_ref,
        "button"
    );
    assert_eq!(
        super::actions::semantic_hit(&nodes, ActionKind::SetValue, 10.0, 10.0)
            .unwrap()
            .node_ref,
        "editable"
    );
    assert!(super::actions::semantic_hit(&nodes, ActionKind::Key, 10.0, 10.0).is_none());
    let tied = [
        node("first", 40.0, &["click"]),
        node("second", 40.0, &["click"]),
    ];
    assert!(super::actions::semantic_hit(&tied, ActionKind::Click, 10.0, 10.0).is_none());
}

#[test]
fn non_primary_coordinate_clicks_are_not_changed_to_single_semantic_clicks() {
    for parameters in [
        serde_json::json!({"count":2}),
        serde_json::json!({"button":"right"}),
    ] {
        let (broker, fake, target, gen, obs) = ready(true);
        let mut req = coord_req(
            "run",
            &target,
            gen,
            &obs.snapshot_id,
            obs.geometry_revision,
            "exact-click",
        );
        req.parameters = parameters;
        let target = req.target.clone();
        assert!(broker.act(req).executed);
        assert_eq!(fake.last_dispatch_target(), Some(target));
    }
}

#[test]
fn adapter_fallback_stays_on_same_target_and_never_uses_desktop_scope() {
    let (broker, fake, target, gen, obs) = ready(true);
    fake.set_fail_semantic(true);
    let out = broker.act(click_req(
        "run",
        &target,
        gen,
        &obs.snapshot_id,
        obs.geometry_revision,
        "sem-fail",
    ));
    assert_ne!(out.kind, OutcomeKind::Rejected, "{:?}", out.reason);
    assert_eq!(fake.executions(), ["sem-fail"]);
    assert_eq!(fake.semantic_fallback(), 1);
    assert_eq!(fake.last_dispatch_scope(), Some(ActionScope::Directed));
    assert_eq!(fake.last_dispatch_target_id(), target);
    assert_eq!(fake.desktop_fallback(), 0);
}

#[test]
fn dead_target_does_not_fallback_to_desktop_scope() {
    let (broker, fake, target, gen, obs) = ready(true);
    fake.set_alive(false);
    let out = broker.act(click_req(
        "run",
        &target,
        gen,
        &obs.snapshot_id,
        obs.geometry_revision,
        "dead-click",
    ));
    assert_eq!(out.kind, OutcomeKind::Rejected);
    assert!(!out.executed);
    assert!(fake.executions().is_empty());
    assert_eq!(fake.desktop_fallback(), 0);
}

#[test]
fn adapter_revalidates_identity_and_cancellation_before_side_effect() {
    let fake = FakeAdapter::new();
    let target = fake.fixture_id();
    let mut req = DispatchRequest {
        managed_request: None,
        cancellation: Default::default(),
        run_id: "run".into(),
        action_id: "stale".into(),
        generation: 1,
        target_id: target.clone(),
        target_generation: 1,
        snapshot_id: "s1".into(),
        geometry_revision: 1,
        action: ActionKind::Click,
        target: ActionTarget::Coord { x: 40.0, y: 20.0 },
        parameters: serde_json::json!({}),
        scope: ActionScope::Directed,
    };
    fake.bump_geometry();
    let err = fake.act(&req).unwrap_err();
    assert!(err.contains("geometry"), "{err}");
    assert!(fake.executions().is_empty());

    req.geometry_revision = fake.current_geometry_revision();
    req.cancellation.cancel();
    let err = fake.act(&req).unwrap_err();
    assert!(err.contains("cancel"), "{err}");
    assert!(fake.executions().is_empty());
    assert_eq!(fake.desktop_fallback(), 0);
}

#[test]
fn first_version_typed_actions_all_run_on_the_same_directed_target() {
    let (broker, fake, target, gen, obs) = ready(true);
    let snap = obs.snapshot_id.clone();
    let geo = obs.geometry_revision;
    let mut rows: Vec<(ActionRequest, bool)> = vec![
        (click_req("run", &target, gen, &snap, geo, "a-click"), true),
        (
            {
                let mut req = click_req("run", &target, gen, &snap, geo, "a-set");
                req.action = ActionKind::SetValue;
                req.parameters = serde_json::json!({"text": "hi"});
                req
            },
            true,
        ),
        (
            {
                let mut req = click_req("run", &target, gen, &snap, geo, "a-type");
                req.action = ActionKind::TypeText;
                req.parameters = serde_json::json!({"text": "hi"});
                req
            },
            true,
        ),
        (
            {
                let mut req = click_req("run", &target, gen, &snap, geo, "a-key");
                req.action = ActionKind::Key;
                req.parameters = serde_json::json!({"key": "enter"});
                req
            },
            true,
        ),
        (
            {
                let mut req = coord_req("run", &target, gen, &snap, geo, "a-scroll");
                req.action = ActionKind::Scroll;
                req.target = ActionTarget::Coord { x: 40.0, y: 20.0 };
                req.parameters = serde_json::json!({"delta": -120});
                req
            },
            true,
        ),
        (
            {
                let mut req = coord_req("run", &target, gen, &snap, geo, "a-drag");
                req.action = ActionKind::Drag;
                req.target = ActionTarget::Coord { x: 40.0, y: 20.0 };
                req.parameters = serde_json::json!({"toX": 50.0, "toY": 20.0});
                req
            },
            true,
        ),
        (
            wait_req(&target, gen, &snap, geo, "a-wait", "Count", 200),
            false,
        ),
    ];
    let mut executed = Vec::new();
    for (req, hits_adapter) in rows.drain(..) {
        let id = req.action_id.clone();
        let out = broker.act(req);
        assert_ne!(out.kind, OutcomeKind::Rejected, "{id} {:?}", out.reason);
        if hits_adapter {
            executed.push(id);
        }
    }
    assert_eq!(
        fake.executions(),
        executed,
        "wait must not count as desktop input"
    );
    assert_eq!(fake.desktop_fallback(), 0);
    assert_eq!(fake.last_dispatch_target_id(), target);
    assert_eq!(fake.last_dispatch_scope(), Some(ActionScope::Directed));
}

#[test]
fn browser_navigate_and_download_remain_typed_model_tools() {
    assert!(crate::tools::MODEL_TOOLS.contains(&"computer_navigate"));
    assert!(crate::tools::MODEL_TOOLS.contains(&"computer_download"));
    assert!(crate::tools::MODEL_TOOLS.contains(&"computer_wait"));
    assert!(crate::tools::MODEL_TOOLS.contains(&"computer_stop"));
    assert!(!crate::tools::MODEL_TOOLS.contains(&"computer_resume"));
    assert!(crate::browser::is_allowed_navigate_url("javascript:alert(1)").is_err());
}
