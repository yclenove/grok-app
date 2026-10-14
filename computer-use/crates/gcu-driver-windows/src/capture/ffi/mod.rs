//! All native capture calls are confined to dedicated MTA/DPI-aware threads.
pub(super) mod print_window;
pub(super) mod wgc;
use super::{check_display_affinity, native, permission, CapturePath, RawPixels, SourceWindow};
use gcu_core::driver::{Cancel, DriverError, Result, DRIVER_DEADLINE};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    time::{Duration, Instant},
};
use tokio::sync::oneshot;
use windows::{
    core::Error,
    Win32::{
        Foundation::{HWND, RECT},
        System::WinRT::{RoInitialize, RoUninitialize, RO_INIT_MULTITHREADED},
        UI::{
            HiDpi::GetDpiForWindow,
            WindowsAndMessaging::{GetWindowDisplayAffinity, GetWindowRect, IsIconic, IsWindow},
        },
    },
};

pub(super) fn map_error(error: Error) -> DriverError {
    if error.code().0 == 0x80070005u32 as i32 {
        permission(false)
    } else {
        native("Windows capture API failed")
    }
}
struct Context {
    _dpi: crate::ffi::DpiContext,
}
impl Context {
    fn new() -> Result<Self> {
        // SAFETY: initializes this fresh dedicated worker's MTA; balanced below.
        unsafe { RoInitialize(RO_INIT_MULTITHREADED) }.map_err(map_error)?;
        let dpi = crate::ffi::DpiContext::new();
        match dpi {
            Ok(dpi) => Ok(Self { _dpi: dpi }),
            Err(error) => {
                // SAFETY: balances the successful initialization on this thread.
                unsafe {
                    RoUninitialize();
                }
                Err(error)
            }
        }
    }
}
impl Drop for Context {
    fn drop(&mut self) {
        // SAFETY: worker-local guard balances one successful RoInitialize.
        unsafe {
            RoUninitialize();
        }
    }
}
pub(super) struct Request {
    abandoned: AtomicBool,
    pub(super) deadline: Instant,
    cancel: Cancel,
}
impl Request {
    pub(super) fn check(&self) -> Result<()> {
        if self.cancel.is_cancelled() {
            return Err(DriverError::Cancelled {
                maybe_executed: false,
            });
        }
        if self.abandoned.load(Ordering::Acquire) || Instant::now() >= self.deadline {
            return Err(DriverError::Timeout {
                maybe_executed: false,
            });
        }
        Ok(())
    }
}
struct Abandon(Arc<Request>);
impl Drop for Abandon {
    fn drop(&mut self) {
        self.0.abandoned.store(true, Ordering::Release);
    }
}
struct Job {
    window: SourceWindow,
    request: Arc<Request>,
    reply: oneshot::Sender<Result<RawPixels>>,
}
pub(super) struct Worker {
    tx: mpsc::SyncSender<Job>,
    pub(super) available: bool,
}
impl Worker {
    pub(super) async fn start(
        name: &str,
        probe: fn() -> bool,
        capture: fn(&SourceWindow, &Request) -> Result<RawPixels>,
    ) -> Result<Self> {
        let (tx, rx) = mpsc::sync_channel::<Job>(8);
        let (ready, started) = oneshot::channel();
        std::thread::Builder::new()
            .name(name.into())
            .spawn(move || {
                let _context = match Context::new() {
                    Ok(context) => context,
                    Err(error) => {
                        let _ = ready.send(Err(error));
                        return;
                    }
                };
                let available = probe();
                if ready.send(Ok(available)).is_err() {
                    return;
                }
                while let Ok(job) = rx.recv() {
                    if job.reply.is_closed() || job.request.check().is_err() {
                        continue;
                    }
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        let pixels = capture(&job.window, &job.request)?;
                        job.request.check()?;
                        Ok(pixels)
                    }))
                    .unwrap_or(Err(DriverError::Crashed {
                        maybe_executed: false,
                    }));
                    // A dropped/expired caller can never receive a late image.
                    if !job.reply.is_closed() {
                        let result = match job.request.check() {
                            Ok(()) => result,
                            Err(error) => Err(error),
                        };
                        let _ = job.reply.send(result);
                    }
                }
            })
            .map_err(|_| DriverError::Crashed {
                maybe_executed: false,
            })?;
        let available = tokio::time::timeout(DRIVER_DEADLINE, started)
            .await
            .map_err(|_| DriverError::Timeout {
                maybe_executed: false,
            })?
            .map_err(|_| DriverError::Crashed {
                maybe_executed: false,
            })??;
        Ok(Self { tx, available })
    }
    pub(super) async fn capture(
        &self,
        window: &SourceWindow,
        deadline: Duration,
        cancel: &Cancel,
    ) -> Result<RawPixels> {
        let budget = deadline.min(DRIVER_DEADLINE);
        let request = Arc::new(Request {
            abandoned: AtomicBool::new(false),
            deadline: Instant::now() + budget,
            cancel: cancel.clone(),
        });
        request.check()?;
        let _abandon = Abandon(request.clone());
        let (reply, result) = oneshot::channel();
        self.tx
            .try_send(Job {
                window: *window,
                request,
                reply,
            })
            .map_err(|_| DriverError::Crashed {
                maybe_executed: false,
            })?;
        tokio::select! { biased;
            _ = cancel.cancelled() => Err(DriverError::Cancelled { maybe_executed: false }),
            _ = tokio::time::sleep(budget) => Err(DriverError::Timeout { maybe_executed: false }),
            result = result => result.map_err(|_| DriverError::Crashed { maybe_executed: false })?,
        }
    }
}
pub(super) fn preflight(
    window: &SourceWindow,
    request: &Request,
    path: CapturePath,
) -> Result<HWND> {
    request.check()?;
    let hwnd = HWND(window.hwnd as usize as *mut _);
    // SAFETY: read-only queries of a trusted HWND; all outputs are live stack
    // buffers. A vanished/reused window is also checked by discovery postflight.
    unsafe {
        if !IsWindow(Some(hwnd)).as_bool() {
            return Err(DriverError::WindowGone);
        }
        if IsIconic(hwnd).as_bool() {
            return Err(DriverError::WindowMinimized);
        }
        let mut affinity = 0;
        match GetWindowDisplayAffinity(hwnd, &mut affinity) {
            Ok(()) => check_display_affinity(path, Some(affinity))?,
            Err(error) if error.code().0 == 0x80070005u32 as i32 => return Err(permission(false)),
            // The documented getter requires a layered window and DWM. Only
            // WGC itself can enforce exclusion when this probe is unavailable.
            Err(_) => check_display_affinity(path, None)?,
        }
        let mut rect = RECT::default();
        GetWindowRect(hwnd, &mut rect).map_err(map_error)?;
        let dpi = GetDpiForWindow(hwnd);
        let frame = gcu_core::model::Rect {
            x: f64::from(rect.left),
            y: f64::from(rect.top),
            w: f64::from(rect.right - rect.left),
            h: f64::from(rect.bottom - rect.top),
        };
        if dpi == 0 {
            return Err(native("window DPI unavailable"));
        }
        if frame != window.frame || f64::from(dpi) / 96. != window.scale {
            return Err(native("capture geometry changed"));
        }
    }
    request.check()?;
    Ok(hwnd)
}
