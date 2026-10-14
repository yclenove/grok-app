//! ScreenCaptureKit window screenshots. macOS 14+ only.
//!
//! `SCShareableContent` lists windows, including ones another window covers.
//! `SCContentFilter` is built with `initWithDesktopIndependentWindow:` so the
//! pixels are that window's, not a crop of the display. The screenshot callback
//! is asynchronous. Copied RGBA bytes cross a channel; Objective-C objects stay
//! on the callback thread. A dropped receiver discards the image. This module
//! never calls `CGRequestScreenCaptureAccess` and never clicks a permission dialog.

use super::{
    bgra_to_rgba, is_screen_recording_denial, screenshot_config, CaptureSource, RawPixels,
    SourceWindow, PIXEL_FORMAT_BGRA, SCREEN_RECORDING_MISSING_ENTITLEMENT,
    SCREEN_RECORDING_USER_DECLINED,
};
use async_trait::async_trait;
use block2::RcBlock;
use gcu_core::driver::{Cancel, DriverError, Result};
use objc2::rc::Retained;
use objc2::AllocAnyThread;
use objc2_core_foundation::CFRetained;
use objc2_core_graphics::{CGDataProvider, CGImage};
use objc2_foundation::{NSArray, NSError};
use objc2_screen_capture_kit::{
    SCContentFilter, SCScreenshotManager, SCShareableContent, SCStreamConfiguration,
    SCStreamErrorCode, SCWindow,
};
use std::ptr::NonNull;
use std::sync::mpsc::{self, RecvTimeoutError, SyncSender};
use std::time::{Duration, Instant};

const _: () = {
    assert!(SCStreamErrorCode::UserDeclined.0 as i64 == SCREEN_RECORDING_USER_DECLINED as i64);
    assert!(
        SCStreamErrorCode::MissingEntitlements.0 as i64
            == SCREEN_RECORDING_MISSING_ENTITLEMENT as i64
    );
};

/// Production capture. Safe to share across tasks. Does not prompt.
#[derive(Clone, Copy, Debug, Default)]
pub struct SckCapture;

