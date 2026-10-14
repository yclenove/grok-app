//! Capture backend, physical-pixel geometry, and the portable fake.
//!
//! [`CaptureSource`] returns tightly packed RGBA. The driver encodes PNG and
//! fills [`gcu_core::driver::Capture`]. Redaction stays in `gcu_core::redact`.
//! `Geometry.scale` is the display backing factor. Pixels are already physical,
//! so a later [`gcu_core::redact::RedactedImage::scale_to_window`] starts at 1.
//! The observer stamps `state_version` and the timestamp on
//! [`gcu_core::redact::CapturedFrame`]; this driver does not.

use async_trait::async_trait;
use gcu_core::{
    driver::{Cancel, Capture, DriverError, Result, DRIVER_DEADLINE},
    model::{Geometry, Rect},
};
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

/// `kCVPixelFormatType_32BGRA` (`'BGRA'`). ScreenCaptureKit's SDR `CGImage` layout.
pub const PIXEL_FORMAT_BGRA: u32 = 0x4247_5241;
/// `SCStreamError.userDeclined`. Screen Recording was refused.
pub const SCREEN_RECORDING_USER_DECLINED: isize = -3801;
/// `SCStreamError.missingEntitlements`. The process cannot capture the screen.
pub const SCREEN_RECORDING_MISSING_ENTITLEMENT: isize = -3803;

/// True for the ScreenCaptureKit codes that mean Screen Recording is absent.
pub fn is_screen_recording_denial(code: isize) -> bool {
    code == SCREEN_RECORDING_USER_DECLINED || code == SCREEN_RECORDING_MISSING_ENTITLEMENT
}

/// One display in the same point space as Core Graphics window frames.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DisplayMetrics {
    /// Display bounds in points. Origin matches `CGWindow` frames.
    pub bounds: Rect,
    /// Physical width from `CGDisplayPixelsWide`.
    pub pixels_wide: u32,
    /// Physical height from `CGDisplayPixelsHigh`.
    pub pixels_high: u32,
}

/// Backing scale of the display that covers most of `window`.
///
/// Empty or non-overlapping lists use `1.0`. The factor is
/// `pixels_wide / bounds.w`.
pub fn backing_scale(window: Rect, displays: &[DisplayMetrics]) -> f64 {
    let mut best_area = 0.0;
    let mut best = 1.0;
    for display in displays {
        let area = intersection_area(window, display.bounds);
        if area > best_area {
            best_area = area;
            best = display_scale(display);
        }
    }
    if best.is_finite() && best > 0.0 {
        best
    } else {
        1.0
    }
}

/// Point size times `scale`, rounded to physical pixels. Rejects empty and huge frames.
pub fn physical_size(points_w: f64, points_h: f64, scale: f64) -> Option<(u32, u32)> {
    if !points_w.is_finite() || !points_h.is_finite() || !scale.is_finite() || scale <= 0.0 {
        return None;
    }
    let width = (points_w * scale).round();
    let height = (points_h * scale).round();
    if !(1.0..=16_384.0).contains(&width) || !(1.0..=16_384.0).contains(&height) {
        return None;
    }
    Some((width as u32, height as u32))
}

/// Screen geometry whose width and height are the physical pixel counts.
pub fn physical_geometry(frame: Rect, scale: f64, width: u32, height: u32) -> Geometry {
    Geometry {
        frame: Rect {
            x: frame.x * scale,
            y: frame.y * scale,
            w: f64::from(width),
            h: f64::from(height),
        },
        scale,
    }
}

/// Inner capture budget. Never longer than [`DRIVER_DEADLINE`].
pub fn capture_budget(requested: Duration) -> Duration {
    requested.min(DRIVER_DEADLINE)
}

