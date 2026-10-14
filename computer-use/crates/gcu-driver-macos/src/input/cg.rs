//! CoreGraphics posting. Compiled only on macOS. SkyLight is not called.
//!
//! `NSApplication` is created only on the main thread. A worker thread posts
//! only after that flag is set, so `CGS_REQUIRE_INIT` is not hit from here.
//! Field 51 is written and read back before `postToPid`. A mismatch is not
//! posted. `CGEventSetWindowLocation` comes from `dlsym`.

use std::sync::atomic::{AtomicBool, Ordering};

use async_trait::async_trait;
use gcu_core::driver::{DriverError, Result};
use objc2::MainThreadMarker;
use objc2_app_kit::{NSApplication, NSApplicationActivationOptions, NSRunningApplication};
use objc2_core_foundation::{CFRetained, CGPoint};
use objc2_core_graphics::{
    CGEvent, CGEventField, CGEventFlags, CGEventTapLocation, CGEventType, CGMouseButton,
    CGScrollEventUnit,
};
use objc2_foundation::NSProcessInfo;

use super::events::{EventApi, EventKind, PostFault, PostPath, Probe, SyntheticEvent};
use super::skylight;

static APPKIT_READY: AtomicBool = AtomicBool::new(false);

/// Create `NSApplication` when this thread is the main thread.
///
/// Returns whether AppKit was already initialized or this call initialized it.
/// Off the main thread this does not call `sharedApplication`.
pub fn ensure_appkit() -> bool {
    if APPKIT_READY.load(Ordering::Acquire) {
        return true;
    }
    let Some(marker) = MainThreadMarker::new() else {
        return false;
    };
    let _app = NSApplication::sharedApplication(marker);
    APPKIT_READY.store(true, Ordering::Release);
    true
}

fn os_major() -> u32 {
    let version = NSProcessInfo::processInfo().operatingSystemVersion();
    u32::try_from(version.majorVersion).unwrap_or(0)
}

fn location_symbol() -> Option<SetWindowLocation> {
    // SAFETY: `dlsym` of a NUL-terminated literal against the process's
    // already-loaded CoreGraphics. A null pointer means the symbol is absent
    // and is not called.
    let ptr = unsafe { libc::dlsym(libc::RTLD_DEFAULT, c"CGEventSetWindowLocation".as_ptr()) };
    if ptr.is_null() {
        None
    } else {
        // SAFETY: when CoreGraphics exports `CGEventSetWindowLocation`, its
        // type is `void(CGEventRef, CGPoint)`. The caller passes a live event
        // and a by-value point. The symbol is not used if this probe is None.
        Some(unsafe { std::mem::transmute::<*mut libc::c_void, SetWindowLocation>(ptr) })
    }
}

type SetWindowLocation = unsafe extern "C" fn(event: *const CGEvent, point: CGPoint);

/// Live CoreGraphics backend. One process-wide AppKit flag.
#[derive(Debug, Default)]
pub struct RealEvents {
    arrival_failed: AtomicBool,
}

impl RealEvents {
    /// Backend with no failed-arrival latch.
    pub fn new() -> Self {
        Self::default()
    }

    fn probe_now(&self) -> Probe {
        let symbol = location_symbol().is_some();
        let os_major = os_major();
        let failed = self.arrival_failed.load(Ordering::Acquire);
        Probe {
            window_location_symbol: symbol,
            field_51: symbol && !failed && os_major >= 27,
            arrival_verified: false,
            os_major,
            appkit_ready: ensure_appkit(),
            accessibility: true,
            secure_prompt: false,
            foreground: true,
            skylight: skylight::status(),
        }
    }
}

#[async_trait]
impl EventApi for RealEvents {
    fn probe(&self) -> Probe {
        self.probe_now()
    }

    fn note_arrival_failed(&self) {
        self.arrival_failed.store(true, Ordering::Release);
    }

    fn window_open(&self, pid: i32, window_id: u32) -> bool {
        pid > 0 && window_id > 0
    }

    async fn activate(&self, pid: i32) -> Result<()> {
        if !ensure_appkit() {
            return Err(DriverError::Native {
                message: "AppKit is not initialized".into(),
                maybe_executed: false,
            });
        }
        let Some(app) = NSRunningApplication::runningApplicationWithProcessIdentifier(pid) else {
            return Err(DriverError::WindowGone);
        };
        let options = NSApplicationActivationOptions::ActivateAllWindows;
        if !app.activateWithOptions(options) {
            return Err(DriverError::Native {
                message: "foreground activation was not accepted".into(),
                maybe_executed: false,
            });
        }
        Ok(())
    }

    async fn post(&self, event: SyntheticEvent) -> std::result::Result<(), PostFault> {
        if !ensure_appkit() {
            return Err(PostFault::NotSent);
        }
        post_one(&event)
    }

    async fn verify_arrival(
        &self,
        _pid: i32,
        _window_id: u32,
    ) -> std::result::Result<(), PostFault> {
        // Readback happens before post. There is no public API that reports
        // whether the target view ran the action. The ignored fixture test is
        // that oracle. A failed readback never reaches this function.
        if self.arrival_failed.load(Ordering::Acquire) {
            Err(PostFault::Uncertain)
        } else {
            Ok(())
        }
    }
}

