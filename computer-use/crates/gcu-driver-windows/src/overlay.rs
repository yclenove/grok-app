//! Exclusion probes never equate WDA_MONITOR with WDA_EXCLUDEFROMCAPTURE.
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
#[cfg(windows)]
mod ffi;
#[cfg(windows)]
pub use ffi::RealOverlayApi;
/// Exclusion appeared in Windows 10 build 19041 (2004). Earlier systems may
/// accept 0x11 but implement only WDA_MONITOR; readback alone is insufficient.
pub fn supports_exclusion_build(major: u32, build: u32) -> bool {
    major > 10 || (major == 10 && build >= 19041)
}
/// Injectable display-affinity boundary; only this process's top-level windows.
pub trait OverlayApi: Send + Sync + 'static {
    /// Create a disposable own window and prove 0x11 readback (Win10 2004+).
    fn probe(&self) -> bool;
    /// Set exclusion and verify exact readback on an owned top-level HWND.
    fn exclude(&self, hwnd: u64) -> bool;
}
/// Cached, conservative exclusion capability. Native calls run off the runtime
/// with a 100ms caller deadline; failure revokes the advertised capability.
pub struct OverlayExclusion {
    api: Arc<dyn OverlayApi>,
    available: AtomicBool,
}
impl OverlayExclusion {
    /// Probe once, without spawning replacement threads after a stuck probe.
    pub async fn probe(api: Arc<dyn OverlayApi>) -> Arc<Self> {
        let probe = api.clone();
        let available = tokio::time::timeout(
            Duration::from_millis(100),
            tokio::task::spawn_blocking(move || probe.probe()),
        )
        .await
        .is_ok_and(|r| matches!(r, Ok(true)));
        Arc::new(Self {
            api,
            available: AtomicBool::new(available),
        })
    }
    /// Apply to an actual Host overlay. A failed/slow call revokes capability;
    /// the Host must hide the overlay or mask its verified region in redact.
    pub async fn exclude(&self, hwnd: u64) -> bool {
        if !self.available() {
            return false;
        }
        let api = self.api.clone();
        let success = tokio::time::timeout(
            Duration::from_millis(100),
            tokio::task::spawn_blocking(move || api.exclude(hwnd)),
        )
        .await
        .is_ok_and(|r| matches!(r, Ok(true)));
        if !success {
            self.available.store(false, Ordering::Release);
        }
        success
    }
    /// Whether exact exclusion is currently proven; false needs redact handling.
    pub fn available(&self) -> bool {
        self.available.load(Ordering::Acquire)
    }
}
/// Portable pre-2004 fallback/failure simulator. Never touches a real HWND.
pub struct FakeOverlayApi {
    /// Whether the affinity setter succeeds.
    pub supported: bool,
    /// Actual affinity returned after setting: 1 is only WDA_MONITOR.
    pub readback: u32,
}
impl OverlayApi for FakeOverlayApi {
    fn probe(&self) -> bool {
        self.supported && self.readback == 0x11
    }
    fn exclude(&self, _: u64) -> bool {
        self.probe()
    }
}
