//! Portal input contracts run without a desktop and use Tokio's injected clock.
#![allow(clippy::unwrap_used, clippy::expect_used)]

#[path = "portal_input/support.rs"]
mod support;
use std::future::Future;
#[cfg(unix)]
#[path = "portal_input/identity.rs"]
mod identity;
#[cfg(target_os = "linux")]
#[path = "portal_input/reis.rs"]
mod reis_protocol;
use support::*;

#[tokio::test(start_paused = true)]
async fn first_use_requires_user_consent_before_input() {
    let h = Harness::new();
    h.portal.deny.store(true, Ordering::SeqCst);
    assert!(h.open().await.is_err());
    assert!(h.ei.events().is_empty());
    h.portal.deny.store(false, Ordering::SeqCst);
    let input = h.open().await.unwrap();
    assert_eq!(h.portal.tokens.lock().unwrap().as_slice(), &[None, None]);
    input.perform(&h.click(), &Cancel::default()).await.unwrap();
    assert!(h
        .ei
        .events()
        .iter()
        .any(|e| matches!(e, InputEvent::Button { pressed: true, .. })));
    drive(input.close()).await.unwrap();
}

#[tokio::test(start_paused = true)]
async fn session_close_releases_input_and_removes_only_its_token() {
    let h = Harness::new();
    let input = h.open().await.unwrap();
    assert!(!h.store.list_keys().unwrap().is_empty());
    h.store
        .save(
            "other",
            &RestoreRecord::new("other-token", h.config().origin(), 1000, 2000),
        )
        .unwrap();
    drive(input.close()).await.unwrap();
    assert!(!h.portal.active.load(Ordering::SeqCst));
    assert_eq!(h.store.list_keys().unwrap(), vec!["other"]);
}

