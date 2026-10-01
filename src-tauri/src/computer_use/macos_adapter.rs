//! macOS desktop adapter: CGWindowList + Screen Recording / Accessibility probes.
//! Native fixture proof on Apple Silicon / Intel remains not_run until those hosts run.

use grok_computer_use_core::native_action::NativeActionSlot;
use grok_computer_use_core::quartz_frame::{
    CapturedFrame, ModelObservation, ObservationRegistry, WindowBounds, WindowInstance, WindowKey,
};
use std::ffi::{c_void, CStr, CString};
use std::sync::Arc;

#[path = "macos_adapter/ax_api.rs"]
mod ax_api;
#[path = "macos_adapter/ax_tree.rs"]
mod ax_tree;
#[path = "macos_adapter/ax_window.rs"]
mod ax_window;
#[path = "macos_adapter/capture.rs"]
mod capture;
#[path = "macos_adapter/coordinate.rs"]
mod coordinate;
#[path = "macos_adapter/identity.rs"]
mod identity;
#[path = "macos_adapter/keyboard.rs"]
mod keyboard;
#[path = "macos_adapter/pointer.rs"]
mod pointer;
#[path = "macos_adapter/text.rs"]
mod text;

use super::adapter::{
    AdapterActResult, Capabilities, CaptureOptions, ComputerUseAdapter, DispatchRequest, TargetInfo,
};
use super::protocol::{ActionKind, ActionTarget, Observation, ObservationImage, PROTOCOL_VERSION};

type CfTypeRef = *const c_void;
type CgEventRef = *mut c_void;

const K_CG_WINDOW_LIST_OPTION_ON_SCREEN_ONLY: u32 = 1;
const K_CG_WINDOW_LIST_EXCLUDE_DESKTOP: u32 = 1 << 4;
const K_CG_NULL_WINDOW_ID: u32 = 0;
const K_CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;
const K_CF_NUMBER_SINT32: i32 = 3;
const K_CF_NUMBER_DOUBLE: i32 = 13;
const K_CG_EVENT_LEFT_MOUSE_DOWN: u32 = 1;
const K_CG_EVENT_LEFT_MOUSE_UP: u32 = 2;
const K_CG_MOUSE_BUTTON_LEFT: u32 = 0;
const K_CG_EVENT_SOURCE_STATE_HID: u32 = 1;

#[repr(C)]
#[derive(Clone, Copy)]
struct CgPoint {
    x: f64,
    y: f64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct CgSize {
    width: f64,
    height: f64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct CgRect {
    origin: CgPoint,
    size: CgSize,
}

#[cfg_attr(target_os = "macos", link(name = "CoreGraphics", kind = "framework"))]
extern "C" {
    static CGRectNull: CgRect;
    fn CGWindowListCopyWindowInfo(option: u32, relative: u32) -> CfTypeRef;
    fn CGWindowListCreateImage(
        bounds: CgRect,
        list_option: u32,
        window_id: u32,
        image_option: u32,
    ) -> CfTypeRef;
    fn CGPreflightScreenCaptureAccess() -> bool;
    fn CGEventSourceCreate(state_id: u32) -> CfTypeRef;
    fn CGEventCreateMouseEvent(
        source: CfTypeRef,
        mouse_type: u32,
        pos: CgPoint,
        button: u32,
    ) -> CgEventRef;
    fn CGEventPostToPid(pid: i32, event: CgEventRef);
    fn CGEventSetIntegerValueField(event: CgEventRef, field: u32, value: i64);
    fn CGImageGetWidth(image: CfTypeRef) -> usize;
    fn CGImageGetHeight(image: CfTypeRef) -> usize;
    fn CGGetActiveDisplayList(max: u32, displays: *mut u32, count: *mut u32) -> i32;
    fn CGDisplayBounds(display: u32) -> CgRect;
    fn CGDisplayCopyDisplayMode(display: u32) -> CfTypeRef;
    fn CGDisplayModeGetPixelWidth(mode: CfTypeRef) -> usize;
    fn CGDisplayModeGetPixelHeight(mode: CfTypeRef) -> usize;
    fn CGDisplayRotation(display: u32) -> f64;
}

#[cfg_attr(target_os = "macos", link(name = "CoreFoundation", kind = "framework"))]
extern "C" {
    fn CFRelease(cf: CfTypeRef);
    fn CFArrayGetCount(arr: CfTypeRef) -> isize;
    fn CFArrayGetValueAtIndex(arr: CfTypeRef, idx: isize) -> CfTypeRef;
    fn CFDictionaryGetValue(dict: CfTypeRef, key: CfTypeRef) -> CfTypeRef;
    fn CFStringCreateWithCString(alloc: CfTypeRef, c_str: *const i8, encoding: u32) -> CfTypeRef;
    fn CFStringGetCString(
        the_string: CfTypeRef,
        buffer: *mut i8,
        buffer_size: isize,
        encoding: u32,
    ) -> bool;
    fn CFNumberGetValue(number: CfTypeRef, the_type: i32, value_ptr: *mut c_void) -> bool;
    fn CFDataGetLength(data: CfTypeRef) -> isize;
    fn CFDataGetBytePtr(data: CfTypeRef) -> *const u8;
    fn CFDataCreateMutable(alloc: CfTypeRef, capacity: isize) -> *mut c_void;
}

#[cfg_attr(target_os = "macos", link(name = "ImageIO", kind = "framework"))]
extern "C" {
    fn CGImageDestinationCreateWithData(
        data: *mut c_void,
        image_type: CfTypeRef,
        count: usize,
        options: CfTypeRef,
    ) -> CfTypeRef;
    fn CGImageDestinationAddImage(destination: CfTypeRef, image: CfTypeRef, properties: CfTypeRef);
    fn CGImageDestinationFinalize(destination: CfTypeRef) -> bool;
}

#[cfg_attr(
    target_os = "macos",
    link(name = "ApplicationServices", kind = "framework")
)]
extern "C" {
    fn AXIsProcessTrusted() -> bool;
}

