//! macOS-only unsafe boundary: Carbon reservation and listen-only CG event tap.
//!
//! Carbon reserves the chord (and detects conflicts); the session tap observes
//! it before application delivery. Neither consumes or modifies user events.
//! A disabled tap is never automatically re-enabled: it revokes the route.

use super::{
    activity::{ActivityMonitor, OWN_EVENT_TAG},
    hotkey::{register_hotkey, Guards, HotkeyConfig, HotkeyRegistrar, Registration},
};
use std::{
    ffi::c_void,
    panic::{catch_unwind, AssertUnwindSafe},
    ptr,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

type Ref = *mut c_void;

#[repr(C)]
#[derive(Clone, Copy)]
struct HotKeyId {
    signature: u32,
    id: u32,
}

// SAFETY: These declarations match the public Carbon/CoreGraphics/CoreFoundation
// C headers. Opaque objects stay on their creating listener thread.
#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
    fn RegisterEventHotKey(
        code: u32,
        modifiers: u32,
        id: HotKeyId,
        target: Ref,
        options: u32,
        result: *mut Ref,
    ) -> i32;
    fn UnregisterEventHotKey(hotkey: Ref) -> i32;
    fn GetEventDispatcherTarget() -> Ref;
}
// SAFETY: ABI signatures are from CGEvent.h / CGEventTypes.h / AXUIElement.h.
#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn CGPreflightListenEventAccess() -> bool;
    fn AXIsProcessTrusted() -> bool;
    fn CGEventTapCreate(
        tap: u32,
        place: u32,
        options: u32,
        mask: u64,
        callback: unsafe extern "C" fn(Ref, u32, Ref, Ref) -> Ref,
        context: Ref,
    ) -> Ref;
    fn CGEventTapIsEnabled(tap: Ref) -> bool;
    fn CGEventGetIntegerValueField(event: Ref, field: u32) -> i64;
    fn CGEventGetFlags(event: Ref) -> u64;
}
// SAFETY: ABI signatures are from CFMachPort.h / CFRunLoop.h / CFBase.h.
#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    static kCFRunLoopDefaultMode: Ref;
    fn CFRunLoopGetCurrent() -> Ref;
    fn CFRunLoopRunInMode(mode: Ref, seconds: f64, return_after_source: bool) -> i32;
    fn CFMachPortCreateRunLoopSource(allocator: Ref, port: Ref, order: isize) -> Ref;
    fn CFRunLoopAddSource(loop_: Ref, source: Ref, mode: Ref);
    fn CFRunLoopRemoveSource(loop_: Ref, source: Ref, mode: Ref);
    fn CFMachPortInvalidate(port: Ref);
    fn CFRelease(object: Ref);
}

struct Context {
    monitor: Arc<ActivityMonitor>,
    guards: Arc<Guards>,
    config: HotkeyConfig,
}

impl Context {
    fn revoke(&self) {
        self.monitor.connected(false);
        self.guards.request_stop();
    }
}

unsafe extern "C" fn input_callback(_proxy: Ref, kind: u32, event: Ref, context: Ref) -> Ref {
    if context.is_null() {
        return event;
    }
    // SAFETY: `listen` owns a boxed Context until the port is invalidated and
    // its run-loop source is removed. Callbacks run only on that same thread.
    let context = unsafe { &*context.cast::<Context>() };
    let result = catch_unwind(AssertUnwindSafe(|| {
        if kind == u32::MAX || kind == u32::MAX - 1 {
            context.revoke();
            return;
        }
        if event.is_null() {
            return;
        }
        // SAFETY: CoreGraphics supplied a live event for this callback.
        let marker = unsafe { CGEventGetIntegerValueField(event, 42) };
        context.monitor.observe(marker);
        if kind == 10 && marker != OWN_EVENT_TAG {
            // SAFETY: These read fields from that same borrowed event.
            let (keycode, flags) = unsafe {
                (
                    CGEventGetIntegerValueField(event, 9),
                    CGEventGetFlags(event),
                )
            };
            if let Ok(keycode) = u16::try_from(keycode) {
                context
                    .guards
                    .key_down(&context.config, keycode, flags, marker);
            }
        }
    }));
    if result.is_err() {
        let _ = catch_unwind(AssertUnwindSafe(|| context.revoke()));
    }
    event
}

/// Listener lifetime. Drop asks its bounded run loop to stop; no join on FFI.
pub(super) struct Listener {
    quit: Arc<AtomicBool>,
}
impl Listener {
    pub(super) fn start(
        monitor: Arc<ActivityMonitor>,
        guards: Arc<Guards>,
        config: HotkeyConfig,
    ) -> std::io::Result<Self> {
        let quit = Arc::new(AtomicBool::new(false));
        let stop = quit.clone();
        std::thread::Builder::new()
            .name("gcu-mac-tap".into())
            .spawn(move || {
                let context = Box::new(Context {
                    monitor,
                    guards,
                    config,
                });
                if catch_unwind(AssertUnwindSafe(|| listen(&context, &stop))).is_err() {
                    context.revoke();
                }
            })?;
        Ok(Self { quit })
    }
}
impl Drop for Listener {
    fn drop(&mut self) {
        self.quit.store(true, Ordering::Release);
    }
}

