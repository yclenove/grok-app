//! Portable screenshot tests. `FakeCapture` stands in for ScreenCaptureKit.
//!
//! Redaction is mandatory: a password tree plus the fake image goes through
//! `gcu_core::redact` before any thumbnail, crop, or encode. The ignored test
//! at the bottom is the real ScreenCaptureKit path and does not run in CI.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use gcu_core::{
    driver::{
        Cancel, CaptureOpts, Driver, DriverError, TreeOpts, WindowRef, DRIVER_DEADLINE,
        NATIVE_TIMEOUT,
    },
    model::{Fingerprint, NativeHandle, NodeValue, RawNode, RawTree, Rect, Role, States},
    redact::{self, CapturedFrame, EncodeOptions, NEUTRAL_COLOR},
    tree::Pipeline,
};
use gcu_driver_macos::{
    api::MacApi,
    capture::{
        backing_scale, bgra_to_rgba, capture_budget, is_screen_recording_denial, physical_size,
        screenshot_config, CaptureFault, CaptureSource, DisplayMetrics, FakeCapture, Paint,
        SourceWindow, PIXEL_FORMAT_BGRA, SCREEN_RECORDING_MISSING_ENTITLEMENT,
        SCREEN_RECORDING_USER_DECLINED,
    },
    testing::{ax_window, cg_window, rect, signed_facts, FakeMacApi},
    MacosDriver,
};
use gcu_protocol::capabilities::{BackgroundOperation, ForegroundCapability};
use std::{sync::Arc, time::Duration};

const SECRET: [u8; 4] = [255, 0, 180, 255];
const OWN: [u8; 4] = [10, 40, 80, 255];
const COVER: [u8; 4] = [1, 2, 3, 255];
const STAMP: Duration = Duration::from_secs(1);

struct Session {
    driver: MacosDriver,
    api: Arc<FakeMacApi>,
    win: WindowRef,
    fake: Arc<FakeCapture>,
}

async fn session(frame: Rect) -> Session {
    let api = Arc::new(FakeMacApi::default());
    let facts = signed_facts("com.example.Editor", "/Applications/Editor.app");
    api.add_running(4, &facts);
    api.update(|state| {
        state.cg_windows.push(cg_window(7, 4, "Editor", frame));
        state
            .ax_windows
            .insert(4, vec![ax_window(0, "Editor", frame)]);
    });
    let driver = MacosDriver::with_api(Arc::clone(&api) as Arc<dyn MacApi>)
        .await
        .unwrap();
    let app = driver
        .resolve_app(&gcu_core::driver::AppQuery::from("Editor"))
        .await
        .unwrap();
    let windows = driver.list_windows(Some(&app)).await.unwrap();
    let win = WindowRef {
        app,
        id: windows[0].id.clone(),
    };
    let fake = Arc::new(FakeCapture::new());
    driver.inject_capture(Arc::clone(&fake) as Arc<dyn CaptureSource>);
    Session {
        driver,
        api,
        win,
        fake,
    }
}

fn paint_secret(mark: Option<Rect>) -> Paint {
    Paint {
        own: OWN,
        cover: COVER,
        occluded: false,
        mark: mark.map(|rect| (SECRET, rect)),
    }
}

fn decode_png(bytes: &[u8]) -> (u32, u32, Vec<u8>) {
    let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    let mut reader = decoder.read_info().expect("png info");
    let mut buf = vec![0; reader.output_buffer_size().unwrap_or(4096)];
    let info = reader.next_frame(&mut buf).expect("png frame");
    buf.truncate(info.buffer_size());
    (info.width, info.height, buf)
}

fn assert_no_color(bytes: &[u8], color: [u8; 4]) {
    assert!(
        bytes.chunks(4).all(|pixel| pixel != color),
        "color {color:?} leaked into a derivative"
    );
}

fn make_raw_node(role: Role, name: &str, frame: Option<Rect>, states: States) -> RawNode {
    RawNode {
        role,
        native_role: format!("{role:?}").into(),
        subrole: None,
        name: (!name.is_empty()).then(|| name.into()),
        value: None,
        description: None,
        placeholder: None,
        help: None,
        identifier: None,
        states,
        actions: Default::default(),
        frame,
        text_extras: None,
        url: None,
        children: vec![],
        handle: NativeHandle::new(()),
        fingerprint: Fingerprint::default(),
    }
}