/// Only Create/Copy results go here. Borrowed dictionary members remain owned
/// by their enclosing array. Errors release every successfully created object.
struct OwnedCf(CfTypeRef);

impl OwnedCf {
    fn new(raw: CfTypeRef, failure: &str) -> Result<Self, String> {
        if raw.is_null() {
            Err(failure.into())
        } else {
            Ok(Self(raw))
        }
    }
}

impl Drop for OwnedCf {
    fn drop(&mut self) {
        unsafe { CFRelease(self.0) };
    }
}

fn cf_str(key: &str) -> CfTypeRef {
    let c = CString::new(key).unwrap_or_else(|_| CString::new("x").unwrap());
    unsafe { CFStringCreateWithCString(std::ptr::null(), c.as_ptr(), K_CF_STRING_ENCODING_UTF8) }
}

fn cf_string_value(item: CfTypeRef) -> String {
    if item.is_null() {
        return String::new();
    }
    let mut buf = [0i8; 1024];
    unsafe {
        if CFStringGetCString(
            item,
            buf.as_mut_ptr(),
            buf.len() as isize,
            K_CF_STRING_ENCODING_UTF8,
        ) {
            CStr::from_ptr(buf.as_ptr()).to_string_lossy().into_owned()
        } else {
            String::new()
        }
    }
}

fn cf_i32(item: CfTypeRef) -> i32 {
    if item.is_null() {
        return 0;
    }
    let mut v = 0i32;
    unsafe {
        let _ = CFNumberGetValue(item, K_CF_NUMBER_SINT32, &mut v as *mut i32 as *mut c_void);
    }
    v
}

fn dict_str(dict: CfTypeRef, key: &str) -> String {
    if dict.is_null() {
        return String::new();
    }
    let k = cf_str(key);
    if k.is_null() {
        return String::new();
    }
    let v = unsafe { CFDictionaryGetValue(dict, k) };
    if !k.is_null() {
        unsafe { CFRelease(k) };
    }
    cf_string_value(v)
}

fn dict_i32(dict: CfTypeRef, key: &str) -> i32 {
    if dict.is_null() {
        return 0;
    }
    let k = cf_str(key);
    if k.is_null() {
        return 0;
    }
    let v = unsafe { CFDictionaryGetValue(dict, k) };
    if !k.is_null() {
        unsafe { CFRelease(k) };
    }
    cf_i32(v)
}

