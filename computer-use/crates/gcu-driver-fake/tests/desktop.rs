//! Independent-oracle regressions for scenes, actions, pixels, and failure semantics.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use gcu_core::{driver::*, model::*};
use gcu_driver_fake::{
    faults::Fault,
    render::{FIELD_COLOR, PASSWORD_COLOR},
    scene::builtin,
    FakeDriver,
};
use std::{io::Cursor, time::Duration};

async fn setup(scene: &str) -> (FakeDriver, WindowRef) {
    let driver = FakeDriver::from_scene(scene).unwrap();
    let win = driver
        .fixture_window(&format!("fake.{scene}"))
        .await
        .unwrap();
    (driver, win)
}
async fn press(driver: &FakeDriver, win: &WindowRef, id: &str) -> Result<Delivery> {
    let el = driver.element(win, id).await?;
    driver
        .perform(&DriverAction::Press { el }, &Cancel::default())
        .await
}
fn find<'a>(root: &'a RawNode, id: &str) -> Option<&'a RawNode> {
    if root.identifier.as_deref() == Some(id) {
        Some(root)
    } else {
        root.children.iter().find_map(|n| find(n, id))
    }
}
fn decode(capture: &Capture) -> (u32, u32, Vec<u8>) {
    let mut reader = png::Decoder::new(Cursor::new(&capture.bytes))
        .read_info()
        .unwrap();
    let mut bytes = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut bytes).unwrap();
    bytes.truncate(info.buffer_size());
    (info.width, info.height, bytes)
}
fn pixel(bytes: &[u8], width: u32, x: usize, y: usize) -> [u8; 4] {
    bytes[(y * width as usize + x) * 4..(y * width as usize + x + 1) * 4]
        .try_into()
        .unwrap()
}

#[tokio::test]
async fn scene_loading_all_builtins_preserves_native_roles_and_shapes() {
    for name in [
        "form",
        "list",
        "modal",
        "chat",
        "webarea",
        "canvas",
        "terminal",
        "login",
        "ide",
        "web_terminal",
        "submit_unknown",
    ] {
        let (driver, win) = setup(name).await;
        assert!(driver.doctor().await.ready);
        assert_eq!(&driver.doctor().await.capabilities, driver.capabilities());
        let tree = driver
            .snapshot_tree(&win, &TreeOpts::default(), &Cancel::default())
            .await
            .unwrap();
        assert_eq!(tree.root.is_none(), name == "canvas");
        if name == "terminal" {
            let console = find(tree.root.as_ref().unwrap(), "console").unwrap();
            assert_eq!(console.role, Role::Unknown);
            assert_eq!(console.native_role, "terminal");
        }
        if name == "modal" {
            assert!(find(tree.root.as_ref().unwrap(), "modal")
                .unwrap()
                .states
                .contains(States::MODAL));
        }
        if name == "webarea" {
            assert_eq!(
                find(tree.root.as_ref().unwrap(), "link")
                    .unwrap()
                    .url
                    .as_deref(),
                Some("https://example.invalid/docs")
            );
        }
    }
}

#[test]
fn scene_loading_invalid_ids_geometry_and_repetition_fail() {
    let source = builtin("form").unwrap();
    for invalid in [
        source.replace("id: \"agree\"", "id: \"text\""),
        source.replace("w: 800.0", "w: -1.0"),
        source.replace("scale: 1.0", "scale: 0.0"),
        source.replace("h: 480.0", "h: 480.5"),
    ] {
        assert!(FakeDriver::from_ron(&invalid).is_err());
    }
    assert!(FakeDriver::from_scene("missing").is_err());
    assert!(FakeDriver::from_ron(
        &builtin("list")
            .unwrap()
            .replace("repeat: 2000", "repeat: 5001")
    )
    .is_err());
}

