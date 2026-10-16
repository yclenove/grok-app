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

#[path = "capture/integration.rs"]
mod integration;
#[cfg(target_os = "linux")]
#[path = "capture/live.rs"]
mod live;
