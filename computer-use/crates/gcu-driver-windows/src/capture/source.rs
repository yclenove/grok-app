//! Portable pixel boundary and geometry helpers. No platform handles escape.
use async_trait::async_trait;
use gcu_core::{
    driver::{Cancel, Result},
    model::Rect,
};
use std::time::Duration;

/// Path actually used; intentionally outside the public protocol Capture type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CapturePath {
    /// Windows.Graphics.Capture single frame.
    Wgc,
    /// PrintWindow with PW_RENDERFULLCONTENT.
    PrintWindow,
}
/// Interpret the OS display-affinity probe. WGC enforces capture exclusion even
/// when a nonlayered window cannot be queried. PrintWindow is permitted only
/// after an explicit WDA_NONE result; unknown protection must never be bypassed.
pub fn check_display_affinity(path: CapturePath, affinity: Option<u32>) -> Result<()> {
    match affinity {
        Some(0) => Ok(()),
        Some(_) => Err(super::permission(true)),
        None if path == CapturePath::Wgc => Ok(()),
        None => Err(gcu_core::driver::DriverError::PermissionMissing {
            permission: "window_capture_protection_unknown".into(),
        }),
    }
}
/// Exact window selected by the discovery/security gate.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SourceWindow {
    /// HWND, private to the trusted driver adapter.
    pub hwnd: u64,
    /// Physical screen bounds, including the nonclient area; never logical DIPs.
    pub frame: Rect,
    /// GetDpiForWindow / 96. Pixels and origin are already physical.
    pub scale: f64,
}
/// Bounded tightly packed RGBA8. Debug deliberately omits pixel data.
#[derive(Clone, PartialEq, Eq)]
pub struct RawPixels {
    /// Unredacted pixels; core is the only delivery boundary.
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
            .finish_non_exhaustive()
    }
}
impl RawPixels {
    pub(super) fn validate(&self) -> Result<()> {
        if byte_len(self.width, self.height) != Some(self.rgba.len()) {
            return Err(super::native("capture buffer dimensions invalid"));
        }
        if self
            .rgba
            .as_chunks::<4>()
            .0
            .iter()
            .all(|p| p[..3] == [0, 0, 0])
        {
            return Err(super::native(
                "all-black capture; content protected or unavailable",
            ));
        }
        Ok(())
    }
}
/// Window-only source. Implementations must bound native calls, omit cursors,
/// refuse minimized/protected windows, and discard cancelled or late pixels.
#[async_trait]
pub trait CaptureSource: Send + Sync {
    /// Runtime availability only; does not grant permission or prompt.
    fn available(&self) -> bool;
    /// Actual native backend (also used in payload-free tracing).
    fn path(&self) -> CapturePath;
    /// Capture once within the remaining shared deadline. Reads have no input
    /// effects, so all cancellations/timeouts report maybe_executed=false.
    async fn capture(
        &self,
        window: &SourceWindow,
        deadline: Duration,
        cancel: &Cancel,
    ) -> Result<RawPixels>;
}
/// Capture metadata only; no image, window title, or UI text is retained.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CaptureStats {
    /// WGC attempts, including unsupported WGC.
    pub wgc_attempts: u64,
    /// PrintWindow attempts.
    pub print_window_attempts: u64,
    /// WGC failures which selected fallback.
    pub fallbacks: u64,
    /// Successfully returned raw captures (not yet safe for delivery).
    pub published: u64,
    /// Failed/abandoned captures, whose pixels were discarded.
    pub discarded: u64,
    /// Latest successful capture's path.
    pub last_path: Option<CapturePath>,
    /// Latest total capture budget, capped at DRIVER_DEADLINE.
    pub deadline: Option<Duration>,
}
pub(super) fn byte_len(width: u32, height: u32) -> Option<usize> {
    if width == 0 || height == 0 || width > 16_384 || height > 16_384 {
        return None;
    }
    let len = (width as usize)
        .checked_mul(height as usize)?
        .checked_mul(4)?;
    (len <= gcu_core::redact::MAX_CAPTURE_BYTES).then_some(len)
}
/// Validate already-physical bounds without multiplying by the DPI scale again.
pub fn physical_size(width: f64, height: f64) -> Option<(u32, u32)> {
    if !width.is_finite()
        || !height.is_finite()
        || width.fract() != 0.
        || height.fract() != 0.
        || width < 1.
        || height < 1.
        || width > 16_384.
        || height > 16_384.
    {
        return None;
    }
    byte_len(width as u32, height as u32)?;
    Some((width as u32, height as u32))
}
/// Copy padded BGRA rows to opaque packed RGBA. Rejects short strides/buffers
/// and excessive allocations; alpha is never meaningful for window captures.
pub fn bgra_to_rgba(src: &[u8], width: u32, height: u32, stride: usize) -> Option<Vec<u8>> {
    let len = byte_len(width, height)?;
    let row = (width as usize).checked_mul(4)?;
    if stride < row || stride.checked_mul(height as usize)? > src.len() {
        return None;
    }
    let mut rgba = vec![0; len];
    for y in 0..height as usize {
        for x in 0..width as usize {
            let s = y * stride + x * 4;
            let d = y * row + x * 4;
            rgba[d..d + 4].copy_from_slice(&[src[s + 2], src[s + 1], src[s], 255]);
        }
    }
    Some(rgba)
}

