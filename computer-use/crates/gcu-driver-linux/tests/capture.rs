//! Tests for X11 window capture, EWMH alignment, degradation decisions, and mandatory redaction.
//!
//! Includes:
//! 1. Platform-independent pure logic tests using `FakeX`.
//! 2. Integration with `gcu_core::redact` (pixel scanning, budget pruning, stale timestamps, handoff).
//! 3. Live Linux X11 contract test against `gcu-fixture` inside Xvfb (ignored by default).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use gcu_core::{
    driver::{Capture, CaptureOpts, Driver, DriverError, TreeOpts, WindowRef},
    model::{Fingerprint, Geometry, NativeHandle, NodeValue, RawNode, RawTree, Rect, Role, States},
    redact::{self, CapturedFrame, NEUTRAL_COLOR},
    tree::{Pipeline, ProcessedTree},
};
use gcu_driver_linux::{
    x11::{
        align_windows, capture_window, check_window_visibility, decide_capture_strategy,
        windows::{score_match, AtspiWindowCandidate, XWindowCandidate},
        zpixmap_to_rgba, CaptureStrategy, FakeWindow, FakeX, RawImage, XRootGeometry,
        XWindowGeometry,
    },
    LinuxDriver,
};
use gcu_protocol::{AppInfo, WindowInfo};
use std::{collections::HashMap, sync::Arc, time::Duration};

const SECRET_COLOR: [u8; 4] = [255, 0, 180, 255];
const STAMP: Duration = Duration::from_secs(1);

fn test_root_geom() -> XRootGeometry {
    XRootGeometry {
        root_id: 1,
        width: 1280,
        height: 1024,
        scale: 1.0,
    }
}

fn create_solid_image(w: u32, h: u32, color: [u8; 4]) -> RawImage {
    let mut data = Vec::with_capacity((w * h * 4) as usize);
    for _ in 0..(w * h) {
        data.extend_from_slice(&color);
    }
    RawImage {
        width: w,
        height: h,
        data,
    }
}

