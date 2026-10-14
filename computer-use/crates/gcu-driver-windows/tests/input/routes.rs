use super::*;
use gcu_core::model::NativeHandle;
use gcu_driver_windows::{
    input::api::Event,
    uia::{source::NodeId, UiaElement},
};
#[tokio::test]
async fn activate_authorized_route_verifies_without_pointer_or_key_injection() {
    let (api, driver, win) = fixture().await;
    assert_eq!(
        driver
            .perform(&DriverAction::Activate { win }, &Cancel::default())
            .await,
        Ok(Delivery::Foreground)
    );
    assert_eq!(api.state().activations, 1);
    assert!(api.state().sent.is_empty());
}
#[tokio::test]
async fn scroll_background_and_foreground_preserve_axis_deltas() {
    for route in [PointerRoute::TargetedOnly, PointerRoute::AllowForeground] {
        let (api, driver, win) = fixture().await;
        let req = DriverAction::PointerScroll {
            win,
            at: WindowPoint { x: 20., y: 30. },
            dx: -120,
            dy: 240,
            route,
        };
        let delivery = if route == PointerRoute::AllowForeground {
            Delivery::Foreground
        } else {
            Delivery::TargetedEvent
        };
        assert_eq!(driver.perform(&req, &Cancel::default()).await, Ok(delivery));
        assert_eq!(
            api.state().sent[1].event,
            Event::Wheel(-120, 240, WindowPoint { x: 20., y: 30. })
        );
        assert_eq!(
            api.state().activations,
            usize::from(route == PointerRoute::AllowForeground)
        );
    }
}
#[tokio::test]
async fn foreground_lost_between_packets_returns_unknown_and_stops_input() {
    let (api, driver, win) = fixture().await;
    api.update(|s| s.foreground_after_packet = Some(999));
    assert!(driver
        .perform(
            &click(&win, PointerRoute::AllowForeground),
            &Cancel::default()
        )
        .await
        .unwrap_err()
        .maybe_executed());
    assert_eq!(
        api.state().sent.len(),
        1,
        "no click into the newly foreground window"
    );
}
#[tokio::test]
async fn exact_focus_routes_only_focus_after_foreground_permission() {
    for route in [KeyRoute::TargetedOnly, KeyRoute::AllowForeground] {
        let (api, driver, win) = fixture().await;
        let focus = NativeHandle::new(UiaElement {
            window: win.clone(),
            hwnd: 8,
            runtime_id: vec![1, 1],
            fingerprint: 1,
            node: NodeId {
                generation: 1,
                slot: 1,
            },
        });
        let req = DriverAction::TypeText {
            win,
            focus: Some(focus),
            text: "中".into(),
            route,
        };
        assert!(driver.perform(&req, &Cancel::default()).await.is_ok());
        assert_eq!(
            api.state().focuses,
            usize::from(route == KeyRoute::AllowForeground)
        );
        assert_eq!(
            api.state().activations,
            usize::from(route == KeyRoute::AllowForeground)
        );
    }
}
#[tokio::test]
async fn mismatched_focus_window_refuses_before_any_activation_or_delivery() {
    let (api, driver, win) = fixture().await;
    let mut other = win.clone();
    other.id = "other".into();
    let focus = NativeHandle::new(UiaElement {
        window: other,
        hwnd: 8,
        runtime_id: vec![1, 1],
        fingerprint: 1,
        node: NodeId {
            generation: 1,
            slot: 1,
        },
    });
    let req = DriverAction::TypeText {
        win,
        focus: Some(focus),
        text: "中".into(),
        route: KeyRoute::AllowForeground,
    };
    assert_eq!(
        driver.perform(&req, &Cancel::default()).await,
        Err(DriverError::StaleElement)
    );
    assert_eq!(api.state().activations, 0);
    assert!(api.state().sent.is_empty());
}
#[tokio::test]
async fn common_integrity_gate_precedes_every_input_action_variant() {
    let (api, driver, _, win) = common::fixture().await;
    let input = Arc::new(FakeInput::default());
    let driver = driver.with_input(Arc::new(InputBackend::new(input.clone())));
    api.update(|s| {
        s.processes.get_mut(&7).unwrap().integrity = gcu_driver_windows::gate::IntegrityLevel::High
    });
    for req in common::actions(&win) {
        assert_eq!(
            driver.perform(&req, &Cancel::default()).await,
            Err(DriverError::ElevationRequired)
        );
    }
    assert!(input.state().sent.is_empty());
    assert_eq!(input.state().activations, 0);
}
