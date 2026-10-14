//! Native omissions must never certify a screenshot as safely redacted.
#![allow(clippy::unwrap_used, clippy::expect_used)]
mod router_support;
mod support;

use gcu_core::{
    driver::TreeOpts,
    model::{Incomplete, IncompleteReason, NodeValue, Role, States},
    redact::{self, CapturedFrame, RedactionError},
    render::{render_structured, render_text},
    tree::Pipeline,
};
use gcu_driver_fake::FakeDriver;
use gcu_protocol::ErrorCode;
use router_support::Harness;
use support::{node, opts, raw};

fn incomplete(reason: IncompleteReason) -> Incomplete {
    Incomplete {
        reason,
        read_nodes: 2,
        unread_nodes: None,
    }
}

#[tokio::test]
async fn screenshot_incomplete_login_or_ordinary_tree_requires_handoff() {
    for scene in ["login", "form", "canvas"] {
        for reason in [
            IncompleteReason::Deadline,
            IncompleteReason::NodeCap,
            IncompleteReason::PartialError,
            IncompleteReason::DepthCap,
        ] {
            let h = Harness::driver(
                FakeDriver::from_scene(scene)
                    .unwrap()
                    .with_incomplete_tree(incomplete(reason)),
            );
            let args =
                serde_json::from_value(serde_json::json!({"app":format!("fake.{scene}")})).unwrap();
            let error = h.router.observe(&h.session, args).await.unwrap_err();
            assert_eq!(error.code, ErrorCode::HandoffRequired);
            assert!(error.state.is_none());
        }
    }
}

#[tokio::test]
async fn text_only_incomplete_login_delivers_tree_and_truncation() {
    let fact = incomplete(IncompleteReason::NodeCap);
    let h = Harness::driver(
        FakeDriver::from_scene("login")
            .unwrap()
            .with_incomplete_tree(fact),
    );
    let args =
        serde_json::from_value(serde_json::json!({"app":"fake.login", "include_screenshot":false}))
            .unwrap();
    let state = h.router.observe(&h.session, args).await.unwrap();
    assert!(state.image.is_none());
    assert!(state.state.screenshot.is_none());
    assert!(state.text.contains("[truncated: native tree incomplete"));
    assert!(state.text.contains("Value: •••"));
    assert_eq!(state.state.truncated.unwrap().native_incomplete, Some(fact));
    let args =
        serde_json::from_value(serde_json::json!({"app":"fake.login", "include_screenshot":false}))
            .unwrap();
    let diff = h.router.observe(&h.session, args).await.unwrap();
    assert!(diff.text.contains("Changes since last state"));
    assert!(diff.text.contains("[truncated: native tree incomplete"));
    assert!(diff.image.is_none());
}

#[tokio::test]
async fn screenshot_complete_login_keeps_existing_safe_delivery() {
    let h = Harness::driver(FakeDriver::from_scene("login").unwrap());
    let args = serde_json::from_value(serde_json::json!({"app":"fake.login"})).unwrap();
    let state = h.router.observe(&h.session, args).await.unwrap();
    assert!(state.image.is_some());
    assert!(state.state.screenshot.is_some());
    assert!(state.state.truncated.is_none());
    assert!(state.text.contains("Value: •••"));
}

#[tokio::test]
async fn screenshot_recheck_incomplete_tree_requires_handoff() {
    use gcu_driver_fake::faults::Fault;
    let h = Harness::driver(FakeDriver::from_scene("login").unwrap());
    // A complete first tree, then capture, then an incomplete verification tree.
    h.driver
        .inject(Fault::Delay(std::time::Duration::ZERO))
        .await
        .unwrap();
    h.driver
        .inject(Fault::Delay(std::time::Duration::ZERO))
        .await
        .unwrap();
    h.driver
        .inject(Fault::IncompleteTree(incomplete(
            IncompleteReason::Deadline,
        )))
        .await
        .unwrap();
    let args = serde_json::from_value(serde_json::json!({"app":"fake.login"})).unwrap();
    let error = h.router.observe(&h.session, args).await.unwrap_err();
    assert_eq!(error.code, ErrorCode::HandoffRequired);
    assert!(error.state.is_none());
}

