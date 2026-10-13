use crate::router_support::*;
use gcu_core::{
    action::{gate::screenshot_point, ToolCall},
    model::{Geometry, NodeValue, Rect},
};
use gcu_driver_fake::{faults::Fault, FakeDriver};
use gcu_protocol::{result::Outcome, tools::*, ErrorCode};

#[tokio::test]
async fn coordinate_moved_or_scaled_window_returns_stale_geometry() {
    for scale in [1., 2.] {
        let h = Harness::new(true, true);
        h.observe().await;
        h.driver
            .inject(Fault::GeometryChanged {
                window: "form-window".into(),
                geometry: Geometry {
                    frame: Rect {
                        x: 101.,
                        y: 100.,
                        w: 800.,
                        h: 480.,
                    },
                    scale,
                },
            })
            .await
            .unwrap();
        let out = h.execute(point(80., 145.)).await;
        assert_eq!(out.error.as_ref().unwrap().code, ErrorCode::StaleGeometry);
        assert!(out.error.unwrap().state.is_some());
        assert_eq!(h.driver.perform_calls(), 0);
    }
}
#[tokio::test]
async fn screenshot_old_id_or_missing_observation_returns_stale_screenshot() {
    let h = Harness::new(true, true);
    assert_eq!(
        h.execute(point(80., 145.)).await.error.unwrap().code,
        ErrorCode::StaleScreenshot
    );
    let first = h.observe().await;
    h.observe().await;
    let mut step = point(80., 145.);
    if let BatchStep::Click(a) = &mut step {
        a.screenshot_id = Some(first.state.screenshot.unwrap().id);
    }
    let out = h.execute(step).await;
    assert_eq!(out.error.as_ref().unwrap().code, ErrorCode::StaleScreenshot);
    assert!(out.error.unwrap().state.is_some());
    assert_eq!(h.driver.perform_calls(), 0);
}
#[tokio::test]
async fn coordinate_same_geometry_replacement_returns_stale_screenshot() {
    let h = Harness::new(true, true);
    h.observe().await;
    h.driver
        .inject(Fault::NodeChanged {
            window: "form-window".into(),
            identifier: "submit".into(),
            name: "Pay someone else".into(),
            value: None,
        })
        .await
        .unwrap();
    // The small patch at the button's center need not contain its label: the
    // coordinate's native target identity must also remain unchanged.
    let out = h.execute(point(80., 145.)).await;
    assert_eq!(out.error.as_ref().unwrap().code, ErrorCode::StaleScreenshot);
    assert!(out.state.unwrap().text.contains("Pay someone else"));
    assert_eq!(h.driver.perform_calls(), 0);
}
#[tokio::test]
async fn coordinate_changed_region_hash_returns_stale_screenshot() {
    let h = Harness::new(true, true);
    h.observe().await;
    // Same role/name/frame/handle/fingerprint, different pixels in the 32x32 patch.
    h.driver
        .inject(Fault::NodeChanged {
            window: "form-window".into(),
            identifier: "text".into(),
            name: "Name".into(),
            value: Some(NodeValue::Text("DIFFERENT".into())),
        })
        .await
        .unwrap();
    let out = h.execute(point(56., 34.)).await;
    assert_eq!(out.error.as_ref().unwrap().code, ErrorCode::StaleScreenshot);
    assert!(out.error.unwrap().state.is_some());
    assert_eq!(h.driver.perform_calls(), 0);
}
#[tokio::test]
async fn element_reused_handle_changed_name_returns_stale_element() {
    let h = Harness::new(true, true);
    h.observe().await;
    h.driver
        .inject(Fault::NodeChanged {
            window: "form-window".into(),
            identifier: "submit".into(),
            name: "Other recipient".into(),
            value: None,
        })
        .await
        .unwrap();
    let out = h.execute(click(3)).await;
    assert_eq!(out.error.as_ref().unwrap().code, ErrorCode::StaleElement);
    assert!(out.state.unwrap().text.contains("Other recipient"));
    assert_eq!(h.driver.perform_calls(), 0);
}
#[tokio::test]
async fn disappeared_window_lists_remaining_without_switching() {
    let h = Harness::driver(FakeDriver::contract_fixture().unwrap());
    h.observe().await;
    h.driver
        .inject(Fault::WindowGone {
            window: "form-window".into(),
        })
        .await
        .unwrap();
    let out = h.execute(click(3)).await;
    assert_eq!(out.error.as_ref().unwrap().code, ErrorCode::WindowGone);
    assert_eq!(out.error.unwrap().state.unwrap().window.id, "form-window");
    assert!(out.windows.iter().any(|w| w.id != "form-window"));
    assert_eq!(h.driver.perform_calls(), 0);
    assert!(h.driver.oracle().await.unwrap().foreground.is_none());
}
#[tokio::test]
async fn typed_driver_gates_return_zero_effect() {
    use gcu_core::driver::DriverError;
    for e in [
        DriverError::ElevationRequired,
        DriverError::PermissionMissing {
            permission: "accessibility".into(),
        },
        DriverError::SecureDesktop,
        DriverError::WindowMinimized,
    ] {
        let h = Harness::new(true, true);
        h.observe().await;
        h.driver.inject(Fault::Error(e.clone())).await.unwrap();
        let out = h.execute(click(3)).await;
        assert_eq!(out.error.unwrap().code, e.error_code());
        assert_eq!(out.action.outcome, Outcome::Refused);
        assert_eq!(h.driver.perform_calls(), 0);
        assert!(h.driver.oracle().await.unwrap().presses.is_empty());
    }
}
#[tokio::test]
async fn dangerous_schemes_and_metadata_block_every_input_path() {
    for input in [
        "javascript:evil()",
        " FILE:/secret",
        "data:text/html,evil",
        "vbscript:evil",
        "blob:evil",
        "mailto:somebody",
        "http://169.254.169.254/latest",
        "http://metadata.google.internal",
    ] {
        let h = Harness::new(true, true);
        h.observe().await;
        for step in [text(input), set(1, input)] {
            assert_eq!(
                h.execute(step).await.error.unwrap().code,
                ErrorCode::KeyBlocked
            );
        }
        assert_eq!(h.driver.perform_calls(), 0);
    }
}
#[tokio::test]
async fn focused_address_bar_refuses_text_value_and_submit() {
    let source = gcu_driver_fake::scene::builtin("form")
        .unwrap()
        .replace("Some(\"Name\")", "Some(\"Address\")");
    let h = Harness::driver(FakeDriver::from_ron(&source).unwrap());
    h.observe().await;
    for step in [
        text("ordinary text"),
        set(1, "ordinary text"),
        key("Return"),
    ] {
        assert_eq!(
            h.execute(step).await.error.unwrap().code,
            ErrorCode::KeyBlocked
        );
    }
    assert_eq!(h.driver.perform_calls(), 0);
}
#[tokio::test]
async fn invalid_parameters_and_unknown_index_do_not_deliver() {
    let h = Harness::new(true, true);
    h.observe().await;
    assert_eq!(
        h.execute(click(999)).await.error.unwrap().code,
        ErrorCode::StaleElement
    );
    assert_eq!(
        h.execute(key("not+a+key")).await.error.unwrap().code,
        ErrorCode::InvalidKey
    );
    let mut step = point(5., 6.);
    if let BatchStep::Click(a) = &mut step {
        a.element_index = Some(1);
    }
    assert_eq!(
        h.execute(step).await.error.unwrap().code,
        ErrorCode::CapabilityUnavailable
    );
    assert_eq!(h.driver.perform_calls(), 0);
}
#[test]
fn screenshot_mapping_scales_and_rejects_nonfinite_or_edge_points() {
    let at = screenshot_point(10., 20., 100, 100, 2.).unwrap();
    assert_eq!((at.x, at.y), (20., 40.));
    for (x, y, s) in [
        (f64::NAN, 0., 1.),
        (0., f64::INFINITY, 1.),
        (100., 0., 1.),
        (0., 100., 1.),
        (0., 0., 0.),
        (-1., 0., 1.),
    ] {
        assert_eq!(
            screenshot_point(x, y, 100, 100, s),
            Err(ErrorCode::OutOfBounds)
        );
    }
}
#[tokio::test]
async fn downscaled_screenshot_coordinates_map_to_window_pixels() {
    use gcu_host_api::PolicyStore;
    let h = Harness::new(true, true);
    let mut config = h.policy.load();
    config.max_image_edge = 400;
    h.policy.stage(config);
    h.router.policy_changed();
    let state = h.observe().await;
    assert_eq!(state.state.screenshot.unwrap().scale_to_window, Some(2.));
    let out = h.execute(point(40., 72.5)).await;
    assert_eq!(out.action.outcome, Outcome::Done);
    assert_eq!(h.driver.oracle().await.unwrap().presses["submit"], 1);
}
#[tokio::test]
async fn explicit_window_id_never_uses_key_window() {
    let h = Harness::new(true, true);
    h.observe().await;
    let mut call = ToolCall::new("wrong-window", "fake.form", click(3));
    call.window = Some("missing".into());
    let out = h.router.execute(&h.session, call).await;
    assert_eq!(out.error.as_ref().unwrap().code, ErrorCode::WindowGone);
    assert!(out.error.unwrap().state.is_some());
    assert_eq!(h.driver.perform_calls(), 0);
}