fn password_tree(frame: Option<Rect>, max_nodes: usize) -> gcu_core::tree::ProcessedTree {
    let mut node = make_raw_node(Role::TextField, "Password", frame, States::PASSWORD);
    node.value = Some(NodeValue::Text("sensitive_pass".into()));
    node.text_extras = Some(gcu_core::model::TextExtras {
        text: Some("sensitive_pass".into()),
        ..Default::default()
    });
    let mut tree = Pipeline::default().run(
        RawTree { root: Some(node) },
        &TreeOpts {
            max_tree_nodes: max_nodes,
            ..Default::default()
        },
    );
    tree.state_version = Some(42);
    tree.observed_at = Some(STAMP);
    tree
}

#[test]
fn backing_scale_physical_size_and_budget_match_the_display() {
    let retina = DisplayMetrics {
        bounds: rect(0.0, 0.0, 1000.0, 800.0),
        pixels_wide: 2000,
        pixels_high: 1600,
    };
    let plain = DisplayMetrics {
        bounds: rect(1000.0, 0.0, 800.0, 600.0),
        pixels_wide: 800,
        pixels_high: 600,
    };
    assert_eq!(
        backing_scale(rect(10.0, 10.0, 100.0, 80.0), &[retina, plain]),
        2.0
    );
    assert_eq!(
        backing_scale(rect(1100.0, 10.0, 100.0, 80.0), &[retina, plain]),
        1.0
    );
    assert_eq!(
        backing_scale(rect(900.0, 0.0, 200.0, 100.0), &[retina, plain]),
        2.0
    );
    assert_eq!(backing_scale(rect(10.0, 10.0, 40.0, 40.0), &[]), 1.0);
    assert_eq!(physical_size(100.2, 40.2, 2.0), Some((200, 80)));
    assert_eq!(physical_size(0.0, 10.0, 2.0), None);
    assert_eq!(capture_budget(NATIVE_TIMEOUT), DRIVER_DEADLINE);
    assert!(DRIVER_DEADLINE < NATIVE_TIMEOUT);
    let config = screenshot_config(200, 80);
    assert!(!config.shows_cursor && !config.show_mouse_clicks && !config.scales_to_fit);
    assert_eq!(config.pixel_format, PIXEL_FORMAT_BGRA);
    assert_eq!(config.width, 200);
    assert!(is_screen_recording_denial(SCREEN_RECORDING_USER_DECLINED));
    assert!(is_screen_recording_denial(
        SCREEN_RECORDING_MISSING_ENTITLEMENT
    ));
    assert!(!is_screen_recording_denial(-3802));
    let rgba = bgra_to_rgba(&[1, 2, 3, 4], 1, 1, 4).unwrap();
    assert_eq!(rgba, vec![3, 2, 1, 4]);
    assert!(bgra_to_rgba(&[1, 2, 3], 1, 1, 4).is_none());
}

#[tokio::test]
async fn occluded_window_returns_its_own_pixels() {
    let frame = rect(8.0, 12.0, 40.0, 20.0);
    let session = session(frame).await;
    session.fake.paint(
        7,
        Paint {
            own: OWN,
            cover: COVER,
            occluded: true,
            mark: None,
        },
    );
    let capture = session
        .driver
        .capture(&session.win, &CaptureOpts::default(), &Cancel::default())
        .await
        .unwrap();
    let (width, height, pixels) = decode_png(&capture.bytes);
    assert_eq!((width, height), (80, 40));
    assert!(pixels.chunks(4).all(|pixel| pixel == OWN));
    assert_no_color(&pixels, COVER);
    assert_eq!(capture.geometry.scale, 2.0);
    assert_eq!(capture.geometry.frame.w, 80.0);
    assert_eq!(capture.geometry.frame.h, 40.0);
    assert_eq!(capture.geometry.frame.x, 16.0);
    assert_eq!(capture.geometry.frame.y, 24.0);
    assert!(!session.fake.stats().shows_cursor);
    assert_eq!(session.fake.stats().deadline, Some(DRIVER_DEADLINE));
}

#[tokio::test]
async fn retina_points_convert_to_physical_pixels_and_ignore_cursor() {
    let frame = rect(0.0, 0.0, 100.2, 40.2);
    let session = session(frame).await;
    session.fake.paint(7, paint_secret(None));
    let opts = CaptureOpts {
        timeout: NATIVE_TIMEOUT,
        include_cursor: true,
    };
    let capture = session
        .driver
        .capture(&session.win, &opts, &Cancel::default())
        .await
        .unwrap();
    assert_eq!((capture.width, capture.height), (200, 80));
    assert_eq!(capture.geometry.frame.w, f64::from(capture.width));
    assert_eq!(capture.geometry.frame.h, f64::from(capture.height));
    assert_eq!(capture.mime, "image/png");
    assert!(!session.fake.stats().shows_cursor);
    let geom = session.driver.window_geometry(&session.win).await.unwrap();
    assert_eq!(geom.frame.w, 200.0);
    assert_eq!(geom.frame.h, 80.0);
    assert_eq!(geom.scale, 2.0);
}

