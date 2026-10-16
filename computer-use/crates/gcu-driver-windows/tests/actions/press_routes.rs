use super::*;
use gcu_driver_windows::input::{api::Event, fake::FakeInput, InputBackend};
use std::sync::Arc;

#[tokio::test]
async fn virtual_press_and_confirm_use_invoke_or_legacy_without_pointer_input() {
    for pattern in [Pattern::Invoke, Pattern::Legacy] {
        for secondary in [false, true] {
            // Virtual elements need no child HWND or pointer bounds for semantics.
            let (source, driver, _, el) = fixture(only(pattern)).await;
            let api = Arc::new(FakeInput::default());
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
                Ok(Delivery::Accessibility)
            );
            let expected = if pattern == Pattern::Invoke {
                PatternAction::Invoke
            } else {
                PatternAction::LegacyDefault
            };
            assert_eq!(source.action_calls(), [expected]);
            assert!(api.state().sent.is_empty());
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
                .any(|s| s.contains("UIA Invoke/Legacy may activate the target window")));
        }
    }
}

fn targeted(win: WindowRef) -> DriverAction {
    DriverAction::PointerClick {
        win,
        at: gcu_core::model::WindowPoint { x: 30., y: 40. },
        button: Button::Left,
        count: 1,
        route: PointerRoute::TargetedOnly,
    }
}

#[tokio::test]
async fn explicit_targeted_click_unsupported_classes_never_use_uia_or_foreground() {
    for class in ["Chrome_RenderWidgetHostHWND", "Windows.UI.Core.CoreWindow"] {
        let (source, driver, win, _) = fixture(only(Pattern::Invoke)).await;
        let api = Arc::new(FakeInput::default());
        api.update(|s| s.class = class.into());
        let driver = driver.with_input(Arc::new(InputBackend::new(api.clone())));
        assert_eq!(
            driver.perform(&targeted(win), &Cancel::default()).await,
            Err(DriverError::CapabilityUnavailable)
        );
        assert!(source.action_calls().is_empty());
        assert!(api.state().sent.is_empty());
        assert_eq!(api.state().activations, 0);
    }
}

#[tokio::test]
async fn explicit_targeted_click_partial_dispatch_releases_without_replay_or_invoke() {
    let (source, driver, win, _) = fixture(only(Pattern::Invoke)).await;
    let api = Arc::new(FakeInput::default());
    api.update(|s| s.fail_at = Some(2));
    let backend = Arc::new(InputBackend::new(api.clone()));
    let driver = driver.with_input(backend.clone());
    assert!(driver
        .perform(&targeted(win), &Cancel::default())
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
    let mut props = only(Pattern::Invoke);
    props.class_name = Some("Button".into());
    let (source, driver, _, el) = fixture(props).await;
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
async fn virtual_press_revalidates_after_pattern_preparation_without_pointer_fallback() {
    for disabled in [false, true] {
        let (source, driver, _, el) = fixture(only(Pattern::Invoke)).await;
        let observed_calls = source.stats().0;
        source.set_action_delay(Duration::from_millis(200), Duration::ZERO);
        let task = tokio::spawn(async move {
            driver
                .perform(&DriverAction::Press { el }, &Cancel::default())
                .await
        });
        // Wait for the action's validation read before changing the live target.
        tokio::time::timeout(Duration::from_secs(3), async {
            while source.stats().0 == observed_calls {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        if disabled {
            source.update_properties(1, |p| p.enabled = false);
        } else {
            source.set_runtime_id(1, vec![999]);
        }
        let error = task.await.unwrap().unwrap_err();
        assert!(!error.maybe_executed());
        assert!(source.action_calls().is_empty());
    }
}
