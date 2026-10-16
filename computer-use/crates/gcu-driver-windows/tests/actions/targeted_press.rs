use super::*;
use gcu_driver_windows::input::{api::Event, fake::FakeInput, InputBackend};
use std::sync::Arc;

fn virtual_button() -> Properties {
    Properties {
        control_type: 50000,
        frame: Some(gcu_protocol::Rect {
            x: 20.,
            y: 30.,
            w: 80.,
            h: 40.,
        }),
        patterns: Patterns {
            invoke: true,
            ..Default::default()
        },
        ..Default::default()
    }
}

#[tokio::test]
async fn native_press_and_confirm_use_accounted_targeted_click_without_invoke_or_activation() {
    for secondary in [false, true] {
        let (source, driver, _, el) = fixture(virtual_button()).await;
        source.use_targeted_press();
        let api = Arc::new(FakeInput::default());
        api.update(|s| s.foreground = 999);
        let backend = Arc::new(InputBackend::new(api.clone()));
        let driver = driver.with_input(backend.clone());
        let req = if secondary {
            DriverAction::SecondaryAction {
                el,
                action: NodeAction::Confirm,
            }
        } else {
            DriverAction::Press { el }
        };
        assert_eq!(
            driver.perform(&req, &Cancel::default()).await,
            Ok(Delivery::TargetedEvent)
        );
        assert!(
            source.action_calls().is_empty(),
            "no UIA Invoke/Legacy fallback"
        );
        let state = api.state();
        assert_eq!(state.foreground, 999);
        assert_eq!(state.activations, 0);
        assert_eq!(state.focuses, 0);
        assert_eq!(state.sent.len(), 3);
        assert!(state
            .sent
            .iter()
            .all(|s| s.delivery == Delivery::TargetedEvent));
        let down = state
            .sent
            .iter()
            .find(|s| matches!(s.event, Event::Button(_, true, ..)))
            .unwrap();
        assert!(down.recorded_before_down);
        assert!(backend.ledger_empty());
        assert_eq!(
            driver.capabilities().background_operation,
            gcu_protocol::capabilities::BackgroundOperation::Partial
        );
        assert!(driver
            .doctor()
            .await
            .issues
            .iter()
            .any(|s| s.contains("Invoke/Legacy can activate")));
    }
}

#[tokio::test]
async fn targeted_press_unsupported_classes_never_escalate_to_uia_or_foreground() {
    for class in ["Chrome_RenderWidgetHostHWND", "Windows.UI.Core.CoreWindow"] {
        let (source, driver, _, el) = fixture(virtual_button()).await;
        source.use_targeted_press();
        let api = Arc::new(FakeInput::default());
        api.update(|s| s.class = class.into());
        let driver = driver.with_input(Arc::new(InputBackend::new(api.clone())));
        assert_eq!(
            driver
                .perform(&DriverAction::Press { el }, &Cancel::default())
                .await,
            Err(DriverError::CapabilityUnavailable)
        );
        assert!(source.action_calls().is_empty());
        assert!(api.state().sent.is_empty());
        assert_eq!(api.state().activations, 0);
    }
}

#[tokio::test]
async fn targeted_press_stale_hidden_or_unpositioned_target_never_sends() {
    for fault in 0..4 {
        let mut props = virtual_button();
        if fault == 0 {
            props.frame = None;
        }
        if fault == 1 {
            props.offscreen = true;
        }
        if fault == 2 {
            props.frame.as_mut().unwrap().x = 1000.;
        }
        let (source, driver, _, el) = fixture(props).await;
        source.use_targeted_press();
        if fault == 3 {
            source.set_runtime_id(1, vec![999]);
        }
        let api = Arc::new(FakeInput::default());
        let driver = driver.with_input(Arc::new(InputBackend::new(api.clone())));
        let error = driver
            .perform(&DriverAction::Press { el }, &Cancel::default())
            .await
            .unwrap_err();
        assert!(!error.maybe_executed());
        assert!(source.action_calls().is_empty());
        assert!(api.state().sent.is_empty());
    }
}

#[tokio::test]
async fn targeted_press_partial_dispatch_releases_up_without_replay_or_invoke() {
    let (source, driver, _, el) = fixture(virtual_button()).await;
    source.use_targeted_press();
    let api = Arc::new(FakeInput::default());
    api.update(|s| {
        s.fail_at = Some(2);
    });
    let backend = Arc::new(InputBackend::new(api.clone()));
    let driver = driver.with_input(backend.clone());
    assert!(driver
        .perform(&DriverAction::Press { el }, &Cancel::default())
        .await
        .unwrap_err()
        .maybe_executed());
    assert!(source.action_calls().is_empty());
    assert_eq!(api.state().activations, 0);
    assert_eq!(
        api.state()
            .sent
            .iter()
            .filter(|s| matches!(s.event, Event::Button(_, true, ..)))
            .count(),
        1
    );
    assert!(backend.ledger_empty());
}

#[tokio::test]
async fn standard_button_message_delivery_is_reported_as_targeted() {
    let mut props = virtual_button();
    props.class_name = Some("Button".into());
    let (source, driver, _, el) = fixture(props).await;
    source.use_targeted_press();
    source.reply(Pattern::Invoke, Ok(PatternResult::TargetedPerformed));
    assert_eq!(
        driver
            .perform(&DriverAction::Press { el }, &Cancel::default())
            .await,
        Ok(Delivery::TargetedEvent)
    );
    assert_eq!(source.action_calls(), [PatternAction::Invoke]);
}

#[tokio::test]
async fn targeted_press_mode_preserves_non_invoke_semantic_patterns() {
    for (control_type, expected) in [
        (50002, PatternAction::Toggle),
        (50007, PatternAction::Select),
        (50023, PatternAction::Expand),
    ] {
        let props = Properties {
            control_type,
            ..all_patterns()
        };
        let (source, driver, _, el) = fixture(props).await;
        source.use_targeted_press();
        assert_eq!(
            driver
                .perform(&DriverAction::Press { el }, &Cancel::default())
                .await,
            Ok(Delivery::Accessibility)
        );
        assert_eq!(source.action_calls(), [expected]);
    }
}

#[tokio::test]
async fn targeted_press_revalidates_exact_element_after_waiting_for_input_preflight() {
    for disabled in [false, true] {
        let (source, driver, _, el) = fixture(virtual_button()).await;
        source.use_targeted_press();
        let api = Arc::new(FakeInput::default());
        api.update(|s| s.resolve_delay = Duration::from_secs(1));
        let driver = driver.with_input(Arc::new(InputBackend::new(api.clone())));
        let task = tokio::spawn(async move {
            driver
                .perform(&DriverAction::Press { el }, &Cancel::default())
                .await
        });
        tokio::time::timeout(Duration::from_secs(3), api.wait_until_resolving())
            .await
            .unwrap();
        assert!(
            api.state().sent.is_empty(),
            "mutation occurs during the queued read"
        );
        if disabled {
            source.update_properties(1, |p| p.enabled = false);
        } else {
            source.set_runtime_id(1, vec![999]);
        }
        assert_eq!(
            task.await.unwrap(),
            Err(if disabled {
                DriverError::ElementNotActionable
            } else {
                DriverError::StaleElement
            })
        );
        assert!(api.state().sent.is_empty());
        assert!(source.action_calls().is_empty());
    }
}
