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

/// Hardware finding (click-matrix4, 2026-10-21): egui/winit drops a click when the first in-window
/// pointer move lands on the target after the pointer arrived from outside the window. A first
/// move one pixel away, a short dwell, then the target move restores clicks.
#[tokio::test]
async fn foreground_click_enters_the_window_with_an_offset_move_and_dwell() {
    let (api, driver, win) = fixture().await;
    assert_eq!(
        driver
            .perform(
                &click(&win, PointerRoute::AllowForeground),
                &Cancel::default()
            )
            .await,
        Ok(Delivery::Foreground)
    );
    let sent = api.state().sent;
    let moves: Vec<_> = sent
        .iter()
        .filter_map(|s| match s.event {
            Event::Move(p, false) => Some((p, s.at)),
            _ => None,
        })
        .collect();
    assert_eq!(moves.len(), 2, "approach move then target move");
    let (approach, first) = moves[0];
    let (target, second) = moves[1];
    assert_eq!(
        (approach.x - target.x)
            .abs()
            .max((approach.y - target.y).abs()),
        1.,
        "approach is exactly one pixel from the target"
    );
    assert!(
        second - first >= std::time::Duration::from_millis(30),
        "dwell between approach and target: {:?}",
        second - first
    );
    assert!(matches!(sent[0].event, Event::Move(..)));
    assert!(matches!(sent[1].event, Event::Move(..)));
    assert!(matches!(sent[2].event, Event::Button(_, true, p, _) if p == target));
    assert!(matches!(sent[3].event, Event::Button(_, false, p, _) if p == target));
}
#[tokio::test]
async fn targeted_click_and_scroll_keep_their_single_move() {
    let (api, driver, win) = fixture().await;
    driver
        .perform(&click(&win, PointerRoute::TargetedOnly), &Cancel::default())
        .await
        .unwrap();
    assert_eq!(
        api.state()
            .sent
            .iter()
            .filter(|s| matches!(s.event, Event::Move(..)))
            .count(),
        1
    );
}