/// Pixel format and cursor policy applied to every screenshot.
///
/// `shows_cursor` is always false. [`gcu_core::driver::CaptureOpts::include_cursor`]
/// does not override it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScreenshotConfig {
    /// Output width in physical pixels.
    pub width: usize,
    /// Output height in physical pixels.
    pub height: usize,
    /// Cursor pixels are omitted.
    pub shows_cursor: bool,
    /// Click rings are omitted.
    pub show_mouse_clicks: bool,
    /// Output is not scaled down to fit a smaller buffer.
    pub scales_to_fit: bool,
    /// Fourcc passed to `setPixelFormat:`.
    pub pixel_format: u32,
}

/// Configuration for one window. Cursor and click rings stay off.
pub fn screenshot_config(width: u32, height: u32) -> ScreenshotConfig {
    ScreenshotConfig {
        width: width as usize,
        height: height as usize,
        shows_cursor: false,
        show_mouse_clicks: false,
        scales_to_fit: false,
        pixel_format: PIXEL_FORMAT_BGRA,
    }
}

/// Copy a BGRA buffer into tightly packed RGBA. `None` when the stride is short.
pub fn bgra_to_rgba(src: &[u8], width: u32, height: u32, stride: usize) -> Option<Vec<u8>> {
    let width = width as usize;
    let height = height as usize;
    let row_bytes = width.checked_mul(4)?;
    if stride < row_bytes || height.saturating_mul(stride) > src.len() {
        return None;
    }
    let mut out = vec![0u8; width * height * 4];
    for y in 0..height {
        let row = &src[y * stride..y * stride + row_bytes];
        for x in 0..width {
            let s = x * 4;
            let d = (y * width + x) * 4;
            out[d] = row[s + 2];
            out[d + 1] = row[s + 1];
            out[d + 2] = row[s];
            out[d + 3] = row[s + 3];
        }
    }
    Some(out)
}

/// Encode RGBA into a PNG [`Capture`]. The MIME is `image/png`.
pub fn encode_capture(rgba: &[u8], width: u32, height: u32, geometry: Geometry) -> Result<Capture> {
    if rgba.len() != (width as usize) * (height as usize) * 4 || width == 0 || height == 0 {
        return Err(DriverError::Native {
            message: "capture buffer does not match its dimensions".into(),
            maybe_executed: false,
        });
    }
    let mut bytes = Vec::new();
    let mut encoder = png::Encoder::new(&mut bytes, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(|_| DriverError::Native {
        message: "png encode failed".into(),
        maybe_executed: false,
    })?;
    writer
        .write_image_data(rgba)
        .map_err(|_| DriverError::Native {
            message: "png encode failed".into(),
            maybe_executed: false,
        })?;
    writer.finish().map_err(|_| DriverError::Native {
        message: "png encode failed".into(),
        maybe_executed: false,
    })?;
    Ok(Capture {
        bytes,
        width,
        height,
        mime: "image/png".into(),
        geometry,
    })
}

/// Window identity handed to a [`CaptureSource`]. Frames are points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SourceWindow {
    /// `CGWindowID`.
    pub cg_id: u32,
    /// Screen rectangle in points.
    pub frame: Rect,
    /// Display backing factor recorded with the window.
    pub scale: f64,
}

/// Tightly packed RGBA plus the physical size that was captured.
#[derive(Clone, PartialEq, Eq)]
pub struct RawPixels {
    /// RGBA8, length `width * height * 4`.
    pub rgba: Vec<u8>,
    /// Physical width.
    pub width: u32,
    /// Physical height.
    pub height: u32,
}

impl std::fmt::Debug for RawPixels {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RawPixels")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("bytes", &self.rgba.len())
            .finish()
    }
}

/// One screenshot backend. Occlusion must not refuse the capture.
#[async_trait]
pub trait CaptureSource: Send + Sync {
    /// Screen Recording preflight. Must not prompt or click a system dialog.
    fn screen_recording_ok(&self) -> bool;

    /// Capture `window` within `deadline`. `shows_cursor` is ignored by production
    /// and recorded by [`FakeCapture`]. Cancellation and timeout drop pixels.
    async fn capture(
        &self,
        window: &SourceWindow,
        deadline: Duration,
        shows_cursor: bool,
        cancel: &Cancel,
    ) -> Result<RawPixels>;
}

