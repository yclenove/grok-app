//! Portable delivery contracts; native checks are explicitly ignored.
#![allow(clippy::unwrap_used, clippy::expect_used)]
mod common;
use gcu_core::{driver::*, model::WindowPoint};
use gcu_driver_windows::input::{fake::FakeInput, InputBackend};
use std::sync::Arc;

async fn fixture() -> (Arc<FakeInput>, gcu_driver_windows::WindowsDriver, WindowRef) {
    let (_, driver, _, win) = common::fixture().await;
    let api = Arc::new(FakeInput::default());
    let backend = Arc::new(InputBackend::new(api.clone()));
    (
        api,
        driver.with_input(backend).with_safety(common::fake_guard()),
        win,
    )
}
fn click(win: &WindowRef, route: PointerRoute) -> DriverAction {
    DriverAction::PointerClick {
        win: win.clone(),
        at: WindowPoint { x: 20., y: 30. },
        button: Button::Left,
        count: 1,
        route,
    }
}
#[tokio::test]
async fn targeted_click_child_hwnd_never_activates() {
    let (api, driver, win) = fixture().await;
    assert_eq!(
        driver
            .perform(&click(&win, PointerRoute::TargetedOnly), &Cancel::default())
            .await,
        Ok(Delivery::TargetedEvent)
    );
    let state = api.state();
    assert_eq!(state.sent.len(), 3);
    assert!(state
        .sent
        .iter()
        .all(|s| s.target == 9 && s.delivery == Delivery::TargetedEvent));
    assert_eq!(state.activations, 0);
    assert_eq!(state.samples, 3);
}
#[tokio::test]
async fn chromium_uwp_background_routes_refuse_without_escalation() {
    for class in ["Chrome_RenderWidgetHostHWND", "Windows.UI.Core.CoreWindow"] {
        for route in [PointerRoute::Auto, PointerRoute::TargetedOnly] {
            let (api, driver, win) = fixture().await;
            api.update(|s| s.class = class.into());
            assert_eq!(
                driver
                    .perform(&click(&win, route), &Cancel::default())
                    .await,
                Err(DriverError::CapabilityUnavailable)
            );
            assert!(api.state().sent.is_empty());
            assert_eq!(api.state().activations, 0);
        }
    }
}
#[tokio::test]
async fn chromium_foreground_approved_route_reports_foreground() {
    let (api, driver, win) = fixture().await;
    api.update(|s| s.class = "Chrome_RenderWidgetHostHWND".into());
    assert_eq!(
        driver
            .perform(
                &click(&win, PointerRoute::AllowForeground),
                &Cancel::default()
            )
            .await,
        Ok(Delivery::Foreground)
    );
    assert_eq!(api.state().activations, 1);
}
#[tokio::test]
async fn foreground_activation_unverified_sends_no_input() {
    let (api, driver, win) = fixture().await;
    api.update(|s| s.activate_succeeds = false);
    let error = driver
        .perform(
            &click(&win, PointerRoute::AllowForeground),
            &Cancel::default(),
        )
        .await
        .unwrap_err();
    assert!(error.maybe_executed()); // Activation itself was attempted.
    assert!(api.state().sent.is_empty());
}
#[path = "input/edges.rs"]
mod edges;
#[cfg(windows)]
#[path = "input/native.rs"]
mod native;
#[path = "input/routes.rs"]
mod routes;