fn decode_png_to_rgba(bytes: &[u8]) -> (u32, u32, Vec<u8>) {
    let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    let mut reader = decoder.read_info().expect("read PNG info");
    let buf_size = reader.output_buffer_size().unwrap_or(4096);
    let mut buf = vec![0; buf_size];
    let info = reader.next_frame(&mut buf).expect("read PNG frame");
    buf.truncate(info.buffer_size());
    (info.width, info.height, buf)
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

// ---------------------------------------------------------------------------
// 1. ZPixmap conversion
// ---------------------------------------------------------------------------

#[test]
fn test_zpixmap_to_rgba_truecolor_and_custom_masks() {
    // 2x2 image in BGR0 format: [B, G, R, 0]
    let bgr0_data = vec![
        10, 20, 30, 0, // pixel (0,0): R=30, G=20, B=10
        40, 50, 60, 0, // pixel (1,0): R=60, G=50, B=40
        70, 80, 90, 0, // pixel (0,1): R=90, G=80, B=70
        100, 110, 120, 0, // pixel (1,1): R=120, G=110, B=100
    ];

    let rgba = zpixmap_to_rgba(&bgr0_data, 2, 2, 24, 0x00ff_0000, 0x0000_ff00, 0x0000_00ff)
        .expect("standard TrueColor conversion should succeed");

    assert_eq!(
        rgba,
        vec![30, 20, 10, 255, 60, 50, 40, 255, 90, 80, 70, 255, 120, 110, 100, 255,]
    );

    // Buffer underflow error
    let short_data = vec![1, 2, 3];
    assert!(zpixmap_to_rgba(&short_data, 2, 2, 24, 0xff0000, 0x00ff00, 0x0000ff).is_err());

    // Unsupported depth error
    assert!(zpixmap_to_rgba(&bgr0_data, 2, 2, 16, 0xff0000, 0x00ff00, 0x0000ff).is_err());
}

// ---------------------------------------------------------------------------
// 2. Window Alignment
// ---------------------------------------------------------------------------

#[test]
fn test_score_match_and_align_windows_by_pid_title_and_geometry() {
    let atspi_cands = vec![
        AtspiWindowCandidate {
            path: "/org/a11y/atspi/accessible/win1".into(),
            title: "Editor - main.rs".into(),
            frame: Rect {
                x: 100.0,
                y: 100.0,
                w: 800.0,
                h: 600.0,
            },
            pid: Some(1001),
        },
        AtspiWindowCandidate {
            path: "/org/a11y/atspi/accessible/win2".into(),
            title: "Calculator".into(),
            frame: Rect {
                x: 950.0,
                y: 100.0,
                w: 300.0,
                h: 400.0,
            },
            pid: Some(1002),
        },
    ];

    let x_cands = vec![
        XWindowCandidate {
            id: 0x10001,
            title: Some("Editor - main.rs".into()),
            pid: Some(1001),
            geometry: XWindowGeometry {
                x: 100,
                y: 100,
                width: 800,
                height: 600,
                border_width: 0,
                depth: 24,
            },
            is_mapped: true,
        },
        XWindowCandidate {
            id: 0x10002,
            title: Some("Calculator".into()),
            pid: Some(1002),
            geometry: XWindowGeometry {
                x: 950,
                y: 100,
                width: 300,
                height: 400,
                border_width: 0,
                depth: 24,
            },
            is_mapped: true,
        },
    ];

    assert!(score_match(&atspi_cands[0], &x_cands[0]) >= 100);
    assert!(score_match(&atspi_cands[0], &x_cands[1]) < 0);

    let result = align_windows(&atspi_cands, &x_cands, 1.0);
    assert_eq!(result.windows.len(), 2);
    assert_eq!(result.windows[0].id, "65537"); // 0x10001 in decimal string
    assert_eq!(result.windows[1].id, "65538"); // 0x10002 in decimal string
    assert_eq!(
        result.atspi_to_x.get("/org/a11y/atspi/accessible/win1"),
        Some(&0x10001)
    );
    assert_eq!(
        result.x_to_atspi.get(&0x10001),
        Some(&"/org/a11y/atspi/accessible/win1".to_string())
    );
}

#[test]
fn test_align_windows_retains_atspi_approximation_when_unmatched() {
    let atspi_cands = vec![AtspiWindowCandidate {
        path: "/org/a11y/atspi/accessible/orphan".into(),
        title: "Unmatched Window".into(),
        frame: Rect {
            x: 50.0,
            y: 50.0,
            w: 400.0,
            h: 300.0,
        },
        pid: Some(9999),
    }];

    let x_cands = vec![XWindowCandidate {
        id: 0x20001,
        title: Some("Completely Different".into()),
        pid: Some(8888),
        geometry: XWindowGeometry {
            x: 800,
            y: 800,
            width: 200,
            height: 200,
            border_width: 0,
            depth: 24,
        },
        is_mapped: true,
    }];

    let result = align_windows(&atspi_cands, &x_cands, 1.0);
    assert_eq!(result.windows.len(), 1);
    // Unmatched window retains its AT-SPI path
    assert_eq!(result.windows[0].id, "/org/a11y/atspi/accessible/orphan");
    assert!(result.atspi_to_x.is_empty());
}

// ---------------------------------------------------------------------------
// 3. Visibility and Degradation Decisions
// ---------------------------------------------------------------------------

#[test]
fn test_visibility_and_degradation_decisions() {
    let screen_w = 1280;
    let screen_h = 1024;

    let target_geom = XWindowGeometry {
        x: 100,
        y: 100,
        width: 400,
        height: 300,
        border_width: 0,
        depth: 24,
    };

    // Case 1: Fully visible, no other windows
    let stacking = vec![10];
    let mut geoms = HashMap::new();
    geoms.insert(10, (target_geom, true));
    let vis = check_window_visibility(&target_geom, screen_w, screen_h, &stacking, 10, &geoms);
    assert!(vis.is_fully_on_screen);
    assert!(!vis.is_occluded);
    assert_eq!(
        decide_capture_strategy(false, vis),
        CaptureStrategy::RootCrop
    );
    assert_eq!(
        decide_capture_strategy(true, vis),
        CaptureStrategy::Composite
    );

    // Case 2: Occluded by window 20 above window 10
    let above_geom = XWindowGeometry {
        x: 150,
        y: 150,
        width: 400,
        height: 300,
        border_width: 0,
        depth: 24,
    };
    let stacking = vec![10, 20];
    geoms.insert(20, (above_geom, true));
    let vis = check_window_visibility(&target_geom, screen_w, screen_h, &stacking, 10, &geoms);
    assert!(vis.is_fully_on_screen);
    assert!(vis.is_occluded);
    // Without composite, occluded window MUST degrade to OccludedUnsafe!
    assert_eq!(
        decide_capture_strategy(false, vis),
        CaptureStrategy::OccludedUnsafe
    );
    // With composite, occluded window can be captured safely
    assert_eq!(
        decide_capture_strategy(true, vis),
        CaptureStrategy::Composite
    );

    // Case 3: Partially off-screen
    let offscreen_geom = XWindowGeometry {
        x: -50,
        y: 100,
        width: 400,
        height: 300,
        border_width: 0,
        depth: 24,
    };
    let stacking = vec![10];
    let mut geoms = HashMap::new();
    geoms.insert(10, (offscreen_geom, true));
    let vis = check_window_visibility(&offscreen_geom, screen_w, screen_h, &stacking, 10, &geoms);
    assert!(!vis.is_fully_on_screen);
    assert_eq!(
        decide_capture_strategy(false, vis),
        CaptureStrategy::OccludedUnsafe
    );
}

// ---------------------------------------------------------------------------
// 4. FakeX Capture Logic
// ---------------------------------------------------------------------------

#[test]
fn test_capture_composite_available_captures_occluded_window() {
    let fake_x = FakeX::new(test_root_geom(), true);

    let target_geom = XWindowGeometry {
        x: 100,
        y: 100,
        width: 300,
        height: 200,
        border_width: 0,
        depth: 24,
    };

    let target_img = create_solid_image(300, 200, [10, 20, 30, 255]);
    fake_x.add_window(FakeWindow {
        id: 100,
        name: Some("Target Window".into()),
        pid: Some(1234),
        geometry: target_geom,
        is_mapped: true,
        image: Some(target_img),
    });

    // Occluding window
    fake_x.add_window(FakeWindow {
        id: 200,
        name: Some("Occluding Window".into()),
        pid: Some(5678),
        geometry: XWindowGeometry {
            x: 150,
            y: 150,
            width: 300,
            height: 200,
            border_width: 0,
            depth: 24,
        },
        is_mapped: true,
        image: Some(create_solid_image(300, 200, [90, 90, 90, 255])),
    });

    fake_x.set_stacking_order(vec![100, 200]);

    let capture = capture_window(&fake_x, 100).expect("composite capture should succeed");
    assert_eq!(capture.width, 300);
    assert_eq!(capture.height, 200);
    assert_eq!(capture.mime, "image/png");

    let (w, h, pixels) = decode_png_to_rgba(&capture.bytes);
    assert_eq!((w, h), (300, 200));
    assert_eq!(&pixels[0..4], &[10, 20, 30, 255]);
}

#[test]
fn test_capture_no_composite_occluded_window_fails_with_capability_unavailable() {
    let fake_x = FakeX::new(test_root_geom(), false);

    fake_x.add_window(FakeWindow {
        id: 100,
        name: Some("Target Window".into()),
        pid: Some(1234),
        geometry: XWindowGeometry {
            x: 100,
            y: 100,
            width: 300,
            height: 200,
            border_width: 0,
            depth: 24,
        },
        is_mapped: true,
        image: None,
    });

    fake_x.add_window(FakeWindow {
        id: 200,
        name: Some("Occluding Window".into()),
        pid: Some(5678),
        geometry: XWindowGeometry {
            x: 150,
            y: 150,
            width: 300,
            height: 200,
            border_width: 0,
            depth: 24,
        },
        is_mapped: true,
        image: None,
    });

    fake_x.set_stacking_order(vec![100, 200]);

    let err =
        capture_window(&fake_x, 100).expect_err("must refuse occluded capture without composite");
    assert_eq!(err, DriverError::CapabilityUnavailable);
}

#[test]
fn test_capture_no_composite_fully_visible_window_succeeds_with_root_crop() {
    let fake_x = FakeX::new(test_root_geom(), false);

    fake_x.add_window(FakeWindow {
        id: 100,
        name: Some("Top Window".into()),
        pid: Some(1234),
        geometry: XWindowGeometry {
            x: 50,
            y: 50,
            width: 200,
            height: 150,
            border_width: 0,
            depth: 24,
        },
        is_mapped: true,
        image: None,
    });

    fake_x.set_stacking_order(vec![100]);

    // Create root image with a distinct color patch at the window's position
    let mut root_data = vec![200u8; (1280 * 1024 * 4) as usize];
    for y in 50..200 {
        for x in 50..250 {
            let idx = ((y * 1280 + x) * 4) as usize;
            root_data[idx] = 42;
            root_data[idx + 1] = 84;
            root_data[idx + 2] = 126;
            root_data[idx + 3] = 255;
        }
    }
    fake_x.set_root_image(RawImage {
        width: 1280,
        height: 1024,
        data: root_data,
    });

    let capture = capture_window(&fake_x, 100).expect("root crop must succeed when fully visible");
    assert_eq!(capture.width, 200);
    assert_eq!(capture.height, 150);

    let (w, h, pixels) = decode_png_to_rgba(&capture.bytes);
    assert_eq!((w, h), (200, 150));
    assert_eq!(&pixels[0..4], &[42, 84, 126, 255]);
}

// ---------------------------------------------------------------------------
// 5. Mandatory Redaction Integration
// ---------------------------------------------------------------------------

fn make_password_tree(frame: Option<Rect>, max_nodes: usize) -> ProcessedTree {
    let mut n = make_raw_node(Role::TextField, "Password", frame, States::PASSWORD);
    n.value = Some(NodeValue::Text("sensitive_pass".into()));
    n.text_extras = Some(gcu_core::model::TextExtras {
        text: Some("sensitive_pass".into()),
        ..Default::default()
    });

    let mut tree = Pipeline::default().run(
        RawTree {
            root: Some(n),
            incomplete: None,
        },
        &TreeOpts {
            max_tree_nodes: max_nodes,
            ..Default::default()
        },
    );
    tree.state_version = Some(42);
    tree.observed_at = Some(STAMP);
    tree
}

fn create_capture_with_secret(w: u32, h: u32, secret_rect: Rect) -> Capture {
    let mut data = vec![245u8; (w * h * 4) as usize];
    for y in 0..h {
        for x in 0..w {
            if f64::from(x) >= secret_rect.x
                && f64::from(x) < secret_rect.x + secret_rect.w
                && f64::from(y) >= secret_rect.y
                && f64::from(y) < secret_rect.y + secret_rect.h
            {
                let idx = ((y * w + x) * 4) as usize;
                data[idx..idx + 4].copy_from_slice(&SECRET_COLOR);
            }
        }
    }

    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, w, h);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(&data).unwrap();
        writer.finish().unwrap();
    }

    Capture {
        bytes,
        width: w,
        height: h,
        mime: "image/png".into(),
        geometry: Geometry {
            frame: Rect {
                x: 0.0,
                y: 0.0,
                w: f64::from(w),
                h: f64::from(h),
            },
            scale: 1.0,
        },
    }
}

