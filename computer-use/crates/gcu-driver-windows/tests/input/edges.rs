use super::*;
use gcu_driver_windows::input::{api::Event, messages::*, Effect};
fn text(win: &WindowRef, value: String, route: KeyRoute) -> DriverAction {
    DriverAction::TypeText {
        win: win.clone(),
        focus: None,
        text: value,
        route,
    }
}
#[tokio::test]
async fn targeted_keyboard_chinese_preserve_units_and_child_recipient() {
    let (api, driver, win) = fixture().await;
    let req = DriverAction::PressKeys {
        win: win.clone(),
        chord: KeyChord::parse("ctrl+shift+a").unwrap(),
        route: KeyRoute::TargetedOnly,
    };
    assert_eq!(
        driver.perform(&req, &Cancel::default()).await,
        Ok(Delivery::TargetedEvent)
    );
    let state = api.state();
    assert_eq!(
        state.sent.iter().map(|s| s.event).collect::<Vec<_>>(),
        vec![
            Event::Key(17, true, false, false),
            Event::Key(16, true, false, false),
            Event::Key(65, true, false, false),
            Event::Key(65, false, false, false),
            Event::Key(16, false, false, false),
            Event::Key(17, false, false, false)
        ]
    );
    assert!(state
        .sent
        .iter()
        .filter(|s| s.event.up_for_test())
        .all(|s| s.recorded_before_down));
    api.update(|s| s.sent.clear());
    assert_eq!(
        driver
            .perform(
                &text(&win, "中文😀\r\n".into(), KeyRoute::TargetedOnly),
                &Cancel::default()
            )
            .await,
        Ok(Delivery::TargetedEvent)
    );
    let units: Vec<_> = api
        .state()
        .sent
        .iter()
        .filter_map(|s| {
            if let Event::Unicode(u, true) = s.event {
                Some(u)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(String::from_utf16(&units).unwrap(), "中文😀\r\n");
    assert_eq!(api.state().activations, 0);
}
trait DownForTest {
    fn up_for_test(self) -> bool;
}
impl DownForTest for Event {
    fn up_for_test(self) -> bool {
        matches!(
            self,
            Event::Key(_, true, ..) | Event::Button(_, true, ..) | Event::Unicode(_, true)
        )
    }
}
#[test]
fn text_boundary_surrogate_crlf_and_limit_are_exact() {
    for suffix in ["😀", "\r\n", "中文"] {
        let value = format!("{}{}", "a".repeat(TEXT_CHUNK_UNITS - 1), suffix);
        let packets = text_chunks(&value).unwrap();
        assert!(packets.iter().all(|p| p.len() <= TEXT_CHUNK_UNITS * 2));
        for p in &packets {
            let units: Vec<_> = p
                .iter()
                .filter_map(|e| {
                    if let Event::Unicode(u, true) = e {
                        Some(*u)
                    } else {
                        None
                    }
                })
                .collect();
            assert!(String::from_utf16(&units).is_ok());
            assert!(!units.ends_with(&[13]));
        }
        let units: Vec<_> = packets
            .into_iter()
            .flatten()
            .filter_map(|e| {
                if let Event::Unicode(u, true) = e {
                    Some(u)
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(String::from_utf16(&units).unwrap(), value);
    }
    assert!(text_chunks(&"中".repeat(TEXT_MAX_UNITS)).is_ok());
    assert_eq!(
        text_chunks(&"a".repeat(TEXT_MAX_UNITS + 1)),
        Err(DriverError::InvalidRequest)
    );
}
#[tokio::test]
async fn drag_interpolation_releases_at_endpoint() {
    let (api, driver, win) = fixture().await;
    let req = DriverAction::PointerDrag {
        win,
        path: vec![
            WindowPoint { x: 10., y: 10. },
            WindowPoint { x: 110., y: 10. },
        ],
        route: PointerRoute::AllowForeground,
    };
    assert_eq!(
        driver.perform(&req, &Cancel::default()).await,
        Ok(Delivery::Foreground)
    );
    let state = api.state();
    let moves: Vec<_> = state
        .sent
        .iter()
        .filter_map(|s| {
            if let Event::Move(p, _) = s.event {
                Some(p)
            } else {
                None
            }
        })
        .collect();
    // One approach move one pixel off the start, then the start and ten interpolated moves.
    assert_eq!(moves.len(), 12);
    assert!((moves[0].x - moves[1].x).abs() <= 1. && (moves[0].y - moves[1].y).abs() <= 1.);
    assert!(moves[1..]
        .windows(2)
        .all(|p| (p[1].x - p[0].x).abs() <= 10.));
    assert_eq!(
        state.sent.len(),
        14,
        "no spurious cleanup after acknowledged drag up"
    );
    assert_eq!(
        state.sent.last().unwrap().event,
        Event::Button(Button::Left, false, WindowPoint { x: 110., y: 10. }, false)
    );
}
#[tokio::test]
async fn ledger_cancel_error_and_stop_release_all_ups_without_down_replay() {
    for fail in [false, true] {
        let (api, driver, win) = fixture().await;
        let cancel = Cancel::default();
        api.update(|s| {
            if fail {
                s.fail_at = Some(1)
            } else {
                s.cancel_at = Some((1, cancel.clone()))
            }
        });
        let req = DriverAction::PressKeys {
            win,
            chord: KeyChord::parse("ctrl+a").unwrap(),
            route: KeyRoute::AllowForeground,
        };
        let error = driver.perform(&req, &cancel).await.unwrap_err();
        assert!(error.maybe_executed());
        assert_eq!(
            api.state()
                .sent
                .iter()
                .filter(|s| s.event.up_for_test())
                .count(),
            2
        );
        assert_eq!(
            api.state().sent.last().unwrap().event,
            Event::Key(17, false, false, false)
        );
        let count = api.state().sent.len();
        driver.release_all_input().await;
        assert_eq!(api.state().sent.len(), count, "cleanup idempotent");
    }
}
#[tokio::test]
async fn uipi_gate_returns_existing_elevation_required_with_zero_input() {
    let (api, driver, win) = fixture().await;
    api.update(|s| s.validation_error = Some(DriverError::ElevationRequired));
    let error = driver
        .perform(
            &click(&win, PointerRoute::AllowForeground),
            &Cancel::default(),
        )
        .await
        .unwrap_err();
    assert_eq!(
        error.error_code(),
        gcu_protocol::ErrorCode::ElevationRequired
    );
    assert!(!error.maybe_executed());
    assert_eq!(api.state().activations, 0);
    assert!(api.state().sent.is_empty());
}
#[tokio::test]
async fn no_visible_change_never_retries_or_upgrades() {
    let fake = Arc::new(FakeInput::default());
    fake.update(|s| s.changes = false);
    let backend = Arc::new(InputBackend::new(fake.clone()));
    let (_, driver, _, win) = common::fixture().await;
    let driver = driver.with_input(backend.clone());
    assert_eq!(
        driver
            .perform(&click(&win, PointerRoute::Auto), &Cancel::default())
            .await,
        Ok(Delivery::TargetedEvent)
    );
    assert_eq!(backend.last_effect(), Some(Effect::NoVisibleChange));
    assert_eq!(fake.state().sent.len(), 3);
    assert_eq!(fake.state().activations, 0);
}
#[tokio::test]
async fn unknown_effect_failure_never_retries_or_activates() {
    let (api, driver, win) = fixture().await;
    api.update(|s| {
        s.sample_error = Some(DriverError::Timeout {
            maybe_executed: false,
        })
    });
    let error = driver
        .perform(&click(&win, PointerRoute::Auto), &Cancel::default())
        .await
        .unwrap_err();
    assert_eq!(
        error,
        DriverError::Timeout {
            maybe_executed: true
        }
    );
    assert_eq!(api.state().sent.len(), 3);
    assert_eq!(api.state().activations, 0);
}
#[tokio::test]
async fn invalid_text_and_cancelled_preflight_send_nothing() {
    let (api, driver, win) = fixture().await;
    assert_eq!(
        driver
            .perform(
                &text(
                    &win,
                    "x".repeat(gcu_driver_windows::input::text::TEXT_MAX_UNITS + 1),
                    KeyRoute::AllowForeground
                ),
                &Cancel::default()
            )
            .await,
        Err(DriverError::InvalidRequest)
    );
    let cancel = Cancel::default();
    cancel.cancel();
    assert_eq!(
        driver
            .perform(&click(&win, PointerRoute::AllowForeground), &cancel)
            .await,
        Err(DriverError::Cancelled {
            maybe_executed: false
        })
    );
    assert_eq!(api.state().activations, 0);
    assert!(api.state().sent.is_empty());
}

#[tokio::test]
async fn policy_background_ceiling_rejects_foreground_only_keyboard_classes() {
    // Policy is enforced by core (its Never/Ask contract tests run separately).
    // The driver receives no foreground authorization under Never.
    for route in [KeyRoute::Auto, KeyRoute::TargetedOnly] {
        let (api, driver, win) = fixture().await;
        api.update(|s| s.class = "Windows.UI.Core.CoreWindow".into());
        for req in [
            text(&win, "中文".into(), route),
            DriverAction::PressKeys {
                win: win.clone(),
                chord: KeyChord::parse("Return").unwrap(),
                route,
            },
        ] {
            assert_eq!(
                driver.perform(&req, &Cancel::default()).await,
                Err(DriverError::CapabilityUnavailable)
            );
        }
        assert_eq!(api.state().activations, 0);
        assert!(api.state().sent.is_empty());
    }
}
#[tokio::test]
async fn stalled_preflight_deadline_proves_zero_delivery_and_no_late_input() {
    let fake = Arc::new(FakeInput::default());
    fake.update(|s| s.resolve_delay = std::time::Duration::from_millis(200));
    let backend = InputBackend::new(fake.clone());
    let (_, _, _, win) = common::fixture().await;
    let start = tokio::time::Instant::now();
    let result = backend
        .perform(
            gcu_driver_windows::testing::window(8, 7),
            &click(&win, PointerRoute::AllowForeground),
            &Cancel::default(),
            start + std::time::Duration::from_millis(180),
        )
        .await;
    assert_eq!(
        result,
        Err(DriverError::Timeout {
            maybe_executed: false
        })
    );
    assert!(start.elapsed() < std::time::Duration::from_millis(180));
    tokio::time::sleep(std::time::Duration::from_millis(220)).await;
    assert!(fake.state().sent.is_empty());
    assert_eq!(fake.state().activations, 0);
}
#[tokio::test]
async fn stop_during_held_input_releases_without_waiting_for_stalled_worker() {
    let fake = Arc::new(FakeInput::default());
    fake.update(|s| s.send_pause = Some((0, std::time::Duration::from_millis(350))));
    let backend = Arc::new(InputBackend::new(fake.clone()));
    let (_, driver, _, win) = common::fixture().await;
    let driver = Arc::new(driver.with_input(backend.clone()));
    let task_driver = driver.clone();
    let task = tokio::spawn(async move {
        task_driver
            .perform(
                &DriverAction::PressKeys {
                    win,
                    chord: KeyChord::parse("ctrl+a").unwrap(),
                    route: KeyRoute::TargetedOnly,
                },
                &Cancel::default(),
            )
            .await
    });
    tokio::time::timeout(std::time::Duration::from_secs(1), async {
        while fake.state().sent.is_empty() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(!backend.ledger_empty());
    let start = std::time::Instant::now();
    driver.release_all_input().await;
    assert!(start.elapsed() < std::time::Duration::from_millis(100));
    assert!(backend.ledger_empty());
    assert!(task.await.unwrap().unwrap_err().maybe_executed());
    assert_eq!(
        fake.state()
            .sent
            .iter()
            .filter(|s| s.event.up_for_test())
            .count(),
        1
    );
}
#[tokio::test]
async fn abandoned_action_releases_before_next_action_starts() {
    let fake = Arc::new(FakeInput::default());
    fake.update(|s| s.send_pause = Some((0, std::time::Duration::from_secs(1))));
    let backend = Arc::new(InputBackend::new(fake.clone()));
    let (_, driver, _, win) = common::fixture().await;
    let driver = Arc::new(driver.with_input(backend.clone()));
    let task_driver = driver.clone();
    let first_win = win.clone();
    let task = tokio::spawn(async move {
        task_driver
            .perform(
                &DriverAction::PressKeys {
                    win: first_win,
                    chord: KeyChord::parse("ctrl+a").unwrap(),
                    route: KeyRoute::TargetedOnly,
                },
                &Cancel::default(),
            )
            .await
    });
    tokio::time::timeout(std::time::Duration::from_secs(1), async {
        while fake.state().sent.is_empty() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    fake.update(|s| s.send_pause = None);
    assert_eq!(
        driver
            .perform(
                &text(&win, "中".into(), KeyRoute::TargetedOnly),
                &Cancel::default()
            )
            .await,
        Ok(Delivery::TargetedEvent)
    );
    let state = fake.state();
    assert_eq!(state.sent[1].event, Event::Key(17, false, false, false));
    assert_eq!(state.sent[2].event, Event::Unicode('中' as u16, true));
    assert!(backend.ledger_empty());
}
#[tokio::test]
async fn failed_up_stays_pending_until_explicit_stop_cleanup() {
    let fake = Arc::new(FakeInput::default());
    fake.update(|s| {
        s.fail_at = Some(1);
        s.cancel_at = Some((0, Cancel::default()));
        s.release_error = true;
    });
    let backend = Arc::new(InputBackend::new(fake.clone()));
    let (_, driver, _, win) = common::fixture().await;
    let driver = driver.with_input(backend.clone());
    let cancel = Cancel::default();
    fake.update(|s| s.cancel_at = Some((0, cancel.clone())));
    let req = DriverAction::PressKeys {
        win,
        chord: KeyChord::parse("ctrl+a").unwrap(),
        route: KeyRoute::TargetedOnly,
    };
    assert!(driver
        .perform(&req, &cancel)
        .await
        .unwrap_err()
        .maybe_executed());
    assert!(!backend.ledger_empty());
    driver.release_all_input().await;
    assert!(backend.ledger_empty());
    let count = fake.state().sent.len();
    driver.release_all_input().await;
    assert_eq!(fake.state().sent.len(), count);
}
#[tokio::test]
async fn zero_insertion_failure_discards_unexecuted_downs_and_never_retries() {
    let (api, driver, win) = fixture().await;
    api.update(|s| {
        s.fail_at = Some(0);
        s.send_error = DriverError::Timeout {
            maybe_executed: false,
        };
    });
    let req = DriverAction::PressKeys {
        win,
        chord: KeyChord::parse("ctrl+a").unwrap(),
        route: KeyRoute::TargetedOnly,
    };
    assert_eq!(
        driver.perform(&req, &Cancel::default()).await,
        Err(DriverError::Timeout {
            maybe_executed: false
        })
    );
    assert_eq!(api.state().sent.len(), 1);
    assert_eq!(api.state().activations, 0);
}
#[tokio::test]
async fn type_text_native_packets_bound_units_and_report_foreground() {
    let (api, driver, win) = fixture().await;
    let value = format!("{}😀\r\n中", "a".repeat(TEXT_CHUNK_UNITS - 1));
    assert_eq!(
        driver
            .perform(
                &text(&win, value, KeyRoute::AllowForeground),
                &Cancel::default()
            )
            .await,
        Ok(Delivery::Foreground)
    );
    assert_eq!(api.state().packet_sizes, vec![510, 10]);
    assert!(api
        .state()
        .sent
        .iter()
        .all(|s| s.delivery == Delivery::Foreground));
}
