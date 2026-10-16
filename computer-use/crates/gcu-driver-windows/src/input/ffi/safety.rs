//! Thread-affine low-level hooks; global hotkey has its own message pump.
use crate::input::{
    hooks::HookEvent,
    hotkey::{
        GuardEvents, HotkeyConfig, SafetyApi, SafetyPump, HOTKEY_STARTUP_TIMEOUT,
        STARTUP_STEP_TIMEOUT,
    },
    watchdog::POLL_INTERVAL,
};
use std::{
    cell::RefCell,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    time::Instant,
};
use windows::Win32::{
    Foundation::{
        GetLastError, SetLastError, ERROR_SUCCESS, HANDLE, HINSTANCE, LPARAM, LRESULT, WPARAM,
    },
    System::{LibraryLoader::GetModuleHandleW, StationsAndDesktops::*},
    UI::{Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
};
const HOTKEY_ID: i32 = 0x4732;
const PROBE_TAG: usize = 0x47435532;
mod probe;
struct Context {
    events: Arc<GuardEvents>,
    keyboard: Option<Instant>,
    mouse: Option<Instant>,
    keyboard_probe_flags: Option<u32>,
    mouse_probe_flags: Option<u32>,
    last_keyboard_flags: Option<u32>,
    last_mouse_flags: Option<u32>,
}
thread_local! { static CONTEXT: RefCell<Option<Context>> = const { RefCell::new(None) }; }

/// Production parent listener. Hooks and RegisterHotKey use different threads,
/// so a hung hook/desktop FFI cannot queue Stop behind that call.
pub struct RealSafetyApi;
impl SafetyApi for RealSafetyApi {
    fn open(
        &self,
        config: HotkeyConfig,
        events: Arc<GuardEvents>,
    ) -> Result<Box<dyn SafetyPump>, String> {
        events.startup_step("hook_desktop", STARTUP_STEP_TIMEOUT);
        events.startup_progress("hook_desktop", "OpenInputDesktop pending");
        interactive_desktop()?;
        events.startup_progress("hook_desktop", "Default desktop verified");
        let hotkey = HotkeyThread::start(config, events.clone())?;
        let hooks = Hooks::install(Some(&events))?;
        events.startup_progress("hook_install", "keyboard and mouse installed; Win32=0");
        let pump_events = events.clone();
        CONTEXT.with(|slot| {
            *slot.borrow_mut() = Some(Context {
                events,
                keyboard: None,
                mouse: None,
                keyboard_probe_flags: None,
                mouse_probe_flags: None,
                last_keyboard_flags: None,
                last_mouse_flags: None,
            })
        });
        let probe = probe::Sender::start(pump_events.clone())?;
        Ok(Box::new(Pump {
            hooks,
            _hotkey: hotkey,
            events: pump_events,
            first_probe: true,
            probe,
        }))
    }
}
struct Hooks {
    keyboard: HHOOK,
    mouse: HHOOK,
}
impl Hooks {
    fn install(events: Option<&GuardEvents>) -> Result<Self, String> {
        if let Some(events) = events {
            events.startup_step("hook_install", STARTUP_STEP_TIMEOUT);
        }
        let progress = |detail| {
            if let Some(events) = events {
                events.startup_progress("hook_install", detail);
            }
        };
        // SAFETY: process module is borrowed, callbacks have static code lifetime
        // and run only on this installing message-pump thread. No cross-thread COM.
        unsafe {
            progress("GetModuleHandleW pending");
            let module = GetModuleHandleW(None).map_err(|e| failure("GetModuleHandleW", e))?;
            let instance = Some(HINSTANCE(module.0));
            progress("SetWindowsHookExW(WH_KEYBOARD_LL) pending");
            let keyboard = SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_hook), instance, 0)
                .map_err(|e| failure("SetWindowsHookExW(WH_KEYBOARD_LL)", e))?;
            progress("SetWindowsHookExW(WH_MOUSE_LL) pending");
            match SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_hook), instance, 0) {
                Ok(mouse) => Ok(Self { keyboard, mouse }),
                Err(error) => {
                    let reason = failure("SetWindowsHookExW(WH_MOUSE_LL)", error);
                    let _ = UnhookWindowsHookEx(keyboard);
                    Err(reason)
                }
            }
        }
    }
}
impl Drop for Hooks {
    fn drop(&mut self) {
        // SAFETY: owned hook handles, retired on the installing thread. Silent
        // OS removal may already have retired them; failure still closes health.
        unsafe {
            let _ = UnhookWindowsHookEx(self.keyboard);
            let _ = UnhookWindowsHookEx(self.mouse);
        }
    }
}
// SAFETY: Windows invokes this static callback with hook ABI arguments; owned
// Rust work is caught and callback pointers are never retained past the call.
unsafe extern "system" fn keyboard_hook(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 && lp.0 != 0 {
        let _ = std::panic::catch_unwind(|| {
            // SAFETY: Windows supplies KBDLLHOOKSTRUCT only for HC_ACTION;
            // copied flags/tag are consumed before returning, pointer is not retained.
            let event = unsafe { &*(lp.0 as *const KBDLLHOOKSTRUCT) };
            callback(HookEvent::Keyboard(event.flags.0), event.dwExtraInfo);
        });
    }
    // SAFETY: forward the unchanged borrowed hook arguments; never suppress input.
    unsafe { CallNextHookEx(None, code, wp, lp) }
}
// SAFETY: Windows supplies the low-level mouse hook ABI; all Rust work is caught
// before returning and the OS event structure is read only during this call.
unsafe extern "system" fn mouse_hook(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 && lp.0 != 0 {
        let _ = std::panic::catch_unwind(|| {
            // SAFETY: Windows supplies MSLLHOOKSTRUCT only for HC_ACTION.
            // The borrowed pointer is read only and never leaves this callback.
            let event = unsafe { &*(lp.0 as *const MSLLHOOKSTRUCT) };
            callback(HookEvent::Mouse(event.flags), event.dwExtraInfo);
        });
    }
    // SAFETY: next hook receives the original Windows-owned callback data.
    unsafe { CallNextHookEx(None, code, wp, lp) }
}
fn callback(event: HookEvent, tag: usize) {
    CONTEXT.with(|slot| {
        if let Ok(mut slot) = slot.try_borrow_mut() {
            if let Some(context) = slot.as_mut() {
                let now = Instant::now();
                match event {
                    HookEvent::Keyboard(flags) => context.last_keyboard_flags = Some(flags),
                    HookEvent::Mouse(flags) => context.last_mouse_flags = Some(flags),
                }
                if tag == PROBE_TAG {
                    match event {
                        HookEvent::Keyboard(flags) => context.keyboard_probe_flags = Some(flags),
                        HookEvent::Mouse(flags) => context.mouse_probe_flags = Some(flags),
                    }
                }
                if event.injected() && tag == PROBE_TAG {
                    match event {
                        HookEvent::Keyboard(_) => context.keyboard = Some(now),
                        HookEvent::Mouse(_) => context.mouse = Some(now),
                    }
                }
                // A tagged synthetic probe is never physical activity, even if
                // an upstream filter unexpectedly strips its injected flag.
                if tag != PROBE_TAG {
                    context.events.clock().event(event, now);
                }
            }
        }
    });
}
struct Pump {
    hooks: Hooks,
    _hotkey: HotkeyThread,
    events: Arc<GuardEvents>,
    first_probe: bool,
    probe: probe::Sender,
}
impl SafetyPump for Pump {
    fn poll(&mut self, reinstall: bool) -> Result<(), String> {
        interactive_desktop()?;
        if reinstall {
            // Install replacements before retiring old hooks: no unmonitored gap.
            self.hooks = Hooks::install(None)?;
        }
        if self.first_probe {
            self.events
                .startup_step("hook_proofs", STARTUP_STEP_TIMEOUT);
            self.events
                .startup_progress("hook_probe_pump", "PeekMessageW pending");
        }
        pump_messages(false, None)?;
        if self.first_probe {
            self.events
                .startup_progress("hook_probe_pump", "message queue drained");
            self.first_probe = false;
        }
        // Never call SendInput on its own hook thread: Windows may wait for
        // that thread to service the injected low-level callback. One bounded
        // sender lets this thread keep pumping throughout native dispatch.
        self.probe.poll()?;
        self.confirm_probes()?;

        Ok(())
    }
}
impl Pump {
    fn confirm_probes(&self) -> Result<(), String> {
        CONTEXT.with(|slot| {
            if let Some(context) = slot.borrow().as_ref() {
                context
                    .events
                    .confirm_hook_proofs(context.keyboard, context.mouse)?;
            }
            Ok(())
        })
    }
}
impl Drop for Pump {
    fn drop(&mut self) {
        CONTEXT.with(|slot| *slot.borrow_mut() = None);
    }
}
struct HotkeyThread(Arc<AtomicBool>);
impl HotkeyThread {
    fn start(config: HotkeyConfig, events: Arc<GuardEvents>) -> Result<Self, String> {
        events.startup_step("hotkey_register", HOTKEY_STARTUP_TIMEOUT);
        let close = Arc::new(AtomicBool::new(false));
        let closed = close.clone();
        let (ready, wait) = mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("gcu-win-hotkey".into())
            .spawn(move || {
                events.startup_progress("hotkey_thread", "entered; PeekMessageW pending");
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    // SAFETY: initialize this thread's user32 message queue before
                    // registration; only this thread consumes its hotkey messages.
                    unsafe {
                        let _ = PeekMessageW(&mut MSG::default(), None, 0, 0, PM_NOREMOVE);
                    }
                    events.startup_progress("hotkey_register", "RegisterHotKey pending");
                    // SAFETY: thread-owned registration, numeric validated chord.
                    if let Err(error) = unsafe {
                        RegisterHotKey(
                            None,
                            HOTKEY_ID,
                            HOT_KEY_MODIFIERS(config.modifiers) | MOD_NOREPEAT,
                            config.virtual_key,
                        )
                    } {
                        let _ = ready.send(Err(failure("RegisterHotKey", error)));
                        return;
                    }
                    events.startup_progress("hotkey_register", "registered; Win32=0");
                    struct Registration;
                    impl Drop for Registration {
                        fn drop(&mut self) {
                            // SAFETY: unregister this thread's owned hotkey exactly once.
                            unsafe {
                                let _ = UnregisterHotKey(None, HOTKEY_ID);
                            }
                        }
                    }
                    let _registration = Registration;
                    let _ = ready.send(Ok(()));
                    while !closed.load(Ordering::Acquire) {
                        if let Err(reason) = pump_messages(true, Some(&events)) {
                            events.fail(reason);
                            break;
                        }
                        events.hotkey_heartbeat();
                        std::thread::sleep(POLL_INTERVAL);
                    }
                }));
                if result.is_err() {
                    events.fail("Stop hotkey thread crashed");
                }
            })
            .map_err(|e| format!("Stop hotkey thread spawn: {e}"))?;
        let owner = Self(close);
        wait.recv_timeout(HOTKEY_STARTUP_TIMEOUT)
            .map_err(|e| format!("RegisterHotKey startup/FFI timed out: {e}"))??;
        Ok(owner)
    }
}
impl Drop for HotkeyThread {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}
fn pump_messages(hotkey: bool, events: Option<&GuardEvents>) -> Result<(), String> {
    let mut message = MSG::default();
    // Bound message floods so they cannot starve probes, replacement or shutdown.
    for _ in 0..128 {
        // SAFETY: initialized writable MSG; PeekMessage delivers same-thread hooks.
        if !unsafe { PeekMessageW(&mut message, None, 0, 0, PM_REMOVE) }.as_bool() {
            break;
        }
        if message.message == WM_QUIT {
            return Err("Stop listener message pump exited".into());
        }
        if hotkey && message.message == WM_HOTKEY && message.wParam.0 == HOTKEY_ID as usize {
            if let Some(events) = events {
                events.request_stop();
            }
        }
    }
    Ok(())
}
fn interactive_desktop() -> Result<(), String> {
    // SAFETY: read-only input desktop, noninherited. Access denied is secure.
    let desktop = unsafe { OpenInputDesktop(DESKTOP_CONTROL_FLAGS(0), false, DESKTOP_READOBJECTS) }
        .map_err(|e| failure("OpenInputDesktop", e))?;
    let mut name = [0u16; 256];
    // SAFETY: valid owned desktop and bounded writable UTF-16 output.
    let read = unsafe {
        GetUserObjectInformationW(
            HANDLE(desktop.0),
            UOI_NAME,
            Some(name.as_mut_ptr().cast()),
            (name.len() * 2) as u32,
            None,
        )
    };
    // SAFETY: owned desktop handle is closed after its last use.
    unsafe {
        let _ = CloseDesktop(desktop);
    }
    let name =
        String::from_utf16_lossy(&name[..name.iter().position(|c| *c == 0).unwrap_or(name.len())]);
    if let Err(error) = read {
        Err(failure("GetUserObjectInformationW(UOI_NAME)", error))
    } else if !name.eq_ignore_ascii_case("Default") {
        Err(format!("UAC/secure desktop active: {name}"))
    } else {
        Ok(())
    }
}

fn failure(api: &str, error: windows::core::Error) -> String {
    let hresult = error.code().0 as u32;
    let win32 = if hresult & 0xffff0000 == 0x80070000 {
        Some(hresult & 0xffff)
    } else {
        None
    };
    format!("{api}: HRESULT=0x{hresult:08x}, GetLastError={win32:?}; {error}")
}