#[test]
fn test_capture_redaction_pipeline_mandatory_fills_neutral_color() {
    let secret_box = Rect {
        x: 20.0,
        y: 20.0,
        w: 60.0,
        h: 20.0,
    };
    let tree = make_password_tree(Some(secret_box), 1200);
    let raw_cap = create_capture_with_secret(200, 100, secret_box);

    let frame = CapturedFrame::new(raw_cap, 42, STAMP);
    let redacted = redact::apply(&tree, frame).expect("redaction must succeed");

    // Scan pixels: SECRET_COLOR MUST NOT remain in the redacted output!
    assert!(
        !redacted.pixels().as_chunks::<4>().0.contains(&SECRET_COLOR),
        "secret color must be completely absent from redacted pixels"
    );

    // Assert that the sensitive region is filled with NEUTRAL_COLOR
    for y in 20..40 {
        for x in 20..80 {
            let idx = ((y * 200 + x) * 4) as usize;
            assert_eq!(
                &redacted.pixels()[idx..idx + 4],
                &NEUTRAL_COLOR,
                "sensitive pixel at ({x},{y}) must be neutral color"
            );
        }
    }
}

#[test]
fn test_capture_redact_with_budget_removed_password_censors_pixels_and_secret_absent() {
    let secret_box = Rect {
        x: 10.0,
        y: 10.0,
        w: 50.0,
        h: 20.0,
    };
    // Tree nodes budgeted to 0 (pruned out of the visible tree)
    let tree = make_password_tree(Some(secret_box), 0);
    assert!(tree.root.is_none());
    assert_eq!(tree.sensitive_regions.len(), 1);

    let raw_cap = create_capture_with_secret(150, 80, secret_box);
    let frame = CapturedFrame::new(raw_cap, 42, STAMP);
    let redacted =
        redact::apply(&tree, frame).expect("redaction must succeed even if node budgeted out");

    assert!(
        !redacted.pixels().as_chunks::<4>().0.contains(&SECRET_COLOR),
        "secret color must be absent even when password node was budgeted out"
    );
}

