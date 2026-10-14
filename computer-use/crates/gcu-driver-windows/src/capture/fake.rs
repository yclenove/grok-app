//! Portable capture faults and window-owned paint, never desktop composites.
use super::{CapturePath, CaptureSource, CaptureStats, RawPixels, SourceWindow};
use async_trait::async_trait;
use gcu_core::{
    driver::{Cancel, DriverError, Result},
    model::Rect,
};
use std::{collections::HashMap, sync::Mutex, time::Duration};

/// Scripted capture failures, without native permissions or desktop access.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CaptureFault {
    /// Return the target's own pixels.
    #[default]
    None,
    /// Backend unsupported.
    Unavailable,
    /// Frame callback/PrintWindow failed.
    CallbackFailed,
    /// Wait indefinitely; coordinator must cancel/drop this future.
    Hang,
    /// Access denied.
    PermissionMissing,
    /// Display-affinity/DRM protected window.
    Protected,
    /// Permission revoked immediately before returning pixels.
    RevokeBeforePublish,
    /// Produce a ready frame, then cancel before the coordinator can publish it.
    CancelBeforePublish,
    /// A black capture must be refused by the shared validator.
    Black,
}
/// Target-owned pixels and a foreign cover color that must never appear.
#[derive(Clone, Copy, Debug)]
pub struct Paint {
    /// Target background.
    pub own: [u8; 4],
    /// Occluding window background; never sampled.
    pub cover: [u8; 4],
    /// Target is covered; capture must still use own pixels.
    pub occluded: bool,
    /// Optional physical window-local painted region.
    pub mark: Option<([u8; 4], Rect)>,
}
struct Inner {
    fault: CaptureFault,
    affinity: Option<u32>,
    paints: HashMap<u64, Paint>,
    stats: CaptureStats,
}
/// In-memory stand-in for either backend, usable on every platform.
pub struct FakeCapture {
    path: CapturePath,
    inner: Mutex<Inner>,
}
impl FakeCapture {
    /// Ready source with no target windows. No IO or native calls.
    pub fn new(path: CapturePath) -> Self {
        Self {
            path,
            inner: Mutex::new(Inner {
                fault: CaptureFault::None,
                affinity: Some(0),
                paints: HashMap::new(),
                stats: CaptureStats::default(),
            }),
        }
    }
    /// Set a failure mode; painted pixels are retained.
    pub fn set_fault(&self, fault: CaptureFault) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.fault = fault;
        }
    }
    /// Script GetWindowDisplayAffinity; None means the OS probe is unavailable.
    pub fn set_display_affinity(&self, affinity: Option<u32>) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.affinity = affinity;
        }
    }
    /// Install target-owned physical pixels for an exact HWND.
    pub fn paint(&self, hwnd: u64, paint: Paint) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.paints.insert(hwnd, paint);
        }
    }
    /// Source completions/budgets, without payloads.
    pub fn stats(&self) -> CaptureStats {
        self.inner.lock().map(|i| i.stats).unwrap_or_default()
    }
}
#[async_trait]
impl CaptureSource for FakeCapture {
    fn available(&self) -> bool {
        self.inner
            .lock()
            .is_ok_and(|i| i.fault != CaptureFault::Unavailable)
    }
    fn path(&self) -> CapturePath {
        self.path
    }
    async fn capture(
        &self,
        window: &SourceWindow,
        deadline: Duration,
        cancel: &Cancel,
    ) -> Result<RawPixels> {
        let (fault, paint, affinity) = {
            let mut inner = self.inner.lock().map_err(|_| DriverError::Crashed {
                maybe_executed: false,
            })?;
            inner.stats.deadline = Some(deadline);
            (
                inner.fault,
                inner.paints.get(&window.hwnd).copied(),
                inner.affinity,
            )
        };
        if cancel.is_cancelled() {
            return Err(DriverError::Cancelled {
                maybe_executed: false,
            });
        }
        super::check_display_affinity(self.path, affinity)?;
        match fault {
            CaptureFault::Unavailable => return Err(DriverError::CapabilityUnavailable),
            CaptureFault::CallbackFailed => return Err(super::native("capture callback failed")),
            CaptureFault::PermissionMissing | CaptureFault::RevokeBeforePublish => {
                return Err(super::permission(false))
            }
            CaptureFault::Protected => return Err(super::permission(true)),
            CaptureFault::Hang => {
                std::future::pending::<()>().await;
            }
            CaptureFault::None | CaptureFault::Black | CaptureFault::CancelBeforePublish => {}
        }
        let paint = paint.ok_or(DriverError::WindowGone)?;
        let (width, height) = super::physical_size(window.frame.w, window.frame.h)
            .ok_or(DriverError::InvalidRequest)?;
        let own = if fault == CaptureFault::Black {
            [0, 0, 0, 255]
        } else {
            paint.own
        };
        let mut rgba = Vec::with_capacity(
            super::source::byte_len(width, height).ok_or(DriverError::InvalidRequest)?,
        );
        for y in 0..height {
            for x in 0..width {
                let color = paint
                    .mark
                    .filter(|_| fault != CaptureFault::Black)
                    .filter(|(_, r)| {
                        f64::from(x) >= r.x
                            && f64::from(x) < r.x + r.w
                            && f64::from(y) >= r.y
                            && f64::from(y) < r.y + r.h
                    })
                    .map_or(own, |(c, _)| c);
                rgba.extend_from_slice(&color);
            }
        }
        if fault == CaptureFault::CancelBeforePublish {
            cancel.cancel();
        }
        if let Ok(mut inner) = self.inner.lock() {
            inner.stats.published += 1;
            inner.stats.last_path = Some(self.path);
        }
        Ok(RawPixels {
            rgba,
            width,
            height,
        })
    }
}
