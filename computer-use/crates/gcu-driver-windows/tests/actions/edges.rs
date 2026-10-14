//! Failure boundaries that must never become a fallback input route.
use super::*;

#[tokio::test]
async fn every_pattern_absent_or_pre_cancelled_has_zero_native_calls() {
    for &pattern in PATTERNS {
        let (source, el) = portable(Properties {
            read_only: Some(false),
            ..Default::default()
        })
        .await;
        let expected = if matches!(pattern, Pattern::Value | Pattern::RangeValue) {
            DriverError::NotSettable
        } else {
            DriverError::ElementNotActionable
        };
        assert_eq!(
            actions::perform(&source, 8, &request(pattern, el).0, &Cancel::default()).await,
            Err(expected)
        );
        assert!(source.action_calls().is_empty());
        let (source, el) = portable(only(pattern)).await;
        let before = source.stats();
        let cancel = Cancel::default();
        cancel.cancel();
        assert_eq!(
            actions::perform(&source, 8, &request(pattern, el).0, &cancel).await,
            Err(DriverError::Cancelled {
                maybe_executed: false
            })
        );
        assert_eq!(source.stats(), before);
        assert!(source.action_calls().is_empty());
    }
}

#[tokio::test]
async fn press_complete_priority_chain_reaches_legacy_only_after_explicit_absence() {
    let (source, el) = portable(all_patterns()).await;
    for pattern in [
        Pattern::Invoke,
        Pattern::Toggle,
        Pattern::SelectionItem,
        Pattern::ExpandCollapse,
    ] {
        source.reply(pattern, Ok(PatternResult::Unsupported));
    }
    assert_eq!(
        actions::perform(&source, 8, &DriverAction::Press { el }, &Cancel::default()).await,
        Ok(Delivery::Accessibility)
    );
    assert_eq!(
        source.action_calls(),
        vec![
            PatternAction::Invoke,
            PatternAction::Toggle,
            PatternAction::Select,
            PatternAction::Expand,
            PatternAction::LegacyDefault
        ]
    );
    for (control_type, first) in [
        (50002, PatternAction::Toggle),
        (50007, PatternAction::Select),
        (50023, PatternAction::Expand),
    ] {
        let (source, el) = portable(Properties {
            control_type,
            ..all_patterns()
        })
        .await;
        assert!(
            actions::perform(&source, 8, &DriverAction::Press { el }, &Cancel::default())
                .await
                .is_ok()
        );
        assert_eq!(source.action_calls(), vec![first]);
    }
}

#[tokio::test]
async fn explicit_secondary_absence_never_runs_an_unrelated_default_action() {
    let (source, el) = portable(all_patterns()).await;
    source.reply(Pattern::Toggle, Ok(PatternResult::Unavailable));
    assert_eq!(
        actions::perform(
            &source,
            8,
            &DriverAction::SecondaryAction {
                el,
                action: NodeAction::Toggle
            },
            &Cancel::default()
        )
        .await,
        Err(DriverError::ElementNotActionable)
    );
    assert_eq!(source.action_calls(), vec![PatternAction::Toggle]);
}

#[tokio::test(start_paused = true)]
async fn set_value_becoming_readonly_during_preparation_has_zero_writes() {
    let (source, el) = portable(all_patterns()).await;
    source.set_action_delay(Duration::from_millis(50), Duration::ZERO);
    let changed = source.clone();
    let trigger = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(10)).await;
        changed.update_properties(1, |p| p.read_only = Some(true));
    });
    assert_eq!(
        actions::perform(
            &source,
            8,
            &DriverAction::SetValue {
                el,
                value: "12".into()
            },
            &Cancel::default()
        )
        .await,
        Err(DriverError::NotSettable)
    );
    trigger.await.unwrap();
    assert!(source.action_calls().is_empty());
}