impl SckCapture {
    /// Create the backend. Construction does not touch Screen Recording.
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl CaptureSource for SckCapture {
    fn screen_recording_ok(&self) -> bool {
        // Preflight only. Requesting access would raise a system dialog.
        objc2_core_graphics::CGPreflightScreenCaptureAccess()
    }

    async fn capture(
        &self,
        window: &SourceWindow,
        deadline: Duration,
        _shows_cursor: bool,
        cancel: &Cancel,
    ) -> Result<RawPixels> {
        if !self.screen_recording_ok() {
            return Err(permission_missing());
        }
        let Some((width, height)) =
            super::physical_size(window.frame.w, window.frame.h, window.scale)
        else {
            return Err(DriverError::Native {
                message: "window frame is not a physical size".into(),
                maybe_executed: false,
            });
        };
        let cg_id = window.cg_id;
        let cancel = cancel.clone();
        // Objective-C objects are `!Send`. The blocking thread owns them and only
        // sends copied RGBA bytes back to this task.
        let joined = tokio::task::spawn_blocking(move || {
            blocking_capture(cg_id, width, height, deadline, &cancel)
        })
        .await;
        match joined {
            Ok(result) => result,
            Err(_) => Err(DriverError::Crashed {
                maybe_executed: false,
            }),
        }
    }
}

fn permission_missing() -> DriverError {
    DriverError::PermissionMissing {
        permission: "screen_recording".into(),
    }
}

fn blocking_capture(
    cg_id: u32,
    width: u32,
    height: u32,
    deadline: Duration,
    cancel: &Cancel,
) -> Result<RawPixels> {
    if cancel.is_cancelled() {
        return Err(DriverError::Cancelled {
            maybe_executed: false,
        });
    }
    let (tx, rx) = mpsc::sync_channel(1);
    let report = tx.clone();
    let block: RcBlock<dyn Fn(*mut SCShareableContent, *mut NSError)> = RcBlock::new(
        move |content: *mut SCShareableContent, error: *mut NSError| {
            if let Err(failure) = schedule_shot(content, error, cg_id, width, height, tx.clone()) {
                let _ = tx.send(Err(failure));
            }
        },
    );
    // SAFETY: ScreenCaptureKit copies `block` before returning. `onScreenWindowsOnly`
    // is false so a covered window is still listed. Minimization is decided by the
    // driver. The block schedules the screenshot on this callback's thread, where
    // the `SCWindow` is valid, and never sends that object to another thread.
    unsafe {
        SCShareableContent::getShareableContentExcludingDesktopWindows_onScreenWindowsOnly_completionHandler(
            true,
            false,
            &block,
        );
    }
    let pixels = recv_pixels(rx, deadline, cancel)?;
    drop(block);
    drop(report);
    if !objc2_core_graphics::CGPreflightScreenCaptureAccess() {
        drop(pixels);
        return Err(permission_missing());
    }
    Ok(pixels)
}

fn schedule_shot(
    content: *mut SCShareableContent,
    error: *mut NSError,
    cg_id: u32,
    width: u32,
    height: u32,
    tx: SyncSender<Result<RawPixels>>,
) -> Result<()> {
    let windows = shareable_windows(content, error)?;
    let target = find_window(&windows, cg_id)?;
    let config = stream_config(width, height)?;
    // SAFETY: `alloc` is a partial init. `initWithDesktopIndependentWindow`
    // consumes it and returns +1. The filter captures this window alone, including
    // when another window covers it. It is not a display-rectangle crop.
    let filter = unsafe {
        SCContentFilter::initWithDesktopIndependentWindow(SCContentFilter::alloc(), &target)
    };
    let kept_filter = filter.clone();
    let kept_config = config.clone();
    let shot: RcBlock<dyn Fn(*mut CGImage, *mut NSError)> =
        RcBlock::new(move |image: *mut CGImage, error: *mut NSError| {
            // Keep the filter (and the window it retains) until this copied block
            // is released. Referencing them keeps the closure `Fn`. A dropped
            // sender discards the copied pixels.
            let _keep = (&kept_filter, &kept_config);
            let _ = tx.send(image_result(image, error));
        });
    // SAFETY: ScreenCaptureKit copies `shot` before returning. The copy owns
    // `kept_filter`. Bytes are copied inside the callback; the `CGImage` is not sent.
    unsafe {
        SCScreenshotManager::captureImageWithFilter_configuration_completionHandler(
            &filter,
            &config,
            Some(&*shot),
        );
    }
    Ok(())
}

fn recv_pixels(
    rx: mpsc::Receiver<Result<RawPixels>>,
    deadline: Duration,
    cancel: &Cancel,
) -> Result<RawPixels> {
    let started = Instant::now();
    loop {
        if cancel.is_cancelled() {
            return Err(DriverError::Cancelled {
                maybe_executed: false,
            });
        }
        let remaining = deadline.saturating_sub(started.elapsed());
        if remaining.is_zero() {
            return Err(DriverError::Timeout {
                maybe_executed: false,
            });
        }
        match rx.recv_timeout(remaining.min(Duration::from_millis(50))) {
            Ok(result) => return result,
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => {
                return Err(DriverError::Native {
                    message: "capture callback dropped".into(),
                    maybe_executed: false,
                });
            }
        }
    }
}

fn shareable_windows(
    content: *mut SCShareableContent,
    error: *mut NSError,
) -> Result<Retained<NSArray<SCWindow>>> {
    if let Some(error) = unsafe_error(error) {
        return Err(map_ns_error(&error));
    }
    if content.is_null() {
        return Err(DriverError::Native {
            message: "shareable content missing".into(),
            maybe_executed: false,
        });
    }
    // SAFETY: `content` is a live object for this callback, or we returned above.
    // `retain` takes one +1 that `Retained` owns. The callback's own claim is
    // released by its autorelease pool, not by us. `windows` returns +1.
    let content = unsafe { Retained::retain(content) }.ok_or(DriverError::Native {
        message: "shareable content missing".into(),
        maybe_executed: false,
    })?;
    Ok(unsafe { content.windows() })
}

fn find_window(windows: &NSArray<SCWindow>, cg_id: u32) -> Result<Retained<SCWindow>> {
    let count = windows.count();
    for index in 0..count {
        // `objectAtIndex` returns +1. `index` stays below `count`.
        let window = windows.objectAtIndex(index);
        // SAFETY: `windowID` reads a `CGWindowID` from this live `SCWindow`.
        if unsafe { window.windowID() } == cg_id {
            return Ok(window);
        }
    }
    Err(DriverError::WindowGone)
}

fn stream_config(width: u32, height: u32) -> Result<Retained<SCStreamConfiguration>> {
    let spec = screenshot_config(width, height);
    // SAFETY: `new` returns +1. Setters write plain integers and bools on that object.
    // `setPixelFormat:` takes an `OSType` (`u32`, `'BGRA'`). The binding is present
    // without `objc2-core-media`; that crate is not a dependency.
    let config = unsafe { SCStreamConfiguration::new() };
    unsafe {
        config.setWidth(spec.width);
        config.setHeight(spec.height);
        config.setShowsCursor(spec.shows_cursor);
        config.setShowMouseClicks(spec.show_mouse_clicks);
        config.setScalesToFit(spec.scales_to_fit);
        config.setPixelFormat(spec.pixel_format);
    }
    if spec.pixel_format != PIXEL_FORMAT_BGRA || spec.shows_cursor {
        return Err(DriverError::Native {
            message: "screenshot configuration rejected".into(),
            maybe_executed: false,
        });
    }
    Ok(config)
}

fn image_result(image: *mut CGImage, error: *mut NSError) -> Result<RawPixels> {
    if let Some(error) = unsafe_error(error) {
        return Err(map_ns_error(&error));
    }
    let Some(image) = NonNull::new(image) else {
        return Err(DriverError::Native {
            message: "capture image missing".into(),
            maybe_executed: false,
        });
    };
    // SAFETY: `image` is a live `CGImage` for this callback. `retain` takes +1
    // owned by `CFRetained`. Bytes are copied before that retain is dropped.
    let image = unsafe { CFRetained::<CGImage>::retain(image) };
    copy_image(&image)
}

fn copy_image(image: &CGImage) -> Result<RawPixels> {
    let width = u32::try_from(CGImage::width(Some(image))).unwrap_or(0);
    let height = u32::try_from(CGImage::height(Some(image))).unwrap_or(0);
    let stride = CGImage::bytes_per_row(Some(image));
    if width == 0 || height == 0 || CGImage::bits_per_pixel(Some(image)) != 32 {
        return Err(DriverError::Native {
            message: "capture image is not 32-bit".into(),
            maybe_executed: false,
        });
    }
    // `data_provider` retains the provider. `data` copies provider bytes into a
    // new `CFData` (+1). Both drop at the end of this function.
    let provider = CGImage::data_provider(Some(image)).ok_or(DriverError::Native {
        message: "capture image has no data provider".into(),
        maybe_executed: false,
    })?;
    let data = CGDataProvider::data(Some(&provider)).ok_or(DriverError::Native {
        message: "capture image has no bytes".into(),
        maybe_executed: false,
    })?;
    let length = data.length();
    if length < 0 {
        return Err(DriverError::Native {
            message: "capture image has no bytes".into(),
            maybe_executed: false,
        });
    }
    let ptr = data.byte_ptr();
    if ptr.is_null() {
        return Err(DriverError::Native {
            message: "capture image has no bytes".into(),
            maybe_executed: false,
        });
    }
    // SAFETY: `byte_ptr` and `length` describe the buffer owned by `data`.
    // The slice is copied into `rgba` before `data` is released.
    let bytes = unsafe { std::slice::from_raw_parts(ptr, length as usize) };
    let rgba = bgra_to_rgba(bytes, width, height, stride).ok_or(DriverError::Native {
        message: "capture image stride is short".into(),
        maybe_executed: false,
    })?;
    Ok(RawPixels {
        rgba,
        width,
        height,
    })
}

fn unsafe_error(error: *mut NSError) -> Option<Retained<NSError>> {
    if error.is_null() {
        return None;
    }
    // SAFETY: a non-null callback error is a live `NSError` for this call.
    // `retain` takes +1 so the code can be read after the callback returns.
    unsafe { Retained::retain(error) }
}

fn map_ns_error(error: &NSError) -> DriverError {
    // `code` is a plain `NSInteger`. The localized description is not read;
    // it can contain window titles.
    let code = error.code();
    if is_screen_recording_denial(code) {
        permission_missing()
    } else {
        DriverError::Native {
            message: format!("screencapturekit error {code}"),
            maybe_executed: false,
        }
    }
}
