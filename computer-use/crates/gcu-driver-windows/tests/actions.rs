//! Portable UIA semantic contracts. No real desktop or account access.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use gcu_core::{
    driver::*,
    model::{NativeHandle, NodeAction, TextRange},
};
use gcu_driver_windows::uia::{
    actions,
    cache::{Patterns, Properties},
    events::FakeUiaEvents,
    patterns::{press_patterns, Pattern, PatternAction, PatternResult},
    source::{FakeNode, FakeUia},
};
use std::time::Duration;
mod common;
#[path = "actions/fixtures.rs"]
mod fixtures;
use fixtures::*;

#[test]
fn priority_control_types_use_semantically_equivalent_patterns_only() {
    assert_eq!(
        press_patterns(50000),
        &[
            Pattern::Invoke,
            Pattern::Toggle,
            Pattern::SelectionItem,
            Pattern::ExpandCollapse,
            Pattern::Legacy
        ]
    );
    assert_eq!(press_patterns(50002)[0], Pattern::Toggle);
    assert_eq!(press_patterns(50007)[0], Pattern::SelectionItem);
    assert_eq!(press_patterns(50023)[0], Pattern::ExpandCollapse);
}

#[tokio::test]
async fn press_explicit_unsupported_and_unavailable_fall_back_in_order() {
    let (source, driver, _, el) = fixture(all_patterns()).await;
    source.reply(Pattern::Invoke, Ok(PatternResult::Unsupported));
    source.reply(Pattern::Toggle, Ok(PatternResult::Unavailable));
    assert_eq!(
        driver
            .perform(&DriverAction::Press { el }, &Cancel::default())
            .await
            .unwrap(),
        Delivery::Accessibility
    );
    assert_eq!(
        source.action_calls(),
        vec![
            PatternAction::Invoke,
            PatternAction::Toggle,
            PatternAction::Select
        ]
    );
}

#[tokio::test]
async fn press_unknown_never_retries_or_upgrades() {
    let (source, driver, _, el) = fixture(all_patterns()).await;
    let error = DriverError::Timeout {
        maybe_executed: true,
    };
    source.reply(Pattern::Invoke, Err(error.clone()));
    assert_eq!(
        driver
            .perform(&DriverAction::Press { el }, &Cancel::default())
            .await
            .unwrap_err(),
        error
    );
    assert_eq!(source.action_calls(), vec![PatternAction::Invoke]);
}

#[tokio::test]
async fn set_value_readonly_or_password_refuses_without_pattern_calls() {
    for properties in [
        Properties {
            read_only: Some(true),
            ..all_patterns()
        },
        Properties {
            password: true,
            ..all_patterns()
        },
    ] {
        let (source, driver, _, el) = fixture(properties).await;
        assert_eq!(
            driver
                .perform(
                    &DriverAction::SetValue {
                        el,
                        value: "fixture".into()
                    },
                    &Cancel::default()
                )
                .await
                .unwrap_err(),
            DriverError::NotSettable
        );
        assert!(source.action_calls().is_empty());
    }
}

#[tokio::test]
async fn every_pattern_success_unsupported_unavailable_and_errors_delivers_at_most_once() {
    let results = [
        Ok(PatternResult::Performed),
        Ok(PatternResult::Unsupported),
        Ok(PatternResult::Unavailable),
        Err(DriverError::StaleElement),
        Err(DriverError::Timeout {
            maybe_executed: false,
        }),
        Err(DriverError::Timeout {
            maybe_executed: true,
        }),
        Err(DriverError::Cancelled {
            maybe_executed: false,
        }),
        Err(DriverError::Cancelled {
            maybe_executed: true,
        }),
        Err(DriverError::Crashed {
            maybe_executed: true,
        }),
        Err(DriverError::Native {
            message: "provider".into(),
            maybe_executed: false,
        }),
        Err(DriverError::Native {
            message: "provider".into(),
            maybe_executed: true,
        }),
    ];
    for &pattern in PATTERNS {
        for reply in &results {
            let (source, el) = portable(only(pattern)).await;
            let (req, call) = request(pattern, el);
            source.reply(pattern, reply.clone());
            let result = actions::perform(&source, 8, &req, &Cancel::default()).await;
            let absent = if matches!(pattern, Pattern::Value | Pattern::RangeValue) {
                DriverError::NotSettable
            } else {
                DriverError::ElementNotActionable
            };
            let expected = match reply {
                Ok(PatternResult::Performed) => Ok(Delivery::Accessibility),
                Ok(_) => Err(absent),
                Err(e) => Err(e.clone()),
            };
            assert_eq!(result, expected, "{pattern:?} / {reply:?}");
            assert_eq!(source.action_calls(), vec![call], "{pattern:?}");
        }
    }
}

