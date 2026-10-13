//! Redaction is mandatory before every image derivative and snapshot hash.
#![allow(clippy::unwrap_used, clippy::expect_used)]
mod support;
use gcu_core::{
    driver::*,
    model::*,
    redact::{
        self, CapturedFrame, EncodeOptions, ImageFormat, RedactionError, MAX_STATE_SKEW,
        NEUTRAL_COLOR,
    },
    render::{render_structured, render_text},
    snapshot::{SnapshotStore, WindowKey},
    tree::{Pipeline, ProcessedTree, SensitiveRegion},
};
use gcu_driver_fake::FakeDriver;
use image::{ImageBuffer, Rgba, RgbaImage};
use proptest::prelude::*;
use std::{io::Cursor, time::Duration};
use support::*;
const SECRET: [u8; 4] = [255, 0, 180, 255];
const STAMP: Duration = Duration::from_secs(1);

fn password(frame: Option<Rect>) -> RawNode {
    let mut n = node(Role::TextField, "Password", vec![]);
    n.states |= States::PASSWORD;
    n.frame = frame;
    n.value = Some(NodeValue::Text("never-log-password".into()));
    n.text_extras = Some(TextExtras {
        text: Some("never-log-password".into()),
        ..Default::default()
    });
    n
}
fn processed(n: RawNode, max_nodes: usize) -> ProcessedTree {
    let mut tree = Pipeline::default().run(
        raw(vec![n]),
        &TreeOpts {
            max_tree_nodes: max_nodes,
            ..Default::default()
        },
    );
    tree.state_version = Some(7);
    tree.observed_at = Some(STAMP);
    tree
}
fn capture(pixels: RgbaImage) -> CapturedFrame {
    let (width, height) = pixels.dimensions();
    let mut bytes = Cursor::new(Vec::new());
    pixels
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
    CapturedFrame::new(
        Capture {
            bytes: bytes.into_inner(),
            width,
            height,
            mime: "image/png".into(),
            geometry: Geometry {
                frame: Rect {
                    x: 0.,
                    y: 0.,
                    w: width as f64,
                    h: height as f64,
                },
                scale: 2.,
            },
        },
        7,
        STAMP,
    )
}
fn frame() -> Rect {
    Rect {
        x: 10.,
        y: 10.,
        w: 20.,
        h: 20.,
    }
}
fn marked(rect: Rect) -> RgbaImage {
    let mut pixels = ImageBuffer::from_pixel(64, 64, Rgba([245, 245, 245, 255]));
    for (x, y, p) in pixels.enumerate_pixels_mut() {
        if f64::from(x) >= rect.x
            && f64::from(x) < rect.x + rect.w
            && f64::from(y) >= rect.y
            && f64::from(y) < rect.y + rect.h
        {
            *p = Rgba(SECRET);
        }
    }
    pixels
}
fn assert_clean(image: &redact::RedactedImage) {
    assert!(!image.pixels().as_chunks::<4>().0.contains(&SECRET));
    assert!(!format!("{image:?}").contains("never-log-password"));
    for format in [ImageFormat::Png, ImageFormat::Jpeg] {
        let encoded = image
            .encode(EncodeOptions {
                format,
                ..Default::default()
            })
            .unwrap();
        let decoded = image::load_from_memory(encoded.bytes()).unwrap().to_rgba8();
        assert!(!decoded.pixels().any(|p| p.0 == SECRET));
        assert!(!format!("{encoded:?}").contains("never-log-password"));
        assert!(encoded.bytes().len() <= 1_500_000);
    }
}

#[test]
fn redact_password_before_resize_fills_region_and_four_pixel_padding() {
    let result = redact::apply(
        &processed(password(Some(frame())), 1200),
        capture(marked(frame())),
    )
    .unwrap();
    assert_clean(&result);
    for y in 6..34 {
        for x in 6..34 {
            assert_eq!(
                &result.pixels()[(y * 64 + x) * 4..(y * 64 + x) * 4 + 4],
                NEUTRAL_COLOR
            );
        }
    }
    assert_ne!(
        &result.pixels()[(5 * 64 + 5) * 4..(5 * 64 + 5) * 4 + 4],
        NEUTRAL_COLOR
    );
    assert_clean(&result.thumbnail(16).unwrap());
    let crop = result.approval_crop(frame()).unwrap();
    assert!(crop.width().min(crop.height()) >= 256);
    assert_clean(&crop);
    assert_eq!(result.thumbnail(32).unwrap().scale_to_window(), 2.);
}