#[cfg(any(windows, test))]
pub(super) fn align_pixels(
    window: &SourceWindow,
    bounds: Rect,
    pixels: RawPixels,
) -> Result<RawPixels> {
    pixels.validate()?;
    let (width, height) = physical_size(window.frame.w, window.frame.h)
        .ok_or_else(|| super::native("capture geometry invalid"))?;
    if (pixels.width, pixels.height) == (width, height) {
        return Ok(pixels);
    }
    let dx = bounds.x - window.frame.x;
    let dy = bounds.y - window.frame.y;
    if physical_size(bounds.w, bounds.h) != Some((pixels.width, pixels.height))
        || !dx.is_finite()
        || !dy.is_finite()
        || dx < 0.
        || dy < 0.
        || dx.fract() != 0.
        || dy.fract() != 0.
        || dx + bounds.w > window.frame.w
        || dy + bounds.h > window.frame.h
    {
        return Err(super::native("WGC frame cannot be aligned to window"));
    }
    let len = byte_len(width, height).ok_or_else(|| super::native("capture buffer too large"))?;
    let mut rgba = vec![0; len];
    for p in rgba.as_chunks_mut::<4>().0 {
        p[3] = 255;
    }
    let row = pixels.width as usize * 4;
    for y in 0..pixels.height as usize {
        let dst = ((y + dy as usize) * width as usize + dx as usize) * 4;
        rgba[dst..dst + row].copy_from_slice(&pixels.rgba[y * row..(y + 1) * row]);
    }
    Ok(RawPixels {
        rgba,
        width,
        height,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn frame_invisible_borders_preserve_physical_origin_without_rescaling() {
        let window = SourceWindow {
            hwnd: 8,
            frame: Rect {
                x: 10.,
                y: 20.,
                w: 4.,
                h: 4.,
            },
            scale: 1.5,
        };
        let bounds = Rect {
            x: 11.,
            y: 21.,
            w: 2.,
            h: 2.,
        };
        let pixels = RawPixels {
            rgba: [20, 30, 40, 255].repeat(4),
            width: 2,
            height: 2,
        };
        let aligned = align_pixels(&window, bounds, pixels.clone()).unwrap();
        assert_eq!((aligned.width, aligned.height), (4, 4));
        assert_eq!(&aligned.rgba[20..24], &[20, 30, 40, 255]);
        assert_eq!(&aligned.rgba[..4], &[0, 0, 0, 255]);
        assert!(align_pixels(&window, Rect { x: 9., ..bounds }, pixels).is_err());
    }
    #[test]
    fn pixel_short_stride_or_excessive_size_is_rejected() {
        assert_eq!(
            bgra_to_rgba(&[1, 2, 3, 0, 9, 9, 9, 9], 1, 1, 8),
            Some(vec![3, 2, 1, 255])
        );
        assert!(bgra_to_rgba(&[0; 3], 1, 1, 3).is_none());
        assert!(bgra_to_rgba(&[], 16_384, 16_384, 65_536).is_none());
        assert!(physical_size(0., 1.).is_none());
        assert!(physical_size(1.5, 1.).is_none());
    }
}