fn dict_rect(dict: CfTypeRef) -> Option<(f64, f64, f64, f64)> {
    if dict.is_null() {
        return None;
    }
    let k = cf_str("kCGWindowBounds");
    if k.is_null() {
        return None;
    }
    let bounds = unsafe { CFDictionaryGetValue(dict, k) };
    if !k.is_null() {
        unsafe { CFRelease(k) };
    }
    if bounds.is_null() {
        return None;
    }
    let num = |name: &str| -> Option<f64> {
        let nk = cf_str(name);
        if nk.is_null() {
            return None;
        }
        let nv = unsafe { CFDictionaryGetValue(bounds, nk) };
        if !nk.is_null() {
            unsafe { CFRelease(nk) };
        }
        let mut d = 0f64;
        if nv.is_null()
            || !unsafe {
                CFNumberGetValue(nv, K_CF_NUMBER_DOUBLE, &mut d as *mut f64 as *mut c_void)
            }
        {
            return None;
        }
        Some(d)
    };
    Some((num("X")?, num("Y")?, num("Width")?, num("Height")?))
}

fn selectable_window_metadata(dict: CfTypeRef) -> Option<(String, String)> {
    if dict.is_null() || dict_i32(dict, "kCGWindowLayer") != 0 {
        return None;
    }
    let title = dict_str(dict, "kCGWindowName");
    let owner = dict_str(dict, "kCGWindowOwnerName");
    if title.is_empty() || owner.eq_ignore_ascii_case("Window Server") {
        return None;
    }
    Some((title, owner))
}

pub struct MacosAdapter {
    input: NativeActionSlot,
    windows: ax_window::WindowBindings,
    snapshots: ObservationRegistry<ax_tree::Tree>,
}

impl MacosAdapter {
    pub fn new() -> Self {
        Self {
            input: NativeActionSlot::default(),
            windows: ax_window::WindowBindings::default(),
            snapshots: ObservationRegistry::default(),
        }
    }

    fn screen_ok() -> bool {
        unsafe { CGPreflightScreenCaptureAccess() }
    }

    fn ax_ok() -> bool {
        unsafe { AXIsProcessTrusted() }
    }

    fn bound_window(
        &self,
        target_id: &str,
        cancellation: &grok_computer_use_core::execution::ActionCancellation,
    ) -> Result<WindowBounds, String> {
        cancellation.check()?;
        let target = WindowInstance::parse(target_id)?;
        let bounds = window_bounds(target.window).inspect_err(|_| {
            self.windows.retire(target);
            self.snapshots.retire_window(target_id);
        })?;
        self.windows
            .validate(target, bounds, cancellation)
            .inspect_err(|_| self.snapshots.retire_window(target_id))?;
        cancellation.check()?;
        Ok(bounds)
    }
}