#[test]
fn test_capture_redact_stale_skew_or_version_mismatch_fails_closed() {
    let secret_box = Rect {
        x: 10.0,
        y: 10.0,
        w: 30.0,
        h: 20.0,
    };
    let tree = make_password_tree(Some(secret_box), 1200);
    let raw_cap = create_capture_with_secret(100, 60, secret_box);

    // Version mismatch
    let frame_wrong_version = CapturedFrame::new(raw_cap.clone(), 999, STAMP);
    assert_eq!(
        redact::apply(&tree, frame_wrong_version).unwrap_err(),
        redact::RedactionError::Stale
    );

    // Timestamp skew > 100ms
    let frame_skewed = CapturedFrame::new(raw_cap, 42, STAMP + Duration::from_millis(150));
    assert_eq!(
        redact::apply(&tree, frame_skewed).unwrap_err(),
        redact::RedactionError::Stale
    );
}

#[test]
fn test_capture_redact_login_unmappable_sensitive_location_requires_handoff() {
    // Unmappable sensitive region (None or NaN frame)
    let tree = make_password_tree(None, 1200);
    let raw_cap = create_capture_with_secret(
        100,
        60,
        Rect {
            x: 10.0,
            y: 10.0,
            w: 20.0,
            h: 20.0,
        },
    );

    let frame = CapturedFrame::new(raw_cap, 42, STAMP);
    let err = redact::apply(&tree, frame).expect_err("unmappable password must fail");
    assert_eq!(err, redact::RedactionError::NeedsHandoff);
}