struct Resources {
    tap: Ref,
    source: Ref,
    loop_: Ref,
    hotkey: Ref,
}
impl HotkeyRegistrar for Resources {
    fn register(&mut self, config: &HotkeyConfig) -> Registration {
        let Some(modifiers) = carbon_modifiers(config.modifiers) else {
            return Registration::Unavailable;
        };
        // SAFETY: Carbon receives valid scalar key/modifier/id arguments and
        // writes an owned reference into `hotkey` on success. No event handler
        // invokes Rust through Carbon; the listen-only tap handles the chord.
        let status = unsafe {
            RegisterEventHotKey(
                u32::from(config.keycode),
                modifiers,
                HotKeyId {
                    signature: u32::from_be_bytes(*b"GCU2"),
                    id: 1,
                },
                GetEventDispatcherTarget(),
                0,
                &mut self.hotkey,
            )
        };
        match status {
            0 => Registration::Registered,
            -9878 => Registration::Conflict,
            _ => Registration::Unavailable,
        }
    }
}
impl Drop for Resources {
    fn drop(&mut self) {
        // SAFETY: All non-null references were created on this thread, are
        // owned exactly once, and callbacks cannot run after invalidation.
        unsafe {
            if !self.tap.is_null() {
                CFMachPortInvalidate(self.tap);
            }
            if !self.source.is_null() {
                CFRunLoopRemoveSource(self.loop_, self.source, kCFRunLoopDefaultMode);
                CFRelease(self.source);
            }
            if !self.tap.is_null() {
                CFRelease(self.tap);
            }
            if !self.hotkey.is_null() && UnregisterEventHotKey(self.hotkey) != 0 {
                tracing::warn!("macos Stop hotkey unregister failed");
            }
        }
    }
}

fn carbon_modifiers(flags: u64) -> Option<u32> {
    const SUPPORTED: u64 = 131072 | 262144 | 524288 | 1048576;
    if flags & !SUPPORTED != 0 {
        return None;
    }
    let mut modifiers = 0;
    for (cg, carbon) in [
        (131072, 512),
        (262144, 4096),
        (524288, 2048),
        (1048576, 256),
    ] {
        if flags & cg != 0 {
            modifiers |= carbon;
        }
    }
    Some(modifiers)
}

pub(super) fn accessibility_trusted() -> bool {
    // SAFETY: Non-prompting preflight; no pointer arguments or ownership.
    unsafe {
        crate::diagnostics::ax_call("AXIsProcessTrusted", 0, "prompt=false", || {
            AXIsProcessTrusted()
        })
    }
}

fn listen(context: &Context, quit: &AtomicBool) {
    // SAFETY: Preflight functions do not prompt and have no pointer arguments.
    if !unsafe {
        CGPreflightListenEventAccess()
            && crate::diagnostics::ax_call("AXIsProcessTrusted", 0, "listener_preflight", || {
                AXIsProcessTrusted()
            })
    } {
        return;
    }
    // SAFETY: Gets the current listener thread's run loop, borrowed for its life.
    let loop_ = unsafe { CFRunLoopGetCurrent() };
    let mut resources = Resources {
        tap: ptr::null_mut(),
        source: ptr::null_mut(),
        loop_,
        hotkey: ptr::null_mut(),
    };
    let mask = [1, 2, 3, 4, 5, 6, 7, 10, 11, 12, 22, 25, 26, 27]
        .into_iter()
        .fold(0, |bits, event| bits | (1_u64 << event));
    // SAFETY: Context outlives the tap; callback does not mutate or retain
    // events. Session tap=1, head insert=0, listen-only=1. The mask includes
    // keyboard, flags, mouse, drag and wheel input, but no event payload logs.
    resources.tap = unsafe {
        CGEventTapCreate(
            1,
            0,
            1,
            mask,
            input_callback,
            (context as *const Context).cast_mut().cast(),
        )
    };
    if resources.tap.is_null() {
        return;
    }
    // SAFETY: `tap` is live, Create returns an owned run-loop source or null.
    resources.source = unsafe { CFMachPortCreateRunLoopSource(ptr::null_mut(), resources.tap, 0) };
    if resources.source.is_null() {
        return;
    }
    // SAFETY: All references are live and owned on this thread.
    unsafe {
        CFRunLoopAddSource(loop_, resources.source, kCFRunLoopDefaultMode);
    }
    context.monitor.connected(true);
    if register_hotkey(&mut resources, &context.monitor, &context.config)
        != Registration::Registered
    {
        tracing::warn!("macos Stop hotkey conflict or registration failure");
    }
    while !quit.load(Ordering::Acquire) {
        // SAFETY: Runs only this thread's run loop, returning within 10ms
        // absent an OS stall. Independent watchdog detects stalled heartbeats.
        unsafe {
            CFRunLoopRunInMode(kCFRunLoopDefaultMode, 0.01, true);
        }
        // SAFETY: `tap` remains owned by Resources. Preflights do not prompt.
        let healthy = unsafe {
            CGEventTapIsEnabled(resources.tap)
                && CGPreflightListenEventAccess()
                && crate::diagnostics::ax_call("AXIsProcessTrusted", 0, "listener_health", || {
                    AXIsProcessTrusted()
                })
        };
        if !healthy || !context.monitor.is_available() {
            context.revoke();
            return;
        }
        context.monitor.heartbeat();
    }
    context.monitor.connected(false);
}