impl Default for MacosAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl ComputerUseAdapter for MacosAdapter {
    fn backend_id(&self) -> &'static str {
        "macos"
    }

    fn capabilities(&self) -> Capabilities {
        let screen = Self::screen_ok();
        let ax = Self::ax_ok();
        let mut caps = Capabilities::for_surface(super::adapter::SurfaceKind::Desktop, "macos");
        caps.observe_screenshot = screen && ax;
        // Advertise only implemented paths: bounded selected-window AX trees,
        // retained-reference AXPress/AXValue, focused named keys, and frame-bound pointers.
        caps.observe_ax = screen && ax;
        caps.semantic_click = screen && ax;
        caps.coordinate_click = screen && ax;
        caps.unicode_text = screen && ax;
        caps.chinese_ime = false;
        caps.click = if screen && ax {
            super::adapter::ActionSupport::BOTH
        } else {
            super::adapter::ActionSupport::NONE
        };
        caps.set_value = if screen && ax {
            super::adapter::ActionSupport::SEMANTIC
        } else {
            super::adapter::ActionSupport::NONE
        };
        caps.type_text = caps.click;
        caps.key = caps.click;
        caps.scroll = if screen && ax {
            super::adapter::ActionSupport::COORDINATE
        } else {
            super::adapter::ActionSupport::NONE
        };
        caps.drag = caps.scroll;
        caps.wait = caps.set_value;
        caps.notes = vec![
            format!(
                "Screen Recording {} ; Accessibility {}",
                if screen { "granted" } else { "missing" },
                if ax { "granted" } else { "missing" }
            ),
            "Selected-window Quartz capture, bounded AX tree, exact-reference AXPress/AXValue/read-only name wait, focused writable-selection text append and named keys via exact refs or screenshot AX hit binding; screenshot-bound clicks/pixel scroll/drag; clipboard and IME remain incomplete".into(),
            "Screen capture and input require an exposed AX window-instance witness; no PID/window-number-only fallback".into(),
            "Native Apple Silicon / Intel fixture proof is recorded separately (not_run here if unsigned)".into(),
        ];
        caps
    }

    fn list_targets(&self) -> Result<Vec<TargetInfo>, String> {
        let arr = OwnedCf::new(
            unsafe {
                CGWindowListCopyWindowInfo(
                    K_CG_WINDOW_LIST_OPTION_ON_SCREEN_ONLY | K_CG_WINDOW_LIST_EXCLUDE_DESKTOP,
                    K_CG_NULL_WINDOW_ID,
                )
            },
            "CGWindowListCopyWindowInfo failed",
        )?;
        let n = unsafe { CFArrayGetCount(arr.0) };
        let mut out = Vec::new();
        let mut visible = Vec::new();
        for i in 0..n {
            let dict = unsafe { CFArrayGetValueAtIndex(arr.0, i) };
            let Some((title, owner)) = selectable_window_metadata(dict) else {
                continue;
            };
            let pid = dict_i32(dict, "kCGWindowOwnerPID") as u32;
            let wid = dict_i32(dict, "kCGWindowNumber") as u32;
            let Ok(key) = identity::discover(pid, wid) else {
                continue;
            };
            // Discovery's original CG list can be stale. Re-read the live
            // window between two exact birth checks before offering authority.
            visible.push(key);
            let Ok(bounds) = window_bounds(key) else {
                continue;
            };
            let Ok(instance) = self.windows.discover(key, bounds) else {
                continue;
            };
            let target_id = instance.target_id();
            out.push(TargetInfo {
                target_id,
                title: title.clone(),
                app_name: owner,
                kind: "window".into(),
                pid: Some(pid),
                backend: "macos".into(),
                execution_mode: "exclusive".into(),
                replay_policy: "never".into(),
                lifecycle_stamp: instance.lifecycle_stamp(),
                display_id: "quartz".into(),
                coordinate_space: "image-pixels".into(),
                scope_label: format!("macOS window “{title}”"),
            });
        }
        self.windows.retain_visible(&visible);
        Ok(out)
    }

    fn target_alive(&self, target_id: &str) -> bool {
        self.bound_window(target_id, &Default::default()).is_ok()
    }

    fn observe(&self, target_id: &str) -> Result<Observation, String> {
        // Unscoped reads are previews: they never mint model input authority.
        self.capture_for_run(
            "",
            target_id,
            CaptureOptions {
                for_model: false,
                ..CaptureOptions::model(true)
            },
        )
    }

    fn observe_for_run(&self, run_id: &str, target_id: &str) -> Result<Observation, String> {
        self.capture_for_run(run_id, target_id, CaptureOptions::model(true))
    }

    fn capture_for_run(
        &self,
        run_id: &str,
        target_id: &str,
        options: CaptureOptions,
    ) -> Result<Observation, String> {
        options.cancellation.check()?;
        let ticket = options
            .for_model
            .then(|| self.snapshots.begin(run_id, target_id));
        let (mut observation, frame) = capture::capture(self, target_id, &options.cancellation)?;
        let tree = ax_tree::collect(self, target_id, frame.clone(), &options.cancellation)?;
        if self.bound_window(target_id, &options.cancellation)? != frame.bounds
            || capture::display_revision()? != frame.display_revision
        {
            return Err("macOS target changed during AX observation".into());
        }
        observation.nodes = tree.display.clone();
        observation.truncated = tree.truncated;
        options.cancellation.check()?;
        observation.run_id = run_id.into();
        if !options.screenshot {
            observation.image.png_base64 = None;
        }
        if let Some(ticket) = ticket {
            self.snapshots
                .publish(ticket, frame, options.screenshot, tree)?;
        }
        Ok(observation)
    }

    fn act(&self, req: &DispatchRequest) -> Result<AdapterActResult, String> {
        if !self.capabilities().allows(req.action, &req.target)
            || !matches!(
                req.action,
                ActionKind::Click
                    | ActionKind::SetValue
                    | ActionKind::TypeText
                    | ActionKind::Key
                    | ActionKind::Scroll
                    | ActionKind::Drag
                    | ActionKind::Wait
            )
        {
            return Err(
                "unsupported macOS action; no semantic-to-coordinate or desktop fallback".into(),
            );
        }
        let mut owner = self
            .input
            .begin(&req.run_id, req.generation, &req.cancellation)?;
        if req.action == ActionKind::Wait {
            return ax_tree::wait(self, req);
        }
        req.admit(self)?;
        if matches!(req.target, ActionTarget::Element { .. })
            || matches!(req.action, ActionKind::Key | ActionKind::TypeText)
        {
            return ax_tree::act(self, req, &mut owner);
        }
        let observation = self.snapshots.get(&req.run_id, &req.target_id)?;
        if !observation.has_screenshot() {
            return Err("a fresh model screenshot is required for macOS coordinate input".into());
        }
        let ActionTarget::Coord { x, y } = req.target else {
            return Err("macOS semantic click requires a native AX element".into());
        };
        let result = match req.action {
            ActionKind::Click => {
                send_click(self, req, &observation, x, y, &mut owner).map(|_| true)
            }
            ActionKind::Scroll => pointer::scroll(self, req, &observation, x, y),
            ActionKind::Drag => pointer::drag(self, req, &observation, x, y, &mut owner),
            _ => Err("unsupported macOS coordinate operation".into()),
        };
        result.map(|applied| AdapterActResult {
            applied,
            outcome: None,
            postcondition_ok: false,
            verifiable: false,
            detail: if applied {
                "macos cgevent queued; target effect unverified"
            } else {
                "zero scroll delta; no native event posted"
            }
            .into(),
        })
    }

    fn wait_uses_retained_reference(&self) -> bool {
        true
    }

    fn abort(&self, run_id: &str, generation: u64) -> Result<(), String> {
        self.input.cancel_before(run_id, generation);
        self.snapshots.retire_run(run_id);
        Ok(())
    }

    fn is_idle(&self, _run_id: &str) -> bool {
        self.input.is_idle()
    }

    fn input_available(&self) -> bool {
        Self::screen_ok() && Self::ax_ok()
    }

    fn current_geometry_revision(&self) -> u64 {
        capture::display_revision().unwrap_or(0)
    }

    fn current_geometry_revision_for(&self, target_id: &str) -> u64 {
        let result = (|| {
            Ok::<_, String>(self.bound_window(target_id, &Default::default())?.revision(
                WindowInstance::parse(target_id)?.window,
                capture::display_revision()?,
            ))
        })();
        result.unwrap_or(0)
    }

    fn release_target_for_run(&self, run_id: &str, target_id: &str) {
        self.snapshots.retire_target(run_id, target_id);
    }

    fn start_periodic_preview(&self, _target_id: &str) {}
    fn stop_periodic_preview(&self) {}
    fn periodic_preview_active(&self) -> bool {
        false
    }
}

