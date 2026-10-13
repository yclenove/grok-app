use super::{regions, RedactedImage, RedactionError};
use crate::model::{Geometry, Rect, WindowPoint};
use image::{
    codecs::jpeg::JpegEncoder,
    imageops::{self, FilterType},
};
use std::{fmt, io::Cursor, time::Duration};

/// Stable FNV-1a-64 of a redacted physical patch. No dependency or license addition.
/// This is a content-change detector, not an authentication/cryptographic primitive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PixelHash(pub u64);
/// Encodings supported by the mandatory image delivery pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageFormat {
    /// Lossless PNG, the default.
    Png,
    /// JPEG with initial quality 82.
    Jpeg,
}
/// Bounded image delivery options; cannot bypass prior redaction.
#[derive(Debug, Clone, Copy)]
pub struct EncodeOptions {
    /// Preferred encoding (PNG by default).
    pub format: ImageFormat,
    /// Initial maximum long edge (1568 by default).
    pub max_edge: u32,
    /// Byte target, capped at the architecture's hard 1.5 MB maximum.
    pub max_bytes: usize,
}
impl Default for EncodeOptions {
    fn default() -> Self {
        Self {
            format: ImageFormat::Png,
            max_edge: 1568,
            max_bytes: 300_000,
        }
    }
}