// ---------------------------------------------------------------------------
// 6. LinuxDriver Full Integration with FakeX
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_linux_driver_fake_x_integration() {
    let fake_source = gcu_driver_linux::atspi::source::FakeSource::default();
    let root_node = gcu_driver_linux::atspi::source::FakeNode {
        id: 1,
        role: atspi::Role::Frame,
        name: Some("Test App".into()),
        extents: Some(Rect {
            x: 100.0,
            y: 100.0,
            w: 600.0,
            h: 400.0,
        }),
        children: vec![],
        ..Default::default()
    };

    let app_info = AppInfo {
        id: "org.test.app".into(),
        name: "Test App".into(),
        pid: Some(3001),
        identity: None,
    };

    let fake_win = WindowInfo {
        id: "/org/a11y/atspi/accessible/root".into(),
        title: "Test App Window".into(),
        frame: Rect {
            x: 100.0,
            y: 100.0,
            w: 600.0,
            h: 400.0,
        },
        scale: 1.0,
        on_current_desktop: Some(true),
    };

    let fake_x = Arc::new(FakeX::new(test_root_geom(), true));
    fake_x.add_window(FakeWindow {
        id: 0x30001,
        name: Some("Test App Window".into()),
        pid: Some(3001),
        geometry: XWindowGeometry {
            x: 100,
            y: 100,
            width: 600,
            height: 400,
            border_width: 0,
            depth: 24,
        },
        is_mapped: true,
        image: Some(create_solid_image(600, 400, [50, 100, 150, 255])),
    });
    fake_x.set_active_window(Some(0x30001));

    let driver = LinuxDriver::new_fake(
        fake_source,
        root_node,
        vec![app_info.clone()],
        vec![("org.test.app".into(), fake_win)],
    )
    .with_x_server(fake_x);

    let app_ref = driver
        .resolve_app(&gcu_core::driver::AppQuery("Test App".into()))
        .await
        .expect("must resolve app");

    let windows = driver
        .list_windows(Some(&app_ref))
        .await
        .expect("list_windows should succeed");
    assert_eq!(windows.len(), 1);
    // Window ID is upgraded to X11 decimal string
    assert_eq!(windows[0].id, "196609"); // 0x30001

    let key_win = driver
        .key_window(&app_ref)
        .await
        .expect("key_window should succeed");
    assert_eq!(key_win.id, "196609");

    let win_ref = WindowRef {
        app: app_ref,
        id: key_win.id,
    };

    let capture = driver
        .capture(
            &win_ref,
            &CaptureOpts::default(),
            &gcu_core::driver::Cancel::default(),
        )
        .await
        .expect("capture must succeed");

    assert_eq!(capture.width, 600);
    assert_eq!(capture.height, 400);

    let geom = driver
        .window_geometry(&win_ref)
        .await
        .expect("window_geometry should succeed");
    assert_eq!(geom.frame.w, 600.0);
    assert_eq!(geom.frame.h, 400.0);
}