#[tokio::test]
async fn discovery_exact_name_id_path_basename_and_ambiguity_are_safe() {
    let (driver, win) = setup("form").await;
    for query in ["Fixture form", "fake.form", "/fixtures/form", "form"] {
        assert_eq!(
            driver.resolve_app(&AppQuery::from(query)).await.unwrap(),
            win.app
        );
    }
    assert_eq!(
        driver.resolve_app(&AppQuery::from("no app")).await,
        Err(DriverError::AppNotFound)
    );
    let first = builtin("form").unwrap();
    let body = first
        .split("apps: [")
        .nth(1)
        .unwrap()
        .strip_suffix("])\n")
        .unwrap();
    let second = body
        .replace("fake.form", "fake.second")
        .replace("form-window", "second-window")
        .replace("/fixtures/form", "/fixtures/second");
    let duplicate_names = format!("(apps: [{body}, {second}])");
    let driver = FakeDriver::from_ron(&duplicate_names).unwrap();
    assert_eq!(
        driver.resolve_app(&AppQuery::from("Fixture form")).await,
        Err(DriverError::AmbiguousApp)
    );
    assert_eq!(driver.list_windows(None).await.unwrap().len(), 2);
}

#[tokio::test]
async fn chat_type_text_focus_and_send_update_oracle_exactly_once() {
    let (driver, win) = setup("chat").await;
    let input = "你好 👩‍💻\nmessage";
    assert_eq!(
        driver
            .perform(
                &DriverAction::TypeText {
                    win: win.clone(),
                    focus: None,
                    text: input.into(),
                    route: KeyRoute::Auto
                },
                &Cancel::default()
            )
            .await
            .unwrap(),
        Delivery::TargetedEvent
    );
    assert_eq!(driver.oracle().await.unwrap().text["text"], input);
    assert_eq!(
        press(&driver, &win, "send").await.unwrap(),
        Delivery::Accessibility
    );
    assert_eq!(driver.oracle().await.unwrap().presses["send"], 1);
    assert!(driver
        .focused_element(&win.app)
        .await
        .unwrap()
        .unwrap()
        .ptr_eq(&driver.element(&win, "text").await.unwrap()));
}

#[tokio::test]
async fn select_text_unicode_range_replaces_exact_scalars() {
    let (driver, win) = setup("form").await;
    let el = driver.element(&win, "text").await.unwrap();
    let cancel = Cancel::default();
    driver
        .perform(
            &DriverAction::SetValue {
                el: el.clone(),
                value: "甲😀乙".into(),
            },
            &cancel,
        )
        .await
        .unwrap();
    driver
        .perform(
            &DriverAction::SelectText {
                el: el.clone(),
                range: TextRange {
                    start: 1,
                    length: 1,
                },
            },
            &cancel,
        )
        .await
        .unwrap();
    driver
        .perform(
            &DriverAction::TypeText {
                win: win.clone(),
                focus: Some(el.clone()),
                text: "丙".into(),
                route: KeyRoute::Auto,
            },
            &cancel,
        )
        .await
        .unwrap();
    assert_eq!(driver.oracle().await.unwrap().text["text"], "甲丙乙");
    assert_eq!(
        driver
            .perform(
                &DriverAction::SelectText {
                    el,
                    range: TextRange {
                        start: usize::MAX,
                        length: 1
                    }
                },
                &cancel
            )
            .await,
        Err(DriverError::InvalidRequest)
    );
}

#[tokio::test]
async fn secondary_toggle_increment_and_drag_have_real_effects() {
    let (driver, win) = setup("form").await;
    let cancel = Cancel::default();
    let agree = driver.element(&win, "agree").await.unwrap();
    driver
        .perform(
            &DriverAction::SecondaryAction {
                el: agree,
                action: NodeAction::Toggle,
            },
            &cancel,
        )
        .await
        .unwrap();
    let slider = driver.element(&win, "slider").await.unwrap();
    driver
        .perform(
            &DriverAction::SetValue {
                el: slider.clone(),
                value: "40".into(),
            },
            &cancel,
        )
        .await
        .unwrap();
    driver
        .perform(
            &DriverAction::SecondaryAction {
                el: slider.clone(),
                action: NodeAction::Increment,
            },
            &cancel,
        )
        .await
        .unwrap();
    assert_eq!(driver.oracle().await.unwrap().numbers["slider"], 41.0);
    let path = vec![
        WindowPoint { x: 40.0, y: 200.0 },
        WindowPoint { x: 170.0, y: 200.0 },
    ];
    assert_eq!(
        driver
            .perform(
                &DriverAction::PointerDrag {
                    win: win.clone(),
                    path,
                    route: PointerRoute::Auto
                },
                &cancel
            )
            .await
            .unwrap(),
        Delivery::TargetedEvent
    );
    assert_eq!(driver.oracle().await.unwrap().numbers["slider"], 75.0);
    let tree = driver
        .snapshot_tree(&win, &TreeOpts::default(), &cancel)
        .await
        .unwrap();
    assert!(find(tree.root.as_ref().unwrap(), "agree")
        .unwrap()
        .states
        .contains(States::CHECKED));
    assert_eq!(
        driver
            .perform(
                &DriverAction::SetValue {
                    el: slider,
                    value: "NaN".into()
                },
                &cancel
            )
            .await,
        Err(DriverError::InvalidRequest)
    );
    assert_eq!(driver.oracle().await.unwrap().numbers["slider"], 75.0);
}

