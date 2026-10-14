//! Windows.Graphics.Capture adapter. Native calls live in `ffi`.
use super::{ffi, CapturePath, CaptureSource, RawPixels, SourceWindow};
use async_trait::async_trait;
use gcu_core::driver::{Cancel, Result};
use std::time::Duration;

/// Single-frame WGC source backed by one persistent MTA worker. No activation,
/// permission prompts, cursor pixels, or full-desktop fallback. Drop never joins
/// a blocked native call; killable process isolation belongs to P2-04.
pub struct WgcCapture {
    worker: ffi::Worker,
}
impl WgcCapture {
    /// Probe WGC on its dedicated thread under DRIVER_DEADLINE. Unsupported WGC
    /// is a normal unavailable source, allowing the coordinator to use PrintWindow.
    pub async fn new() -> Result<Self> {
        Ok(Self {
            worker: ffi::Worker::start("gcu-win-wgc", ffi::wgc::supported, ffi::wgc::capture)
                .await?,
        })
    }
}
#[async_trait]
impl CaptureSource for WgcCapture {
    fn available(&self) -> bool {
        self.worker.available
    }
    fn path(&self) -> CapturePath {
        CapturePath::Wgc
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
