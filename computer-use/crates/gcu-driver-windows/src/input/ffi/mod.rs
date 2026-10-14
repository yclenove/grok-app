//! Win32 adapter. COM and reads use a persistent MTA; release bypasses it.
mod native;
mod sample;
use super::{api::*, ledger::InputLedger};
use crate::{
    api::{NativeWindow, WinApi},
    uia::{patterns::ActionContext, worker::Worker, RealUia, UiaElement},
};
use async_trait::async_trait;
use gcu_core::{
    driver::*,
    model::{NativeHandle, WindowPoint},
};
use std::sync::Arc;
use windows::Win32::{
    Foundation::{HWND, POINT},
    UI::{Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
};

/// Complete production input adapter. Startup/each call is bounded by the
/// driver's deadline. No COM interface crosses the dedicated worker thread.
pub struct RealInput {
    worker: Worker<State>,
    uia: Arc<RealUia>,
}
/// Up-only sender for the isolated parent. Starts no COM/provider/action worker.
pub struct NativeRelease;
#[async_trait]
impl InputReleaser for NativeRelease {
    async fn release(&self, ledger: Arc<InputLedger>) {
        release_ledger(ledger).await;
    }
}
impl RealInput {
    /// Start the input MTA using the same stamped discovery API as the driver.
    pub async fn start(api: Arc<dyn WinApi>, uia: Arc<RealUia>) -> Result<Self> {
        Ok(Self {
            worker: Worker::start(
                move || {
                    Ok(State {
                        _context: api.enter_thread()?,
                        api,
                    })
                },
                |_| {},
            )
            .await?,
            uia,
        })
    }
}
struct State {
    api: Arc<dyn WinApi>,
    _context: Box<dyn std::any::Any>,
}
fn hwnd(id: u64) -> HWND {
    HWND(id as usize as *mut _)
}
fn id(hwnd: HWND) -> u64 {
    hwnd.0 as usize as u64
}
fn failure() -> DriverError {
    DriverError::Native {
        message: "Windows input API failed".into(),
        maybe_executed: false,
    }
}
impl State {
    fn validate(&self, target: &Target, foreground: bool) -> Result<()> {
        let current = self
            .api
            .enum_windows()?
            .into_iter()
            .find(|w| w.hwnd == target.window.hwnd)
            .ok_or(DriverError::WindowGone)?;
        crate::desktop::HwndStamp::from(&target.window).validate(&current)?;
        if current.frame != target.window.frame || current.scale != target.window.scale {
            return Err(DriverError::InvalidRequest);
        }
        if current.minimized {
            return Err(DriverError::WindowMinimized);
        }
        crate::gate::WinGate.check(
            self.api.as_ref(),
            current.pid,
            foreground,
            self.api.on_current_desktop(current.hwnd)?,
        )?;
        let mut pid = 0;
        // SAFETY: borrowed numeric HWNDs and writable PID; no memory references
        // to a target process. The child must still belong to the exact root.
        unsafe {
            if !IsWindow(Some(hwnd(target.hwnd))).as_bool()
                || GetAncestor(hwnd(target.hwnd), GA_ROOT) != hwnd(current.hwnd)
            {
                return Err(DriverError::WindowGone);
            }
            GetWindowThreadProcessId(hwnd(target.hwnd), Some(&mut pid));
        }
        crate::gate::WinGate.check(
            self.api.as_ref(),
            pid,
            foreground,
            self.api.on_current_desktop(current.hwnd)?,
        )?;
        if native::class(target.hwnd)? != target.class {
            return Err(DriverError::StaleElement);
        }
        Ok(())
    }
    fn resolve(
        &self,
        window: NativeWindow,
        point: Option<WindowPoint>,
        recipient: Option<(u64, String)>,
    ) -> Result<Target> {
        let mut child = hwnd(window.hwnd);
        let mut capable = true;
        let class = if let Some((native, class)) = recipient {
            capable = native != 0;
            if capable {
                child = hwnd(native);
            }
            if capable {
                native::class(id(child))?
            } else {
                let _ = class;
                native::class(window.hwnd)?
            }
        } else if let Some(p) = point {
            // SAFETY: physical screen point converted with this thread's PMv2
            // context, bounded child traversal. Never search another root.
            unsafe {
                for _ in 0..64 {
                    let mut local = POINT {
                        x: p.x.round() as i32,
                        y: p.y.round() as i32,
                    };
                    if !windows::Win32::Graphics::Gdi::ScreenToClient(child, &mut local).as_bool() {
                        return Err(failure());
                    }
                    let next = ChildWindowFromPointEx(
                        child,
                        local,
                        CWP_SKIPINVISIBLE | CWP_SKIPDISABLED | CWP_SKIPTRANSPARENT,
                    );
                    if next.0.is_null() || next == child {
                        break;
                    }
                    child = next;
                }
            }
            native::class(id(child))?
        } else {
            let mut info = GUITHREADINFO {
                cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
                ..Default::default()
            };
            // SAFETY: initialized output and window thread ID. Focus must belong
            // to the authorized root; never use global foreground focus.
            unsafe {
                let thread = GetWindowThreadProcessId(child, None);
                GetGUIThreadInfo(thread, &mut info).map_err(|_| failure())?;
                if !info.hwndFocus.0.is_null() && GetAncestor(info.hwndFocus, GA_ROOT) == child {
                    child = info.hwndFocus;
                } else {
                    return Err(DriverError::ElementNotActionable);
                }
            }
            native::class(id(child))?
        };
        let target = Target {
            window,
            hwnd: id(child),
            class,
            message_capable: capable,
        };
        // A virtual UIA element has no HWND/class to compare. Only its exact
        // top-level recipient participates in global input preflight.
        if target.message_capable {
            self.validate(&target, false)?;
        }
        Ok(target)
    }
}
#[async_trait]
impl InputApi for RealInput {
    async fn resolve(
        &self,
        window: NativeWindow,
        point: Option<WindowPoint>,
        focus: Option<NativeHandle>,
    ) -> Result<Target> {
        let recipient = if let Some(focus) = focus {
            let el = focus
                .downcast_ref::<UiaElement>()
                .ok_or(DriverError::StaleElement)?;
            Some(self.uia.input_recipient(el.node, window.hwnd).await?)
        } else {
            None
        };
        self.worker
            .call(move |s, r| {
                r.check()?;
                s.resolve(window, point, recipient)
            })
            .await
    }
    async fn character_key(
        &self,
        target: Target,
        character: char,
    ) -> Result<(u16, Vec<gcu_protocol::keys::Modifier>)> {
        self.worker
            .call(move |_, r| {
                r.check()?;
                if character.len_utf16() != 1 {
                    return Err(DriverError::CapabilityUnavailable);
                }
                // SAFETY: layout belongs to the exact target's OS thread; no handles
                // are owned or freed here. An unmappable character is refused.
                let result = unsafe {
                    VkKeyScanExW(
                        character as u16,
                        GetKeyboardLayout(GetWindowThreadProcessId(hwnd(target.hwnd), None)),
                    )
                };
                if result == -1 {
                    return Err(DriverError::CapabilityUnavailable);
                }
                let flags = (result as u16) >> 8;
                let mut mods = Vec::new();
                use gcu_protocol::keys::Modifier;
                if flags & 1 != 0 {
                    mods.push(Modifier::Shift);
                }
                if flags & 2 != 0 {
                    mods.push(Modifier::Ctrl);
                }
                if flags & 4 != 0 {
                    mods.push(Modifier::Alt);
                }
                if flags & !7 != 0 {
                    return Err(DriverError::CapabilityUnavailable);
                }
                Ok(((result as u16) & 255, mods))
            })
            .await
    }
    async fn validate(&self, target: Target, foreground: bool) -> Result<()> {
        self.worker
            .call(move |s, r| {
                r.check()?;
                s.validate(&target, foreground)
            })
            .await
    }
    async fn activate(&self, target: Target, context: ActionContext) -> Result<()> {
        self.worker
            .call_action(context.clone(), move |s, r| {
                r.check()?;
                s.validate(&target, true)?;
                context.check()?;
                native::activate(target.window.hwnd, &context)
            })
            .await
    }
    async fn foreground(&self) -> Result<u64> {
        // SAFETY: GetForegroundWindow is nonblocking with no borrowed output.
        Ok(id(unsafe { GetForegroundWindow() }))
    }
    async fn focus(
        &self,
        target: Target,
        focus: NativeHandle,
        context: ActionContext,
    ) -> Result<()> {
        let el = focus
            .downcast_ref::<UiaElement>()
            .ok_or(DriverError::StaleElement)?;
        self.uia
            .focus_input(el.node, target.window.hwnd, context)
            .await
    }
    async fn send(
        &self,
        target: Target,
        events: Vec<Event>,
        delivery: Delivery,
        ledger: Arc<InputLedger>,
        context: ActionContext,
    ) -> Result<()> {
        self.worker
            .call_action(context.clone(), move |s, r| {
                r.check()?;
                s.validate(&target, delivery == Delivery::Foreground)?;
                if delivery == Delivery::Foreground {
                    // SAFETY: nonblocking foreground probe immediately before SendInput.
                    if unsafe { GetForegroundWindow() } != hwnd(target.window.hwnd) {
                        return Err(DriverError::CapabilityUnavailable);
                    }
                    let inputs = native::inputs(&events)?;
                    ledger.dispatch_packet(&target, &events, delivery, &context, || {
                        native::inject(&inputs)
                    })
                } else {
                    for event in events {
                        r.check()?;
                        ledger.dispatch(&target, event, delivery, &context, |_| {
                            native::post(&target, event)
                        })?;
                    }
                    Ok(())
                }
            })
            .await
    }
    async fn sample(&self, target: Target) -> Result<u64> {
        self.worker
            .call(move |s, r| {
                r.check()?;
                s.validate(&target, false)?;
                sample::sample(&target)
            })
            .await
    }
    async fn release(&self, ledger: Arc<InputLedger>) {
        release_ledger(ledger).await;
    }
}
async fn release_ledger(ledger: Arc<InputLedger>) {
    // No provider/worker/gate queue: Stop must still release on a hung read.
    let task = tokio::task::spawn_blocking(move || {
        let _dpi = crate::ffi::DpiContext::new();
        ledger.release(|target, event, delivery| {
            if delivery == Delivery::Foreground {
                native::inject(&native::inputs(&[event])?)
            } else {
                native::post(target, event)
            }
        });
    });
    if tokio::time::timeout(std::time::Duration::from_millis(100), task)
        .await
        .is_err()
    {
        tracing::warn!("Windows input release deadline exceeded");
    }
}
