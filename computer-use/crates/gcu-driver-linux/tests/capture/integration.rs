//! Redaction and driver integration regressions.
use super::*;

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
