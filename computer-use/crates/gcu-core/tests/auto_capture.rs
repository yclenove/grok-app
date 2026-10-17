//! Auto decides delivery only; capture synchronization and redaction remain mandatory.
#![allow(clippy::unwrap_used, clippy::expect_used)]
mod auto_capture_support;
mod support;

use auto_capture_support::*;
use gcu_core::{
    action::ToolCall,
    driver::TreeOpts,
    model::{Incomplete, IncompleteReason, Role, States},
    observe::{AutoCapturePolicy, CaptureEvidence, QualityThresholds, TreeQuality},
    redact::NEUTRAL_COLOR,
    tree::Pipeline,
};
use gcu_protocol::{tools::*, ErrorCode};
use serde_json::json;
use std::sync::atomic::Ordering;
use support::{node, raw};

fn calibrated(minimum: usize) -> AutoCapturePolicy {
    // Synthetic fixture boundary, not a measured production recommendation.
    AutoCapturePolicy::new(QualityThresholds {
        min_interactive_nodes: Some(minimum),
        canvas_role_ratio: None,
    })
}

#[tokio::test]
async fn auto_canvas_or_sparse_tree_attaches_image() {
    let canvas = Rig::new("canvas", calibrated(1));
    assert!(canvas.observe(json!("auto")).await.unwrap().image.is_some());
    let sparse = Rig::new("form", calibrated(1));
    let mut raw = sparse.driver.raw("fake.form").await;
    raw.root.as_mut().unwrap().children.clear();
    sparse.driver.replace(raw);
    let state = sparse.observe(json!("auto")).await.unwrap();
    assert!(state.image.is_some());
    assert!(state.state.screenshot.is_some());
}

#[tokio::test]
async fn auto_uncalibrated_keeps_image_and_calibrated_rich_tree_omits_it() {
    let h = Rig::new("form", AutoCapturePolicy::default());
    assert!(h.observe(json!("auto")).await.unwrap().image.is_some());
    let h = Rig::new("form", calibrated(1));
    let state = h.observe(json!("auto")).await.unwrap();
    assert!(state.image.is_none());
    assert!(state.state.screenshot.is_none());
    assert!(h.driver.captures() > 0);
}

#[tokio::test]
async fn explicit_boolean_or_omitted_mode_preserves_existing_delivery() {
    let h = Rig::new("form", calibrated(usize::MAX));
    let calls = h.driver.captures();
    assert!(h.observe(json!(false)).await.unwrap().image.is_none());
    assert_eq!(h.driver.captures(), calls);
    assert!(h.observe(json!(true)).await.unwrap().image.is_some());
    let h = Rig::new("form", calibrated(1));
    assert!(h.observe_default().await.unwrap().image.is_some());
}

#[test]
fn quality_modal_popup_does_not_borrow_background_controls() {
    let mut popup = node(
        Role::Dialog,
        "Dialog",
        vec![node(Role::StaticText, "Wait", vec![])],
    );
    popup.states |= States::MODAL;
    let input = raw(vec![node(Role::Button, "Background", vec![]), popup]);
    let tree = Pipeline::default().run(input.clone(), &TreeOpts::default());
    let quality = TreeQuality::assess(&input, &tree);
    assert_eq!(quality.popup_count, 1);
    assert_eq!(quality.popup_interactive_nodes, 0);
    assert!(calibrated(1).observation(&quality, CaptureEvidence::default()));

    let input = raw(vec![node(
        Role::Popover,
        "Popup",
        vec![node(Role::Button, "Close", vec![])],
    )]);
    let tree = Pipeline::default().run(input.clone(), &TreeOpts::default());
    let quality = TreeQuality::assess(&input, &tree);
    assert_eq!(quality.popup_count, 1);
    assert_eq!(quality.popup_interactive_nodes, 1);
    assert!(!calibrated(1).observation(&quality, CaptureEvidence::default()));
}