fn window_bounds(key: WindowKey) -> Result<WindowBounds, String> {
    identity::validate(key)?;
    let arr = OwnedCf::new(
        unsafe {
            CGWindowListCopyWindowInfo(
                K_CG_WINDOW_LIST_OPTION_ON_SCREEN_ONLY | K_CG_WINDOW_LIST_EXCLUDE_DESKTOP,
                K_CG_NULL_WINDOW_ID,
            )
        },
        "window list failed",
    )?;
    let n = unsafe { CFArrayGetCount(arr.0) };
    let mut found = None;
    for i in 0..n {
        let dict = unsafe { CFArrayGetValueAtIndex(arr.0, i) };
        if dict_i32(dict, "kCGWindowNumber") as u32 == key.wid
            && dict_i32(dict, "kCGWindowOwnerPID") as u32 == key.pid
        {
            if selectable_window_metadata(dict).is_none() {
                return Err("macOS target is no longer a selectable window".into());
            }
            found = dict_rect(dict).and_then(|(x, y, w, h)| WindowBounds::new(x, y, w, h).ok());
            break;
        }
    }
    identity::validate(key)?;
    found.ok_or_else(|| "window bounds missing".into())
}

/// Reject occlusion rather than click through another window. Quartz returns
/// this list front-to-back, including non-normal-layer overlays.
fn point_owned_by(target: WindowKey, x: f64, y: f64) -> Result<bool, String> {
    identity::validate(target)?;
    let windows = OwnedCf::new(
        unsafe {
            CGWindowListCopyWindowInfo(K_CG_WINDOW_LIST_OPTION_ON_SCREEN_ONLY, K_CG_NULL_WINDOW_ID)
        },
        "cannot verify macOS input hit target",
    )?;
    for index in 0..unsafe { CFArrayGetCount(windows.0) } {
        let dict = unsafe { CFArrayGetValueAtIndex(windows.0, index) };
        let Some((ox, oy, w, h)) = dict_rect(dict) else {
            return Err("cannot verify occluding window bounds".into());
        };
        if x >= ox && y >= oy && x < ox + w && y < oy + h {
            identity::validate(target)?;
            return Ok(dict_i32(dict, "kCGWindowNumber") as u32 == target.wid
                && dict_i32(dict, "kCGWindowOwnerPID") as u32 == target.pid);
        }
    }
    identity::validate(target)?;
    Ok(false)
}

