//! Window-only capture. Raw PNGs must enter `gcu_core::redact` before delivery.
//! The observer owns state-version/timestamp stamps; the driver never mints them.
//! Both paths omit cursors, retain physical resolution, and never restore windows.
mod fake;
#[cfg(windows)]
mod ffi;
mod observe;
#[cfg(windows)]
mod print_window;
mod source;
#[cfg(windows)]
mod wgc;

pub use fake::{CaptureFault, FakeCapture, Paint};
#[cfg(windows)]
pub use print_window::PrintWindowCapture;
pub use source::{
    bgra_to_rgba, check_display_affinity, physical_size, CapturePath, CaptureSource, CaptureStats,
    RawPixels, SourceWindow,
};
#[cfg(windows)]
pub use wgc::WgcCapture;

use gcu_core::driver::{Cancel, Capture, DriverError, Result, DRIVER_DEADLINE};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

/// WGC first, PrintWindow only for unsupported/failed capture (never a permission
/// denial, cancellation, timeout, or vanished window). Shared budget includes both.
pub struct CaptureBackend {
    wgc: Arc<dyn CaptureSource>,
    print: Arc<dyn CaptureSource>,
    stats: Mutex<CaptureStats>,
}
impl CaptureBackend {
    /// Attach real or fake sources. Does not call the OS or prompt for permission.
    pub fn new(wgc: Arc<dyn CaptureSource>, print: Arc<dyn CaptureSource>) -> Self {
        Self {
            wgc,
            print,
            stats: Mutex::new(CaptureStats::default()),
        }
    }
    /// True if at least one attached source passed its runtime availability probe.
    pub fn available(&self) -> bool {
        self.wgc.available() || self.print.available()
    }
    /// Metadata-only counters and the most recent successful path.
    pub fn stats(&self) -> CaptureStats {
        self.stats.lock().map(|s| *s).unwrap_or_default()
    }
    fn note(&self, update: impl FnOnce(&mut CaptureStats)) {
        if let Ok(mut stats) = self.stats.lock() {
            update(&mut stats);
        }
    }
    async fn attempt(
        &self,
        source: &dyn CaptureSource,
        window: &SourceWindow,
        remaining: Duration,
        cancel: &Cancel,
    ) -> Result<RawPixels> {
        self.note(|s| match source.path() {
            CapturePath::Wgc => s.wgc_attempts += 1,
            CapturePath::PrintWindow => s.print_window_attempts += 1,
        });
        let pixels = if source.available() {
            source.capture(window, remaining, cancel).await?
        } else {
            return Err(DriverError::CapabilityUnavailable);
        };
        pixels.validate()?;
        if (pixels.width, pixels.height)
            != physical_size(window.frame.w, window.frame.h).ok_or(DriverError::InvalidRequest)?
        {
            return Err(native("capture geometry changed"));
        }
        Ok(pixels)
    }
}
struct Pending<'a>(&'a CaptureBackend, bool);
impl Drop for Pending<'_> {
    fn drop(&mut self) {
        if !self.1 {
            self.0.note(|s| s.discarded += 1);
        }
    }
}

/// Run both sources under `min(requested, DRIVER_DEADLINE)`. Cancellation wins
/// over ready frames; abandoned/late pixels cannot be published. Never blocks a
/// runtime thread. Redaction is intentionally left to core for both paths.
pub async fn take_capture(
    backend: &CaptureBackend,
    window: &SourceWindow,
    requested: Duration,
    cancel: &Cancel,
) -> Result<RawPixels> {
    let budget = requested.min(DRIVER_DEADLINE);
    let deadline = tokio::time::Instant::now() + budget;
    backend.note(|s| s.deadline = Some(budget));
    let mut pending = Pending(backend, false);
    let operation = async {
        let first = backend
            .attempt(backend.wgc.as_ref(), window, budget, cancel)
            .await;
        match first {
            Ok(pixels) => Ok((pixels, CapturePath::Wgc)),
            Err(DriverError::CapabilityUnavailable | DriverError::Native { .. }) => {
                backend.note(|s| s.fallbacks += 1);
                tracing::debug!(
                    capture_path = "PrintWindow",
                    "WGC failed; attempting window-only fallback"
                );
                let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
                backend
                    .attempt(backend.print.as_ref(), window, remaining, cancel)
                    .await
                    .map(|p| (p, CapturePath::PrintWindow))
            }
            Err(error) => Err(error),
        }
    };
    let (pixels, path) = tokio::select! { biased;
        _ = cancel.cancelled() => return Err(DriverError::Cancelled { maybe_executed: false }),
        _ = tokio::time::sleep_until(deadline) => return Err(DriverError::Timeout { maybe_executed: false }),
        result = operation => result?,
    };
    if cancel.is_cancelled() {
        return Err(DriverError::Cancelled {
            maybe_executed: false,
        });
    }
    if tokio::time::Instant::now() >= deadline {
        return Err(DriverError::Timeout {
            maybe_executed: false,
        });
    }
    backend.note(|s| {
        s.published += 1;
        s.last_path = Some(path);
    });
    pending.1 = true;
    tracing::debug!(capture_path = ?path, width = pixels.width, height = pixels.height, "raw window capture completed");
    Ok(pixels)
}

/// Encode the unscaled raw buffer solely to fit the existing `Capture` boundary.
/// No delivery/derivatives are allowed here; callers must use core redaction.
pub fn capture_from_pixels(window: &SourceWindow, pixels: RawPixels) -> Result<Capture> {
    pixels.validate()?;
    let mut bytes = Vec::new();
    let mut encoder = png::Encoder::new(&mut bytes, pixels.width, pixels.height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder
        .write_header()
        .map_err(|_| native("PNG encoding failed"))?;
    writer
        .write_image_data(&pixels.rgba)
        .map_err(|_| native("PNG encoding failed"))?;
    writer.finish().map_err(|_| native("PNG encoding failed"))?;
    Ok(Capture {
        bytes,
        width: pixels.width,
        height: pixels.height,
        mime: "image/png".into(),
        geometry: gcu_core::model::Geometry {
            frame: window.frame,
            scale: window.scale,
        },
    })
}
pub(super) fn native(message: &str) -> DriverError {
    DriverError::Native {
        message: message.into(),
        maybe_executed: false,
    }
}
pub(super) fn permission(protected: bool) -> DriverError {
    DriverError::PermissionMissing {
        permission: if protected {
            "protected_window_capture"
        } else {
            "window_capture"
        }
        .into(),
    }
}