// ---------------------------------------------------------------------------
// 7. Live X11 Contract Test inside Xvfb (Ignored by default, run in CI)
// ---------------------------------------------------------------------------

#[cfg(target_os = "linux")]
#[tokio::test]
#[ignore]
async fn live_x11_fixture_window_capture_and_canvas_grid() {
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{ConnectionExt as _, CreateWindowAux, WindowClass};
    use x11rb::wrapper::ConnectionExt as _;

    let driver = LinuxDriver::connect()
        .await
        .expect("must connect to live AT-SPI and X11");

    // Discover fixture app
    let mut fixture_app = None;
    for _ in 0..40 {
        if let Ok(app) = driver
            .resolve_app(&gcu_core::driver::AppQuery("fixture".into()))
            .await
        {
            fixture_app = Some(app);
            break;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    let app = fixture_app.expect("gcu-fixture application must be discoverable");

    let win_info = driver
        .key_window(&app)
        .await
        .expect("fixture key window must be discoverable");

    // Assert that the window ID is an exact numeric X11 ID
    assert!(
        win_info.id.parse::<u32>().is_ok(),
        "fixture window id must be an upgraded numeric X11 ID, got: {}",
        win_info.id
    );

    let win_ref = WindowRef {
        app: app.clone(),
        id: win_info.id.clone(),
    };

    let cancel = gcu_core::driver::Cancel::default();
    let capture = driver
        .capture(&win_ref, &CaptureOpts::default(), &cancel)
        .await
        .expect("live window capture must succeed");

    assert_eq!(
        capture.width, win_info.frame.w as u32,
        "capture width must equal window physical width"
    );
    assert_eq!(
        capture.height, win_info.frame.h as u32,
        "capture height must equal window physical height"
    );

    let (w, h, pixels) = decode_png_to_rgba(&capture.bytes);
    assert_eq!((w, h), (capture.width, capture.height));

    // Check for canvas grid color:
    // Cell (c=0, r=0) color from gcu-fixture:
    // red = ((0 * 31 + 45) % 180 + 50) = 95
    // green = ((0 * 41 + 65) % 180 + 50) = 115
    // blue = (((0 + 0) * 29 + 85) % 180 + 50) = 135
    let cell_0_0_color = [95u8, 115u8, 135u8];
    let contains_canvas_cell_color = pixels.as_chunks::<4>().0.iter().any(|p| {
        p[0] == cell_0_0_color[0] && p[1] == cell_0_0_color[1] && p[2] == cell_0_0_color[2]
    });

    assert!(
        contains_canvas_cell_color,
        "fixture capture must contain known canvas grid pastel cell color (95, 115, 135)"
    );

    // Test occlusion under XComposite:
    // Connect to X11 and create a covering window that overlaps the fixture window
    let x_conn = x11rb::connect(None).expect("must connect to X11 for test helper");
    let (conn, screen_num) = x_conn;
    let screen = &conn.setup().roots[screen_num];
    let cover_win = conn.generate_id().expect("generate_id for covering window");

    let win_x = win_info.frame.x as i16;
    let win_y = win_info.frame.y as i16;

    let aux = CreateWindowAux::new()
        .background_pixel(screen.black_pixel)
        .override_redirect(1);

    conn.create_window(
        screen.root_depth,
        cover_win,
        screen.root,
        win_x + 50,
        win_y + 50,
        200,
        200,
        0,
        WindowClass::INPUT_OUTPUT,
        screen.root_visual,
        &aux,
    )
    .expect("create covering window");

    conn.map_window(cover_win).expect("map covering window");
    conn.sync().expect("sync after mapping covering window");

    tokio::time::sleep(Duration::from_millis(150)).await;

    // Capture fixture while occluded: under XComposite, capture MUST still succeed
    // and deliver the fixture window's actual contents, not the black covering window!
    let occluded_capture = driver
        .capture(&win_ref, &CaptureOpts::default(), &cancel)
        .await
        .expect("capture must succeed even when occluded by covering window under XComposite");

    let (_, _, occluded_pixels) = decode_png_to_rgba(&occluded_capture.bytes);

    assert!(
        occluded_pixels
            .as_chunks::<4>()
            .0
            .iter()
            .any(|p| p[0] == cell_0_0_color[0]
                && p[1] == cell_0_0_color[1]
                && p[2] == cell_0_0_color[2]),
        "occluded fixture capture under XComposite must still contain its own canvas grid color"
    );

    // Clean up covering window
    let _ = conn.destroy_window(cover_win);
    let _ = conn.sync();
}