/// How a [`FakeCapture`] fails a call.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CaptureFault {
    /// Return the painted window.
    #[default]
    None,
    /// Callback returned an error. No pixels are published.
    CallbackFailed,
    /// Wait until `deadline` or cancellation.
    Hang,
    /// Clear Screen Recording, then refuse before publishing pixels.
    RevokeBeforePublish,
}

/// Pixels that belong to the target window, plus a cover color that must not appear.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Paint {
    /// The window's own color. Used even when [`Self::occluded`] is set.
    pub own: [u8; 4],
    /// A color that belongs to a window above this one. Never copied into the buffer.
    pub cover: [u8; 4],
    /// Another window completely covers this one. Capture still returns [`Self::own`].
    pub occluded: bool,
    /// Optional window-relative physical rectangle painted over [`Self::own`].
    pub mark: Option<([u8; 4], Rect)>,
}

/// Counters from a [`FakeCapture`]. Debug and Display of driver errors stay separate.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CaptureStats {
    /// Captures whose pixels were returned to the caller.
    pub published: u64,
    /// Captures whose pixels were dropped.
    pub discarded: u64,
    /// Deadline of the latest call.
    pub deadline: Option<Duration>,
    /// Last `shows_cursor` argument. Production passes `false`.
    pub shows_cursor: bool,
}

/// In-memory window capture. Runs on every platform.
pub struct FakeCapture {
    permission: Arc<AtomicBool>,
    published: AtomicU64,
    discarded: AtomicU64,
    inner: Mutex<FakeInner>,
}

struct FakeInner {
    fault: CaptureFault,
    paints: HashMap<u32, Paint>,
    deadline: Option<Duration>,
    shows_cursor: bool,
}

impl Default for FakeCapture {
    fn default() -> Self {
        Self::new()
    }
}

impl FakeCapture {
    /// Granted Screen Recording, no painted windows.
    pub fn new() -> Self {
        Self {
            permission: Arc::new(AtomicBool::new(true)),
            published: AtomicU64::new(0),
            discarded: AtomicU64::new(0),
            inner: Mutex::new(FakeInner {
                fault: CaptureFault::None,
                paints: HashMap::new(),
                deadline: None,
                shows_cursor: false,
            }),
        }
    }

    /// Shared flag. Store `false` to revoke Screen Recording during a call.
    pub fn permission(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.permission)
    }

    /// Replace the failure mode. Does not change painted pixels.
    pub fn set_fault(&self, fault: CaptureFault) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.fault = fault;
        }
    }

    /// Install the pixels for `cg_id`.
    pub fn paint(&self, cg_id: u32, paint: Paint) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.paints.insert(cg_id, paint);
        }
    }

    /// Published and discarded counts, plus the last deadline.
    pub fn stats(&self) -> CaptureStats {
        let (deadline, shows_cursor) = self
            .inner
            .lock()
            .map(|inner| (inner.deadline, inner.shows_cursor))
            .unwrap_or((None, false));
        CaptureStats {
            published: self.published.load(Ordering::Acquire),
            discarded: self.discarded.load(Ordering::Acquire),
            deadline,
            shows_cursor,
        }
    }

    fn note(&self, deadline: Duration, shows_cursor: bool) -> Result<CaptureFault> {
        let mut inner = self.inner.lock().map_err(|_| DriverError::Crashed {
            maybe_executed: false,
        })?;
        inner.deadline = Some(deadline);
        inner.shows_cursor = shows_cursor;
        Ok(inner.fault)
    }

    fn paint_of(&self, cg_id: u32) -> Result<Option<Paint>> {
        let inner = self.inner.lock().map_err(|_| DriverError::Crashed {
            maybe_executed: false,
        })?;
        Ok(inner.paints.get(&cg_id).copied())
    }

    fn discard(&self) {
        self.discarded.fetch_add(1, Ordering::AcqRel);
    }

    fn publish(&self) {
        self.published.fetch_add(1, Ordering::AcqRel);
    }
}