#[tokio::test(start_paused = true)]
async fn every_pattern_real_deadline_distinguishes_preparation_from_dispatch() {
    for &pattern in PATTERNS {
        for dispatched in [false, true] {
            let (source, el) = portable(only(pattern)).await;
            let (req, _) = request(pattern, el);
            let long = Duration::from_secs(10);
            source.set_action_delay(
                if dispatched { Duration::ZERO } else { long },
                if dispatched { long } else { Duration::ZERO },
            );
            let start = tokio::time::Instant::now();
            assert_eq!(
                actions::perform(&source, 8, &req, &Cancel::default()).await,
                Err(DriverError::Timeout {
                    maybe_executed: dispatched
                })
            );
            assert_eq!(tokio::time::Instant::now() - start, DRIVER_DEADLINE);
            assert_eq!(source.action_calls().len(), usize::from(dispatched));
            tokio::time::advance(long).await;
            assert_eq!(source.action_calls().len(), usize::from(dispatched));
        }
    }
}

#[tokio::test(start_paused = true)]
async fn every_pattern_stop_distinguishes_preparation_from_dispatch_without_late_writes() {
    for &pattern in PATTERNS {
        for dispatched in [false, true] {
            let (source, el) = portable(only(pattern)).await;
            let (req, _) = request(pattern, el);
            let long = Duration::from_secs(10);
            source.set_action_delay(
                if dispatched { Duration::ZERO } else { long },
                if dispatched { long } else { Duration::ZERO },
            );
            let cancel = Cancel::default();
            let stop = cancel.clone();
            let trigger = tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(10)).await;
                stop.cancel();
            });
            assert_eq!(
                actions::perform(&source, 8, &req, &cancel).await,
                Err(DriverError::Cancelled {
                    maybe_executed: dispatched
                })
            );
            trigger.await.unwrap();
            tokio::time::advance(long).await;
            assert_eq!(source.action_calls().len(), usize::from(dispatched));
        }
    }
}

#[tokio::test]
async fn every_pattern_stale_identity_or_hwnd_refuses_without_finding_similar_element() {
    for &pattern in PATTERNS {
        for change in 0..6 {
            let (source, el) = portable(only(pattern)).await;
            match change {
                0 => source.set_runtime_id(1, vec![999]),
                1 => source.set_action_hwnd(999),
                2 => source.update_properties(1, |p| p.name = Some("replaced".into())),
                3 => source.update_properties(1, |p| p.control_type = 50004),
                4 => source.update_properties(1, |p| {
                    p.frame = Some(gcu_core::model::Rect {
                        x: 1.,
                        y: 2.,
                        w: 3.,
                        h: 4.,
                    })
                }),
                _ => source.update_properties(1, |p| p.identifier = Some("replaced".into())),
            }
            assert_eq!(
                actions::perform(&source, 8, &request(pattern, el).0, &Cancel::default()).await,
                Err(DriverError::StaleElement)
            );
            assert!(source.action_calls().is_empty());
        }
    }
}

#[tokio::test(start_paused = true)]
async fn identity_changes_during_preparation_are_rechecked_before_mutation() {
    let (source, el) = portable(all_patterns()).await;
    source.set_action_delay(Duration::from_millis(50), Duration::ZERO);
    let changed = source.clone();
    let trigger = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(10)).await;
        changed.set_runtime_id(1, vec![99]);
    });
    assert_eq!(
        actions::perform(&source, 8, &DriverAction::Press { el }, &Cancel::default()).await,
        Err(DriverError::StaleElement)
    );
    trigger.await.unwrap();
    assert!(source.action_calls().is_empty());
}

#[tokio::test]
async fn set_value_missing_writability_sensitive_and_partial_metadata_refuses() {
    for props in [
        Properties {
            read_only: None,
            ..all_patterns()
        },
        Properties {
            patterns: Patterns::default(),
            ..all_patterns()
        },
        Properties {
            class_name: Some("Terminal".into()),
            ..all_patterns()
        },
        Properties {
            partial: true,
            ..all_patterns()
        },
        Properties {
            enabled: false,
            ..all_patterns()
        },
    ] {
        let (source, el) = portable(props).await;
        assert_eq!(
            actions::perform(
                &source,
                8,
                &DriverAction::SetValue {
                    el,
                    value: "secret".into()
                },
                &Cancel::default()
            )
            .await,
            Err(DriverError::NotSettable)
        );
        assert!(source.action_calls().is_empty());
    }
}