fn mapped_point(
    adapter: &MacosAdapter,
    req: &DispatchRequest,
    frame: &CapturedFrame,
    x: f64,
    y: f64,
    cancellation: &grok_computer_use_core::execution::ActionCancellation,
) -> Result<CgPoint, String> {
    let (x, y) = frame.screen_point(
        WindowInstance::parse(&req.target_id)?.window,
        &req.snapshot_id,
        req.geometry_revision,
        adapter.bound_window(&req.target_id, cancellation)?,
        capture::display_revision()?,
        x,
        y,
    )?;
    if !point_owned_by(frame.target, x, y)? {
        return Err("selected macOS window is occluded; input paused".into());
    }
    Ok(CgPoint { x, y })
}

fn send_click(
    adapter: &MacosAdapter,
    req: &DispatchRequest,
    observation: &Arc<ModelObservation<ax_tree::Tree>>,
    x: f64,
    y: f64,
    owner: &mut grok_computer_use_core::native_action::NativeActionGuard<'_>,
) -> Result<(), String> {
    let frame = observation.frame();
    pointer::hardware_clear(None)?;
    let pos = pointer::point(adapter, req, observation, x, y)?;
    let (down_kind, up_kind, button) = match super::protocol::click_button(&req.parameters) {
        "right" => (3, 4, 1),
        "middle" => (25, 26, 2),
        _ => (
            K_CG_EVENT_LEFT_MOUSE_DOWN,
            K_CG_EVENT_LEFT_MOUSE_UP,
            K_CG_MOUSE_BUTTON_LEFT,
        ),
    };
    let src = pointer::source()?;
    let down = OwnedCf::new(
        unsafe { CGEventCreateMouseEvent(src.0, down_kind, pos, button) },
        "mouse-down allocation failed",
    )?;
    let up = OwnedCf::new(
        unsafe { CGEventCreateMouseEvent(src.0, up_kind, pos, button) },
        "mouse-up allocation failed",
    )?;
    pointer::neutral(&down);
    pointer::neutral(&up);
    for count in 1..=super::protocol::click_count(&req.parameters) {
        pointer::point(adapter, req, observation, x, y)?;
        // This may queue events; it does not prove the target handled them.
        // The adapter reports applied/unverified, never a satisfied postcondition.
        unsafe {
            CGEventSetIntegerValueField(down.0 as CgEventRef, 1, i64::from(count));
            CGEventSetIntegerValueField(up.0 as CgEventRef, 1, i64::from(count));
        }
        // Event allocation/configuration can race a move, resize, display
        // change or occlusion even while the retained AX object is identical.
        // Recheck the screenshot mapping, not merely window identity.
        pointer::point(adapter, req, observation, x, y)?;
        // A double-click's first up may still be queued in the combined table.
        // It is our bounded pair; physical user state is never exempted.
        pointer::hardware_clear((count > 1).then_some(button))?;
        unsafe { CGEventPostToPid(frame.target.pid as i32, down.0 as CgEventRef) };
        // Owned release is allowed after cancellation, but never redirected to
        // a replacement/occluded window. An uncertain release retains occupancy.
        pointer::release(adapter, req, observation, (x, y), &up, button, owner)?;
        req.cancellation.check()?;
    }
    Ok(())
}

/// Compile-time runner entry. Returns Err with not_run/permission text; never claims a host it is not.
pub fn run_native_selftest() -> Result<(), String> {
    let adapter = MacosAdapter::new();
    let cap = adapter.capabilities();
    if !cap.observe_screenshot {
        return Err("not_run: Screen Recording or Accessibility permission missing".into());
    }
    let listed = adapter.list_targets()?;
    if listed.is_empty() {
        return Err("not_run: no on-screen windows (unsigned or empty session)".into());
    }
    Ok(())
}