#[tokio::test]
async fn callback_failure_hides_pixels() {
    let session = session(rect(0.0, 0.0, 20.0, 10.0)).await;
    session.fake.paint(7, paint_secret(None));
    session.fake.set_fault(CaptureFault::CallbackFailed);
    let error = session
        .driver
        .capture(&session.win, &CaptureOpts::default(), &Cancel::default())
        .await
        .unwrap_err();
    assert_eq!(
        error,
        DriverError::Native {
            message: "capture callback failed".into(),
            maybe_executed: false,
        }
    );
    let rendered = format!("{error} {error:?}");
    assert!(!rendered.contains("255, 0, 180"));
    assert!(!rendered.contains("sensitive"));
    assert_eq!(session.fake.stats().published, 0);
    assert!(session.fake.stats().discarded >= 1);
}

#[tokio::test]
async fn cancel_discards_the_image_and_is_not_executed() {
    let session = session(rect(0.0, 0.0, 20.0, 10.0)).await;
    session.fake.paint(7, paint_secret(None));
    session.fake.set_fault(CaptureFault::Hang);
    let cancel = Cancel::default();
    let later = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(20)).await;
        later.cancel();
    });
    let error = session
        .driver
        .capture(
            &session.win,
            &CaptureOpts {
                timeout: NATIVE_TIMEOUT,
                include_cursor: false,
            },
            &cancel,
        )
        .await
        .unwrap_err();
    assert_eq!(
        error,
        DriverError::Cancelled {
            maybe_executed: false,
        }
    );
    assert_eq!(session.fake.stats().published, 0);
    let window = SourceWindow {
        cg_id: 7,
        frame: rect(0.0, 0.0, 20.0, 10.0),
        scale: 2.0,
    };
    session.fake.set_fault(CaptureFault::Hang);
    let direct = Cancel::default();
    let direct_later = direct.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(15)).await;
        direct_later.cancel();
    });
    let direct_error = session
        .fake
        .capture(&window, Duration::from_secs(2), false, &direct)
        .await
        .unwrap_err();
    assert_eq!(
        direct_error,
        DriverError::Cancelled {
            maybe_executed: false,
        }
    );
    assert!(session.fake.stats().discarded >= 1);
    assert_eq!(session.fake.stats().published, 0);
}

#[tokio::test]
async fn short_hang_times_out_at_the_requested_budget() {
    let session = session(rect(0.0, 0.0, 20.0, 10.0)).await;
    session.fake.set_fault(CaptureFault::Hang);
    let error = session
        .driver
        .capture(
            &session.win,
            &CaptureOpts {
                timeout: Duration::from_millis(40),
                include_cursor: false,
            },
            &Cancel::default(),
        )
        .await
        .unwrap_err();
    assert_eq!(
        error,
        DriverError::Timeout {
            maybe_executed: false,
        }
    );
    assert_eq!(
        session.fake.stats().deadline,
        Some(Duration::from_millis(40))
    );
    assert_eq!(session.fake.stats().published, 0);
}

#[tokio::test]
async fn missing_and_revoked_screen_recording_share_one_error() {
    let session = session(rect(0.0, 0.0, 20.0, 10.0)).await;
    session.fake.paint(7, paint_secret(None));
    session.api.update(|state| state.screen_recording = false);
    let missing = session
        .driver
        .capture(&session.win, &CaptureOpts::default(), &Cancel::default())
        .await
        .unwrap_err();
    assert_eq!(
        missing,
        DriverError::PermissionMissing {
            permission: "screen_recording".into(),
        }
    );
    assert_eq!(session.fake.stats().published, 0);

    session.api.update(|state| state.screen_recording = true);
    session
        .driver
        .capture(&session.win, &CaptureOpts::default(), &Cancel::default())
        .await
        .unwrap();
    assert_eq!(session.fake.stats().published, 1);
    session.api.update(|state| state.screen_recording = false);
    let revoked = session
        .driver
        .capture(&session.win, &CaptureOpts::default(), &Cancel::default())
        .await
        .unwrap_err();
    assert_eq!(
        revoked,
        DriverError::PermissionMissing {
            permission: "screen_recording".into(),
        }
    );
    assert_eq!(session.fake.stats().published, 1);

    session.api.update(|state| state.screen_recording = true);
    session
        .fake
        .permission()
        .store(true, std::sync::atomic::Ordering::Release);
    session.fake.set_fault(CaptureFault::RevokeBeforePublish);
    let inflight = session
        .driver
        .capture(&session.win, &CaptureOpts::default(), &Cancel::default())
        .await
        .unwrap_err();
    assert_eq!(
        inflight,
        DriverError::PermissionMissing {
            permission: "screen_recording".into(),
        }
    );
    assert_eq!(session.fake.stats().published, 1);
    assert!(!inflight.maybe_executed());
}