#[tokio::test]
async fn value_numeric_fallback_requires_definite_no_effect_and_finite_number() {
    let (source, el) = portable(all_patterns()).await;
    source.reply(Pattern::Value, Ok(PatternResult::Unsupported));
    assert_eq!(
        actions::perform(
            &source,
            8,
            &DriverAction::SetValue {
                el: el.clone(),
                value: "12".into()
            },
            &Cancel::default()
        )
        .await,
        Ok(Delivery::Accessibility)
    );
    assert_eq!(
        source.action_calls(),
        vec![
            PatternAction::SetValue("12".into()),
            PatternAction::SetRangeValue(12.)
        ]
    );
    for value in ["NaN", "inf", "letters"] {
        let (source, el) = portable(only(Pattern::RangeValue)).await;
        assert_eq!(
            actions::perform(
                &source,
                8,
                &DriverAction::SetValue {
                    el,
                    value: value.into()
                },
                &Cancel::default()
            )
            .await,
            Err(DriverError::NotSettable)
        );
        assert!(source.action_calls().is_empty());
    }
    let (source, el) = portable(all_patterns()).await;
    source.reply(
        Pattern::Value,
        Err(DriverError::Timeout {
            maybe_executed: true,
        }),
    );
    assert!(actions::perform(
        &source,
        8,
        &DriverAction::SetValue {
            el,
            value: "12".into()
        },
        &Cancel::default()
    )
    .await
    .unwrap_err()
    .maybe_executed());
    assert_eq!(
        source.action_calls(),
        vec![PatternAction::SetValue("12".into())]
    );
}

#[tokio::test]
async fn scroll_partial_progress_followed_by_failure_is_unknown_without_replay() {
    let (source, el) = portable(only(Pattern::Scroll)).await;
    source.reply(Pattern::Scroll, Ok(PatternResult::Performed));
    source.reply(Pattern::Scroll, Err(DriverError::StaleElement));
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
async fn secondary_explicit_commands_never_substitute_focus_or_legacy_default() {
    for (action, native) in [
        (NodeAction::Collapse, PatternAction::Collapse),
        (NodeAction::Increment, PatternAction::AdjustRange(true)),
        (NodeAction::Decrement, PatternAction::AdjustRange(false)),
    ] {
        let (source, el) = portable(all_patterns()).await;
        assert!(actions::perform(
            &source,
            8,
            &DriverAction::SecondaryAction { el, action },
            &Cancel::default()
        )
        .await
        .is_ok());
        assert_eq!(source.action_calls(), vec![native]);
    }
    for action in [
        NodeAction::Raise,
        NodeAction::ShowMenu,
        NodeAction::Cancel,
        NodeAction::Native("SetFocus".into()),
    ] {
        let (source, el) = portable(all_patterns()).await;
        assert_eq!(
            actions::perform(
                &source,
                8,
                &DriverAction::SecondaryAction { el, action },
                &Cancel::default()
            )
            .await,
            Err(DriverError::ElementNotActionable)
        );
        assert!(source.action_calls().is_empty());
    }
    assert!(!include_str!("../src/uia/ffi/actions.rs").contains(".SetFocus("));
}

#[tokio::test]
async fn invalid_selection_or_scroll_parameters_have_zero_effects() {
    for (source, req) in [
        {
            let (s, el) = portable(all_patterns()).await;
            (
                s,
                DriverAction::SelectText {
                    el,
                    range: TextRange {
                        start: usize::MAX,
                        length: 1,
                    },
                },
            )
        },
        {
            let (s, el) = portable(all_patterns()).await;
            (
                s,
                DriverAction::ScrollElement {
                    el,
                    dir: Dir::Down,
                    pages: f32::NAN,
                },
            )
        },
        {
            let (s, el) = portable(all_patterns()).await;
            (
                s,
                DriverAction::ScrollElement {
                    el,
                    dir: Dir::Down,
                    pages: 0.,
                },
            )
        },
    ] {
        assert_eq!(
            actions::perform(&source, 8, &req, &Cancel::default()).await,
            Err(DriverError::InvalidRequest)
        );
        assert!(source.action_calls().is_empty());
    }
}

#[cfg(windows)]
#[path = "actions/native.rs"]
mod native;

#[test]
fn hresult_classification_after_dispatch_never_marks_disappearance_safe_to_retry() {
    use gcu_driver_windows::uia::patterns::PatternError;
    for status in [
        0x80040201u32,
        0x80004005,
        0x800706ba,
        0x80004002,
        0x80131505,
        0x800705b4,
    ] {
        let PatternError::Failure(error) = PatternError::from_hresult(status as i32, true) else {
            panic!("uncertain HRESULT must stop the chain");
        };
        assert!(error.maybe_executed());
    }
    assert_eq!(
        PatternError::from_hresult(0x80040201u32 as i32, false),
        PatternError::Failure(DriverError::StaleElement)
    );
    for dispatched in [false, true] {
        for status in [0x80040204u32, 0x80004001] {
            assert_eq!(
                PatternError::from_hresult(status as i32, dispatched),
                PatternError::Unsupported
            );
        }
        for status in [0x80040200u32, 0x80131509] {
            assert_eq!(
                PatternError::from_hresult(status as i32, dispatched),
                PatternError::Unavailable
            );
        }
    }
}

#[path = "actions/edges.rs"]
mod edges;