#[test]
fn quality_counts_canvas_share_before_budget_and_only_usable_controls() {
    let mut disabled = node(Role::Button, "Disabled", vec![]);
    disabled.states = States::empty();
    let mut hidden = node(Role::Button, "Hidden", vec![]);
    hidden.states |= States::OFFSCREEN;
    let input = raw(vec![
        node(Role::Canvas, "Drawing", vec![]),
        node(Role::Button, "Save", vec![]),
        disabled,
        hidden,
    ]);
    let tree = Pipeline::default().run(input.clone(), &TreeOpts::default());
    let quality = TreeQuality::assess(&input, &tree);
    assert_eq!(quality.raw_nodes, 5);
    assert_eq!(quality.canvas_nodes, 1);
    assert_eq!(quality.interactive_nodes, 1);
    assert_eq!(quality.canvas_ratio(), 1. / 5.);
    let limited = Pipeline::default().run(
        input.clone(),
        &TreeOpts {
            max_tree_nodes: 1,
            ..TreeOpts::default()
        },
    );
    let quality = TreeQuality::assess(&input, &limited);
    assert_eq!(quality.canvas_nodes, 1);
    assert!(quality.truncated);
    assert!(calibrated(1).observation(&quality, CaptureEvidence::default()));
}

#[test]
fn quality_canvas_ratio_boundary_is_configurable_and_invalid_values_are_conservative() {
    let quality = TreeQuality {
        raw_nodes: 4,
        canvas_nodes: 1,
        interactive_nodes: 2,
        ..TreeQuality::default()
    };
    let policy = |ratio| {
        AutoCapturePolicy::new(QualityThresholds {
            min_interactive_nodes: Some(1),
            canvas_role_ratio: Some(ratio),
        })
    };
    assert!(policy(quality.canvas_ratio()).observation(&quality, CaptureEvidence::default()));
    assert!(!policy(quality.canvas_ratio() + f64::EPSILON)
        .observation(&quality, CaptureEvidence::default()));
    for ratio in [f64::NAN, f64::INFINITY, -1., 0., 2.] {
        assert!(policy(ratio).observation(&quality, CaptureEvidence::default()));
    }
}

#[test]
fn quality_interactive_boundary_uses_supplied_fixture_threshold() {
    let input = raw(vec![node(Role::Button, "Only control", vec![])]);
    let tree = Pipeline::default().run(input.clone(), &TreeOpts::default());
    let quality = TreeQuality::assess(&input, &tree);
    assert_eq!(quality.interactive_nodes, 1);
    assert!(
        !calibrated(quality.interactive_nodes).observation(&quality, CaptureEvidence::default())
    );
    assert!(
        calibrated(quality.interactive_nodes + 1).observation(&quality, CaptureEvidence::default())
    );
}

#[tokio::test]
async fn auto_empty_diff_with_changed_safe_pixels_attaches_image() {
    let h = Rig::new("form", calibrated(1));
    h.observe(json!(true)).await.unwrap();
    h.driver.paint.store(true, Ordering::SeqCst);
    let state = h.observe(json!("auto")).await.unwrap();
    assert!(state.image.is_some());
    assert!(state.text.contains("Unchanged: 6 nodes"));
    assert!(!state
        .text
        .lines()
        .any(|line| line.starts_with(['+', '-', '~'])));
    assert!(h.observe(json!("auto")).await.unwrap().image.is_none());
    assert!(h.observe(json!("auto")).await.unwrap().image.is_none());
}

#[tokio::test]
async fn auto_omitted_speculative_image_does_not_replace_last_delivered_image() {
    let h = Rig::new("form", calibrated(1));
    h.observe(json!(true)).await.unwrap();
    let mut raw = h.driver.raw("fake.form").await;
    raw.root.as_mut().unwrap().children.last_mut().unwrap().name = Some("Updated".into());
    h.driver.replace(raw);
    h.driver.paint.store(true, Ordering::SeqCst);
    assert!(h.observe(json!("auto")).await.unwrap().image.is_none());
    assert!(h.observe(json!("auto")).await.unwrap().image.is_some());
}

