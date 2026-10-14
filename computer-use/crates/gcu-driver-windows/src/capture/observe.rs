//! Discovery/security bracketing around either window-only pixel source.
use super::{capture_from_pixels, take_capture, CaptureBackend, CaptureStats, SourceWindow};
use crate::WindowsDriver;
use gcu_core::driver::{
    Cancel, Capture, CaptureOpts, DriverError, Result, WindowRef, DRIVER_DEADLINE,
};
use std::sync::Arc;

impl WindowsDriver {
    /// Attach a probed capture backend. Pure metadata change; no OS call. Both
    /// sources follow the same security pre/postflight and core redaction boundary.
    pub fn with_capture(mut self, backend: Arc<CaptureBackend>) -> Self {
        let available = backend.available();
        self.caps.observe.screenshot = available;
        self.caps.limits.max_image_edge = if available { 16_384 } else { 0 };
        self.caps.background_operation = if available {
            gcu_protocol::capabilities::BackgroundOperation::Partial
        } else {
            gcu_protocol::capabilities::BackgroundOperation::None
        };
        self.capture_backend = Some(backend);
        self
    }
    /// Payload-free path/counters. A raw capture is not a redacted delivery.
    pub fn capture_stats(&self) -> Option<CaptureStats> {
        self.capture_backend.as_ref().map(|b| b.stats())
    }
    pub(crate) async fn capture_window(
        &self,
        win: &WindowRef,
        opts: &CaptureOpts,
        cancel: &Cancel,
    ) -> Result<Capture> {
        let deadline = tokio::time::Instant::now() + opts.timeout.min(DRIVER_DEADLINE);
        let operation = async {
            let target = win.clone();
            let window = self
                .call(opts.timeout, Some(cancel), false, move |s| {
                    let w = s.observation_window(&target, false)?;
                    if w.minimized {
                        return Err(DriverError::WindowMinimized);
                    }
                    Ok(w)
                })
                .await?;
            let backend = self
                .capture_backend
                .as_ref()
                .ok_or(DriverError::CapabilityUnavailable)?;
            let source = SourceWindow {
                hwnd: window.hwnd,
                frame: window.frame,
                scale: window.scale,
            };
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            let pixels = take_capture(backend, &source, remaining, cancel).await?;
            let target = win.clone();
            let current = self
                .call(DRIVER_DEADLINE, Some(cancel), false, move |s| {
                    s.observation_window(&target, false)
                })
                .await?;
            if current.minimized {
                return Err(DriverError::WindowMinimized);
            }
            if current.frame != source.frame || current.scale != source.scale {
                return Err(super::native("window geometry changed during capture"));
            }
            // PNG encoding is bounded CPU work on the discovery thread, so neither
            // a large frame nor a native callback can block the async runtime.
            self.call(DRIVER_DEADLINE, Some(cancel), false, move |_| {
                capture_from_pixels(&source, pixels)
            })
            .await
        };
        tokio::select! { biased;
            _ = cancel.cancelled() => Err(DriverError::Cancelled { maybe_executed: false }),
            result = tokio::time::timeout_at(deadline, operation) => result.map_err(|_| DriverError::Timeout { maybe_executed: false })?,
        }
    }
}