/// Encoded safe pixels for model images, thumbnails, approvals, and CLI files.
/// Construction is private to redact; Host controls file permissions and cleanup.
/// Debug reports metadata only. Neither this type nor RedactedImage is Deserialize.
pub struct EncodedImage {
    bytes: Vec<u8>,
    width: u32,
    height: u32,
    format: ImageFormat,
    scale_to_window: f64,
    origin: WindowPoint,
    reduced: bool,
}
impl fmt::Debug for EncodedImage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EncodedImage")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("byte_count", &self.bytes.len())
            .field("reduced", &self.reduced)
            .finish()
    }
}
impl EncodedImage {
    /// Safe encoded bytes; no filesystem/network IO performed here.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    /// Encoded pixel width.
    pub fn width(&self) -> u32 {
        self.width
    }
    /// Encoded pixel height.
    pub fn height(&self) -> u32 {
        self.height
    }
    /// PNG or JPEG MIME, independent of driver-provided MIME.
    pub fn mime(&self) -> &'static str {
        match self.format {
            ImageFormat::Png => "image/png",
            ImageFormat::Jpeg => "image/jpeg",
        }
    }
    /// Physical window pixels per encoded pixel.
    pub fn scale_to_window(&self) -> f64 {
        self.scale_to_window
    }
    /// Window-relative physical origin of the encoded crop.
    pub fn origin(&self) -> WindowPoint {
        self.origin
    }
    /// True when the size limit required additional scaling or JPEG quality reduction.
    pub fn reduced(&self) -> bool {
        self.reduced
    }
    /// Whether Grok's image attachment size threshold is met; otherwise use a Host
    /// session-private file reference to these same safe bytes (architecture §4.4).
    pub fn can_attach(&self) -> bool {
        self.width.min(self.height) >= 256
            && u64::from(self.width) * u64::from(self.height) >= 60_000
    }
}
impl RedactedImage {
    /// Physical or derivative image width.
    pub fn width(&self) -> u32 {
        self.pixels.width()
    }
    /// Physical or derivative image height.
    pub fn height(&self) -> u32 {
        self.pixels.height()
    }
    /// Immutable redacted RGBA bytes. There is no mutable/raw-image escape hatch.
    pub fn pixels(&self) -> &[u8] {
        self.pixels.as_raw()
    }
    /// Original window geometry, used for stale-geometry checks.
    pub fn geometry(&self) -> Geometry {
        self.geometry
    }
    /// Trusted captured version.
    pub fn state_version(&self) -> u64 {
        self.state_version
    }
    /// Trusted timestamp on the monotonic session clock.
    pub fn captured_at(&self) -> Duration {
        self.captured_at
    }
    /// Physical window pixels per image pixel (before logical DPI conversion).
    pub fn scale_to_window(&self) -> f64 {
        self.scale_to_window
    }
    /// Physical window origin; nonzero for cropped derivatives.
    pub fn origin(&self) -> WindowPoint {
        self.origin
    }
    /// Owned image memory used by the session retention estimate.
    pub fn memory_bytes(&self) -> usize {
        self.pixels.len()
    }
    pub(crate) fn is_window_capture(&self) -> bool {
        self.origin == (WindowPoint { x: 0., y: 0. })
            && self.scale_to_window == 1.
            && self.geometry.frame.w == f64::from(self.width())
            && self.geometry.frame.h == f64::from(self.height())
    }
    fn resized(&self, width: u32, height: u32) -> Self {
        Self {
            pixels: imageops::resize(&self.pixels, width, height, FilterType::Triangle),
            scale_to_window: self.scale_to_window * f64::from(self.width()) / f64::from(width),
            ..self.clone()
        }
    }
    /// Downscale a safe image to the given long edge. Never exposes raw captures.
    /// Synchronous bounded CPU work; no IO or cancellation waits.
    pub fn thumbnail(&self, max_edge: u32) -> Result<Self, RedactionError> {
        if max_edge == 0 {
            return Err(RedactionError::OutOfBounds);
        }
        let edge = self.width().max(self.height());
        if edge <= max_edge {
            return Ok(self.clone());
        }
        let scale = f64::from(max_edge) / f64::from(edge);
        Ok(self.resized(
            (f64::from(self.width()) * scale).round().max(1.) as u32,
            (f64::from(self.height()) * scale).round().max(1.) as u32,
        ))
    }
    /// Crop in physical window pixels, then enlarge to short edge >=256 for model
    /// visibility. Valid intersections are clipped at the image edge. No IO.
    pub fn approval_crop(&self, rect: Rect) -> Result<Self, RedactionError> {
        let local = Rect {
            x: (rect.x - self.origin.x) / self.scale_to_window,
            y: (rect.y - self.origin.y) / self.scale_to_window,
            w: rect.w / self.scale_to_window,
            h: rect.h / self.scale_to_window,
        };
        let (x0, y0, x1, y1) = regions::bounds(local, self.width(), self.height(), 0.)?;
        let cropped = Self {
            pixels: imageops::crop_imm(&self.pixels, x0, y0, x1 - x0, y1 - y0).to_image(),
            origin: WindowPoint {
                x: self.origin.x + f64::from(x0) * self.scale_to_window,
                y: self.origin.y + f64::from(y0) * self.scale_to_window,
            },
            ..self.clone()
        };
        let short = cropped.width().min(cropped.height());
        if short >= 256 {
            return Ok(cropped);
        }
        let factor = 256. / f64::from(short);
        let width = (f64::from(cropped.width()) * factor).ceil() as u32;
        let height = (f64::from(cropped.height()) * factor).ceil() as u32;
        if u64::from(width) * u64::from(height) * 4 > super::MAX_CAPTURE_BYTES as u64 {
            return Err(RedactionError::OutOfBounds);
        }
        Ok(cropped.resized(width, height))
    }
    /// Hash a clipped 32x32 physical patch around an arbitrary coordinate target.
    /// Requires the original window image (hash before scaling/cropping). This also
    /// lets ActionRouter recheck coordinate targets without an accessibility node.
    pub fn target_hash(&self, at: WindowPoint) -> Result<PixelHash, RedactionError> {
        if !self.is_window_capture() {
            return Err(RedactionError::InvalidCapture);
        }
        if !at.x.is_finite()
            || !at.y.is_finite()
            || at.x < 0.
            || at.y < 0.
            || at.x >= f64::from(self.width())
            || at.y >= f64::from(self.height())
        {
            return Err(RedactionError::OutOfBounds);
        }
        let x = at.x.floor() as i64;
        let y = at.y.floor() as i64;
        let x0 = (x - 16).max(0) as u32;
        let y0 = (y - 16).max(0) as u32;
        let x1 = (x + 16).min(i64::from(self.width())) as u32;
        let y1 = (y + 16).min(i64::from(self.height())) as u32;
        let mut hash = 0xcbf29ce484222325u64;
        let mut feed = |byte: u8| {
            hash = (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3);
        };
        for coord in [x0, y0, x1, y1] {
            for byte in coord.to_le_bytes() {
                feed(byte);
            }
        }
        for y in y0..y1 {
            for x in x0..x1 {
                for byte in self.pixels.get_pixel(x, y).0 {
                    feed(byte);
                }
            }
        }
        Ok(PixelHash(hash))
    }
    /// Scale/encode already-safe pixels, reducing JPEG quality or image dimensions
    /// until within the bound. Returned metadata discloses reduction. No IO; errors
    /// contain no image or codec payload. Raw Capture cannot call this API.
    pub fn encode(&self, opts: EncodeOptions) -> Result<EncodedImage, RedactionError> {
        let bound = opts.max_bytes.min(1_500_000);
        if bound == 0 || opts.max_edge == 0 {
            return Err(RedactionError::Encoding);
        }
        let mut image = self.thumbnail(opts.max_edge)?;
        let mut quality = 82;
        let mut reduced = false;
        loop {
            let mut output = Cursor::new(Vec::new());
            match opts.format {
                ImageFormat::Png => image
                    .pixels
                    .write_to(&mut output, image::ImageFormat::Png)
                    .map_err(|_| RedactionError::Encoding)?,
                ImageFormat::Jpeg => JpegEncoder::new_with_quality(&mut output, quality)
                    .encode_image(&image::DynamicImage::ImageRgba8(image.pixels.clone()))
                    .map_err(|_| RedactionError::Encoding)?,
            }
            let bytes = output.into_inner();
            if bytes.len() <= bound {
                return Ok(EncodedImage {
                    bytes,
                    width: image.width(),
                    height: image.height(),
                    format: opts.format,
                    scale_to_window: image.scale_to_window,
                    origin: image.origin,
                    reduced,
                });
            }
            reduced = true;
            if opts.format == ImageFormat::Jpeg && quality > 40 {
                quality -= 14;
                continue;
            }
            if image.width() == 1 && image.height() == 1 {
                return Err(RedactionError::Encoding);
            }
            image = image.resized(
                (image.width() * 3 / 4).max(1),
                (image.height() * 3 / 4).max(1),
            );
        }
    }
}
