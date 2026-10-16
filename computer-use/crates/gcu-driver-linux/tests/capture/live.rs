//! Native X11 pixels and the shared fixture accessibility contract.
use super::*;

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
    // The preceding AT-SPI conformance test leaves its modal open. Also exercise
    // that same state when this test is run alone, using the real action path.
    let raw = driver
        .snapshot_tree(&win_ref, &TreeOpts::default(), &cancel)
        .await
        .unwrap();
    let root = raw.root.as_ref().expect("fixture tree root");
    if named(root, "modal_confirm_button").is_none() {
        let open = named(root, "open_modal_button").expect("modal trigger");
        driver
            .perform(
                &gcu_core::driver::DriverAction::Press {
                    el: open.handle.clone(),
                },
                &cancel,
            )
            .await
            .unwrap();
    }
    let modal = modal_tree(&driver, &win_ref, true).await;
    let root = modal.root.as_ref().unwrap();
    let canvas = named(root, "grid_canvas").expect("canvas image remains in modal tree");
    assert_eq!(canvas.role, Role::Image);
    assert!(
        canvas.children.is_empty(),
        "canvas must expose no cell children"
    );
    assert!(
        has_dialog(root),
        "AT-SPI must retain the actual modal dialog role"
    );
    let capture = canvas_capture(&driver, &win_ref).await;

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

    let confirm = named(root, "modal_confirm_button").expect("modal confirm");
    driver
        .perform(
            &gcu_core::driver::DriverAction::Press {
                el: confirm.handle.clone(),
            },
            &cancel,
        )
        .await
        .unwrap();
    let closed = modal_tree(&driver, &win_ref, false).await;
    let canvas = named(closed.root.as_ref().unwrap(), "grid_canvas").unwrap();
    assert_eq!(canvas.role, Role::Image);
    assert!(canvas.children.is_empty());
    canvas_capture(&driver, &win_ref).await;
}

fn named<'a>(node: &'a RawNode, name: &str) -> Option<&'a RawNode> {
    if node.name.as_deref() == Some(name) {
        return Some(node);
    }
    node.children.iter().find_map(|child| named(child, name))
}
fn has_dialog(node: &RawNode) -> bool {
    node.role == Role::Dialog || node.children.iter().any(has_dialog)
}
async fn modal_tree(driver: &LinuxDriver, win: &WindowRef, open: bool) -> RawTree {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        let raw = driver
            .snapshot_tree(win, &TreeOpts::default(), &Default::default())
            .await
            .unwrap();
        let root = raw.root.as_ref().expect("fixture tree root");
        if named(root, "modal_confirm_button").is_some() == open {
            return raw;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "modal accessibility state did not become open={open}"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}
async fn canvas_capture(driver: &LinuxDriver, win: &WindowRef) -> Capture {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        let capture = driver
            .capture(win, &CaptureOpts::default(), &Default::default())
            .await
            .expect("live window capture must succeed");
        let (_, _, pixels) = decode_png_to_rgba(&capture.bytes);
        if pixels
            .as_chunks::<4>()
            .0
            .iter()
            .any(|p| p[..3] == [95, 115, 135])
        {
            return capture;
        }
        // Tree publication can precede GPU presentation. Read again under a
        // fixed deadline; keep the exact calibrated RGB assertion, with no
        // pixel tolerance or replay of the modal action.
        assert!(
            tokio::time::Instant::now() < deadline,
            "fixture capture must contain known canvas grid pastel cell color (95, 115, 135)"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}
