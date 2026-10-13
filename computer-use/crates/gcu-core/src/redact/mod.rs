//! Raw captures have no delivery methods. Only this module can mint a safe image.
//! All operations are synchronous, bounded CPU work with no IO, waits, or retries.
//! The observer must bracket tree/capture reads with a trusted state version and
//! monotonic timestamps; missing metadata or a changed version fails closed.
//!
//! External crates cannot construct a redacted image, even from plausible bytes:
//! ```compile_fail
//! use gcu_core::redact::RedactedImage;
//! let forged = RedactedImage { pixels: image::RgbaImage::new(1, 1) };
//! ```
//! Nor can raw captures enter the derivative/delivery API:
//! ```compile_fail
//! use gcu_core::{driver::Capture, redact::{RedactedImage, EncodeOptions}};
//! fn deliver(raw: &Capture) { RedactedImage::encode(raw, EncodeOptions::default()); }
//! ```

/// Scaling, cropping, encoding, and physical target hashes of safe pixels.
mod images;
/// Fail-closed physical region validation, clipping, and padding.
mod regions;
use crate::{
    driver::Capture,
    model::{Geometry, WindowPoint},
    tree::ProcessedTree,
};
pub use images::{EncodeOptions, EncodedImage, ImageFormat, PixelHash};
use std::{fmt, time::Duration};

/// Maximum tree/capture skew within the same trusted state version.
pub const MAX_STATE_SKEW: Duration = Duration::from_millis(100);
/// Opaque, irreversible fill; alpha is always fully opaque.
pub const NEUTRAL_COLOR: [u8; 4] = [128, 128, 128, 255];
/// Expand each sensitive region by four physical pixels before clipping.
pub const REGION_PADDING: f64 = 4.;
/// Hard bounds for decoding raw captures (including decompression bombs).
pub const MAX_CAPTURE_BYTES: usize = 128 * 1024 * 1024;

/// Driver capture plus trusted observer metadata. This is unredacted input only.
/// Debug never emits bytes, MIME payloads, or tree text.
pub struct CapturedFrame {
    /// Window capture from Driver; consumes its encoded pixels during apply.
    pub capture: Capture,
    /// Version observed for the capture; must equal tree.state_version.
    pub state_version: u64,
    /// Capture timestamp on the same monotonic session clock as tree.observed_at.
    pub captured_at: Duration,
}
impl CapturedFrame {
    /// Attach trusted read metadata without changing the native Driver interface.
    pub fn new(capture: Capture, state_version: u64, captured_at: Duration) -> Self {
        Self {
            capture,
            state_version,
            captured_at,
        }
    }
}
impl fmt::Debug for CapturedFrame {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CapturedFrame")
            .field("state_version", &self.state_version)
            .field("captured_at", &self.captured_at)
            .finish_non_exhaustive()
    }
}

/// Payload-free failure; errors never include codec errors or captured bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum RedactionError {
    /// Missing or unmappable sensitive location. Ask the user to handle the scene.
    #[error("sensitive region cannot be located; handoff required")]
    NeedsHandoff,
    /// Missing version/timestamp, version mismatch, or excessive timestamp skew.
    #[error("tree and screenshot are stale")]
    Stale,
    /// Invalid encoding, geometry, dimensions, or allocation bounds.
    #[error("invalid window capture")]
    InvalidCapture,
    /// Requested derivative rectangle or point lies outside the window.
    #[error("image region is out of bounds")]
    OutOfBounds,
    /// No encoding could satisfy the delivery bound.
    #[error("safe image could not be encoded within the size limit")]
    Encoding,
}

/// The only input type accepted by every image delivery/derivative operation.
/// Fields and construction are private to redact (including within gcu-core).
/// Pixels cannot be mutated by callers. Clone preserves the redaction guarantee.
#[derive(Clone)]
pub struct RedactedImage {
    pixels: image::RgbaImage,
    geometry: Geometry,
    state_version: u64,
    captured_at: Duration,
    origin: WindowPoint,
    scale_to_window: f64,
}
impl fmt::Debug for RedactedImage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RedactedImage")
            .field("width", &self.width())
            .field("height", &self.height())
            .field("state_version", &self.state_version)
            .finish_non_exhaustive()
    }
}

/// Validate tree/capture synchronization and all raw sensitive regions, then fill
/// at physical resolution. No image is returned on any error, including a password
/// removed by pruning/budget. Call before scaling, cropping, encoding, or storing.
/// Bounded synchronous work; no IO, blocking native calls, or cancellation waits.
pub fn apply(tree: &ProcessedTree, frame: CapturedFrame) -> Result<RedactedImage, RedactionError> {
    if tree.state_version != Some(frame.state_version)
        || tree
            .observed_at
            .is_none_or(|t| t.abs_diff(frame.captured_at) > MAX_STATE_SKEW)
    {
        return Err(RedactionError::Stale);
    }
    if !tree.sensitive_regions_reliable {
        return Err(RedactionError::NeedsHandoff);
    }
    let capture = frame.capture;
    let geometry = capture.geometry;
    let rect = geometry.frame;
    if ![rect.x, rect.y, rect.w, rect.h, geometry.scale]
        .iter()
        .all(|v| v.is_finite())
        || geometry.scale <= 0.
        || capture.width == 0
        || capture.height == 0
        || rect.w != f64::from(capture.width)
        || rect.h != f64::from(capture.height)
        || capture.width > 16_384
        || capture.height > 16_384
        || u64::from(capture.width) * u64::from(capture.height) * 4 > MAX_CAPTURE_BYTES as u64
        || capture.bytes.len() > MAX_CAPTURE_BYTES
    {
        return Err(RedactionError::InvalidCapture);
    }
    let mut rectangles = Vec::new();
    for region in &tree.sensitive_regions {
        rectangles.push(regions::sensitive(
            region.frame,
            capture.width,
            capture.height,
        )?);
    }
    // Defense in depth for manually assembled ProcessedTree values: PASSWORD may
    // never escape just because its collector entry was omitted by a caller.
    for (_, node) in tree.nodes() {
        if node.node.states.contains(crate::model::States::PASSWORD) {
            rectangles.push(regions::sensitive(
                node.node.frame,
                capture.width,
                capture.height,
            )?);
        }
    }
    let format = match capture.mime.as_str() {
        "image/png" => image::ImageFormat::Png,
        "image/jpeg" => image::ImageFormat::Jpeg,
        _ => return Err(RedactionError::InvalidCapture),
    };
    let mut reader = image::ImageReader::with_format(std::io::Cursor::new(capture.bytes), format);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(capture.width);
    limits.max_image_height = Some(capture.height);
    limits.max_alloc = Some(MAX_CAPTURE_BYTES as u64);
    reader.limits(limits);
    let mut pixels = reader
        .decode()
        .map_err(|_| RedactionError::InvalidCapture)?
        .to_rgba8();
    if pixels.dimensions() != (capture.width, capture.height) {
        return Err(RedactionError::InvalidCapture);
    }
    for (x0, y0, x1, y1) in rectangles {
        for y in y0..y1 {
            for x in x0..x1 {
                pixels.put_pixel(x, y, image::Rgba(NEUTRAL_COLOR));
            }
        }
    }
    Ok(RedactedImage {
        pixels,
        geometry,
        state_version: frame.state_version,
        captured_at: frame.captured_at,
        origin: WindowPoint { x: 0., y: 0. },
        scale_to_window: 1.,
    })
}
