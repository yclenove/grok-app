//! Test-only CoreFoundation/CoreGraphics/ImageIO ABI doubles, never a backend.
#![allow(non_snake_case, non_upper_case_globals)]

use grok_computer_use_core::execution::ActionCancellation;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::ffi::{c_void, CStr, CString};

pub struct State {
    pub x: f64,
    pub captures: Vec<(u32, u32, u32)>,
    pub posts: Vec<(i32, u32, f64, f64, i64)>,
    pub null_capture_bounds: bool,
    pub encodes: usize,
    pub live_owned: usize,
    pub move_during_capture: bool,
    pub fail_capture: bool,
    pub fail_finalize: bool,
    pub fail_destination: bool,
    pub image_size: (usize, usize),
    pub screen_allowed: bool,
    pub occluded: bool,
    pub occlude_on_down: bool,
    pub cancel_on_down: Option<ActionCancellation>,
    pub birth: (u64, u64),
    pub process_info_bytes: i32,
    pub process_info_pid: u32,
    pub process_status: u32,
    pub rebirth_during_capture: bool,
    pub rebirth_on_down: bool,
    pub rebirth_on_window_read: Option<usize>,
    pub window_reads: usize,
    pub rebirth_on_event_configure: bool,
    pub window_layer: i32,
    pub window_title: String,
    pub window_owner: String,
    pub ax_instance: u64,
    pub ax_owned: usize,
    pub ax_allowed: bool,
    pub ax_wrong_pid: bool,
    pub ax_wrong_type: bool,
    pub ax_wrong_bounds: bool,
    pub ax_fail_timeout: bool,
    pub ax_fail_attribute: bool,
    pub ax_replaced_during_capture: bool,
    pub ax_replaced_on_down: bool,
    pub ax_replaced_on_event_configure: bool,
    pub ax_cancel_on_hit: Option<ActionCancellation>,
    pub move_on_event_configure: bool,
    pub window_present: bool,
    pub ax_tree_enabled: bool,
    pub ax_button_generation: u64,
    pub ax_button_name: String,
    pub ax_name_reads: usize,
    pub ax_on_name_read_at: Option<usize>,
    pub ax_on_name_read: Option<Box<dyn FnOnce()>>,
    pub ax_button_enabled: bool,
    pub ax_button_hidden: bool,
    pub ax_secure_reads: usize,
    pub ax_window_escape: bool,
    pub ax_parent_escape: bool,
    pub ax_cycle: bool,
    pub ax_child_count_override: Option<usize>,
    pub ax_press_status: i32,
    pub ax_presses: Vec<u64>,
    pub ax_cancel_on_press: Option<ActionCancellation>,
    pub ax_cancel_on_children: Option<ActionCancellation>,
    pub ax_replace_on_children: bool,
    pub ax_child_wrong_pid: bool,
    pub ax_subrole_secure: bool,
    pub ax_role_changed: bool,
    pub ax_wrong_child_type: bool,
    pub ax_children_requests: Vec<usize>,
    pub ax_button_size: (f64, f64),
    pub ax_press_available: bool,
    pub ax_move_on_action_names: bool,
    pub ax_chain_depth: usize,
    pub ax_group_hidden: bool,
    pub ax_on_children: Option<Box<dyn FnOnce()>>,
    pub on_event_configure: Option<Box<dyn FnOnce()>>,
    pub on_mouse_down: Option<Box<dyn FnOnce()>>,
    pub ax_text_role: Option<&'static str>,
    pub ax_value_settable: u8,
    pub ax_settable_status: i32,
    pub ax_settable_reads: usize,
    pub ax_on_settable: Option<Box<dyn FnOnce()>>,
    pub ax_value_status: i32,
    pub ax_values: Vec<(u64, String)>,
    pub ax_cancel_on_value: Option<ActionCancellation>,
    pub cf_watch_string: Option<String>,
    pub cf_on_string_create: Option<Box<dyn FnOnce()>>,
    pub cf_fail_string: bool,
    pub event_allocations: usize,
    pub fail_event_number: Option<usize>,
    pub fail_event_source: bool,
    pub wheel_creations: Vec<(u32, u32, i32, i32, i32)>,
    pub wheel_posts: Vec<(i32, f64, f64, i32)>,
    pub on_mouse_drag: Option<Box<dyn FnOnce()>>,
    pub on_wheel_post: Option<Box<dyn FnOnce()>>,
    pub occlusion_rect: Option<(f64, f64, f64, f64)>,
    pub ax_focused: bool,
    pub ax_frontmost: bool,
    pub ax_focus_window_escape: bool,
    pub ax_focus_node: u64,
    pub keyboard_flags: u64,
    pub held_key: Option<u16>,
    pub key_sources: Vec<u32>,
    pub key_posts: Vec<(i32, u16, bool, u64)>,
    pub on_key_configure: Option<Box<dyn FnOnce()>>,
    pub on_key_down: Option<Box<dyn FnOnce()>>,
    pub ax_focus_reads: usize,
    pub ax_on_focus_read_at: Option<usize>,
    pub ax_on_focus_read: Option<Box<dyn FnOnce()>>,
    pub synthetic_flags: u64,
    pub synthetic_key: Option<u16>,
    pub held_button: Option<u32>,
    pub synthetic_button: Option<u32>,
    pub pointer_post_flags: Vec<u64>,
    pub ax_text: String,
    pub ax_selection: (isize, isize),
    pub ax_append_settable: (u8, u8),
    pub ax_append_status: (i32, i32),
    pub ax_append_writes: Vec<String>,
    pub ax_selection_ignored: bool,
    pub ax_count_override: Option<f64>,
    pub ax_count_wrong_type: bool,
    pub ax_range_wrong_type: bool,
    pub ax_text_attributes_missing: bool,
    pub ax_fail_range_create: bool,
    pub ax_on_range_create: Option<Box<dyn FnOnce()>>,
    pub ax_on_range_set: Option<Box<dyn FnOnce()>>,
    pub ax_on_text_set: Option<Box<dyn FnOnce()>>,
    pub ax_on_range_read: Option<Box<dyn FnOnce()>>,
    pub ax_on_count_read: Option<Box<dyn FnOnce()>>,
    pub ax_test_point: (f32, f32),
    pub ax_point_hit_node: u64,
    pub ax_point_hit_status: i32,
    pub ax_point_hit_null: bool,
    pub ax_point_hit_calls: usize,
    pub ax_on_point_hit_at: Option<usize>,
    pub ax_on_point_hit: Option<Box<dyn FnOnce()>>,
    pub ax_hit_points: Vec<(f32, f32)>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            x: -1000.0,
            captures: vec![],
            posts: vec![],
            null_capture_bounds: false,
            encodes: 0,
            live_owned: 0,
            move_during_capture: false,
            fail_capture: false,
            fail_finalize: false,
            fail_destination: false,
            image_size: (1200, 800),
            screen_allowed: true,
            occluded: false,
            occlude_on_down: false,
            cancel_on_down: None,
            birth: (1_700_000_000, 123_456),
            process_info_bytes: 136,
            process_info_pid: 7,
            process_status: 2,
            rebirth_during_capture: false,
            rebirth_on_down: false,
            rebirth_on_window_read: None,
            window_reads: 0,
            rebirth_on_event_configure: false,
            window_layer: 0,
            window_title: "Owned test window".into(),
            window_owner: "Native fixture double".into(),
            ax_instance: 1,
            ax_owned: 0,
            ax_allowed: true,
            ax_wrong_pid: false,
            ax_wrong_type: false,
            ax_wrong_bounds: false,
            ax_fail_timeout: false,
            ax_fail_attribute: false,
            ax_replaced_during_capture: false,
            ax_replaced_on_down: false,
            ax_replaced_on_event_configure: false,
            ax_cancel_on_hit: None,
            move_on_event_configure: false,
            window_present: true,
            ax_tree_enabled: false,
            ax_button_generation: 1,
            ax_button_name: "执行按钮 🚀".into(),
            ax_name_reads: 0,
            ax_on_name_read_at: None,
            ax_on_name_read: None,
            ax_button_enabled: true,
            ax_button_hidden: false,
            ax_secure_reads: 0,
            ax_window_escape: false,
            ax_parent_escape: false,
            ax_cycle: false,
            ax_child_count_override: None,
            ax_press_status: 0,
            ax_presses: vec![],
            ax_cancel_on_press: None,
            ax_cancel_on_children: None,
            ax_replace_on_children: false,
            ax_child_wrong_pid: false,
            ax_subrole_secure: false,
            ax_role_changed: false,
            ax_wrong_child_type: false,
            ax_children_requests: vec![],
            ax_button_size: (100.0, 40.0),
            ax_press_available: true,
            ax_move_on_action_names: false,
            ax_chain_depth: 0,
            ax_group_hidden: false,
            ax_on_children: None,
            on_event_configure: None,
            on_mouse_down: None,
            ax_text_role: None,
            ax_value_settable: 1,
            ax_settable_status: 0,
            ax_settable_reads: 0,
            ax_on_settable: None,
            ax_value_status: 0,
            ax_values: vec![],
            ax_cancel_on_value: None,
            cf_watch_string: None,
            cf_on_string_create: None,
            cf_fail_string: false,
            event_allocations: 0,
            fail_event_number: None,
            fail_event_source: false,
            wheel_creations: vec![],
            wheel_posts: vec![],
            on_mouse_drag: None,
            on_wheel_post: None,
            occlusion_rect: None,
            ax_focused: false,
            ax_frontmost: true,
            ax_focus_window_escape: false,
            ax_focus_node: 1,
            keyboard_flags: 0,
            held_key: None,
            key_sources: vec![],
            key_posts: vec![],
            on_key_configure: None,
            on_key_down: None,
            ax_focus_reads: 0,
            ax_on_focus_read_at: None,
            ax_on_focus_read: None,
            synthetic_flags: 0,
            synthetic_key: None,
            held_button: None,
            synthetic_button: None,
            pointer_post_flags: vec![],
            ax_text: "原有😀text".into(),
            ax_selection: (1, 2),
            ax_append_settable: (1, 1),
            ax_append_status: (0, 0),
            ax_append_writes: vec![],
            ax_selection_ignored: false,
            ax_count_override: None,
            ax_count_wrong_type: false,
            ax_range_wrong_type: false,
            ax_text_attributes_missing: false,
            ax_fail_range_create: false,
            ax_on_range_create: None,
            ax_on_range_set: None,
            ax_on_text_set: None,
            ax_on_range_read: None,
            ax_on_count_read: None,
            ax_test_point: (-970.0, 120.0),
            ax_point_hit_node: 1,
            ax_point_hit_status: 0,
            ax_point_hit_null: false,
            ax_point_hit_calls: 0,
            ax_on_point_hit_at: None,
            ax_on_point_hit: None,
            ax_hit_points: vec![],
        }
    }
}

