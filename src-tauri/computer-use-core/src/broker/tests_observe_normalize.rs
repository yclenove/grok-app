//! Named tests for Host-normalized observations.
use super::gates::{click_req, enabled_opts};
use super::*;
use crate::adapter::ComputerUseAdapter;
use crate::fake::FakeAdapter;
use crate::protocol::{ActionTarget, OutcomeKind, COORDINATE_SPACE_IMAGE_PIXELS};
use std::sync::Arc;

fn ready() -> (ComputerUseBroker, Arc<FakeAdapter>, String, u64) {
    let fake = Arc::new(FakeAdapter::new());
    fake.set_include_png(true);
    let broker = ComputerUseBroker::new(fake.clone(), enabled_opts(400));
    broker.open_run("owner", "run").unwrap();
    let target = fake.fixture_id();
    let gen = broker.authorize_target("run", &target).unwrap();
    (broker, fake, target, gen)
}

#[test]
fn observe_canonicalizes_coordinate_space_to_image_pixels() {
    let (broker, _, _, _) = ready();
    let obs = broker.observe("run").unwrap();
    assert_eq!(obs.coordinate_space, COORDINATE_SPACE_IMAGE_PIXELS);
    assert_eq!(obs.geometry_revision, obs.topology_revision);
    assert_eq!(obs.crop_width, obs.image.width);
    assert_eq!(obs.crop_height, obs.image.height);
    assert!(obs.scale > 0.0 && obs.scale.is_finite());
    assert!(obs.dpi > 0.0 && obs.dpi.is_finite());
}

#[test]
fn observe_image_and_ax_share_target_and_revision() {
    let (broker, fake, target, _) = ready();
    let obs = broker.observe("run").unwrap();
    assert_eq!(obs.target_id, target);
    assert_eq!(obs.geometry_revision, fake.current_geometry_revision());
    assert_eq!(obs.topology_revision, fake.current_geometry_revision());
    assert!(!obs.nodes.is_empty());
    assert_eq!(obs.nodes[0].node_ref, "n1");
}

#[test]
fn observe_rejects_stale_capture_revision() {
    let fake = Arc::new(FakeAdapter::new());
    fake.set_include_png(true);
    fake.set_stale_observe_revision(true);
    let broker = ComputerUseBroker::new(fake.clone(), enabled_opts(400));
    broker.open_run("owner", "run").unwrap();
    broker.authorize_target("run", &fake.fixture_id()).unwrap();
    let err = broker.observe("run").unwrap_err();
    assert!(
        err.to_string().contains("observation revision") || err.to_string().contains("identity"),
        "{err}"
    );
}

#[test]
fn truncated_node_cannot_be_acted() {
    let (broker, fake, target, gen) = ready();
    fake.set_truncate_ref("n1");
    let obs = broker.observe("run").unwrap();
    assert!(obs.nodes.iter().any(|n| n.node_ref == "n1" && n.truncated));
    let mut req = click_req(
        "run",
        &target,
        gen,
        &obs.snapshot_id,
        obs.geometry_revision,
        "trunc",
    );
    req.target = ActionTarget::Element {
        element_ref: "n1".into(),
    };
    let out = broker.act(req);
    assert_eq!(out.kind, OutcomeKind::Rejected);
    assert!(!out.executed);
    assert!(fake.executions().is_empty());
}

#[test]
fn preview_snapshot_does_not_pollute_model_stream() {
    let (broker, fake, target, gen) = ready();
    let model = broker.observe("run").unwrap();
    let preview = broker.observe_preview("run").unwrap();
    assert_ne!(preview.snapshot_id, model.snapshot_id);
    let mut req = click_req(
        "run",
        &target,
        gen,
        &preview.snapshot_id,
        preview.geometry_revision,
        "from-preview",
    );
    req.target = ActionTarget::Element {
        element_ref: "n1".into(),
    };
    let out = broker.act(req);
    assert_eq!(out.kind, OutcomeKind::Rejected);
    assert!(fake.executions().is_empty());
    let ok = broker.act(click_req(
        "run",
        &target,
        gen,
        &model.snapshot_id,
        model.geometry_revision,
        "from-model",
    ));
    assert_ne!(ok.kind, OutcomeKind::Rejected);
    assert_eq!(fake.executions(), ["from-model"]);
}