#[tokio::test]
async fn scroll_fractional_pages_and_late_unavailability_preserve_delivery_count() {
    let (source, el) = portable(only(Pattern::Scroll)).await;
    assert!(actions::perform(
        &source,
        8,
        &DriverAction::ScrollElement {
            el,
            dir: Dir::Left,
            pages: 2.5
        },
        &Cancel::default()
    )
    .await
    .is_ok());
    assert_eq!(
        source.action_calls(),
        vec![
            PatternAction::Scroll {
                dir: Dir::Left,
                pages: 1.
            },
            PatternAction::Scroll {
                dir: Dir::Left,
                pages: 1.
            },
            PatternAction::Scroll {
                dir: Dir::Left,
                pages: 0.5
            }
        ]
    );
    let (source, el) = portable(only(Pattern::Scroll)).await;
    source.reply(Pattern::Scroll, Ok(PatternResult::Performed));
    source.reply(Pattern::Scroll, Ok(PatternResult::Unavailable));
    assert!(actions::perform(
        &source,
        8,
        &DriverAction::ScrollElement {
            el,
            dir: Dir::Down,
            pages: 3.
        },
        &Cancel::default()
    )
    .await
    .unwrap_err()
    .maybe_executed());
    assert_eq!(source.action_calls().len(), 2);
}

#[tokio::test]
async fn pointer_keyboard_and_foreground_remain_unavailable_with_uia_attached() {
    let (source, driver, win, _) = fixture(all_patterns()).await;
    for action in common::actions(&win).into_iter().filter(|a| {
        matches!(
            a,
            DriverAction::PointerClick { .. }
                | DriverAction::PointerScroll { .. }
                | DriverAction::PointerDrag { .. }
                | DriverAction::TypeText { .. }
                | DriverAction::PressKeys { .. }
                | DriverAction::Activate { .. }
        )
    }) {
        assert_eq!(
            driver.perform(&action, &Cancel::default()).await,
            Err(DriverError::CapabilityUnavailable)
        );
    }
    assert!(source.action_calls().is_empty());
    assert!(driver.capabilities().delivery.accessibility);
    assert_eq!(
        driver.capabilities().background_operation,
        gcu_protocol::capabilities::BackgroundOperation::Partial
    );
}

#[tokio::test]
async fn new_observation_invalidates_old_generation_without_rebinding_old_handle() {
    let (source, driver, win, old) = fixture(all_patterns()).await;
    let new = driver
        .snapshot_tree(&win, &TreeOpts::default(), &Cancel::default())
        .await
        .unwrap()
        .root
        .unwrap()
        .handle;
    assert_eq!(
        driver
            .perform(&DriverAction::Press { el: old }, &Cancel::default())
            .await,
        Err(DriverError::StaleElement)
    );
    assert!(source.action_calls().is_empty());
    assert_eq!(
        driver
            .perform(&DriverAction::Press { el: new }, &Cancel::default())
            .await,
        Ok(Delivery::Accessibility)
    );
    assert_eq!(source.action_calls(), vec![PatternAction::Invoke]);
}

#[tokio::test]
async fn ancestor_becoming_execution_surface_refuses_ordinary_descendant_actions() {
    use gcu_driver_windows::uia::{source::NodeId, tree, UiaElement};
    let (_, seed) = portable(all_patterns()).await;
    let window = seed.downcast_ref::<UiaElement>().unwrap().window.clone();
    let source = FakeUia::new(FakeNode {
        properties: Properties::default(),
        children: vec![FakeNode {
            properties: all_patterns(),
            ..Default::default()
        }],
    });
    let tree = tree::build_raw_tree(&source, 8, &TreeOpts::default(), &Cancel::default())
        .await
        .unwrap();
    let root = tree.root.unwrap();
    let node = *root.children[0].handle.downcast_ref::<NodeId>().unwrap();
    source.update_properties(1, |p| p.name = Some("Debug Console REPL".into()));
    let el = NativeHandle::new(UiaElement { window, node });
    for &pattern in PATTERNS {
        assert_eq!(
            actions::perform(
                &source,
                8,
                &request(pattern, el.clone()).0,
                &Cancel::default()
            )
            .await,
            Err(DriverError::ElementNotActionable)
        );
    }
    assert!(source.action_calls().is_empty());
}