#[tokio::test(start_paused = true)]
async fn down_error_is_recorded_before_send_and_all_ups_are_attempted() {
    let h = Harness::new();
    let input = h.open().await.unwrap();
    h.ei.fail_at.store(3, Ordering::SeqCst); // ctrl, shift, a: the last down may have arrived.
    let result = input
        .perform(&h.keys(), &Cancel::default())
        .await
        .unwrap_err();
    assert!(result.maybe_executed());
    drive(async {
        while h.portal.closes.load(Ordering::SeqCst) == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await;
    let log = h.ei.events();
    assert_eq!(log.len(), 6);
    for code in [29, 42, 30] {
        assert!(log.contains(&InputEvent::Key {
            code,
            pressed: false
        }));
    }
    assert!(h.ei.disconnected.load(Ordering::SeqCst));
}

#[tokio::test(start_paused = true)]
async fn cancellation_after_modifier_down_releases_without_replay() {
    let h = Harness::new();
    let input = h.open().await.unwrap();
    let cancel = Cancel::default();
    let signal = cancel.clone();
    *h.ei.hook.lock().unwrap() = Some(Arc::new(move |event| {
        if matches!(event, InputEvent::Key { pressed: true, .. }) {
            signal.cancel();
        }
    }));
    assert!(input
        .perform(&h.keys(), &cancel)
        .await
        .unwrap_err()
        .maybe_executed());
    assert_eq!(
        h.ei.events(),
        vec![
            InputEvent::Key {
                code: 29,
                pressed: true
            },
            InputEvent::Key {
                code: 29,
                pressed: false
            }
        ]
    );
    drive(input.close()).await.unwrap();
}

#[tokio::test(start_paused = true)]
async fn stop_releases_within_500ms_after_down() {
    use gcu_driver_linux::x11::hotkey::StopLatch;
    let h = Harness::new();
    let input = h.open().await.unwrap();
    // Suspend after a recorded down, allowing the independent control path to run.
    let stop = h.stop.clone();
    *h.ei.hook.lock().unwrap() = Some(Arc::new(move |event| {
        if matches!(event, InputEvent::Key { pressed: true, .. }) {
            stop.stop();
        }
    }));
    let start = tokio::time::Instant::now();
    assert!(input.perform(&h.keys(), &Cancel::default()).await.is_err());
    assert!(h.latch.is_stopped());
    {
        let log = h.ei.log.lock().unwrap();
        let up = log
            .iter()
            .find(|(_, event)| matches!(event, InputEvent::Key { pressed: false, .. }))
            .unwrap();
        assert!(up.0 - start <= std::time::Duration::from_millis(500));
        assert!(tokio::time::Instant::now() - start <= std::time::Duration::from_millis(100));
    }
    drive(input.close()).await.unwrap();
    assert!(h.open().await.is_err()); // Reconnection never resets the Host latch.
}

#[tokio::test(start_paused = true)]
async fn listener_lease_loss_latches_and_retires_within_100ms() {
    use gcu_driver_linux::x11::hotkey::StopLatch;
    let h = Harness::new();
    let input = h.open().await.unwrap();
    for _ in 0..4 {
        tokio::time::advance(WATCHDOG_TICK).await;
        tokio::task::yield_now().await;
    }
    assert!(h.latch.is_stopped());
    assert!(!input.available());
    drive(input.close()).await.unwrap();
    assert!(h.store.list_keys().unwrap().is_empty());
}

#[tokio::test(start_paused = true)]
async fn dropped_action_future_retires_without_waiting_for_native_reply() {
    let h = Harness::new();
    let input = h.open().await.unwrap();
    h.authority.pending.store(true, Ordering::SeqCst);
    let action = h.keys();
    let cancel = Cancel::default();
    let mut call = Box::pin(input.perform(&action, &cancel));
    assert!(
        std::future::poll_fn(|cx| std::task::Poll::Ready(call.as_mut().poll(cx).is_pending()))
            .await
    );
    drop(call);
    assert!(h.ei.disconnected.load(Ordering::SeqCst));
    drive(input.close()).await.unwrap();
}

#[tokio::test(start_paused = true)]
async fn native_deadline_is_bounded_using_injected_time() {
    let h = Harness::new();
    let input = h.open().await.unwrap();
    h.authority.pending.store(true, Ordering::SeqCst);
    let action = h.keys();
    let cancel = Cancel::default();
    let mut call = Box::pin(input.perform(&action, &cancel));
    assert!(
        std::future::poll_fn(|cx| std::task::Poll::Ready(call.as_mut().poll(cx).is_pending()))
            .await
    );
    // A real independently healthy listener renews throughout the stalled call.
    for _ in 0..190 {
        h.stop.heartbeat();
        tokio::time::advance(WATCHDOG_TICK).await;
        tokio::task::yield_now().await;
    }
    let result = call.await.unwrap_err();
    assert_eq!(
        result,
        DriverError::Timeout {
            maybe_executed: false
        }
    );
    assert!(h.ei.events().is_empty());
    drive(input.close()).await.unwrap();
}

#[tokio::test(start_paused = true)]
async fn unknown_source_wrong_scope_mapping_and_serial_never_send_input() {
    for case in 0..5 {
        let h = Harness::new();
        match case {
            0 => h.authority.fail.store(true, Ordering::SeqCst),
            1 => {
                h.portal.stream.lock().unwrap().source_type =
                    gcu_driver_linux::wayland::PortalSourceKind::Monitor
            }
            2 => *h.ei.mapping.lock().unwrap() = Some("unrelated".into()),
            3 => h.authority.mismatch.store(true, Ordering::SeqCst),
            _ => h.portal.stream.lock().unwrap().mapping_id = None,
        }
        assert!(h.open().await.is_err());
        assert!(h.ei.events().is_empty());
        drive(async {
            while h.portal.closes.load(Ordering::SeqCst) == 0 {
                tokio::task::yield_now().await;
            }
        })
        .await;
        assert!(h.store.list_keys().unwrap().is_empty());
    }
}

#[tokio::test(start_paused = true)]
async fn revoked_closed_or_changed_device_releases_and_clears_token() {
    for case in 0..3 {
        let h = Harness::new();
        let input = h.open().await.unwrap();
        let active = h.portal.active.clone();
        let authority = h.authority.clone();
        let ei = h.ei.clone();
        *h.ei.hook.lock().unwrap() = Some(Arc::new(move |event| {
            if matches!(event, InputEvent::Key { pressed: true, .. }) {
                match case {
                    0 => active.store(false, Ordering::SeqCst),
                    1 => authority.confined.store(false, Ordering::SeqCst),
                    _ => {
                        ei.revision.fetch_add(1, Ordering::SeqCst);
                    }
                }
            }
        }));
        assert!(input
            .perform(&h.keys(), &Cancel::default())
            .await
            .unwrap_err()
            .maybe_executed());
        drive(input.close()).await.unwrap();
        assert!(h.ei.events().contains(&InputEvent::Key {
            code: 29,
            pressed: false
        }));
        assert!(!h.portal.active.load(Ordering::SeqCst));
        assert!(h.store.list_keys().unwrap().is_empty());
    }
}

#[tokio::test(start_paused = true)]
async fn restore_is_bound_to_every_origin_field_and_expires_at_auth_or_24h() {
    for case in 0..9 {
        let h = Harness::new();
        let mut origin = h.config().origin();
        match case {
            0 => origin.compositor = "other".into(),
            1 => origin.source_kind = "monitor".into(),
            2 => origin.window_identity = "reused".into(),
            3 => origin.process_instance = "replaced".into(),
            4 => origin.auth_version = "v0".into(),
            _ => {}
        }
        let (issued, expires) = match case {
            5 => (0, 200_000),
            6 => (900, 999),
            _ => (900, 2000),
        };
        let mut record = RestoreRecord::new("old", origin, issued, expires);
        if case == 5 {
            record.issued_at_secs = 1000 - 1;
            record.expires_at_secs = 1000;
        }
        h.store.save("s1", &record).unwrap();
        if case < 7 {
            assert!(h.open().await.is_err());
            assert!(h.portal.tokens.lock().unwrap().is_empty());
        } else {
            let input = h.open().await.unwrap();
            assert_eq!(
                h.portal.tokens.lock().unwrap().as_slice(),
                &[Some("old".into())]
            );
            assert_eq!(
                h.store
                    .load_and_validate("s1", &h.config().origin(), 1000)
                    .unwrap(),
                "rotated"
            );
            drive(input.close()).await.unwrap();
        }
        assert!(h.store.list_keys().unwrap().is_empty());
    }
    let origin = Harness::new().config().origin();
    assert_eq!(
        RestoreRecord::new("t", origin, 1000, u64::MAX).expires_at_secs,
        1000 + 24 * 3600
    );
}

#[tokio::test(start_paused = true)]
async fn physical_stream_mapping_and_route_ceiling_are_enforced() {
    let h = Harness::new();
    let input = h.open().await.unwrap();
    input.perform(&h.click(), &Cancel::default()).await.unwrap();
    assert_eq!(h.ei.events()[0], InputEvent::Absolute(500., 500.));
    drive(input.close()).await.unwrap();
    for route in [PointerRoute::Auto, PointerRoute::TargetedOnly] {
        let h = Harness::new();
        let input = h.open().await.unwrap();
        let mut click = h.click();
        if let DriverAction::PointerClick { route: ceiling, .. } = &mut click {
            *ceiling = route;
        }
        assert!(input.perform(&click, &Cancel::default()).await.is_err());
        assert!(h.ei.events().is_empty());
        drive(input.close()).await.unwrap();
    }
}

#[test]
fn mapping_rejects_nonfinite_bounds_overflow_and_f32_escape() {
    let region = InputRegion {
        mapping_id: "m".into(),
        x: 100,
        y: 200,
        width: 800,
        height: 600,
    };
    for (x, y) in [
        (f64::NAN, 0.),
        (0., f64::INFINITY),
        (-1., 0.),
        (1600., 0.),
        (1599.9999999, 0.),
    ] {
        assert!(region.map((1600, 1200), WindowPoint { x, y }).is_err());
    }
    assert!(InputRegion {
        x: u32::MAX,
        ..region
    }
    .map((1600, 1200), WindowPoint { x: 0., y: 0. })
    .is_err());
}

#[tokio::test(start_paused = true)]
async fn stop_failure_and_unconfined_source_refuse_before_portal_consent() {
    for case in 0..2 {
        let h = Harness::new();
        if case == 0 {
            h.stop.listener_failed();
        } else {
            h.authority.confined.store(false, Ordering::SeqCst);
        }
        assert!(h.open().await.is_err());
        assert!(h.portal.tokens.lock().unwrap().is_empty());
        assert!(h.ei.events().is_empty());
    }
}

#[tokio::test(start_paused = true)]
async fn utf8_is_preserved_and_all_action_paths_preflight_the_exact_window() {
    let h = Harness::new();
    let input = h.open().await.unwrap();
    let text = "中文👩‍💻\n".repeat(50);
    let action = DriverAction::TypeText {
        win: h.win.clone(),
        focus: None,
        text: text.clone(),
        route: KeyRoute::AllowForeground,
    };
    input.perform(&action, &Cancel::default()).await.unwrap();
    let chunks =
        h.ei.events()
            .into_iter()
            .map(|e| {
                if let InputEvent::Text(s) = e {
                    assert!(s.len() <= 254);
                    s
                } else {
                    panic!("expected text");
                }
            })
            .collect::<String>();
    assert_eq!(chunks, text);
    drive(input.close()).await.unwrap();
}

#[cfg(target_os = "linux")]
#[tokio::test]
#[ignore = "not_run: requires a consented Wayland fixture, source proof and native global Stop listener"]
async fn live_remote_desktop_uses_portal_fd_and_reis() {
    use gcu_driver_linux::wayland::input_portal::AshpdRemoteDesktop;
    let connection = AshpdRemoteDesktop.open(None).await.unwrap();
    assert!(connection.session.is_active());
    connection
        .stream
        .validate(gcu_driver_linux::wayland::PortalSourceKind::Window)
        .unwrap();
    assert_eq!(
        connection.stream.mapping_id.as_deref(),
        Some(connection.ei.snapshot().unwrap().region.mapping_id.as_str())
    );
    connection.ei.disconnect();
    connection.session.close().await.unwrap();
}

#[tokio::test(start_paused = true)]
async fn stop_during_blocked_proof_bypasses_native_action_queue() {
    let h = Harness::new();
    let input = h.open().await.unwrap();
    h.authority.pending.store(true, Ordering::SeqCst);
    let action = h.keys();
    let cancel = Cancel::default();
    let mut call = Box::pin(input.perform(&action, &cancel));
    assert!(
        std::future::poll_fn(|cx| std::task::Poll::Ready(call.as_mut().poll(cx).is_pending()))
            .await
    );
    let start = tokio::time::Instant::now();
    h.stop.stop();
    assert!(call.await.is_err());
    assert!(tokio::time::Instant::now() - start < std::time::Duration::from_millis(100));
    drive(input.close()).await.unwrap();
    assert!(h.ei.events().is_empty());
}

#[tokio::test(start_paused = true)]
async fn failed_release_still_attempts_every_other_up() {
    let h = Harness::new();
    let input = h.open().await.unwrap();
    h.ei.fail_at.store(3, Ordering::SeqCst);
    h.ei.fail_ups.store(true, Ordering::SeqCst);
    assert!(input.perform(&h.keys(), &Cancel::default()).await.is_err());
    assert!(drive(input.close()).await.is_err());
    for code in [29, 42, 30] {
        assert!(h.ei.events().contains(&InputEvent::Key {
            code,
            pressed: false
        }));
    }
    assert!(h.ei.disconnected.load(Ordering::SeqCst));
    assert!(h.store.list_keys().unwrap().is_empty());
}

#[tokio::test(start_paused = true)]
async fn driver_dispatch_and_doctor_share_portal_capabilities_without_x11_fallback() {
    use gcu_core::driver::{Delivery, Driver};
    use gcu_driver_linux::{
        atspi::source::{FakeNode, FakeSource},
        wayland::{Compositor, LinuxEnvironment, LinuxSessionType},
        LinuxDriver,
    };
    let h = Harness::new();
    let input = h.open().await.unwrap();
    let driver = LinuxDriver::new_fake(FakeSource::default(), FakeNode::default(), vec![], vec![])
        .with_environment(LinuxEnvironment {
            session_type: LinuxSessionType::WaylandPure,
            compositor: Compositor::Gnome,
            desktop_name: None,
            wayland_display: Some("wayland-0".into()),
            x11_display: None,
        })
        .with_portal_input(input.clone());
    assert_eq!(driver.capabilities(), &driver.doctor().await.capabilities);
    assert_eq!(
        driver
            .perform(&h.click(), &Cancel::default())
            .await
            .unwrap(),
        Delivery::Foreground
    );
    h.stop.listener_failed();
    assert_eq!(
        driver.capabilities().delivery.foreground,
        gcu_protocol::capabilities::ForegroundCapability::Unavailable
    );
    let before = h.ei.events().len();
    assert!(driver
        .perform(&h.click(), &Cancel::default())
        .await
        .is_err());
    assert_eq!(h.ei.events().len(), before);
    drive(input.close()).await.unwrap();
}