#[test]
fn render_native_omission_is_separate_from_budget_omission() {
    let fact = incomplete(IncompleteReason::PartialError);
    for cap in [0, 1, 1200] {
        let mut input = raw(vec![node(Role::Button, "Save", vec![])]);
        input.incomplete = Some(fact);
        let tree = Pipeline::default().run(
            input,
            &TreeOpts {
                max_tree_nodes: cap,
                ..TreeOpts::default()
            },
        );
        assert!(!tree.sensitive_regions_reliable);
        let text = render_text(&tree, &opts());
        assert!(text.contains("[truncated: native tree incomplete"));
        let state: gcu_protocol::AppState =
            serde_json::from_value(render_structured(&tree, &opts())).unwrap();
        let truncation = state.truncated.unwrap();
        assert_eq!(truncation.omitted_nodes as usize, tree.omitted_nodes);
        assert_eq!(truncation.native_incomplete, Some(fact));
    }
}

#[test]
fn redact_incomplete_tree_even_with_known_password_region_requires_handoff() {
    let mut password = node(Role::TextField, "Password", vec![]);
    password.states |= States::PASSWORD;
    password.frame = Some(gcu_core::model::Rect {
        x: 0.,
        y: 0.,
        w: 20.,
        h: 20.,
    });
    password.value = Some(NodeValue::Text("secret".into()));
    let mut input = raw(vec![password]);
    input.incomplete = Some(incomplete(IncompleteReason::Deadline));
    let mut tree = Pipeline::default().run(input, &TreeOpts::default());
    tree.state_version = Some(1);
    tree.observed_at = Some(std::time::Duration::ZERO);
    // Defense in depth: a manually assembled tree cannot override this fact.
    tree.sensitive_regions_reliable = true;
    let capture = gcu_core::driver::Capture {
        bytes: vec![],
        width: 20,
        height: 20,
        mime: "image/png".into(),
        geometry: gcu_core::model::Geometry {
            frame: password_rect(),
            scale: 1.,
        },
    };
    assert_eq!(
        redact::apply(
            &tree,
            CapturedFrame::new(capture, 1, std::time::Duration::ZERO)
        )
        .unwrap_err(),
        RedactionError::NeedsHandoff
    );
}

fn password_rect() -> gcu_core::model::Rect {
    gcu_core::model::Rect {
        x: 0.,
        y: 0.,
        w: 20.,
        h: 20.,
    }
}

#[test]
fn render_incomplete_overflow_keeps_native_marker_inside_transport_cap() {
    let children = (0..1200)
        .map(|i| node(Role::Button, &format!("{i} {}", "界".repeat(600)), vec![]))
        .collect();
    let mut input = raw(children);
    input.incomplete = Some(Incomplete {
        reason: IncompleteReason::NodeCap,
        read_nodes: 1201,
        unread_nodes: Some(7),
    });
    let tree = Pipeline::default().run(input, &TreeOpts::default());
    let text = render_text(&tree, &opts());
    assert!(text.len() <= gcu_core::render::text::FACTS_PREFIX_BYTES);
    assert!(text.contains(
        "[truncated: native tree incomplete (node_cap); 1201 nodes read; 7 nodes unread]"
    ));
}

#[test]
fn raw_tree_default_and_legacy_truncation_remain_complete() {
    assert!(gcu_core::model::RawTree::default().incomplete.is_none());
    let legacy = serde_json::json!({"omitted_nodes":3});
    let value: gcu_protocol::types::Truncation = serde_json::from_value(legacy.clone()).unwrap();
    assert!(value.native_incomplete.is_none());
    assert_eq!(serde_json::to_value(value).unwrap(), legacy);
}
