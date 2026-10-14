//! PrintWindow adapter. All GDI ownership and calls live in `ffi`.
use super::{ffi, CapturePath, CaptureSource, RawPixels, SourceWindow};
use async_trait::async_trait;
use gcu_core::driver::{Cancel, Result};
use std::time::Duration;

/// Window-only fallback using PW_RENDERFULLCONTENT. Some applications cannot
/// paint through PrintWindow; failures/black frames are errors, never success.
/// One persistent worker bounds queued calls; Drop does not join a stuck HWND.
pub struct PrintWindowCapture {
    worker: ffi::Worker,
}
impl PrintWindowCapture {
    /// Initialize a dedicated DPI-aware MTA thread under DRIVER_DEADLINE.
    pub async fn new() -> Result<Self> {
        Ok(Self {
            worker: ffi::Worker::start("gcu-win-print", || true, ffi::print_window::capture)
                .await?,
        })
    }
}
#[async_trait]
impl CaptureSource for PrintWindowCapture {
    fn available(&self) -> bool {
        self.worker.available
    }
    fn path(&self) -> CapturePath {
        CapturePath::PrintWindow
    }
    async fn capture(
        &self,
        window: &SourceWindow,
        deadline: Duration,
        cancel: &Cancel,
    ) -> Result<RawPixels> {
        self.worker.capture(window, deadline, cancel).await
    }
}