#[test]
fn redact_budget_removed_password_still_censors_pixels() {
    let tree = processed(password(Some(frame())), 0);
    assert!(tree.root.is_none());
    assert_eq!(tree.sensitive_regions.len(), 1);
    assert_clean(&redact::apply(&tree, capture(marked(frame()))).unwrap());
}

#[test]
fn redact_any_sensitive_region_source_censors_pixels() {
    let mut tree = processed(node(Role::Button, "Safe", vec![]), 1200);
    tree.sensitive_regions.push(SensitiveRegion {
        fingerprint: Fingerprint(999),
        frame: Some(frame()),
    });
    assert_clean(&redact::apply(&tree, capture(marked(frame()))).unwrap());
}

#[test]
fn redact_missing_nonfinite_outside_frames_require_handoff() {
    for region in [
        None,
        Some(Rect {
            x: f64::NAN,
            ..frame()
        }),
        Some(Rect { x: 100., ..frame() }),
        Some(Rect { w: 0., ..frame() }),
    ] {
        let error = redact::apply(&processed(password(region), 1200), capture(marked(frame())))
            .unwrap_err();
        assert_eq!(error, RedactionError::NeedsHandoff);
        assert!(!format!("{error:?} {error}").contains("never-log-password"));
    }
}

#[test]
fn redact_stale_version_or_timestamp_and_unstamped_tree_deliver_no_image() {
    let tree = processed(password(Some(frame())), 1200);
    let mut frame_capture = capture(marked(frame()));
    frame_capture.state_version = 8;
    assert_eq!(
        redact::apply(&tree, frame_capture).unwrap_err(),
        RedactionError::Stale
    );
    for timestamp in [
        STAMP + MAX_STATE_SKEW + Duration::from_millis(1),
        STAMP - MAX_STATE_SKEW - Duration::from_millis(1),
    ] {
        let mut frame_capture = capture(marked(frame()));
        frame_capture.captured_at = timestamp;
        assert_eq!(
            redact::apply(&tree, frame_capture).unwrap_err(),
            RedactionError::Stale
        );
    }
    let mut unstamped = tree.clone();
    unstamped.state_version = None;
    assert_eq!(
        redact::apply(&unstamped, capture(marked(frame()))).unwrap_err(),
        RedactionError::Stale
    );
    let mut boundary = capture(marked(frame()));
    boundary.captured_at += MAX_STATE_SKEW;
    assert!(redact::apply(&tree, boundary).is_ok());
}

#[test]
fn redact_geometry_decode_dimensions_and_mime_fail_closed() {
    let tree = processed(password(Some(frame())), 1200);
    let mut mismatched = capture(marked(frame()));
    mismatched.capture.width = 63;
    assert_eq!(
        redact::apply(&tree, mismatched).unwrap_err(),
        RedactionError::InvalidCapture
    );
    let mut mismatched = capture(marked(frame()));
    mismatched.capture.geometry.scale = f64::NAN;
    assert_eq!(
        redact::apply(&tree, mismatched).unwrap_err(),
        RedactionError::InvalidCapture
    );
    let mut invalid = capture(marked(frame()));
    invalid.capture.bytes = b"secret-corrupt-image".to_vec();
    let error = redact::apply(&tree, invalid).unwrap_err();
    assert_eq!(error, RedactionError::InvalidCapture);
    assert!(!format!("{error}").contains("secret-corrupt-image"));
    let mut invalid = capture(marked(frame()));
    invalid.capture.mime = "image/gif".into();
    assert_eq!(
        redact::apply(&tree, invalid).unwrap_err(),
        RedactionError::InvalidCapture
    );
}