#[tokio::test]
async fn large_list_raw_reads_keep_all_rows_and_scroll_updates_visibility() {
    let (driver, win) = setup("list").await;
    let cancel = Cancel::default();
    let tree = driver
        .snapshot_tree(&win, &TreeOpts::default(), &cancel)
        .await
        .unwrap();
    let list = find(tree.root.as_ref().unwrap(), "list").unwrap();
    assert_eq!(list.children.len(), 2000);
    assert_eq!(
        list.children
            .iter()
            .filter(|n| !n.states.contains(States::OFFSCREEN))
            .count(),
        15
    );
    let el = list.handle.clone();
    driver
        .perform(
            &DriverAction::ScrollElement {
                el,
                dir: Dir::Down,
                pages: 2.0,
            },
            &cancel,
        )
        .await
        .unwrap();
    assert_eq!(driver.oracle().await.unwrap().scroll_rows["list"], 30);
    driver
        .perform(
            &DriverAction::PointerScroll {
                win: win.clone(),
                at: WindowPoint { x: 100.0, y: 100.0 },
                dx: 0,
                dy: -360,
                route: PointerRoute::Auto,
            },
            &cancel,
        )
        .await
        .unwrap();
    let tree = driver
        .snapshot_tree(&win, &TreeOpts::default(), &cancel)
        .await
        .unwrap();
    assert_eq!(driver.oracle().await.unwrap().scroll_rows["list"], 15);
    let row = find(tree.root.as_ref().unwrap(), "row-15").unwrap();
    assert!(!row.states.contains(States::OFFSCREEN));
    assert_eq!(row.frame.unwrap().y, 20.0);
}

#[tokio::test]
async fn screenshot_pixels_align_with_frames_at_high_dpi_and_password_marker() {
    let (driver, win) = setup("login").await;
    let mut geometry = driver.window_geometry(&win).await.unwrap();
    geometry.scale = 2.0;
    driver
        .inject(Fault::GeometryChanged {
            window: win.id.clone(),
            geometry,
        })
        .await
        .unwrap();
    let capture = driver
        .capture(&win, &CaptureOpts::default(), &Cancel::default())
        .await
        .unwrap();
    let (width, height, bytes) = decode(&capture);
    assert_eq!((width, height), (800, 480));
    assert_eq!(capture.geometry.scale, 2.0);
    assert_eq!(pixel(&bytes, width, 100, 50), FIELD_COLOR);
    assert_eq!(pixel(&bytes, width, 100, 110), PASSWORD_COLOR);
    assert_ne!(pixel(&bytes, width, 19, 110), PASSWORD_COLOR);
    assert_ne!(pixel(&bytes, width, 240, 110), PASSWORD_COLOR);
    let again = driver
        .capture(&win, &CaptureOpts::default(), &Cancel::default())
        .await
        .unwrap();
    assert_eq!(capture.bytes, again.bytes);
    // Named ASCII glyphs are drawn inside the field; fixture rasterization has no font dependency.
    assert!(bytes.as_chunks::<4>().0.contains(&[20, 30, 40, 255]));
}

