//! Raw image Debug must not print pixels.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;

use gcu_driver_linux::wayland::{RawCapturedFrame, VideoTransform};
use gcu_driver_linux::x11::RawImage;

#[test]
fn raw_image_and_frame_debug_omit_pixels() {
    let secret = b"SUPER-SECRET-PIXELS-99".to_vec();
    let image = RawImage {
        width: 2,
        height: 2,
        data: secret.clone(),
    };
    let rendered = format!("{image:?}");
    assert!(!rendered.contains("SUPER-SECRET-PIXELS-99"));
    assert!(rendered.contains("byte_count"));
    let frame = RawCapturedFrame {
        stream_node_id: 1,
        sequence: 2,
        width: 2,
        height: 2,
        crop_region: None,
        transform: VideoTransform::Normal,
        captured_at_secs: 3,
        rgba: Arc::from(secret.into_boxed_slice()),
    };
    let rendered = format!("{frame:?}");
    assert!(!rendered.contains("SUPER-SECRET-PIXELS-99"));
    assert!(rendered.contains("rgba_len"));
}
