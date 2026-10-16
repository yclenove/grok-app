//! CoreGraphics posting. Compiled only on macOS. SkyLight is not called.
//!
//! `NSApplication` is created only on the main thread. A worker thread posts
//! only after that flag is set, so `CGS_REQUIRE_INIT` is not hit from here.
//! Field 51 is written and read back before `postToPid`. A mismatch is not
//! posted. `CGEventSetWindowLocation` comes from `dlsym`.

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use async_trait::async_trait;
use gcu_core::driver::{DriverError, Result};
use objc2::MainThreadMarker;
use objc2_app_kit::{
    NSApplication, NSApplicationActivationOptions, NSApplicationActivationPolicy,
    NSRunningApplication, NSWorkspace,
};
use objc2_core_foundation::{CFRetained, CGPoint};
use objc2_core_graphics::{
    CGEvent, CGEventField, CGEventFlags, CGEventTapLocation, CGEventType, CGMouseButton,
    CGScrollEventUnit,
};
use objc2_foundation::NSProcessInfo;

use super::events::{EventApi, EventKind, PostFault, PostPath, Probe, SyntheticEvent};
use super::skylight;
use super::{
    activity::{ActivityMonitor, OWN_EVENT_TAG},
    hotkey::{Guards, HotkeyConfig, SharedStopLatch, StopLatch},
};

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

/// Create `NSApplication` and mark this process as an accessory (UI-element) app.
///
/// Call on the main thread before starting Tokio. [`ensure_appkit`] is unchanged.
/// Returns false off the main thread or when AppKit refuses the accessory policy.
/// Does not touch the network and does not grant TCC by itself. Blocking: one
/// synchronous AppKit call on the caller thread. Timeout: none. Cancellation: none.
pub fn ensure_agent_appkit() -> bool {
    if !ensure_appkit() {
        return false;
    }
    let Some(marker) = MainThreadMarker::new() else {
        return false;
    };
    NSApplication::sharedApplication(marker)
        .setActivationPolicy(NSApplicationActivationPolicy::Accessory)
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
pub struct RealEvents {
    arrival_failed: AtomicBool,
    guards: Arc<Guards>,
    _listener: Option<super::ffi::Listener>,
    post_gate: Arc<tokio::sync::Semaphore>,
}

impl RealEvents {
    /// Backend initially stopped until a trusted Host is attached. Starting a
    /// new engine must never implicitly reset the previous engine's Stop.
    pub fn new() -> Self {
        Self::with_stop_latch(
            Arc::new(SharedStopLatch::new(true)),
            HotkeyConfig::default(),
            3000,
        )
    }

    /// Attach a persistent Host latch and configurable Stop/idle settings.
    /// AppKit must already be initialized on the main thread. Listener startup
    /// is asynchronous; foreground stays unavailable until it becomes healthy.
    pub fn with_stop_latch(latch: Arc<dyn StopLatch>, config: HotkeyConfig, idle_ms: u64) -> Self {
        let clock = super::clock::runtime_clock();
        let monitor = Arc::new(ActivityMonitor::new(clock.clone()));
        let guards = Arc::new(Guards::new(clock, monitor.clone(), latch, idle_ms));
        let listener = if ensure_appkit() {
            match super::ffi::Listener::start(monitor, guards.clone(), config) {
                Ok(listener) => Some(listener),
                Err(_) => {
                    tracing::warn!("macos Stop listener could not start");
                    None
                }
            }
        } else {
            None
        };
        Self {
            arrival_failed: AtomicBool::new(false),
            guards,
            _listener: listener,
            post_gate: Arc::new(tokio::sync::Semaphore::new(1)),
        }
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
            accessibility: super::ffi::accessibility_trusted(),
            secure_prompt: false,
            foreground: true,
            skylight: skylight::status(),
        }
    }
}

#[async_trait]
impl EventApi for RealEvents {
    fn guards(&self) -> Option<Arc<Guards>> {
        Some(self.guards.clone())
    }