#[tokio::test]
async fn minimized_window_is_not_restored_and_geometry_stays() {
    let frame = rect(4.0, 6.0, 30.0, 16.0);
    let session = session(frame).await;
    session.fake.paint(7, paint_secret(None));
    session.api.update(|state| {
        state.ax_windows.get_mut(&4).unwrap()[0].minimized = true;
    });
    let error = session
        .driver
        .capture(&session.win, &CaptureOpts::default(), &Cancel::default())
        .await
        .unwrap_err();
    assert_eq!(error, DriverError::WindowMinimized);
    assert_eq!(session.fake.stats().published, 0);
    assert!(session.api.state().unwrap().raises.is_empty());
    let geom = session.driver.window_geometry(&session.win).await.unwrap();
    assert_eq!((geom.frame.w, geom.frame.h), (60.0, 32.0));
    assert_eq!(geom.scale, 2.0);

    session.api.update(|state| {
        state.cg_windows.clear();
    });
    let still = session
        .driver
        .capture(&session.win, &CaptureOpts::default(), &Cancel::default())
        .await
        .unwrap_err();
    assert_eq!(still, DriverError::WindowMinimized);
}

#[tokio::test]
async fn closed_window_is_gone_even_if_another_window_is_minimized() {
    let frame = rect(0.0, 0.0, 20.0, 10.0);
    let session = session(frame).await;
    session.api.update(|state| {
        state.cg_windows.clear();
        state.ax_windows.get_mut(&4).unwrap()[0].minimized = false;
        state
            .ax_windows
            .get_mut(&4)
            .unwrap()
            .push(ax_window(1, "Other", frame));
        state.ax_windows.get_mut(&4).unwrap()[1].minimized = true;
    });
    let error = session
        .driver
        .capture(&session.win, &CaptureOpts::default(), &Cancel::default())
        .await
        .unwrap_err();
    assert_eq!(error, DriverError::WindowGone);
}

#[tokio::test]
async fn password_pixels_are_wiped_in_the_image_and_derivatives() {
    let frame = rect(0.0, 0.0, 100.0, 50.0);
    let session = session(frame).await;
    let secret_box = rect(20.0, 16.0, 30.0, 12.0);
    session.fake.paint(7, paint_secret(Some(secret_box)));
    let capture = session
        .driver
        .capture(&session.win, &CaptureOpts::default(), &Cancel::default())
        .await
        .unwrap();
    let (_, _, raw) = decode_png(&capture.bytes);
    assert!(raw.chunks(4).any(|pixel| pixel == SECRET));
    let tree = password_tree(Some(secret_box), 1200);
    let redacted = redact::apply(&tree, CapturedFrame::new(capture, 42, STAMP)).unwrap();
    assert_eq!(redacted.scale_to_window(), 1.0);
    assert_no_color(redacted.pixels(), SECRET);
    assert_no_color(redacted.pixels(), COVER);
    for y in 16..28 {
        for x in 20..50 {
            let index = ((y * 200 + x) * 4) as usize;
            assert_eq!(&redacted.pixels()[index..index + 4], &NEUTRAL_COLOR);
        }
    }
    let thumb = redacted.thumbnail(32).unwrap();
    assert_no_color(thumb.pixels(), SECRET);
    let crop = redacted.approval_crop(secret_box).unwrap();
    assert_no_color(crop.pixels(), SECRET);
    let encoded = redacted.encode(EncodeOptions::default()).unwrap();
    let (_, _, png_pixels) = decode_png(encoded.bytes());
    assert_no_color(&png_pixels, SECRET);
    assert_no_color(&png_pixels, COVER);

    let pruned = password_tree(Some(secret_box), 0);
    assert!(pruned.root.is_none());
    assert_eq!(pruned.sensitive_regions.len(), 1);
    let again = session
        .driver
        .capture(&session.win, &CaptureOpts::default(), &Cancel::default())
        .await
        .unwrap();
    let censored = redact::apply(&pruned, CapturedFrame::new(again, 42, STAMP)).unwrap();
    assert_no_color(censored.pixels(), SECRET);
}