#[tokio::test]
async fn routes_targeted_only_never_escalate_and_foreground_is_reported() {
    let (driver, win) = setup("form").await;
    let driver = driver.with_targeted_input(false, false);
    for route in [PointerRoute::Auto, PointerRoute::TargetedOnly] {
        assert_eq!(
            driver
                .perform(
                    &DriverAction::PointerClick {
                        win: win.clone(),
                        at: WindowPoint { x: 50.0, y: 140.0 },
                        button: Button::Left,
                        count: 1,
                        route
                    },
                    &Cancel::default()
                )
                .await,
            Err(DriverError::CapabilityUnavailable)
        );
    }
    for route in [KeyRoute::Auto, KeyRoute::TargetedOnly] {
        assert_eq!(
            driver
                .perform(
                    &DriverAction::TypeText {
                        win: win.clone(),
                        focus: None,
                        text: "denied".into(),
                        route
                    },
                    &Cancel::default()
                )
                .await,
            Err(DriverError::CapabilityUnavailable)
        );
    }
    assert!(driver.oracle().await.unwrap().presses.is_empty());
    assert!(driver.oracle().await.unwrap().foreground.is_none());
    assert_eq!(
        driver
            .perform(
                &DriverAction::PointerClick {
                    win: win.clone(),
                    at: WindowPoint { x: 50.0, y: 140.0 },
                    button: Button::Left,
                    count: 1,
                    route: PointerRoute::AllowForeground
                },
                &Cancel::default()
            )
            .await
            .unwrap(),
        Delivery::Foreground
    );
    assert_eq!(driver.oracle().await.unwrap().foreground, Some(win.id));
}

#[tokio::test]
async fn timeout_and_crash_uncertainty_distinguish_zero_from_once() {
    for crash in [false, true] {
        for maybe_executed in [false, true] {
            let (driver, win) = setup("form").await;
            driver
                .inject(if crash {
                    Fault::Crash { maybe_executed }
                } else {
                    Fault::Timeout { maybe_executed }
                })
                .await
                .unwrap();
            let error = press(&driver, &win, "submit").await.unwrap_err();
            assert_eq!(error.maybe_executed(), maybe_executed);
            assert_eq!(
                error,
                if crash {
                    DriverError::Crashed { maybe_executed }
                } else {
                    DriverError::Timeout { maybe_executed }
                }
            );
            assert_eq!(
                driver
                    .oracle()
                    .await
                    .unwrap()
                    .presses
                    .get("submit")
                    .copied()
                    .unwrap_or(0),
                u64::from(maybe_executed)
            );
            let _ = driver
                .snapshot_tree(&win, &TreeOpts::default(), &Cancel::default())
                .await
                .unwrap();
            assert_eq!(
                driver
                    .oracle()
                    .await
                    .unwrap()
                    .presses
                    .get("submit")
                    .copied()
                    .unwrap_or(0),
                u64::from(maybe_executed)
            );
            assert_eq!(
                press(&driver, &win, "submit").await.unwrap(),
                Delivery::Accessibility
            );
            assert_eq!(
                driver.oracle().await.unwrap().presses["submit"],
                1 + u64::from(maybe_executed)
            );
        }
    }
}

#[tokio::test(start_paused = true)]
async fn delayed_read_timeout_and_cancel_have_zero_effects() {
    let (driver, win) = setup("form").await;
    driver
        .inject(Fault::Delay(Duration::from_millis(50)))
        .await
        .unwrap();
    assert!(matches!(
        driver
            .snapshot_tree(
                &win,
                &TreeOpts {
                    timeout: Duration::from_millis(1),
                    ..Default::default()
                },
                &Cancel::default()
            )
            .await,
        Err(DriverError::Timeout {
            maybe_executed: false
        })
    ));
    driver
        .inject(Fault::Delay(Duration::from_secs(1)))
        .await
        .unwrap();
    let cancel = Cancel::default();
    let clone = cancel.clone();
    let other = driver.clone();
    let other_win = win.clone();
    let call = tokio::spawn(async move {
        other
            .snapshot_tree(&other_win, &TreeOpts::default(), &clone)
            .await
    });
    tokio::task::yield_now().await;
    cancel.cancel();
    assert!(matches!(
        call.await.unwrap(),
        Err(DriverError::Cancelled {
            maybe_executed: false
        })
    ));
    assert!(driver.oracle().await.unwrap().presses.is_empty());
}

#[tokio::test(start_paused = true)]
async fn slow_settle_delivers_once_and_later_exposes_effect() {
    let (driver, win) = setup("form").await;
    driver
        .inject(Fault::SlowSettle(Duration::from_millis(20)))
        .await
        .unwrap();
    assert_eq!(
        press(&driver, &win, "submit").await.unwrap(),
        Delivery::Accessibility
    );
    assert!(driver.oracle().await.unwrap().presses.is_empty());
    tokio::time::advance(Duration::from_millis(30)).await;
    assert_eq!(driver.oracle().await.unwrap().presses["submit"], 1);
    assert_eq!(driver.oracle().await.unwrap().presses["submit"], 1);
}