    async fn frontmost_pid(&self) -> Result<Option<i32>> {
        tokio::task::spawn_blocking(frontmost_pid)
            .await
            .map_err(|_| DriverError::Crashed {
                maybe_executed: false,
            })
    }
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
        let guards = self.guards.clone();
        tokio::task::spawn_blocking(move || {
            guards.check(true)?;
            if !ensure_appkit() {
                return Err(DriverError::CapabilityUnavailable);
            }
            let app = NSRunningApplication::runningApplicationWithProcessIdentifier(pid)
                .ok_or(DriverError::WindowGone)?;
            if !app.activateWithOptions(NSApplicationActivationOptions::ActivateAllWindows) {
                return Err(DriverError::Native {
                    message: "foreground activation was not accepted".into(),
                    maybe_executed: true,
                });
            }
            Ok(())
        })
        .await
        .map_err(|_| DriverError::Crashed {
            maybe_executed: true,
        })?
    }

    async fn post(&self, event: SyntheticEvent) -> std::result::Result<(), PostFault> {
        if !ensure_appkit() {
            return Err(PostFault::NotSent);
        }
        let guards = self.guards.clone();
        // A stalled down keeps this permit; no new downs are queued behind it.
        // Ups use the independent release executor and must bypass the permit.
        let permit = if event.down {
            Some(
                self.post_gate
                    .clone()
                    .try_acquire_owned()
                    .map_err(|_| PostFault::NotSent)?,
            )
        } else {
            None
        };
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            if event.down {
                guards
                    .check(event.path == PostPath::HidTap)
                    .map_err(|_| PostFault::NotSent)?;
                if event.path == PostPath::HidTap && frontmost_pid() != Some(event.pid) {
                    return Err(PostFault::NotSent);
                }
                if !super::ffi::accessibility_trusted() {
                    return Err(PostFault::Permission);
                }
            }
            post_one(&event)
        })
        .await
        .map_err(|_| PostFault::Uncertain)?
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

impl Default for RealEvents {
    fn default() -> Self {
        Self::new()
    }
}

fn frontmost_pid() -> Option<i32> {
    NSWorkspace::sharedWorkspace()
        .frontmostApplication()
        .map(|app| app.processIdentifier())
}

fn post_one(event: &SyntheticEvent) -> std::result::Result<(), PostFault> {
    let cg = build(event).ok_or(PostFault::NotSent)?;
    CGEvent::set_integer_value_field(Some(&cg), CGEventField::EventSourceUserData, OWN_EVENT_TAG);
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
        EventKind::Scroll => build_scroll(event, point),
        EventKind::MouseMove => {
            let mouse = CGEvent::new_mouse_event(
                None,
                CGEventType::MouseMoved,
                point,
                CGMouseButton::Left,
            )?;
            Some(mouse)
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

fn build_scroll(event: &SyntheticEvent, point: CGPoint) -> Option<CFRetained<CGEvent>> {
    let (dx, dy) = event.scroll?;
    let (line_x, line_y) = super::events::wheel_axes(dx, dy);
    // Line units with phase Changed. Pixel + a continuous flag often yields a
    // zero precise delta, and phase Ended clears egui's wheel delta.
    let wheel =
        CGEvent::new_scroll_wheel_event2(None, CGScrollEventUnit::Line, 2, line_y, line_x, 0)?;
    CGEvent::set_integer_value_field(Some(&wheel), CGEventField::ScrollWheelEventIsContinuous, 0);
    CGEvent::set_integer_value_field(Some(&wheel), CGEventField::ScrollWheelEventScrollPhase, 4);
    CGEvent::set_integer_value_field(Some(&wheel), CGEventField::ScrollWheelEventMomentumPhase, 0);
    CGEvent::set_integer_value_field(
        Some(&wheel),
        CGEventField::ScrollWheelEventPointDeltaAxis1,
        i64::from(dy),
    );
    CGEvent::set_integer_value_field(
        Some(&wheel),
        CGEventField::ScrollWheelEventPointDeltaAxis2,
        i64::from(dx),
    );
    CGEvent::set_location(Some(&wheel), point);
    Some(wheel)
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