#[tokio::test]
async fn auto_unknown_sensitive_region_refuses_image_even_when_tree_is_rich() {
    let h = Rig::new("login", calibrated(1));
    let mut raw = h.driver.raw("fake.login").await;
    raw.root.as_mut().unwrap().children[1].frame = None;
    h.driver.replace(raw);
    for mode in [json!("auto"), json!(true)] {
        let error = h.observe(mode).await.unwrap_err();
        assert_eq!(error.code, ErrorCode::HandoffRequired);
        assert!(error.state.is_none());
    }
    let state = h.observe(json!(false)).await.unwrap();
    assert!(state.image.is_none());
    assert!(state.text.contains("Value: •••"));
}

#[tokio::test]
async fn auto_incomplete_before_or_after_capture_requires_handoff() {
    for after_capture in [false, true] {
        for reason in [
            IncompleteReason::Deadline,
            IncompleteReason::NodeCap,
            IncompleteReason::DepthCap,
            IncompleteReason::PartialError,
        ] {
            let h = Rig::new("form", calibrated(1));
            let mut raw = h.driver.raw("fake.form").await;
            raw.incomplete = Some(Incomplete {
                reason,
                read_nodes: 6,
                unread_nodes: None,
            });
            if after_capture {
                h.driver.replace_after_capture(raw);
            } else {
                h.driver.replace(raw);
            }
            let error = h.observe(json!("auto")).await.unwrap_err();
            assert_eq!(error.code, ErrorCode::HandoffRequired);
            assert!(error.state.is_none());
            if !after_capture {
                assert_eq!(h.driver.captures(), 0);
            }
        }
    }
}

#[tokio::test]
async fn auto_tree_capture_state_mismatch_does_not_deliver_an_image() {
    let h = Rig::new("form", calibrated(1));
    let mut raw = h.driver.raw("fake.form").await;
    raw.root.as_mut().unwrap().name = Some("Changed during capture".into());
    h.driver.replace_after_capture(raw);
    let error = h.observe(json!("auto")).await.unwrap_err();
    assert_eq!(error.code, ErrorCode::StaleScreenshot);
    assert!(error.state.is_none());
}

#[tokio::test]
async fn auto_geometry_change_during_capture_does_not_deliver_an_image() {
    use gcu_core::driver::Driver;
    use gcu_driver_fake::faults::Fault;
    let h = Rig::new("form", calibrated(1));
    let win = h.driver.inner.fixture_window("fake.form").await.unwrap();
    let mut geometry = h.driver.window_geometry(&win).await.unwrap();
    geometry.frame.x += 1.;
    h.driver
        .inner
        .inject(Fault::Delay(std::time::Duration::ZERO))
        .await
        .unwrap();
    h.driver
        .inner
        .inject(Fault::GeometryChanged {
            window: win.id,
            geometry,
        })
        .await
        .unwrap();
    let error = h.observe(json!("auto")).await.unwrap_err();
    assert_eq!(error.code, ErrorCode::StaleGeometry);
    assert!(error.state.is_none());
}

#[tokio::test]
async fn auto_image_and_all_derivatives_keep_password_redaction() {
    let h = Rig::new("login", AutoCapturePolicy::default());
    let state = h.observe(json!("auto")).await.unwrap();
    let image = state.image.unwrap();
    // The fake renderer paints a conspicuous magenta marker before core redaction.
    let offset = ((100 * image.width() + 30) * 4) as usize;
    assert_eq!(&image.pixels()[offset..offset + 4], &NEUTRAL_COLOR);
    let thumbnail = image.thumbnail(400).unwrap();
    assert!(thumbnail
        .pixels()
        .chunks_exact(4)
        .any(|pixel| pixel == NEUTRAL_COLOR));
    assert!(!image
        .pixels()
        .chunks_exact(4)
        .any(|pixel| pixel == gcu_driver_fake::render::PASSWORD_COLOR));
    let encoded = image
        .encode(gcu_core::redact::EncodeOptions::default())
        .unwrap();
    let decoded = image::load_from_memory(encoded.bytes()).unwrap().to_rgba8();
    assert_eq!(decoded.get_pixel(30, 100).0, NEUTRAL_COLOR);
    let crop = image
        .approval_crop(gcu_core::model::Rect {
            x: 25.,
            y: 95.,
            w: 10.,
            h: 10.,
        })
        .unwrap();
    assert!(crop
        .pixels()
        .chunks_exact(4)
        .all(|pixel| pixel == NEUTRAL_COLOR));
}