#[tokio::test]
async fn stale_foreign_handles_deleted_nodes_and_closed_windows_never_substitute() {
    let (driver, win) = setup("form").await;
    let (foreign, other_win) = setup("form").await;
    let el = foreign.element(&other_win, "submit").await.unwrap();
    assert_eq!(
        driver
            .perform(&DriverAction::Press { el }, &Cancel::default())
            .await,
        Err(DriverError::StaleElement)
    );
    let el = driver.element(&win, "submit").await.unwrap();
    driver
        .inject(Fault::ElementDeleted {
            window: win.id.clone(),
            identifier: "submit".into(),
        })
        .await
        .unwrap();
    assert_eq!(
        driver
            .perform(&DriverAction::Press { el }, &Cancel::default())
            .await,
        Err(DriverError::StaleElement)
    );
    driver
        .inject(Fault::WindowGone {
            window: win.id.clone(),
        })
        .await
        .unwrap();
    assert!(matches!(
        driver
            .capture(&win, &CaptureOpts::default(), &Cancel::default())
            .await,
        Err(DriverError::WindowGone)
    ));
    assert!(driver.list_windows(None).await.unwrap().is_empty());
    assert!(driver.oracle().await.unwrap().presses.is_empty());
    assert!(foreign.oracle().await.unwrap().presses.is_empty());
}

#[tokio::test]
async fn geometry_fault_changes_capture_size_and_invalid_points_have_no_effects() {
    let (driver, win) = setup("form").await;
    let mut geometry = driver.window_geometry(&win).await.unwrap();
    geometry.frame.x += 50.0;
    geometry.frame.w = 900.0;
    driver
        .inject(Fault::GeometryChanged {
            window: win.id.clone(),
            geometry,
        })
        .await
        .unwrap();
    let capture = driver
        .capture(&win, &CaptureOpts::default(), &Cancel::default())
        .await
        .unwrap();
    assert_eq!(capture.width, 900);
    assert_eq!(driver.window_geometry(&win).await.unwrap(), geometry);
    for at in [
        WindowPoint { x: -1.0, y: 0.0 },
        WindowPoint {
            x: f64::NAN,
            y: 0.0,
        },
        WindowPoint { x: 900.0, y: 100.0 },
    ] {
        assert_eq!(
            driver
                .perform(
                    &DriverAction::PointerClick {
                        win: win.clone(),
                        at,
                        button: Button::Left,
                        count: 1,
                        route: PointerRoute::Auto
                    },
                    &Cancel::default()
                )
                .await,
            Err(DriverError::OutOfBounds)
        );
    }
    assert!(driver.oracle().await.unwrap().presses.is_empty());
}

#[tokio::test]
async fn modal_coordinate_hit_blocks_background_and_canvas_capture_has_no_tree() {
    let (driver, win) = setup("modal").await;
    assert_eq!(
        driver
            .perform(
                &DriverAction::PointerClick {
                    win: win.clone(),
                    at: WindowPoint { x: 50.0, y: 110.0 },
                    button: Button::Left,
                    count: 1,
                    route: PointerRoute::Auto
                },
                &Cancel::default()
            )
            .await,
        Err(DriverError::ElementNotActionable)
    );
    assert_eq!(
        press(&driver, &win, "confirm").await.unwrap(),
        Delivery::Accessibility
    );
    let (driver, win) = setup("canvas").await;
    let capture = driver
        .capture(&win, &CaptureOpts::default(), &Cancel::default())
        .await
        .unwrap();
    assert_eq!((capture.width, capture.height), (800, 480));
    assert!(driver
        .snapshot_tree(&win, &TreeOpts::default(), &Cancel::default())
        .await
        .unwrap()
        .root
        .is_none());
}