#[test]
fn snapshot_hash_records_redacted_physical_patch_and_detects_content_change() {
    let mut button = node(Role::Button, "Press", vec![]);
    button.frame = Some(frame());
    button.actions.push(NodeAction::Press);
    let tree = processed(button, 1200);
    let a = redact::apply(&tree, capture(marked(frame()))).unwrap();
    let mut pixels = marked(frame());
    pixels.put_pixel(20, 20, Rgba([10, 20, 30, 255]));
    let b = redact::apply(&tree, capture(pixels)).unwrap();
    let center = WindowPoint { x: 20., y: 20. };
    assert_ne!(
        a.target_hash(center).unwrap(),
        b.target_hash(center).unwrap()
    );
    let expected = a.target_hash(center).unwrap();
    let mut opts = opts();
    opts.window.frame.w = 64.;
    opts.window.frame.h = 64.;
    opts.window.scale = 2.;
    let mut store = SnapshotStore::default();
    let observation = store.record(tree, &opts, Some(a)).unwrap();
    let snapshot = store
        .get(&WindowKey::from_options(&opts), observation.snapshot_id)
        .unwrap();
    assert_eq!(snapshot.target_hash(1), Some(expected));
    assert!(snapshot.screenshot().is_some());
    assert!(store.memory_bytes() >= 64 * 64 * 4);
}

#[tokio::test]
async fn redact_fake_login_sensitive_color_absent_from_all_derivatives() {
    let driver = FakeDriver::from_scene("login").unwrap();
    let app = driver
        .resolve_app(&AppQuery("fake.login".into()))
        .await
        .unwrap();
    let window = driver.key_window(&app).await.unwrap();
    let win = WindowRef {
        app: app.clone(),
        id: window.id.clone(),
    };
    let raw = driver
        .snapshot_tree(&win, &TreeOpts::default(), &Cancel::default())
        .await
        .unwrap();
    let mut tree = Pipeline::default().run(raw, &TreeOpts::default());
    tree.state_version = Some(7);
    tree.observed_at = Some(STAMP);
    let captured = driver
        .capture(&win, &CaptureOpts::default(), &Cancel::default())
        .await
        .unwrap();
    assert!(image::load_from_memory(&captured.bytes)
        .unwrap()
        .to_rgba8()
        .pixels()
        .any(|p| p.0 == SECRET));
    let result = redact::apply(&tree, CapturedFrame::new(captured, 7, STAMP)).unwrap();
    assert_clean(&result);
    assert_clean(&result.thumbnail(128).unwrap());
    assert_clean(
        &result
            .approval_crop(tree.sensitive_regions[0].frame.unwrap())
            .unwrap(),
    );
    let options = gcu_core::render::RenderOpts::new(app.info, window);
    let text = render_text(&tree, &options);
    let structured = render_structured(&tree, &options).to_string();
    assert!(!text.contains("fixture-secret"));
    assert!(!structured.contains("fixture-secret"));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(96))]
    #[test]
    fn redact_random_clipped_missing_and_outside_frames_never_leak(x in -40i32..90, y in -40i32..90, w in 1u32..40, h in 1u32..40, missing in any::<bool>(), budget in 0usize..3) {
        let region = Rect { x:x as f64,y:y as f64,w:w as f64,h:h as f64 };
        let tree = processed(password((!missing).then_some(region)),budget);
        let result = redact::apply(&tree,capture(marked(region)));
        if let Ok(result) = result {
            prop_assert!(!missing);
            prop_assert!(!result.pixels().as_chunks::<4>().0.contains(&SECRET));
            prop_assert!(!result.thumbnail(16).unwrap().pixels().as_chunks::<4>().0.contains(&SECRET));
        } else {
            prop_assert_eq!(result.unwrap_err(),RedactionError::NeedsHandoff);
        }
        let options = opts();
        prop_assert!(!render_text(&tree,&options).contains("never-log-password"));
        prop_assert!(!render_structured(&tree,&options).to_string().contains("never-log-password"));
        let debug = format!("{tree:?}");
        prop_assert!(!debug.contains("never-log-password"));
    }
}

#[test]
fn redact_password_patch_hash_depends_only_on_safe_pixels() {
    let tree = processed(password(Some(frame())), 1200);
    let mut changed = marked(frame());
    for y in 10..30 {
        for x in 10..30 {
            changed.put_pixel(x, y, Rgba([17, 231, 13, 255]));
        }
    }
    let a = redact::apply(&tree, capture(marked(frame()))).unwrap();
    let b = redact::apply(&tree, capture(changed)).unwrap();
    assert_eq!(a.pixels(), b.pixels());
    assert_eq!(
        a.target_hash(WindowPoint { x: 20., y: 20. }).unwrap(),
        b.target_hash(WindowPoint { x: 20., y: 20. }).unwrap()
    );
    assert_eq!(
        a.target_hash(WindowPoint { x: 0., y: 0. }),
        b.target_hash(WindowPoint { x: 0., y: 0. })
    );
    assert_eq!(
        a.target_hash(WindowPoint { x: 64., y: 0. }).unwrap_err(),
        RedactionError::OutOfBounds
    );
    assert_eq!(
        a.thumbnail(32)
            .unwrap()
            .target_hash(WindowPoint { x: 0., y: 0. })
            .unwrap_err(),
        RedactionError::InvalidCapture
    );
}

