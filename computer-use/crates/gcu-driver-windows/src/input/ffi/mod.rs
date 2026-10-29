//! Win32 adapter. COM and reads use a persistent MTA; release bypasses it.
pub(crate) mod clipboard;
mod native;
mod safety;
#[cfg(windows)]
pub(crate) use safety::wait_for_messages;
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
pub use safety::RealSafetyApi;
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
/// Up-only sender and lazy clipboard owner for the isolated parent.
/// Starts no COM/provider/input-action worker.
#[derive(Default)]
pub struct NativeRelease {
    clipboard: Arc<clipboard::NativeClipboard>,
}
#[async_trait]
impl InputReleaser for NativeRelease {
    fn clipboard(&self) -> Option<Arc<dyn super::clipboard::ClipboardApi>> {
        Some(self.clipboard.clone())
    }
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
/// Identity and `INPUT` records built before the ledger and the pointer gate.
struct PreparedSend {
    identity: super::admission::ExpectedIdentity,
    inputs: Vec<INPUT>,
}
/// Scalar reads allowed inside the pointer gate. No enumeration, COM, or log write.
struct WinScalar;
impl super::admission::ScalarAdmission for WinScalar {
    fn foreground(&self) -> u64 {
        // SAFETY: nonblocking foreground read; no borrowed output.
        id(unsafe { GetForegroundWindow() })
    }
    fn is_window(&self, window: u64) -> bool {
        // SAFETY: borrowed numeric HWND; nonblocking liveness query.
        unsafe { IsWindow(Some(hwnd(window))).as_bool() }
    }
    fn root(&self, window: u64) -> u64 {
        // SAFETY: borrowed numeric HWND; returns another HWND, no allocation.
        id(unsafe { GetAncestor(hwnd(window), GA_ROOT) })
    }
    fn thread_process(&self, window: u64) -> (u32, u32) {
        let mut pid = 0;
        // SAFETY: borrowed numeric HWND and a writable pid slot. No memory from the target.
        let tid = unsafe { GetWindowThreadProcessId(hwnd(window), Some(&mut pid)) };
        (tid, pid)
    }
    fn class_matches(&self, window: u64, class: &[u16]) -> bool {
        native::class_matches(window, class)
    }
    fn frame(&self, window: u64) -> Option<super::admission::FramePx> {
        native::frame_px(window).ok()
    }
    fn iconic(&self, window: u64) -> bool {
        native::iconic(window)
    }
    fn generation(&self, window: u64, property: &[u16]) -> Option<u64> {
        native::generation(window, property)
    }
    fn dpi(&self, window: u64) -> u32 {
        native::dpi_of(window)
    }
}
/// Scalar snapshot taken at the end of `State::validate`, from the same calls the gate repeats.
struct Snapshot {
    tid: u32,
    pid: u32,
    frame: super::admission::FramePx,
    minimized: bool,
    root_pid: u32,
    root_class_units: Vec<u16>,
    class_units: Vec<u16>,
    created: u64,
    generation_property: Vec<u16>,
    dpi: u32,
}
fn same_frame(frame: super::admission::FramePx, recorded: &gcu_protocol::Rect) -> bool {
    recorded.x == f64::from(frame.left)
        && recorded.y == f64::from(frame.top)
        && recorded.w == f64::from(frame.right - frame.left)
        && recorded.h == f64::from(frame.bottom - frame.top)
}
fn expected_identity(target: &Target, snap: Snapshot) -> super::admission::ExpectedIdentity {
    super::admission::ExpectedIdentity {
        root: target.window.hwnd,
        hwnd: target.hwnd,
        class_units: snap.class_units,
        pid: snap.pid,
        tid: snap.tid,
        frame: snap.frame,
        minimized: snap.minimized,
        root_pid: snap.root_pid,
        root_class_units: snap.root_class_units,
        created: snap.created,
        generation_property: snap.generation_property,
        dpi: snap.dpi,
    }
}
fn scale_of(dpi: u32) -> f64 {
    if dpi == 0 {
        1.0
    } else {
        f64::from(dpi) / 96.0
    }
}
impl State {
    /// Blocking checks (enumeration, the security gate, COM). Returns the scalar snapshot
    /// the pointer gate re-reads. `process_created` and tokens are checked here and not returned.
    fn validate(&self, target: &Target, foreground: bool) -> Result<Snapshot> {
        let current = self
            .api
            .enum_windows()?
            .into_iter()
            .find(|w| w.hwnd == target.window.hwnd)
            .ok_or(DriverError::WindowGone)?;
        crate::desktop::HwndStamp::from(&target.window).validate(&current)?;
        crate::scope_identity::require_delivery_scalars(
            current.hwnd,
            current.pid,
            current.process_created,
            current.created,
            target.hwnd,
        )?;
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
        let tid = unsafe {
            if !IsWindow(Some(hwnd(target.hwnd))).as_bool()
                || GetAncestor(hwnd(target.hwnd), GA_ROOT) != hwnd(current.hwnd)
            {
                return Err(DriverError::WindowGone);
            }
            GetWindowThreadProcessId(hwnd(target.hwnd), Some(&mut pid))
        };
        if tid == 0 {
            return Err(DriverError::WindowGone);
        }
        crate::gate::WinGate.check(
            self.api.as_ref(),
            pid,
            foreground,
            self.api.on_current_desktop(current.hwnd)?,
        )?;
        let class_units = native::class_units(target.hwnd)?;
        if String::from_utf16_lossy(&class_units) != target.class {
            return Err(DriverError::StaleElement);
        }
        let root_class_units = native::class_units(current.hwnd)?;
        if String::from_utf16_lossy(&root_class_units) != current.class {
            return Err(DriverError::WindowGone);
        }
        let frame = native::frame_px(current.hwnd).map_err(|_| DriverError::WindowGone)?;
        if !same_frame(frame, &current.frame) {
            return Err(DriverError::InvalidRequest);
        }
        let minimized = native::iconic(current.hwnd);
        if minimized || minimized != current.minimized {
            return Err(DriverError::WindowMinimized);
        }
        let dpi = native::dpi_of(current.hwnd);
        if scale_of(dpi) != current.scale {
            return Err(DriverError::InvalidRequest);
        }
        let mut root_pid = 0;
        // SAFETY: borrowed root HWND and a writable pid slot. No process handle.
        let root_tid = unsafe { GetWindowThreadProcessId(hwnd(current.hwnd), Some(&mut root_pid)) };
        if root_tid == 0 || root_pid != current.pid {
            return Err(DriverError::WindowGone);
        }
        let generation_property = self.api.generation_property().to_vec();
        let created = if generation_property.is_empty() {
            current.created
        } else {
            match native::generation(current.hwnd, &generation_property) {
                Some(created) if created == current.created => created,
                _ => return Err(DriverError::WindowGone),
            }
        };
        Ok(Snapshot {
            tid,
            pid,
            frame,
            minimized,
            root_pid,
            root_class_units,
            class_units,
            created,
            generation_property,
            dpi,
        })
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
    async fn clipboard_safe(
        &self,
        _: Target,
        focus: Option<NativeHandle>,
        text: String,
    ) -> Result<bool> {
        let Some(el) = focus
            .as_ref()
            .and_then(|f| f.downcast_ref::<UiaElement>())
            .cloned()
        else {
            return Ok(false);
        };
        self.uia.clipboard_safe(el, text).await
    }
    async fn before_paste(
        &self,
        _: Target,
        focus: Option<NativeHandle>,
        context: ActionContext,
    ) -> Result<()> {
        let el = focus
            .as_ref()
            .and_then(|f| f.downcast_ref::<UiaElement>())
            .cloned()
            .ok_or(DriverError::CapabilityUnavailable)?;
        self.uia.verify_paste_focus(el, context).await
    }
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
                s.validate(&target, foreground).map(|_| ())
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
                if delivery != Delivery::Foreground {
                    // One full stamp, security, and recipient validation for this packet,
                    // outside the ledger and the gate. Each post then rechecks scalar identity
                    // and does not require the foreground window. This branch does not take the gate.
                    let live = WinScalar;
                    return super::admission::dispatch_targeted(
                        super::admission::TargetedSend {
                            ledger: ledger.as_ref(),
                            target: &target,
                            events: &events,
                            delivery,
                            context: &context,
                        },
                        || r.check(),
                        || {
                            r.check()?;
                            Ok(expected_identity(&target, s.validate(&target, false)?))
                        },
                        &live,
                        |event| native::post(&target, event),
                    );
                }
                // Enumeration, the security gate, and COM finish in `prepare`, before the ledger
                // and the pointer gate. The gate then only re-reads cancellation, the deadline,
                // the foreground window, and scalar identity.
                let pointer = super::admission::packet_is_pointer(&events);
                super::admission::prepare_then_dispatch(
                    || {
                        r.check()?;
                        let snap = s.validate(&target, true)?;
                        let inputs = native::inputs(&events)?;
                        Ok(PreparedSend {
                            identity: expected_identity(&target, snap),
                            inputs,
                        })
                    },
                    super::admission::Dispatch {
                        ledger: ledger.as_ref(),
                        target: &target,
                        events: &events,
                        delivery,
                        context: &context,
                        pointer,
                    },
                    |prepared| {
                        super::admission::bounded_admit(
                            || r.check(),
                            &prepared.identity,
                            &WinScalar,
                        )
                    },
                    |prepared| native::deliver(&prepared.inputs),
                )
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
                native::inject_release(&native::inputs(&[event])?)
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