#[tokio::test]
async fn stale_or_unmappable_login_scene_returns_no_image() {
    let frame = rect(0.0, 0.0, 40.0, 20.0);
    let session = session(frame).await;
    let secret_box = rect(4.0, 4.0, 10.0, 8.0);
    session.fake.paint(7, paint_secret(Some(secret_box)));
    let capture = session
        .driver
        .capture(&session.win, &CaptureOpts::default(), &Cancel::default())
        .await
        .unwrap();
    let tree = password_tree(Some(secret_box), 1200);
    let stale = redact::apply(&tree, CapturedFrame::new(capture.clone(), 999, STAMP)).unwrap_err();
    assert_eq!(stale, redact::RedactionError::Stale);
    let skewed = redact::apply(
        &tree,
        CapturedFrame::new(capture.clone(), 42, STAMP + Duration::from_millis(150)),
    )
    .unwrap_err();
    assert_eq!(skewed, redact::RedactionError::Stale);
    let unmapped = password_tree(None, 1200);
    let handoff = redact::apply(&unmapped, CapturedFrame::new(capture, 42, STAMP)).unwrap_err();
    assert_eq!(handoff, redact::RedactionError::NeedsHandoff);
}

#[tokio::test]
async fn screenshot_capability_is_partial_and_input_stays_unavailable() {
    let session = session(rect(0.0, 0.0, 20.0, 10.0)).await;
    let caps = session.driver.capabilities();
    assert!(caps.observe.screenshot);
    assert_eq!(caps.background_operation, BackgroundOperation::Partial);
    assert_eq!(caps.limits.max_image_edge, 2560);
    assert!(!caps.overlay_excluded_from_capture);
    assert!(!caps.user_activity_guard);
    assert_eq!(caps.delivery.foreground, ForegroundCapability::Unavailable);
    let bare = MacosDriver::with_api(Arc::clone(&session.api) as Arc<dyn MacApi>)
        .await
        .unwrap();
    let app = bare
        .resolve_app(&gcu_core::driver::AppQuery::from("Editor"))
        .await
        .unwrap();
    let windows = bare.list_windows(Some(&app)).await.unwrap();
    let known = WindowRef {
        app,
        id: windows[0].id.clone(),
    };
    let error = bare
        .capture(&known, &CaptureOpts::default(), &Cancel::default())
        .await
        .unwrap_err();
    assert_eq!(error, DriverError::CapabilityUnavailable);
}

/// Real ScreenCaptureKit capture. CI compiles this and does not run it: the
/// macOS runner has no Screen Recording permission (`not_run`).
///
/// Grant Screen Recording without waiting for a prompt, build `gcu-fixture`,
/// leave it open, and cover its window completely with another window. The
/// canvas cell color must still be present. See the crate README.
#[cfg(target_os = "macos")]
#[tokio::test]
#[ignore = "needs Screen Recording; macOS CI runner has no TCC grant (not_run)"]
async fn live_sck_occluded_fixture_pixels_match() {
    let driver = MacosDriver::new().await.expect("live driver");
    let mut app = None;
    for _ in 0..40 {
        if let Ok(found) = driver
            .resolve_app(&gcu_core::driver::AppQuery::from("fixture"))
            .await
        {
            app = Some(found);
            break;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    let app = app.expect("gcu-fixture must be running");
    let info = driver.key_window(&app).await.expect("fixture window");
    let win = WindowRef {
        app,
        id: info.id.clone(),
    };
    let capture = driver
        .capture(&win, &CaptureOpts::default(), &Cancel::default())
        .await
        .expect("occluded window capture");
    let (width, height) = physical_size(info.frame.w, info.frame.h, info.scale).expect("size");
    assert_eq!((capture.width, capture.height), (width, height));
    assert_eq!(capture.geometry.frame.w, f64::from(capture.width));
    assert_eq!(capture.geometry.frame.h, f64::from(capture.height));
    let (_, _, pixels) = decode_png(&capture.bytes);
    let cell = [95u8, 115, 135];
    assert!(
        pixels.chunks(4).any(|pixel| pixel[..3] == cell),
        "covered fixture must still show canvas cell (95, 115, 135)"
    );
}