#[test]
fn redact_encode_noise_reduces_size_and_preserves_coordinate_mapping() {
    let tree = processed(node(Role::Window, "Safe", vec![]), 1200);
    let mut noise = ImageBuffer::new(512, 256);
    let mut state = 0x12345678u32;
    for (_, _, pixel) in noise.enumerate_pixels_mut() {
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state as u8
        };
        *pixel = Rgba([next(), next(), next(), 255]);
    }
    let safe = redact::apply(&tree, capture(noise)).unwrap();
    let encoded = safe.encode(EncodeOptions::default()).unwrap();
    assert!(encoded.reduced());
    assert!(encoded.bytes().len() <= 300_000);
    assert!(encoded.scale_to_window() > 1.);
    assert_eq!(encoded.origin(), WindowPoint { x: 0., y: 0. });
    assert_eq!(encoded.mime(), "image/png");
    assert_eq!(
        safe.encode(EncodeOptions {
            max_bytes: 1,
            ..Default::default()
        })
        .unwrap_err(),
        RedactionError::Encoding
    );
    let crop = safe
        .approval_crop(Rect {
            x: 10.,
            y: 10.,
            w: 20.,
            h: 20.,
        })
        .unwrap();
    assert!(crop.encode(EncodeOptions::default()).unwrap().can_attach());
}

#[test]
fn snapshot_rejects_image_version_geometry_and_derivative_without_mutation() {
    let tree = processed(node(Role::Button, "Safe", vec![]), 1200);
    let image = redact::apply(&tree, capture(marked(frame()))).unwrap();
    let mut options = opts();
    options.window.frame.w = 64.;
    options.window.frame.h = 64.;
    options.window.scale = 2.;
    let mut store = SnapshotStore::default();
    let mut stale = tree.clone();
    stale.state_version = Some(8);
    assert!(matches!(
        store.record(stale, &options, Some(image.clone())),
        Err(gcu_core::snapshot::SnapshotError::StaleScreenshot)
    ));
    let mut geometry = options.clone();
    geometry.window.scale = 1.;
    assert!(matches!(
        store.record(tree.clone(), &geometry, Some(image.clone())),
        Err(gcu_core::snapshot::SnapshotError::StaleScreenshot)
    ));
    assert!(matches!(
        store.record(tree, &options, Some(image.thumbnail(32).unwrap())),
        Err(gcu_core::snapshot::SnapshotError::StaleScreenshot)
    ));
    assert!(store.is_empty());
}

#[test]
fn redact_uncollected_password_and_pruned_sensitive_region_still_fail_closed() {
    let mut missing = processed(password(None), 1200);
    missing.sensitive_regions.clear();
    missing.sensitive_regions_reliable = true;
    assert_eq!(
        redact::apply(&missing, capture(marked(frame()))).unwrap_err(),
        RedactionError::NeedsHandoff
    );
    let mut hidden = password(Some(frame()));
    hidden.states |= States::OFFSCREEN;
    let mut hidden = processed(hidden, 1200);
    // P0-06 deliberately protects PASSWORD nodes from pruning. Collected region
    // metadata must still survive if a downstream adapter clears that state flag.
    assert!(hidden
        .nodes()
        .iter()
        .any(|(_, n)| n.node.states.contains(States::PASSWORD)));
    hidden.root.as_mut().unwrap().children[0]
        .node
        .states
        .remove(States::PASSWORD);
    let hidden =
        gcu_core::tree::transforms::prune_offscreen::transform(hidden, &TreeOpts::default());
    let hidden =
        gcu_core::tree::transforms::assign_indices::transform(hidden, &TreeOpts::default());
    assert_eq!(hidden.sensitive_regions.len(), 1);
    assert!(hidden.root.as_ref().unwrap().children.is_empty());
    assert!(hidden
        .nodes()
        .iter()
        .all(|(_, n)| !n.node.states.contains(States::PASSWORD)));
    assert_clean(&redact::apply(&hidden, capture(marked(frame()))).unwrap());
}