thread_local! { static STATE: RefCell<State> = RefCell::new(State::default()); }
pub fn reset() {
    STATE.with(|s| *s.borrow_mut() = State::default());
}
pub fn update(f: impl FnOnce(&mut State)) {
    STATE.with(|s| f(&mut s.borrow_mut()));
}
pub fn inspect<R>(f: impl FnOnce(&State) -> R) -> R {
    STATE.with(|s| f(&s.borrow()))
}

// Public Darwin proc_bsdinfo layout, independently encoded rather than sharing
// the production Rust struct. ABI still requires an actual macOS SDK/native run.
#[no_mangle]
unsafe extern "C" fn proc_pidinfo(
    pid: i32,
    flavor: i32,
    arg: u64,
    buffer: *mut c_void,
    size: i32,
) -> i32 {
    if pid != 7 || flavor != 3 || arg != 0 || buffer.is_null() || size != 136 {
        return 0;
    }
    let (birth, bytes, actual_pid, status) = inspect(|s| {
        (
            s.birth,
            s.process_info_bytes,
            s.process_info_pid,
            s.process_status,
        )
    });
    let mut encoded = [0u8; 136];
    encoded[4..8].copy_from_slice(&status.to_ne_bytes());
    encoded[12..16].copy_from_slice(&actual_pid.to_ne_bytes());
    encoded[120..128].copy_from_slice(&birth.0.to_ne_bytes());
    encoded[128..136].copy_from_slice(&birth.1.to_ne_bytes());
    if bytes > 0 {
        std::ptr::copy_nonoverlapping(encoded.as_ptr(), buffer.cast(), (bytes as usize).min(136));
    }
    bytes
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Point {
    x: f64,
    y: f64,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Size {
    width: f64,
    height: f64,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct TextRange {
    location: isize,
    length: isize,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Rect {
    origin: Point,
    size: Size,
}
#[no_mangle]
pub static CGRectNull: Rect = Rect {
    origin: Point {
        x: f64::INFINITY,
        y: f64::INFINITY,
    },
    size: Size {
        width: 0.0,
        height: 0.0,
    },
};

#[derive(Clone)]
enum Object {
    Array(Vec<Object>),
    Dict(BTreeMap<String, Object>),
    String(CString),
    Number(f64),
    Index(isize),
    Image(usize, usize),
    Data(Vec<u8>),
    Destination(*mut Object),
    DisplayMode,
    Source,
    Event {
        kind: u32,
        pos: Point,
        count: i64,
        flags: u64,
    },
    Wheel {
        pos: Point,
        delta: i32,
        flags: u64,
    },
    Key {
        code: u16,
        down: bool,
        flags: u64,
    },
    Ax {
        instance: u64,
        role: &'static str,
        node: u64,
        generation: u64,
    },
    Boolean(bool),
    Point(Point),
    Size(Size),
    TextRange(TextRange),
}
fn owned(object: Object) -> *const c_void {
    update(|s| {
        if matches!(object, Object::Ax { .. }) {
            s.ax_owned += 1;
        } else {
            s.live_owned += 1;
        }
    });
    Box::into_raw(Box::new(object)).cast()
}
unsafe fn object<'a>(ptr: *const c_void) -> &'a Object {
    &*ptr.cast::<Object>()
}
unsafe fn object_mut<'a>(ptr: *mut c_void) -> &'a mut Object {
    &mut *ptr.cast::<Object>()
}
fn string(value: &str) -> Object {
    Object::String(CString::new(value).unwrap())
}
fn window(pid: u32, wid: u32, x: f64) -> Object {
    let (layer, title, owner) = inspect(|s| {
        (
            s.window_layer,
            s.window_title.clone(),
            s.window_owner.clone(),
        )
    });
    Object::Dict(BTreeMap::from([
        ("kCGWindowNumber".into(), Object::Number(wid as f64)),
        ("kCGWindowOwnerPID".into(), Object::Number(pid as f64)),
        ("kCGWindowLayer".into(), Object::Number(layer as f64)),
        ("kCGWindowName".into(), string(&title)),
        ("kCGWindowOwnerName".into(), string(&owner)),
        (
            "kCGWindowBounds".into(),
            Object::Dict(BTreeMap::from([
                ("X".into(), Object::Number(x)),
                ("Y".into(), Object::Number(80.0)),
                ("Width".into(), Object::Number(600.0)),
                ("Height".into(), Object::Number(400.0)),
            ])),
        ),
    ]))
}

#[no_mangle]
unsafe extern "C" fn CFRelease(ptr: *const c_void) {
    let ax = matches!(object(ptr), Object::Ax { .. });
    drop(Box::from_raw(ptr as *mut Object));
    update(|s| {
        if ax {
            s.ax_owned -= 1;
        } else {
            s.live_owned -= 1;
        }
    });
}
#[no_mangle]
unsafe extern "C" fn CFArrayGetCount(ptr: *const c_void) -> isize {
    match object(ptr) {
        Object::Array(a) => a.len() as isize,
        _ => 0,
    }
}
#[no_mangle]
unsafe extern "C" fn CFArrayGetValueAtIndex(ptr: *const c_void, index: isize) -> *const c_void {
    match object(ptr) {
        Object::Array(a) => a
            .get(index as usize)
            .map(|v| v as *const Object as *const c_void)
            .unwrap_or(std::ptr::null()),
        _ => std::ptr::null(),
    }
}
#[no_mangle]
unsafe extern "C" fn CFDictionaryGetValue(ptr: *const c_void, key: *const c_void) -> *const c_void {
    match (object(ptr), object(key)) {
        (Object::Dict(d), Object::String(k)) => d
            .get(k.to_str().unwrap())
            .map(|v| v as *const Object as *const c_void)
            .unwrap_or(std::ptr::null()),
        _ => std::ptr::null(),
    }
}
#[no_mangle]
unsafe extern "C" fn CFStringCreateWithCString(
    _: *const c_void,
    s: *const i8,
    _: u32,
) -> *const c_void {
    if inspect(|state| {
        state.cf_watch_string.as_deref().map(str::as_bytes) == Some(CStr::from_ptr(s).to_bytes())
    }) {
        let callback = STATE.with(|state| state.borrow_mut().cf_on_string_create.take());
        if let Some(callback) = callback {
            callback();
        }
        if inspect(|state| state.cf_fail_string) {
            return std::ptr::null();
        }
    }
    owned(Object::String(CStr::from_ptr(s).into()))
}
#[no_mangle]
unsafe extern "C" fn CFStringGetCString(
    ptr: *const c_void,
    buffer: *mut i8,
    capacity: isize,
    _: u32,
) -> bool {
    let Object::String(s) = object(ptr) else {
        return false;
    };
    let bytes = s.as_bytes_with_nul();
    if capacity < bytes.len() as isize {
        return false;
    }
    std::ptr::copy_nonoverlapping(bytes.as_ptr(), buffer.cast(), bytes.len());
    true
}
#[no_mangle]
unsafe extern "C" fn CFNumberGetValue(ptr: *const c_void, kind: i32, out: *mut c_void) -> bool {
    if let Object::Index(n) = object(ptr) {
        if kind != 14 {
            return false;
        }
        *out.cast::<isize>() = *n;
        return true;
    }
    let Object::Number(n) = object(ptr) else {
        return false;
    };
    match kind {
        3 => *out.cast::<i32>() = *n as i32,
        13 => *out.cast::<f64>() = *n,
        14 if n.is_finite()
            && n.fract() == 0.0
            && *n >= isize::MIN as f64
            && *n < isize::MAX as f64 =>
        {
            *out.cast::<isize>() = *n as isize
        }
        _ => return false,
    }
    true
}
#[no_mangle]
unsafe extern "C" fn CFDataCreateMutable(_: *const c_void, _: isize) -> *mut c_void {
    owned(Object::Data(vec![])) as *mut c_void
}
#[no_mangle]
unsafe extern "C" fn CFDataGetLength(ptr: *const c_void) -> isize {
    match object(ptr) {
        Object::Data(d) => d.len() as isize,
        _ => -1,
    }
}
#[no_mangle]
unsafe extern "C" fn CFDataGetBytePtr(ptr: *const c_void) -> *const u8 {
    match object(ptr) {
        Object::Data(d) => d.as_ptr(),
        _ => std::ptr::null(),
    }
}

#[no_mangle]
unsafe extern "C" fn CGWindowListCopyWindowInfo(_: u32, _: u32) -> *const c_void {
    let windows = inspect(|s| {
        let mut windows = if s.window_present {
            vec![window(7, 11, s.x)]
        } else {
            vec![]
        };
        if s.occluded {
            windows.insert(0, window(99, 77, s.x));
        }
        if let Some((x, y, width, height)) = s.occlusion_rect {
            let mut overlay = window(99, 77, x);
            if let Object::Dict(fields) = &mut overlay {
                fields.insert(
                    "kCGWindowBounds".into(),
                    Object::Dict(BTreeMap::from([
                        ("X".into(), Object::Number(x)),
                        ("Y".into(), Object::Number(y)),
                        ("Width".into(), Object::Number(width)),
                        ("Height".into(), Object::Number(height)),
                    ])),
                );
            }
            windows.insert(0, overlay);
        }
        windows
    });
    update(|s| {
        s.window_reads += 1;
        if s.rebirth_on_window_read == Some(s.window_reads) {
            s.birth.1 += 1;
        }
    });
    owned(Object::Array(windows))
}
#[no_mangle]
unsafe extern "C" fn CGWindowListCreateImage(
    bounds: Rect,
    list: u32,
    wid: u32,
    options: u32,
) -> *const c_void {
    update(|s| {
        s.captures.push((list, wid, options));
        s.null_capture_bounds = bounds.origin.x == f64::INFINITY
            && bounds.origin.y == f64::INFINITY
            && bounds.size.width == 0.0
            && bounds.size.height == 0.0;
        if s.move_during_capture {
            s.x += 10.0;
        }
        if s.rebirth_during_capture {
            s.birth.1 += 1;
        }
        if s.ax_replaced_during_capture {
            s.ax_instance += 1;
        }
    });
    if inspect(|s| s.fail_capture) {
        return std::ptr::null();
    }
    let (w, h) = inspect(|s| s.image_size);
    owned(Object::Image(w, h))
}
#[no_mangle]
unsafe extern "C" fn CGPreflightScreenCaptureAccess() -> bool {
    inspect(|s| s.screen_allowed)
}
#[no_mangle]
unsafe extern "C" fn AXIsProcessTrusted() -> bool {
    inspect(|s| s.ax_allowed)
}
#[no_mangle]
unsafe extern "C" fn CGImageGetWidth(ptr: *const c_void) -> usize {
    match object(ptr) {
        Object::Image(w, _) => *w,
        _ => 0,
    }
}
#[no_mangle]
unsafe extern "C" fn CGImageGetHeight(ptr: *const c_void) -> usize {
    match object(ptr) {
        Object::Image(_, h) => *h,
        _ => 0,
    }
}
#[no_mangle]
unsafe extern "C" fn CGImageDestinationCreateWithData(
    data: *mut c_void,
    kind: *const c_void,
    count: usize,
    _: *const c_void,
) -> *const c_void {
    if inspect(|s| s.fail_destination) {
        return std::ptr::null();
    }
    let Object::String(kind) = object(kind) else {
        return std::ptr::null();
    };
    if kind.to_bytes() != b"public.png" || count != 1 {
        return std::ptr::null();
    }
    owned(Object::Destination(data.cast()))
}
#[no_mangle]
unsafe extern "C" fn CGImageDestinationAddImage(
    _: *const c_void,
    _: *const c_void,
    _: *const c_void,
) {
    update(|s| s.encodes += 1);
}
#[no_mangle]
unsafe extern "C" fn CGImageDestinationFinalize(ptr: *const c_void) -> bool {
    if inspect(|s| s.fail_finalize) {
        return false;
    }
    let Object::Destination(data) = object(ptr) else {
        return false;
    };
    let Object::Data(bytes) = &mut **data else {
        return false;
    };
    // A transport sentinel, not a claimed image or native pixel/color proof.
    bytes.extend_from_slice(b"test-imageio-png");
    true
}
#[no_mangle]
unsafe extern "C" fn CGGetActiveDisplayList(_: u32, displays: *mut u32, count: *mut u32) -> i32 {
    *displays = 1;
    *count = 1;
    0
}
#[no_mangle]
unsafe extern "C" fn CGDisplayBounds(_: u32) -> Rect {
    Rect {
        origin: Point { x: -1200.0, y: 0.0 },
        size: Size {
            width: 1200.0,
            height: 800.0,
        },
    }
}
#[no_mangle]
unsafe extern "C" fn CGDisplayCopyDisplayMode(_: u32) -> *const c_void {
    owned(Object::DisplayMode)
}
#[no_mangle]
unsafe extern "C" fn CGDisplayModeGetPixelWidth(_: *const c_void) -> usize {
    2400
}
#[no_mangle]
unsafe extern "C" fn CGDisplayModeGetPixelHeight(_: *const c_void) -> usize {
    1600
}
#[no_mangle]
unsafe extern "C" fn CGDisplayRotation(_: u32) -> f64 {
    0.0
}
#[no_mangle]
unsafe extern "C" fn CGEventSourceCreate(state: u32) -> *const c_void {
    update(|s| s.key_sources.push(state));
    if inspect(|s| s.fail_event_source) {
        return std::ptr::null();
    }
    owned(Object::Source)
}
fn event_allocation_fails() -> bool {
    update(|s| s.event_allocations += 1);
    inspect(|s| s.fail_event_number == Some(s.event_allocations))
}
#[no_mangle]
unsafe extern "C" fn CGEventCreateMouseEvent(
    _: *const c_void,
    kind: u32,
    pos: Point,
    _: u32,
) -> *mut c_void {
    if event_allocation_fails() {
        return std::ptr::null_mut();
    }
    owned(Object::Event {
        kind,
        pos,
        count: 1,
        flags: inspect(|s| s.keyboard_flags | s.synthetic_flags),
    }) as *mut c_void
}
#[no_mangle]
unsafe extern "C" fn CGEventCreateScrollWheelEvent2(
    _: *const c_void,
    units: u32,
    count: u32,
    y: i32,
    x: i32,
    z: i32,
) -> *mut c_void {
    update(|s| s.wheel_creations.push((units, count, y, x, z)));
    if event_allocation_fails() {
        return std::ptr::null_mut();
    }
    owned(Object::Wheel {
        pos: Point { x: 0.0, y: 0.0 },
        delta: y,
        flags: inspect(|s| s.keyboard_flags | s.synthetic_flags),
    }) as *mut c_void
}
#[no_mangle]
unsafe extern "C" fn CGEventSetLocation(ptr: *mut c_void, pos: Point) {
    match object_mut(ptr) {
        Object::Event { pos: current, .. } | Object::Wheel { pos: current, .. } => *current = pos,
        _ => panic!("not a pointer event"),
    }
    let callback = STATE.with(|s| s.borrow_mut().on_event_configure.take());
    if let Some(callback) = callback {
        callback();
    }
}
#[no_mangle]
unsafe extern "C" fn CGEventSetIntegerValueField(ptr: *mut c_void, _: u32, count: i64) {
    update(|s| {
        if s.move_on_event_configure {
            s.x += 10.0;
        }
        if s.ax_replaced_on_event_configure {
            s.ax_instance += 1;
        }
        if s.rebirth_on_event_configure {
            s.birth.1 += 1;
        }
    });
    if let Object::Event { count: c, .. } = object_mut(ptr) {
        *c = count;
    }
    let callback = STATE.with(|s| s.borrow_mut().on_event_configure.take());
    if let Some(callback) = callback {
        callback();
    }
}
#[no_mangle]
unsafe extern "C" fn CGEventPostToPid(pid: i32, ptr: *mut c_void) {
    if let Object::Key { code, down, flags } = object(ptr) {
        update(|s| s.key_posts.push((pid, *code, *down, *flags)));
        if *down {
            let callback = STATE.with(|s| s.borrow_mut().on_key_down.take());
            if let Some(callback) = callback {
                callback();
            }
        }
    }
    if let Object::Wheel { pos, delta, flags } = object(ptr) {
        update(|s| {
            s.wheel_posts.push((pid, pos.x, pos.y, *delta));
            s.pointer_post_flags.push(*flags);
        });
        let callback = STATE.with(|s| s.borrow_mut().on_wheel_post.take());
        if let Some(callback) = callback {
            callback();
        }
    }
    if let Object::Event {
        kind,
        pos,
        count,
        flags,
    } = object(ptr)
    {
        update(|s| {
            s.posts.push((pid, *kind, pos.x, pos.y, *count));
            s.pointer_post_flags.push(*flags);
            if matches!(*kind, 1 | 3 | 25) {
                if s.ax_replaced_on_down {
                    s.ax_instance += 1;
                }
                if s.rebirth_on_down {
                    s.birth.1 += 1;
                }
                if s.occlude_on_down {
                    s.occluded = true;
                }
                if let Some(token) = &s.cancel_on_down {
                    token.cancel();
                }
            }
        });
        if matches!(*kind, 1 | 3 | 25) {
            let callback = STATE.with(|s| s.borrow_mut().on_mouse_down.take());
            if let Some(callback) = callback {
                callback();
            }
        }
        if *kind == 6 {
            let callback = STATE.with(|s| s.borrow_mut().on_mouse_drag.take());
            if let Some(callback) = callback {
                callback();
            }
        }
    }
}

#[no_mangle]
unsafe extern "C" fn CFGetTypeID(value: *const c_void) -> usize {
    match object(value) {
        Object::String(_) => 1,
        Object::Ax { .. } => 2,
        Object::Point(_) | Object::Size(_) | Object::TextRange(_) => 3,
        Object::Array(_) => 4,
        Object::Boolean(_) => 5,
        Object::Number(_) | Object::Index(_) => 7,
        _ => 6,
    }
}
#[no_mangle]
unsafe extern "C" fn CFStringGetTypeID() -> usize {
    1
}
#[no_mangle]
unsafe extern "C" fn CFNumberGetTypeID() -> usize {
    7
}
#[no_mangle]
unsafe extern "C" fn AXUIElementGetTypeID() -> usize {
    2
}
#[no_mangle]
unsafe extern "C" fn AXValueGetTypeID() -> usize {
    3
}
#[no_mangle]
unsafe extern "C" fn CFEqual(a: *const c_void, b: *const c_void) -> bool {
    match (object(a), object(b)) {
        (
            Object::Ax {
                instance: a,
                node: an,
                generation: ag,
                ..
            },
            Object::Ax {
                instance: b,
                node: bn,
                generation: bg,
                ..
            },
        ) => a == b && an == bn && ag == bg,
        _ => false,
    }
}
#[no_mangle]
unsafe extern "C" fn AXUIElementCreateApplication(pid: i32) -> *const c_void {
    if pid != 7 {
        return std::ptr::null();
    }
    owned(Object::Ax {
        instance: inspect(|s| s.ax_instance),
        role: "AXApplication",
        node: u64::MAX,
        generation: 0,
    })
}
#[no_mangle]
unsafe extern "C" fn AXUIElementSetMessagingTimeout(_: *const c_void, seconds: f32) -> i32 {
    if inspect(|s| s.ax_fail_timeout) || seconds <= 0.0 || seconds > 0.2 {
        -25201
    } else {
        0
    }
}
#[no_mangle]
unsafe extern "C" fn AXUIElementGetPid(element: *const c_void, pid: *mut i32) -> i32 {
    if !matches!(object(element), Object::Ax { .. }) {
        return -25202;
    }
    let child = matches!(object(element), Object::Ax { node: 1..=1000, .. });
    *pid = if inspect(|s| s.ax_wrong_pid || (child && s.ax_child_wrong_pid)) {
        8
    } else {
        7
    };
    0
}
#[no_mangle]
unsafe extern "C" fn AXUIElementCopyElementAtPosition(
    _: *const c_void,
    x: f32,
    y: f32,
    out: *mut *const c_void,
) -> i32 {
    let (id, wrong, cancel) =
        inspect(|s| (s.ax_instance, s.ax_wrong_type, s.ax_cancel_on_hit.clone()));
    if let Some(cancel) = cancel {
        cancel.cancel();
    }
    let requested = inspect(|s| (x, y) == s.ax_test_point);
    update(|s| {
        s.ax_hit_points.push((x, y));
        if requested {
            s.ax_point_hit_calls += 1;
        }
    });
    let node = if requested {
        inspect(|s| s.ax_point_hit_node)
    } else {
        1
    };
    *out = if requested && inspect(|s| s.ax_point_hit_null) {
        std::ptr::null()
    } else if wrong {
        owned(string("not an AX element"))
    } else {
        owned(Object::Ax {
            instance: id,
            role: if node == 0 { "AXWindow" } else { "AXButton" },
            node,
            generation: if node == 1 {
                inspect(|s| s.ax_button_generation)
            } else {
                0
            },
        })
    };
    let status = if requested {
        inspect(|s| s.ax_point_hit_status)
    } else {
        0
    };
    let callback = STATE.with(|s| {
        let mut s = s.borrow_mut();
        if requested && s.ax_on_point_hit_at == Some(s.ax_point_hit_calls) {
            s.ax_on_point_hit.take()
        } else {
            None
        }
    });
    if let Some(callback) = callback {
        callback();
    }
    status
}
#[no_mangle]
unsafe extern "C" fn AXUIElementCopyAttributeValue(
    element: *const c_void,
    name: *const c_void,
    out: *mut *const c_void,
) -> i32 {
    *out = std::ptr::null();
    let (
        Object::Ax {
            instance,
            role,
            node,
            generation,
        },
        Object::String(name),
    ) = (object(element), object(name))
    else {
        return -25202;
    };
    if inspect(|s| {
        s.ax_fail_attribute
            || s.ax_instance != *instance
            || (*node == 1 && *generation != s.ax_button_generation)
    }) {
        return -25202;
    }
    let attr = match name.to_bytes() {
        b"AXRole" => string(if *node == 1 && inspect(|s| s.ax_role_changed) {
            "AXCheckBox"
        } else if *node == 1 {
            inspect(|s| s.ax_text_role).unwrap_or(role)
        } else {
            role
        }),
        b"AXSubrole" if *node == 2 || (*node == 1 && inspect(|s| s.ax_subrole_secure)) => {
            string("AXSecureTextField")
        }
        b"AXFocused" => Object::Boolean(*node == 1 && inspect(|s| s.ax_focused)),
        b"AXFrontmost" => Object::Boolean(*node == u64::MAX && inspect(|s| s.ax_frontmost)),
        b"AXFocusedWindow" if *node == u64::MAX => Object::Ax {
            instance: *instance + u64::from(inspect(|s| s.ax_focus_window_escape)),
            role: "AXWindow",
            node: 0,
            generation: 0,
        },
        b"AXFocusedUIElement" if *node == u64::MAX => Object::Ax {
            instance: *instance,
            role: "AXButton",
            node: inspect(|s| s.ax_focus_node),
            generation: inspect(|s| s.ax_button_generation),
        },
        b"AXWindow" => Object::Ax {
            instance: *instance + u64::from(*node != 0 && inspect(|s| s.ax_window_escape)),
            role: "AXWindow",
            node: 0,
            generation: 0,
        },
        b"AXParent" if *node != 0 => Object::Ax {
            instance: *instance,
            role: "AXWindow",
            node: if inspect(|s| s.ax_parent_escape) {
                u64::MAX
            } else if *node == 1 && inspect(|s| s.ax_chain_depth > 0) {
                9 + inspect(|s| s.ax_chain_depth as u64)
            } else if *node > 10 && *node < u64::MAX {
                *node - 1
            } else {
                0
            },
            generation: 0,
        },
        b"AXTitle" => {
            if *node == 2 {
                update(|s| s.ax_secure_reads += 1);
            }
            string(&inspect(|s| {
                if *node == 0 {
                    s.window_title.clone()
                } else {
                    s.ax_button_name.clone()
                }
            }))
        }
        b"AXValue" => {
            update(|s| s.ax_secure_reads += 1);
            string("NEVER EXPORT THIS VALUE")
        }
        b"AXNumberOfCharacters" | b"AXSelectedTextRange" if *node == 1 => {
            if inspect(|s| s.ax_text_attributes_missing) {
                return -25205;
            }
            if inspect(|s| s.ax_subrole_secure) {
                update(|s| s.ax_secure_reads += 1);
            }
            if name.to_bytes() == b"AXNumberOfCharacters" {
                inspect(|s| {
                    if s.ax_count_wrong_type {
                        string("wrong count")
                    } else if let Some(n) = s.ax_count_override {
                        Object::Number(n)
                    } else {
                        Object::Index(s.ax_text.encode_utf16().count() as isize)
                    }
                })
            } else {
                inspect(|s| {
                    if s.ax_range_wrong_type {
                        Object::Point(Point { x: 0.0, y: 0.0 })
                    } else {
                        Object::TextRange(TextRange {
                            location: s.ax_selection.0,
                            length: s.ax_selection.1,
                        })
                    }
                })
            }
        }
        b"AXEnabled" => Object::Boolean(*node == 1 && inspect(|s| s.ax_button_enabled)),
        b"AXHidden" => Object::Boolean(
            (*node == 1 && inspect(|s| s.ax_button_hidden))
                || (*node == 10 && inspect(|s| s.ax_group_hidden)),
        ),
        b"AXPosition" => Object::Point(Point {
            x: inspect(|s| {
                s.x + if s.ax_wrong_bounds { 1.0 } else { 0.0 }
                    + if s.ax_tree_enabled && *node != 0 {
                        20.0
                    } else {
                        0.0
                    }
            }),
            y: if inspect(|s| s.ax_tree_enabled) && *node != 0 {
                110.0
            } else {
                80.0
            },
        }),
        b"AXSize" => Object::Size(Size {
            width: if inspect(|s| s.ax_tree_enabled) && *node != 0 {
                inspect(|s| s.ax_button_size.0)
            } else {
                600.0
            },
            height: if inspect(|s| s.ax_tree_enabled) && *node != 0 {
                inspect(|s| s.ax_button_size.1)
            } else {
                400.0
            },
        }),
        _ => return -25205,
    };
    *out = owned(attr);
    if name.to_bytes() == b"AXTitle" && *node == 1 {
        let callback = STATE.with(|s| {
            let mut s = s.borrow_mut();
            s.ax_name_reads += 1;
            if s.ax_on_name_read_at == Some(s.ax_name_reads) {
                s.ax_on_name_read.take()
            } else {
                None
            }
        });
        if let Some(callback) = callback {
            callback();
        }
    }
    let text_callback = STATE.with(|s| {
        let mut s = s.borrow_mut();
        match name.to_bytes() {
            b"AXNumberOfCharacters" => s.ax_on_count_read.take(),
            b"AXSelectedTextRange" => s.ax_on_range_read.take(),
            _ => None,
        }
    });
    if let Some(callback) = text_callback {
        callback();
    }
    if name.to_bytes() == b"AXFocusedUIElement" {
        let callback = STATE.with(|s| {
            let mut s = s.borrow_mut();
            s.ax_focus_reads += 1;
            if s.ax_on_focus_read_at == Some(s.ax_focus_reads) {
                s.ax_on_focus_read.take()
            } else {
                None
            }
        });
        if let Some(callback) = callback {
            callback();
        }
    }
    0
}

#[no_mangle]
unsafe extern "C" fn CFRetain(ptr: *const c_void) -> *const c_void {
    owned(object(ptr).clone())
}
#[no_mangle]
unsafe extern "C" fn CFStringGetLength(ptr: *const c_void) -> isize {
    match object(ptr) {
        Object::String(s) => s.to_str().unwrap().encode_utf16().count() as isize,
        _ => -1,
    }
}
#[no_mangle]
unsafe extern "C" fn CFArrayGetTypeID() -> usize {
    4
}
#[no_mangle]
unsafe extern "C" fn CFBooleanGetTypeID() -> usize {
    5
}
#[no_mangle]
unsafe extern "C" fn CFBooleanGetValue(ptr: *const c_void) -> bool {
    matches!(object(ptr), Object::Boolean(true))
}

fn child_count(element: &Object) -> usize {
    match element {
        Object::Ax { node: 0, .. } if inspect(|s| s.ax_tree_enabled) => {
            inspect(|s| s.ax_child_count_override.unwrap_or(2))
        }
        Object::Ax { node: 1, .. } if inspect(|s| s.ax_cycle) => 1,
        Object::Ax {
            node: 10..=1000, ..
        } => 1,
        _ => 0,
    }
}
#[no_mangle]
unsafe extern "C" fn AXUIElementGetAttributeValueCount(
    element: *const c_void,
    _: *const c_void,
    out: *mut isize,
) -> i32 {
    *out = child_count(object(element)) as isize;
    0
}
#[no_mangle]
unsafe extern "C" fn AXUIElementCopyAttributeValues(
    element: *const c_void,
    _: *const c_void,
    start: isize,
    count: isize,
    out: *mut *const c_void,
) -> i32 {
    if start != 0 || count < 0 {
        return -25201;
    }
    let Object::Ax { instance, node, .. } = object(element) else {
        return -25202;
    };
    let len = (count as usize).min(child_count(object(element)));
    update(|s| {
        s.ax_children_requests.push(count as usize);
        if let Some(cancel) = &s.ax_cancel_on_children {
            cancel.cancel();
        }
        if s.ax_replace_on_children {
            s.ax_instance += 1;
        }
    });
    let values = (0..len)
        .map(|i| {
            if inspect(|s| s.ax_wrong_child_type) {
                return string("invalid child");
            }
            let child = if *node >= 10 {
                if *node + 1 < 10 + inspect(|s| s.ax_chain_depth as u64) {
                    *node + 1
                } else {
                    1
                }
            } else if *node == 1 {
                0
            } else if i == 1 {
                2
            } else if inspect(|s| s.ax_chain_depth > 0) {
                10
            } else {
                1
            };
            Object::Ax {
                instance: *instance,
                role: match child {
                    0 => "AXWindow",
                    1 => "AXButton",
                    2 => "AXTextField",
                    _ => "AXGroup",
                },
                node: child,
                generation: if child == 1 {
                    inspect(|s| s.ax_button_generation)
                } else {
                    0
                },
            }
        })
        .collect();
    *out = owned(Object::Array(values));
    // Run hooks outside the RefCell borrow. They exercise adapter revocation
    // during a native read, not a second emulated native worker/thread.
    let callback = STATE.with(|s| s.borrow_mut().ax_on_children.take());
    if let Some(callback) = callback {
        callback();
    }
    0
}
#[no_mangle]
unsafe extern "C" fn AXUIElementCopyActionNames(
    element: *const c_void,
    out: *mut *const c_void,
) -> i32 {
    let Object::Ax { node, .. } = object(element) else {
        return -25202;
    };
    if *node == 2 {
        update(|s| s.ax_secure_reads += 1);
    }
    if *node == 1 && inspect(|s| s.ax_move_on_action_names) {
        update(|s| s.x += 10.0);
    }
    *out = owned(Object::Array(
        if *node == 1 && inspect(|s| s.ax_press_available) {
            vec![string("AXPress")]
        } else {
            vec![]
        },
    ));
    0
}
#[no_mangle]
unsafe extern "C" fn AXUIElementPerformAction(
    element: *const c_void,
    action: *const c_void,
) -> i32 {
    let (
        Object::Ax {
            node,
            generation,
            instance,
            ..
        },
        Object::String(action),
    ) = (object(element), object(action))
    else {
        return -25201;
    };
    if *node != 1
        || action.to_bytes() != b"AXPress"
        || inspect(|s| *generation != s.ax_button_generation || *instance != s.ax_instance)
    {
        return -25202;
    }
    update(|s| {
        s.ax_presses.push(*generation);
        if let Some(cancel) = &s.ax_cancel_on_press {
            cancel.cancel();
        }
    });
    inspect(|s| s.ax_press_status)
}
#[no_mangle]
unsafe extern "C" fn AXUIElementIsAttributeSettable(
    element: *const c_void,
    attribute: *const c_void,
    out: *mut u8,
) -> i32 {
    let (
        Object::Ax {
            node,
            instance,
            generation,
            ..
        },
        Object::String(attribute),
    ) = (object(element), object(attribute))
    else {
        return -25201;
    };
    update(|s| {
        s.ax_settable_reads += 1;
        if *node == 2 || (*node == 1 && s.ax_subrole_secure) {
            s.ax_secure_reads += 1;
        }
    });
    if *node != 1
        || !matches!(
            attribute.to_bytes(),
            b"AXValue" | b"AXSelectedTextRange" | b"AXSelectedText"
        )
    {
        return -25205;
    }
    if inspect(|s| *instance != s.ax_instance || *generation != s.ax_button_generation) {
        return -25202;
    }
    *out = inspect(|s| match attribute.to_bytes() {
        b"AXSelectedTextRange" => s.ax_append_settable.0,
        b"AXSelectedText" => s.ax_append_settable.1,
        _ => s.ax_value_settable,
    });
    let status = inspect(|s| s.ax_settable_status);
    let callback = STATE.with(|s| s.borrow_mut().ax_on_settable.take());
    if let Some(callback) = callback {
        callback();
    }
    status
}
#[no_mangle]
unsafe extern "C" fn AXUIElementSetAttributeValue(
    element: *const c_void,
    attribute: *const c_void,
    value: *const c_void,
) -> i32 {
    let (
        Object::Ax {
            node,
            instance,
            generation,
            ..
        },
        Object::String(attribute),
    ) = (object(element), object(attribute))
    else {
        return -25201;
    };
    if *node != 1
        || inspect(|s| *instance != s.ax_instance || *generation != s.ax_button_generation)
    {
        return -25202;
    }
    if attribute.to_bytes() == b"AXSelectedTextRange" {
        let Object::TextRange(range) = object(value) else {
            return -25201;
        };
        update(|s| {
            s.ax_append_writes.push("range".into());
            if !s.ax_selection_ignored {
                s.ax_selection = (range.location, range.length);
            }
        });
        let status = inspect(|s| s.ax_append_status.0);
        let callback = STATE.with(|s| s.borrow_mut().ax_on_range_set.take());
        if let Some(callback) = callback {
            callback();
        }
        return status;
    }
    if attribute.to_bytes() == b"AXSelectedText" {
        let Object::String(value) = object(value) else {
            return -25201;
        };
        let text = value.to_str().unwrap();
        // Independent control model: replace CURRENT selection, including a
        // nonempty or moved selection. Tests prove production avoids clobbering.
        let result = STATE.with(|s| {
            let mut s = s.borrow_mut();
            let mut units = s.ax_text.encode_utf16().collect::<Vec<_>>();
            let (start, length) = s.ax_selection;
            if start < 0
                || length < 0
                || start.checked_add(length).is_none()
                || start + length > units.len() as isize
            {
                return -25201;
            }
            let suffix = text.encode_utf16().collect::<Vec<_>>();
            units.splice(
                start as usize..(start + length) as usize,
                suffix.iter().copied(),
            );
            let Ok(next) = String::from_utf16(&units) else {
                return -25201;
            };
            s.ax_text = next;
            s.ax_selection = (start + suffix.len() as isize, 0);
            s.ax_append_writes.push("text".into());
            s.ax_append_status.1
        });
        let callback = STATE.with(|s| s.borrow_mut().ax_on_text_set.take());
        if let Some(callback) = callback {
            callback();
        }
        return result;
    }
    if attribute.to_bytes() != b"AXValue" {
        return -25205;
    }
    let Object::String(value) = object(value) else {
        return -25201;
    };
    update(|s| {
        s.ax_values
            .push((*generation, value.to_str().unwrap().to_owned()));
        if s.ax_subrole_secure {
            s.ax_secure_reads += 1;
        }
        if let Some(cancel) = &s.ax_cancel_on_value {
            cancel.cancel();
        }
    });
    inspect(|s| s.ax_value_status)
}
#[no_mangle]
unsafe extern "C" fn AXValueGetType(value: *const c_void) -> u32 {
    match object(value) {
        Object::Point(_) => 1,
        Object::Size(_) => 2,
        Object::TextRange(_) => 4,
        _ => 0,
    }
}
#[no_mangle]
unsafe extern "C" fn AXValueGetValue(value: *const c_void, kind: u32, out: *mut c_void) -> bool {
    match object(value) {
        Object::Point(p) if kind == 1 => {
            *out.cast::<Point>() = *p;
            true
        }
        Object::Size(s) if kind == 2 => {
            *out.cast::<Size>() = *s;
            true
        }
        Object::TextRange(range) if kind == 4 => {
            *out.cast::<TextRange>() = *range;
            true
        }
        _ => false,
    }
}

#[no_mangle]
unsafe extern "C" fn AXValueCreate(kind: u32, value: *const c_void) -> *const c_void {
    if kind != 4 || value.is_null() || inspect(|s| s.ax_fail_range_create) {
        return std::ptr::null();
    }
    let result = owned(Object::TextRange(*value.cast::<TextRange>()));
    let callback = STATE.with(|s| s.borrow_mut().ax_on_range_create.take());
    if let Some(callback) = callback {
        callback();
    }
    result
}

#[no_mangle]
unsafe extern "C" fn CGEventCreateKeyboardEvent(
    _: *const c_void,
    code: u16,
    down: bool,
) -> *mut c_void {
    if event_allocation_fails() {
        return std::ptr::null_mut();
    }
    owned(Object::Key {
        code,
        down,
        flags: inspect(|s| s.keyboard_flags),
    }) as *mut c_void
}
#[no_mangle]
unsafe extern "C" fn CGEventSetFlags(ptr: *mut c_void, flags: u64) {
    let key = match object_mut(ptr) {
        Object::Key { flags: current, .. } => {
            *current = flags;
            true
        }
        Object::Event { flags: current, .. } | Object::Wheel { flags: current, .. } => {
            *current = flags;
            false
        }
        _ => panic!("not an input event"),
    };
    let callback = STATE.with(|s| {
        if key {
            s.borrow_mut().on_key_configure.take()
        } else {
            s.borrow_mut().on_event_configure.take()
        }
    });
    if let Some(callback) = callback {
        callback();
    }
}
#[no_mangle]
unsafe extern "C" fn CGEventSourceFlagsState(state: u32) -> u64 {
    inspect(|s| s.keyboard_flags | if state == 0 { s.synthetic_flags } else { 0 })
}
#[no_mangle]
unsafe extern "C" fn CGEventSourceKeyState(state: u32, key: u16) -> bool {
    inspect(|s| s.held_key == Some(key) || (state == 0 && s.synthetic_key == Some(key)))
}

#[no_mangle]
unsafe extern "C" fn CGEventSourceButtonState(state: u32, button: u32) -> bool {
    inspect(|s| s.held_button == Some(button) || (state == 0 && s.synthetic_button == Some(button)))
}