#[tokio::test]
async fn event_subscription_reports_mutation_geometry_and_window_close() {
    let (driver, win) = setup("form").await;
    let mut events = driver.subscribe(&win).unwrap();
    press(&driver, &win, "submit").await.unwrap();
    let event = std::future::poll_fn(|cx| events.as_mut().poll_next(cx))
        .await
        .unwrap();
    assert_eq!(event, UiEvent::Changed);
    let geometry = driver.window_geometry(&win).await.unwrap();
    driver
        .inject(Fault::GeometryChanged {
            window: win.id.clone(),
            geometry,
        })
        .await
        .unwrap();
    driver
        .snapshot_tree(&win, &TreeOpts::default(), &Cancel::default())
        .await
        .unwrap();
    assert_eq!(
        std::future::poll_fn(|cx| events.as_mut().poll_next(cx))
            .await
            .unwrap(),
        UiEvent::GeometryChanged
    );
    driver
        .inject(Fault::WindowGone {
            window: win.id.clone(),
        })
        .await
        .unwrap();
    assert!(driver
        .snapshot_tree(&win, &TreeOpts::default(), &Cancel::default())
        .await
        .is_err());
    assert_eq!(
        std::future::poll_fn(|cx| events.as_mut().poll_next(cx))
            .await
            .unwrap(),
        UiEvent::WindowGone
    );
}

#[cfg(feature = "testing")]
#[tokio::test]
async fn contract_skips_match_actual_capabilities_and_scope_registry() {
    use gcu_core::testing::{run_case, CaseOutcome, CASES};
    let fixture = FakeDriver::contract_fixture()
        .unwrap()
        .with_targeted_input(false, false);
    for id in ["C06", "C08", "C09", "C19", "C23"] {
        let case = CASES.iter().find(|c| c.id == id).unwrap();
        assert!(!case.requires.supported(fixture.capabilities()));
        assert!(
            matches!(run_case(&fixture,id).await.unwrap(),CaseOutcome::Skipped(reason) if reason.starts_with("capability:"))
        );
    }
    for id in ["C17", "C24"] {
        let fixture = FakeDriver::contract_fixture().unwrap();
        assert!(
            matches!(run_case(&fixture,id).await.unwrap(),CaseOutcome::Skipped(reason) if reason.starts_with("scope:"))
        );
    }
}

#[tokio::test(start_paused = true)]
async fn slow_settle_event_arrives_without_an_observation_trigger() {
    let (driver, win) = setup("form").await;
    let mut events = driver.subscribe(&win).unwrap();
    driver
        .inject(Fault::SlowSettle(Duration::from_millis(10)))
        .await
        .unwrap();
    press(&driver, &win, "submit").await.unwrap();
    let event = tokio::time::timeout(
        Duration::from_secs(1),
        std::future::poll_fn(|cx| events.as_mut().poll_next(cx)),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(event, UiEvent::Changed);
    assert_eq!(driver.oracle().await.unwrap().presses["submit"], 1);
}

#[tokio::test]
async fn observation_fingerprint_ignores_values_and_debug_omits_password_payloads() {
    let (driver, win) = setup("login").await;
    let tree = driver
        .snapshot_tree(&win, &TreeOpts::default(), &Cancel::default())
        .await
        .unwrap();
    let before = find(tree.root.as_ref().unwrap(), "password")
        .unwrap()
        .fingerprint;
    let el = driver.element(&win, "password").await.unwrap();
    let action = DriverAction::SetValue {
        el,
        value: "fixture-secret".into(),
    };
    assert!(!format!("{action:?}").contains("fixture-secret"));
    driver.perform(&action, &Cancel::default()).await.unwrap();
    let tree = driver
        .snapshot_tree(&win, &TreeOpts::default(), &Cancel::default())
        .await
        .unwrap();
    assert_eq!(
        find(tree.root.as_ref().unwrap(), "password")
            .unwrap()
            .fingerprint,
        before
    );
    assert!(!format!("{tree:?}").contains("fixture-secret"));
    let first = driver
        .capture(&win, &CaptureOpts::default(), &Cancel::default())
        .await
        .unwrap();
    driver
        .perform(
            &DriverAction::SetValue {
                el: driver.element(&win, "password").await.unwrap(),
                value: "another-secret".into(),
            },
            &Cancel::default(),
        )
        .await
        .unwrap();
    let second = driver
        .capture(&win, &CaptureOpts::default(), &Cancel::default())
        .await
        .unwrap();
    assert_eq!(first.bytes, second.bytes);
}
