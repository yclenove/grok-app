//! Thread-affine low-level hooks; global hotkey has its own message pump.
use crate::input::{
    hooks::HookEvent,
    hotkey::{GuardEvents, HotkeyConfig, SafetyApi, SafetyPump},
    watchdog::{HEARTBEAT_TIMEOUT, POLL_INTERVAL},
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
    Foundation::{HANDLE, HINSTANCE, LPARAM, LRESULT, WPARAM},
    System::{LibraryLoader::GetModuleHandleW, StationsAndDesktops::*},
    UI::{Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
};
const HOTKEY_ID: i32 = 0x4732;
const PROBE_TAG: usize = 0x47435532;
struct Context {
    events: Arc<GuardEvents>,
    keyboard: Option<Instant>,
    mouse: Option<Instant>,
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
    ) -> Result<Box<dyn SafetyPump>, &'static str> {
        interactive_desktop()?;
        let hotkey = HotkeyThread::start(config, events.clone())?;
        CONTEXT.with(|slot| {
            *slot.borrow_mut() = Some(Context {
                events,
                keyboard: None,
                mouse: None,
            })
        });
        let hooks = Hooks::install()?;
        Ok(Box::new(Pump {
            hooks,
            _hotkey: hotkey,
        }))
    }
}
struct Hooks {
    keyboard: HHOOK,
    mouse: HHOOK,
}
impl Hooks {
    fn install() -> Result<Self, &'static str> {
        // SAFETY: process module is borrowed, callbacks have static code lifetime
        // and run only on this installing message-pump thread. No cross-thread COM.
        unsafe {
            let module = GetModuleHandleW(None).map_err(|_| "hook module unavailable")?;
            let instance = Some(HINSTANCE(module.0));
            let keyboard = SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_hook), instance, 0)
                .map_err(|_| "WH_KEYBOARD_LL registration failed")?;
            match SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_hook), instance, 0) {
                Ok(mouse) => Ok(Self { keyboard, mouse }),
                Err(_) => {
                    let _ = UnhookWindowsHookEx(keyboard);
                    Err("WH_MOUSE_LL registration failed")
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
                if event.injected() && tag == PROBE_TAG {
                    match event {
                        HookEvent::Keyboard(_) => context.keyboard = Some(now),
                        HookEvent::Mouse(_) => context.mouse = Some(now),
                    }
                }
                context.events.clock().event(event, now);
            }
        }
    });
}
struct Pump {
    hooks: Hooks,
    _hotkey: HotkeyThread,
}
impl SafetyPump for Pump {
    fn poll(&mut self, reinstall: bool) -> Result<(), &'static str> {
        interactive_desktop()?;
        if reinstall {
            // Install replacements before retiring old hooks: no unmonitored gap.
            self.hooks = Hooks::install()?;
        }
        pump_messages(false, None)?;
        let proof = CONTEXT.with(|slot| {
            let slot = slot.borrow();
            slot.as_ref()
                .and_then(|context| Some(context.keyboard?.min(context.mouse?)))
        });
        if let Some(at) = proof {
            if at.elapsed() > HEARTBEAT_TIMEOUT {
                return Err("low-level hook probe expired (possibly silently removed)");
            }
            CONTEXT.with(|slot| {
                if let Some(context) = slot.borrow().as_ref() {
                    context.events.clock().confirm(at);
                }
            });
        }
        // Up-only unused F24 and zero relative mouse move test both hook chains.
        // These are synthetic health probes, never physical activity, normal
        // action downs or foreground activation. No user cursor movement.
        let probes = [
            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VK_F24,
                        dwFlags: KEYEVENTF_KEYUP,
                        dwExtraInfo: PROBE_TAG,
                        ..Default::default()
                    },
                },
            },
            INPUT {
                r#type: INPUT_MOUSE,
                Anonymous: INPUT_0 {
                    mi: MOUSEINPUT {
                        dwFlags: MOUSEEVENTF_MOVE | MOUSEEVENTF_MOVE_NOCOALESCE,
                        dwExtraInfo: PROBE_TAG,
                        ..Default::default()
                    },
                },
            },
        ];
        // SAFETY: fully initialized two-record slice; only up/zero motion probes.
        if unsafe { SendInput(&probes, std::mem::size_of::<INPUT>() as i32) } != 2 {
            return Err("hook probe injection unavailable (desktop/UIPI)");
        }
        Ok(())
    }
}
impl Drop for Pump {
    fn drop(&mut self) {
        CONTEXT.with(|slot| *slot.borrow_mut() = None);
    }
}
struct HotkeyThread(Arc<AtomicBool>);
impl HotkeyThread {
    fn start(config: HotkeyConfig, events: Arc<GuardEvents>) -> Result<Self, &'static str> {
        let close = Arc::new(AtomicBool::new(false));
        let closed = close.clone();
        let (ready, wait) = mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("gcu-win-hotkey".into())
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    // SAFETY: thread-owned registration, numeric validated chord.
                    if unsafe {
                        RegisterHotKey(
                            None,
                            HOTKEY_ID,
                            HOT_KEY_MODIFIERS(config.modifiers) | MOD_NOREPEAT,
                            config.virtual_key,
                        )
                    }
                    .is_err()
                    {
                        let _ = ready.send(Err("RegisterHotKey conflict or registration failed"));
                        return;
                    }
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
            .map_err(|_| "Stop hotkey thread unavailable")?;
        let owner = Self(close);
        wait.recv_timeout(std::time::Duration::from_millis(100))
            .map_err(|_| "RegisterHotKey startup/FFI timed out")??;
        Ok(owner)
    }
}
impl Drop for HotkeyThread {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}
fn pump_messages(hotkey: bool, events: Option<&GuardEvents>) -> Result<(), &'static str> {
    let mut message = MSG::default();
    // Bound message floods so they cannot starve probes, replacement or shutdown.
    for _ in 0..128 {
        // SAFETY: initialized writable MSG; PeekMessage delivers same-thread hooks.
        if !unsafe { PeekMessageW(&mut message, None, 0, 0, PM_REMOVE) }.as_bool() {
            break;
        }
        if message.message == WM_QUIT {
            return Err("Stop listener message pump exited");
        }
        if hotkey && message.message == WM_HOTKEY && message.wParam.0 == HOTKEY_ID as usize {
            if let Some(events) = events {
                events.request_stop();
            }
        }
    }
    Ok(())
}
fn interactive_desktop() -> Result<(), &'static str> {
    // SAFETY: read-only input desktop, noninherited. Access denied is secure.
    let desktop = unsafe { OpenInputDesktop(DESKTOP_CONTROL_FLAGS(0), false, DESKTOP_READOBJECTS) }
        .map_err(|_| "UAC/secure desktop unavailable")?;
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
    if read.is_err() || !name.eq_ignore_ascii_case("Default") {
        Err("UAC/secure desktop active")
    } else {
        Ok(())
    }
}