#[async_trait]
impl CaptureSource for FakeCapture {
    fn screen_recording_ok(&self) -> bool {
        self.permission.load(Ordering::Acquire)
    }

    async fn capture(
        &self,
        window: &SourceWindow,
        deadline: Duration,
        shows_cursor: bool,
        cancel: &Cancel,
    ) -> Result<RawPixels> {
        let fault = self.note(deadline, shows_cursor)?;
        if cancel.is_cancelled() {
            self.discard();
            return Err(DriverError::Cancelled {
                maybe_executed: false,
            });
        }
        if !self.screen_recording_ok() {
            self.discard();
            return Err(permission_missing());
        }
        match fault {
            CaptureFault::CallbackFailed => {
                self.discard();
                Err(DriverError::Native {
                    message: "capture callback failed".into(),
                    maybe_executed: false,
                })
            }
            CaptureFault::Hang => {
                tokio::select! {
                    biased;
                    _ = cancel.cancelled() => {
                        self.discard();
                        Err(DriverError::Cancelled { maybe_executed: false })
                    }
                    _ = tokio::time::sleep(deadline) => {
                        self.discard();
                        Err(DriverError::Timeout { maybe_executed: false })
                    }
                }
            }
            CaptureFault::RevokeBeforePublish => {
                self.permission.store(false, Ordering::Release);
                self.discard();
                Err(permission_missing())
            }
            CaptureFault::None => {
                let Some(paint) = self.paint_of(window.cg_id)? else {
                    self.discard();
                    return Err(DriverError::WindowGone);
                };
                // `paint.occluded` does not change the buffer. Covered windows
                // still return `paint.own`, never `paint.cover`.
                let Some((width, height)) =
                    physical_size(window.frame.w, window.frame.h, window.scale)
                else {
                    self.discard();
                    return Err(DriverError::Native {
                        message: "window frame is not a physical size".into(),
                        maybe_executed: false,
                    });
                };
                if !self.screen_recording_ok() || cancel.is_cancelled() {
                    self.discard();
                    return Err(if cancel.is_cancelled() {
                        DriverError::Cancelled {
                            maybe_executed: false,
                        }
                    } else {
                        permission_missing()
                    });
                }
                let rgba = render(paint, width, height);
                self.publish();
                Ok(RawPixels {
                    rgba,
                    width,
                    height,
                })
            }
        }
    }
}

fn permission_missing() -> DriverError {
    DriverError::PermissionMissing {
        permission: "screen_recording".into(),
    }
}

fn render(paint: Paint, width: u32, height: u32) -> Vec<u8> {
    let mut data = Vec::with_capacity((width * height * 4) as usize);
    for _ in 0..width * height {
        data.extend_from_slice(&paint.own);
    }
    let Some((color, rect)) = paint.mark else {
        return data;
    };
    for y in 0..height {
        for x in 0..width {
            let px = f64::from(x);
            let py = f64::from(y);
            if px >= rect.x && px < rect.x + rect.w && py >= rect.y && py < rect.y + rect.h {
                let index = ((y * width + x) * 4) as usize;
                data[index..index + 4].copy_from_slice(&color);
            }
        }
    }
    data
}

fn display_scale(display: &DisplayMetrics) -> f64 {
    if display.bounds.w <= f64::EPSILON {
        return 1.0;
    }
    let scale = f64::from(display.pixels_wide) / display.bounds.w;
    if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    }
}

fn intersection_area(left: Rect, right: Rect) -> f64 {
    let x1 = left.x.max(right.x);
    let y1 = left.y.max(right.y);
    let x2 = (left.x + left.w).min(right.x + right.w);
    let y2 = (left.y + left.h).min(right.y + right.h);
    (x2 - x1).max(0.0) * (y2 - y1).max(0.0)
}