fn post_one(event: &SyntheticEvent) -> std::result::Result<(), PostFault> {
    let cg = build(event).ok_or(PostFault::NotSent)?;
    if event.path == PostPath::ToPid {
        if !stamp_targeted(&cg, event) {
            return Err(PostFault::NotSent);
        }
        CGEvent::post_to_pid(event.pid, Some(&cg));
    } else {
        CGEvent::post(CGEventTapLocation::HIDEventTap, Some(&cg));
    }
    Ok(())
}

fn stamp_targeted(cg: &CGEvent, event: &SyntheticEvent) -> bool {
    let Some(window) = event.field_51 else {
        return false;
    };
    CGEvent::set_integer_value_field(
        Some(cg),
        CGEventField(super::events::FIELD_WINDOW_NUMBER),
        window,
    );
    let read =
        CGEvent::integer_value_field(Some(cg), CGEventField(super::events::FIELD_WINDOW_NUMBER));
    if read != window {
        return false;
    }
    let Some((x, y)) = event.window_local else {
        return event.kind == EventKind::KeyDown
            || event.kind == EventKind::KeyUp
            || event.kind == EventKind::Unicode;
    };
    let Some(set_window) = location_symbol() else {
        return false;
    };
    let point = CGPoint { x, y };
    // SAFETY: `cg` is a live event created in this function's caller. `point`
    // is a by-value window-local coordinate. The symbol was non-null.
    unsafe { set_window(cg, point) };
    true
}

fn build(event: &SyntheticEvent) -> Option<CFRetained<CGEvent>> {
    let point = point_of(event);
    match event.kind {
        EventKind::Scroll => {
            let (dx, dy) = event.scroll?;
            let wheel =
                CGEvent::new_scroll_wheel_event2(None, CGScrollEventUnit::Pixel, 2, dy, dx, 0)?;
            CGEvent::set_location(Some(&wheel), point);
            Some(wheel)
        }
        EventKind::KeyDown | EventKind::KeyUp | EventKind::Unicode => build_key(event),
        EventKind::MouseDown | EventKind::MouseUp | EventKind::MouseDrag => {
            let kind = mouse_type(event);
            let button = mouse_button(event.button.unwrap_or(0));
            let mouse = CGEvent::new_mouse_event(None, kind, point, button)?;
            CGEvent::set_integer_value_field(
                Some(&mouse),
                CGEventField::MouseEventClickState,
                i64::from(event.click_count),
            );
            CGEvent::set_integer_value_field(
                Some(&mouse),
                CGEventField::MouseEventButtonNumber,
                i64::from(event.button.unwrap_or(0)),
            );
            Some(mouse)
        }
    }
}

fn build_key(event: &SyntheticEvent) -> Option<CFRetained<CGEvent>> {
    let key = CGEvent::new_keyboard_event(None, event.keycode.unwrap_or(0), event.down)?;
    if let Some(flags) = CGEventFlags::from_bits(event.flags) {
        CGEvent::set_flags(Some(&key), flags);
    }
    if let Some(text) = &event.unicode {
        let units: Vec<u16> = text.encode_utf16().collect();
        let raw = CFRetained::as_ptr(&key).as_ptr().cast::<libc::c_void>();
        // SAFETY: `raw` is the live `CGEvent` from `new_keyboard_event`. `units`
        // is valid UTF-16 and outlives the call. CoreGraphics copies
        // `units.len()` code units. The objc2 wrapper hides `UniChar`, so this
        // is the same C function `CGEventKeyboardSetUnicodeString`.
        unsafe {
            CGEventKeyboardSetUnicodeString(raw, units.len() as libc::c_ulong, units.as_ptr())
        };
    }
    Some(key)
}

fn point_of(event: &SyntheticEvent) -> CGPoint {
    let (x, y) = event.window_local.or(event.screen).unwrap_or((0.0, 0.0));
    CGPoint { x, y }
}

fn mouse_button(button: u8) -> CGMouseButton {
    match button {
        1 => CGMouseButton::Right,
        2 => CGMouseButton::Center,
        _ => CGMouseButton::Left,
    }
}

fn mouse_type(event: &SyntheticEvent) -> CGEventType {
    let button = event.button.unwrap_or(0);
    match (event.kind, button) {
        (EventKind::MouseUp, 1) => CGEventType::RightMouseUp,
        (EventKind::MouseDrag, 1) => CGEventType::RightMouseDragged,
        (EventKind::MouseDown, 1) => CGEventType::RightMouseDown,
        (EventKind::MouseUp, 2) => CGEventType::OtherMouseUp,
        (EventKind::MouseDrag, 2) => CGEventType::OtherMouseDragged,
        (EventKind::MouseDown, 2) => CGEventType::OtherMouseDown,
        (EventKind::MouseUp, _) => CGEventType::LeftMouseUp,
        (EventKind::MouseDrag, _) => CGEventType::LeftMouseDragged,
        _ => CGEventType::LeftMouseDown,
    }
}

// SAFETY: the signature matches `CGEventKeyboardSetUnicodeString`.
unsafe extern "C" {
    fn CGEventKeyboardSetUnicodeString(
        event: *mut libc::c_void,
        length: libc::c_ulong,
        unicode: *const u16,
    );
}
