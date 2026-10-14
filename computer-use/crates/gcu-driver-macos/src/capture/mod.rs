//! Window screenshots.
//!
//! The driver returns a raw PNG [`gcu_core::driver::Capture`]. Callers pass it
//! through [`gcu_core::redact`] before any thumbnail, crop, or model encoding.
//! [`CaptureSource`] is the backend. [`SckCapture`] exists only on macOS.
//! [`FakeCapture`] is the portable stand-in.
//!
//! A covered window is still captured (`desktopIndependentWindow`). A minimized
//! window is [`gcu_core::driver::DriverError::WindowMinimized`] and is not
//! restored. Missing or revoked Screen Recording is
//! [`gcu_core::driver::DriverError::PermissionMissing`] with
//! `permission: "screen_recording"`. The wait is capped by
//! [`gcu_core::driver::DRIVER_DEADLINE`]. Cancelling drops the image and reports
//! `maybe_executed: false`.

mod source;

#[cfg(target_os = "macos")]
mod sck;

pub use source::{
    backing_scale, bgra_to_rgba, capture_budget, encode_capture, is_screen_recording_denial,
    physical_geometry, physical_size, screenshot_config, CaptureFault, CaptureSource, CaptureStats,
    DisplayMetrics, FakeCapture, Paint, RawPixels, ScreenshotConfig, SourceWindow,
    PIXEL_FORMAT_BGRA, SCREEN_RECORDING_MISSING_ENTITLEMENT, SCREEN_RECORDING_USER_DECLINED,
};

#[cfg(target_os = "macos")]
pub use sck::SckCapture;

use gcu_core::driver::{Cancel, Capture, DriverError, Result};
use std::{sync::Arc, time::Duration};

/// Run `source` inside `deadline` and drop pixels on cancel, timeout, or a
/// Screen Recording check that fails after the callback.
pub async fn take_capture(
    source: Arc<dyn CaptureSource>,
    window: &SourceWindow,
    deadline: Duration,
    cancel: &Cancel,
) -> Result<RawPixels> {
    if !source.screen_recording_ok() {
        return Err(DriverError::PermissionMissing {
            permission: "screen_recording".into(),
        });
    }
    let pending = source.capture(window, deadline, false, cancel);
    tokio::pin!(pending);
    let pixels = tokio::select! {
        biased;
        _ = cancel.cancelled() => {
            return Err(DriverError::Cancelled { maybe_executed: false });
        }
        _ = tokio::time::sleep(deadline) => {
            return Err(DriverError::Timeout { maybe_executed: false });
        }
        result = &mut pending => result?,
    };
    if !source.screen_recording_ok() {
        drop(pixels);
        return Err(DriverError::PermissionMissing {
            permission: "screen_recording".into(),
        });
    }
    Ok(pixels)
}

/// PNG capture whose geometry matches the returned physical buffer.
pub fn capture_from_pixels(window: &SourceWindow, pixels: RawPixels) -> Result<Capture> {
    if pixels.rgba.len() != (pixels.width as usize) * (pixels.height as usize) * 4 {
        return Err(DriverError::Native {
            message: "capture buffer does not match its dimensions".into(),
            maybe_executed: false,
        });
    }
    let geometry = physical_geometry(window.frame, window.scale, pixels.width, pixels.height);
    encode_capture(&pixels.rgba, pixels.width, pixels.height, geometry)
}