#[tokio::test]
async fn auto_password_only_pixel_changes_do_not_trigger_attachment() {
    let h = Rig::new("login", calibrated(1));
    h.observe(json!(true)).await.unwrap();
    h.driver.paint_password.store(true, Ordering::SeqCst);
    let state = h.observe(json!("auto")).await.unwrap();
    assert!(state.image.is_none());
    assert!(!state
        .text
        .lines()
        .any(|line| line.starts_with(['+', '-', '~'])));
}

#[tokio::test]
async fn auto_budget_removed_password_still_redacts_its_raw_region() {
    let h = Rig::new("login", calibrated(1));
    let state = h
        .router
        .observe(
            &h.session,
            serde_json::from_value(json!({
                "app":"fake.login", "include_screenshot":"auto", "max_tree_nodes":1
            }))
            .unwrap(),
        )
        .await
        .unwrap();
    assert!(state.state.truncated.is_some());
    let image = state.image.unwrap();
    let offset = ((100 * image.width() + 30) * 4) as usize;
    assert_eq!(&image.pixels()[offset..offset + 4], &NEUTRAL_COLOR);
}

fn unchanged_action() -> ToolCall {
    let mut call = ToolCall::new(
        "auto",
        "fake.form",
        BatchStep::SetValue(SetValueArgs {
            element_index: 1,
            value: String::new(),
        }),
    );
    // No timing assertion: a zero settle budget still compares final safe evidence.
    call.settle_budget_ms = Some(0);
    call
}

#[tokio::test]
async fn action_empty_rendered_diff_with_changed_pixels_attaches_despite_raw_change() {
    let h = Rig::new("form", calibrated(1));
    let mut before = h.driver.raw("fake.form").await;
    before.root.as_mut().unwrap().children[0].description = Some(format!("{}old", "x".repeat(501)));
    h.driver.replace(before.clone());
    h.observe(json!(true)).await.unwrap();
    let mut after = before;
    after.root.as_mut().unwrap().children[0].description = Some(format!("{}new", "x".repeat(501)));
    h.driver.replace_after_action(after);
    h.driver.paint_on_action.store(true, Ordering::SeqCst);
    let out = h.router.execute(&h.session, unchanged_action()).await;
    assert!(out.error.is_none(), "{:?}", out.error);
    assert_eq!(out.action.effect, gcu_protocol::result::Effect::Changed);
    let state = out.state.unwrap();
    assert!(state.image.is_some());
    assert!(!state
        .text
        .lines()
        .any(|line| line.starts_with(['+', '-', '~'])));
}

#[tokio::test]
async fn action_explicit_attachment_overrides_rich_tree_omission() {
    let h = Rig::new("form", calibrated(1));
    h.observe(json!(true)).await.unwrap();
    let out = h.router.execute(&h.session, unchanged_action()).await;
    assert!(out.error.is_none());
    assert!(out.state.unwrap().image.is_none());
    let mut call = unchanged_action();
    call.attach_image = true;
    let out = h.router.execute(&h.session, call).await;
    assert!(out.error.is_none());
    assert!(out.state.unwrap().image.is_some());
}

#[tokio::test]
async fn action_coordinate_request_attaches_safe_image() {
    let h = Rig::new("form", calibrated(1));
    h.observe(json!(true)).await.unwrap();
    let mut call = ToolCall::new(
        "coordinate",
        "fake.form",
        BatchStep::Click(ClickArgs {
            element_index: None,
            x: Some(80.),
            y: Some(145.),
            screenshot_id: None,
            click_count: 1,
            mouse_button: MouseButton::Left,
        }),
    );
    call.settle_budget_ms = Some(0);
    let out = h.router.execute(&h.session, call).await;
    assert!(out.error.is_none(), "{:?}", out.error);
    assert!(out.state.unwrap().image.is_some());
}
