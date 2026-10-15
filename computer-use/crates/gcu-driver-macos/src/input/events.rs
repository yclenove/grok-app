//! Synthetic event construction and the injectable [`EventApi`].
//!
//! Targeted mouse events always carry undocumented CoreGraphics field 51 (the
//! window number) and a window-local point. [`crate::input::cg`] posts those
//! on macOS. [`FakeEvents`] records the same sequence on every platform.
//! `PointerRoute::TargetedOnly` and `KeyRoute::TargetedOnly` never become a
//! HID-tap event here; the router returns [`DriverError::CapabilityUnavailable`].

use async_trait::async_trait;
use gcu_core::{
    driver::{Button, DriverError, Result},
    model::WindowPoint,
};
use gcu_protocol::capabilities::PrivateApiStatus;
use std::sync::{Mutex, MutexGuard};

/// Undocumented `CGEvent` field: window number. ADR 0006.
///
/// Field 91 (`kCGMouseEventWindowUnderMousePointer`) does not route a
/// background click. Missing field 51 leaves `windowNumber` at 0 and the
/// point is treated as a screen point.
pub const FIELD_WINDOW_NUMBER: u32 = 51;
/// `kCGEventFlagMaskShift`.
pub const FLAG_SHIFT: u64 = 131_072;
/// `kCGEventFlagMaskControl`.
pub const FLAG_CONTROL: u64 = 262_144;
/// `kCGEventFlagMaskAlternate` (Option).
pub const FLAG_ALTERNATE: u64 = 524_288;
/// `kCGEventFlagMaskCommand`.
pub const FLAG_COMMAND: u64 = 1_048_576;
/// `kCGEventFlagMaskSecondaryFn`.
pub const FLAG_FN: u64 = 8_388_608;

/// Where a constructed event is posted. The router picks this once.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PostPath {
    /// `CGEvent.postToPid` with field 51 and a window-local point.
    ToPid,
    /// `CGEvent.post(kCGHIDEventTap)` at a screen point. Foreground only.
    HidTap,
}

/// One synthetic input. Text payloads stay out of `Debug` via the log helper.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventKind {
    /// Button went down.
    MouseDown,
    /// Button went up.
    MouseUp,
    /// Move while a button is held.
    MouseDrag,
    /// Wheel deltas, not a held button.
    Scroll,
    /// Virtual key went down.
    KeyDown,
    /// Virtual key went up.
    KeyUp,
    /// UTF-16 chunk on a key-down. The matching up is [`EventKind::KeyUp`].
    Unicode,
}

/// One event the backend will post. Coordinates are in points, not pixels.
#[derive(Clone, Debug, PartialEq)]
pub struct SyntheticEvent {
    /// Delivery path chosen before any system call.
    pub path: PostPath,
    /// What changed.
    pub kind: EventKind,
    /// Target pid. Zero is rejected before construction.
    pub pid: i32,
    /// CoreGraphics window number. Zero is rejected before construction.
    pub window_id: u32,
    /// Field 51. `Some` only for [`PostPath::ToPid`].
    pub field_51: Option<i64>,
    /// Window-local point in points. `Some` only for targeted pointer events.
    pub window_local: Option<(f64, f64)>,
    /// Screen point in points. `Some` only for [`PostPath::HidTap`].
    pub screen: Option<(f64, f64)>,
    /// 0 left, 1 right, 2 middle. `None` for keys and scroll.
    pub button: Option<u8>,
    /// `kCGMouseEventClickState`. Zero for non-clicks.
    pub click_count: u8,
    /// `(dx, dy)` wheel pixels. `None` unless [`EventKind::Scroll`].
    pub scroll: Option<(i32, i32)>,
    /// macOS virtual keycode. `None` for pointer events.
    pub keycode: Option<u16>,
    /// Modifier bits. Zero when none are held.
    pub flags: u64,
    /// True for down, drag, scroll, and unicode. False for ups.
    pub down: bool,
    /// Unicode chunk. Never logged by [`SyntheticEvent::kind_name`].
    pub unicode: Option<String>,
}

impl SyntheticEvent {
    /// Metadata name. Does not include typed text.
    pub fn kind_name(&self) -> &'static str {
        match self.kind {
            EventKind::MouseDown => "mouse_down",
            EventKind::MouseUp => "mouse_up",
            EventKind::MouseDrag => "mouse_drag",
            EventKind::Scroll => "scroll",
            EventKind::KeyDown => "key_down",
            EventKind::KeyUp => "key_up",
            EventKind::Unicode => "unicode",
        }
    }

    /// True when the ledger must remember an up before this event is posted.
    pub fn is_press(&self) -> bool {
        self.down
            && matches!(
                self.kind,
                EventKind::MouseDown | EventKind::KeyDown | EventKind::Unicode
            )
    }

    /// The matching up, if this event can leave a button or key held.
    pub fn release_event(&self) -> Option<Self> {
        if !self.is_press() {
            return None;
        }
        let mut up = self.clone();
        up.down = false;
        up.kind = EventKind::KeyUp;
        if matches!(self.kind, EventKind::MouseDown) {
            up.kind = EventKind::MouseUp;
        }
        up.unicode = None;
        Some(up)
    }
}

/// Why a post did not complete. The router does not retry any of these.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PostFault {
    /// The system call did not start. Earlier events may still have been sent.
    NotSent,
    /// The call started or its result could not be proven empty.
    Uncertain,
    /// TCC was missing or revoked around the call.
    Permission,
}

/// Runtime probe. Background clicks stay off unless this says they are usable.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Probe {
    /// `CGEventSetWindowLocation` was found with `dlsym`.
    pub window_location_symbol: bool,
    /// Field 51 is usable on this OS. Cleared after a failed arrival check.
    pub field_51: bool,
    /// An earlier delivery was observed to land. Required when `os_major` < 27.
    pub arrival_verified: bool,
    /// `kern.osproductversion` major, or the value a test installed.
    pub os_major: u32,
    /// `NSApplication` exists. CoreGraphics calls crash without it.
    pub appkit_ready: bool,
    /// Accessibility trust. A later revoke is [`PostFault::Permission`].
    pub accessibility: bool,
    /// A security prompt is covering the session.
    pub secure_prompt: bool,
    /// HID-tap fallback exists. The router uses it only for `AllowForeground`.
    pub foreground: bool,
    /// SkyLight symbol probe. Not a delivery grant. See ADR 0006.
    pub skylight: PrivateApiStatus,
}

impl Probe {
    /// macOS 27 shape: symbols present, arrival not yet observed.
    pub fn verified_os() -> Self {
        Self {
            window_location_symbol: true,
            field_51: true,
            arrival_verified: false,
            os_major: 27,
            appkit_ready: true,
            accessibility: true,
            secure_prompt: false,
            foreground: true,
            skylight: PrivateApiStatus::Available,
        }
    }

    /// Background pointer/keys. Chromium is excluded by the router, not here.
    ///
    /// macOS 14 and 15 stay off until `arrival_verified` is set. A failed
    /// arrival check clears `field_51`, which turns this off without a retry.
    pub fn background_ready(&self) -> bool {
        self.window_location_symbol
            && self.field_51
            && self.appkit_ready
            && self.accessibility
            && !self.secure_prompt
            && (self.os_major >= 27 || self.arrival_verified)
    }
}

/// Window the router already resolved. Points are CoreGraphics points.
#[derive(Clone, Debug, PartialEq)]
pub struct InputTarget {
    /// Owning pid.
    pub pid: i32,
    /// CoreGraphics window number (field 51).
    pub window_id: u32,
    /// Bundle id. Chromium detection uses this.
    pub bundle_id: String,
    /// Display name. Chromium detection also uses this.
    pub app_name: String,
    /// Window origin in screen points.
    pub origin_x: f64,
    /// Window origin in screen points.
    pub origin_y: f64,
    /// Width in physical pixels.
    pub width_px: f64,
    /// Height in physical pixels.
    pub height_px: f64,
    /// Physical pixels per point.
    pub scale: f64,
}

/// Recorded backend call. `Post` owns the constructed event.
#[derive(Clone, Debug, PartialEq)]
pub enum Call {
    /// Foreground activation. Targeted routes must not record this.
    Activate(i32),
    /// One post, in order.
    Post(SyntheticEvent),
    /// Arrival check after a targeted sequence. Not a second post.
    Verify {
        /// Target pid.
        pid: i32,
        /// Window number.
        window_id: u32,
    },
}

/// Backend. Real CoreGraphics and SkyLight live behind this trait.
#[async_trait]
pub trait EventApi: Send + Sync {
    /// Current probe. Cheap and non-blocking.
    fn probe(&self) -> Probe;
    /// Remember that a targeted post could not be observed. No retry.
    fn note_arrival_failed(&self);
    /// `false` when this pid/window pair is gone.
    fn window_open(&self, pid: i32, window_id: u32) -> bool;
    /// Bring `pid` to the foreground. Targeted routes must not call this.
    async fn activate(&self, pid: i32) -> Result<()>;
    /// Post one event. Must not retry internally.
    async fn post(&self, event: SyntheticEvent) -> std::result::Result<(), PostFault>;
    /// Observe whether the targeted event landed. Must not post again.
    async fn verify_arrival(&self, pid: i32, window_id: u32) -> std::result::Result<(), PostFault>;
}

/// In-memory backend. Records every call and never touches CoreGraphics.
#[derive(Debug)]
pub struct FakeEvents {
    inner: Mutex<FakeInner>,
}

#[derive(Debug)]
struct FakeInner {
    probe: Probe,
    calls: Vec<Call>,
    frontmost: Option<i32>,
    /// Non-up posts wait until the future is dropped (cancel/timeout).
    hold: bool,
    fault: Option<PostFault>,
    /// Posts of ups ignore `hold` and `fault` so cleanup can finish.
    fail_arrival: bool,
    closed: Vec<(i32, u32)>,
    activate_fails: bool,
}

impl FakeEvents {
    /// Backend whose probe is `probe`. Frontmost starts at pid 1.
    pub fn new(probe: Probe) -> Self {
        Self {
            inner: Mutex::new(FakeInner {
                probe,
                calls: Vec::new(),
                frontmost: Some(1),
                hold: false,
                fault: None,
                fail_arrival: false,
                closed: Vec::new(),
                activate_fails: false,
            }),
        }
    }

    /// macOS 27 probe with symbols and AppKit.
    pub fn ready() -> Self {
        Self::new(Probe::verified_os())
    }

    fn lock(&self) -> MutexGuard<'_, FakeInner> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Calls so far, in order.
    pub fn calls(&self) -> Vec<Call> {
        self.lock().calls.clone()
    }

    /// Pid the fake considers frontmost.
    pub fn frontmost(&self) -> Option<i32> {
        self.lock().frontmost
    }

    /// Next non-up post waits until the caller drops or times out the future.
    pub fn hold_posts(&self) {
        self.lock().hold = true;
    }

    /// Next non-up post returns this fault immediately.
    pub fn fail_next(&self, fault: PostFault) {
        self.lock().fault = Some(fault);
    }

    /// Arrival checks fail once each. The router must not post again.
    pub fn fail_arrival(&self) {
        self.lock().fail_arrival = true;
    }

    /// `window_open` returns false for this pair.
    pub fn close_window(&self, pid: i32, window_id: u32) {
        self.lock().closed.push((pid, window_id));
    }

    /// Activation returns [`DriverError::CapabilityUnavailable`] and does not
    /// change the frontmost pid.
    pub fn fail_activate(&self) {
        self.lock().activate_fails = true;
    }

    /// Replace the probe. Used for TCC revoke and OS-version cases.
    pub fn set_probe(&self, probe: Probe) {
        self.lock().probe = probe;
    }
}

#[async_trait]
impl EventApi for FakeEvents {
    fn probe(&self) -> Probe {
        self.lock().probe.clone()
    }

    fn note_arrival_failed(&self) {
        let mut inner = self.lock();
        inner.probe.field_51 = false;
        inner.probe.arrival_verified = false;
    }

    fn window_open(&self, pid: i32, window_id: u32) -> bool {
        !self.lock().closed.contains(&(pid, window_id))
    }

    async fn activate(&self, pid: i32) -> Result<()> {
        let mut inner = self.lock();
        inner.calls.push(Call::Activate(pid));
        if inner.activate_fails {
            return Err(DriverError::CapabilityUnavailable);
        }
        inner.frontmost = Some(pid);
        Ok(())
    }

    async fn post(&self, event: SyntheticEvent) -> std::result::Result<(), PostFault> {
        let (hold, fault) = {
            let mut inner = self.lock();
            inner.calls.push(Call::Post(event.clone()));
            if !event.down {
                return Ok(());
            }
            let fault = inner.fault.take();
            (inner.hold, fault)
        };
        if let Some(fault) = fault {
            return Err(fault);
        }
        if hold {
            std::future::pending::<()>().await;
        }
        Ok(())
    }

    async fn verify_arrival(&self, pid: i32, window_id: u32) -> std::result::Result<(), PostFault> {
        let fail = {
            let mut inner = self.lock();
            inner.calls.push(Call::Verify { pid, window_id });
            inner.fail_arrival
        };
        if fail {
            Err(PostFault::Uncertain)
        } else {
            Ok(())
        }
    }
}

/// Physical window point to window-local points. Rejects a bad scale.
pub fn window_local(at: WindowPoint, scale: f64) -> Result<(f64, f64)> {
    if !at.x.is_finite() || !at.y.is_finite() || !scale.is_finite() || scale <= 0.0 {
        return Err(DriverError::OutOfBounds);
    }
    Ok((at.x / scale, at.y / scale))
}

/// Screen point in points for a HID-tap event.
pub fn screen_point(target: &InputTarget, local: (f64, f64)) -> (f64, f64) {
    (target.origin_x + local.0, target.origin_y + local.1)
}

pub(crate) fn require_point(target: &InputTarget, at: WindowPoint) -> Result<(f64, f64)> {
    if !target.width_px.is_finite()
        || !target.height_px.is_finite()
        || at.x < 0.0
        || at.y < 0.0
        || at.x > target.width_px
        || at.y > target.height_px
    {
        return Err(DriverError::OutOfBounds);
    }
    window_local(at, target.scale)
}

fn pointer(
    target: &InputTarget,
    path: PostPath,
    kind: EventKind,
    local: (f64, f64),
    button: u8,
    click_count: u8,
    down: bool,
) -> SyntheticEvent {
    let targeted = path == PostPath::ToPid;
    SyntheticEvent {
        path,
        kind,
        pid: target.pid,
        window_id: target.window_id,
        field_51: targeted.then_some(i64::from(target.window_id)),
        window_local: targeted.then_some(local),
        screen: (!targeted).then_some(screen_point(target, local)),
        button: (kind != EventKind::Scroll).then_some(button),
        click_count,
        scroll: None,
        keycode: None,
        flags: 0,
        down,
        unicode: None,
    }
}

/// Click sequence. `count == 0` is [`DriverError::InvalidRequest`].
pub fn plan_click(
    target: &InputTarget,
    at: WindowPoint,
    button: Button,
    count: u8,
    path: PostPath,
) -> Result<Vec<SyntheticEvent>> {
    if count == 0 {
        return Err(DriverError::InvalidRequest);
    }
    let local = require_point(target, at)?;
    let code = match button {
        Button::Left => 0,
        Button::Right => 1,
        Button::Middle => 2,
    };
    let mut events = Vec::with_capacity(usize::from(count) * 2);
    for index in 1..=count {
        events.push(pointer(
            target,
            path,
            EventKind::MouseDown,
            local,
            code,
            index,
            true,
        ));
        events.push(pointer(
            target,
            path,
            EventKind::MouseUp,
            local,
            code,
            index,
            false,
        ));
    }
    Ok(events)
}

/// One scroll event at `at`. Deltas may be negative or zero.
pub fn plan_scroll(
    target: &InputTarget,
    at: WindowPoint,
    dx: i32,
    dy: i32,
    path: PostPath,
) -> Result<Vec<SyntheticEvent>> {
    let local = require_point(target, at)?;
    let mut event = pointer(target, path, EventKind::Scroll, local, 0, 0, true);
    event.scroll = Some((dx, dy));
    Ok(vec![event])
}

/// Drag along `path_points`. Fewer than two points is [`DriverError::InvalidRequest`].
pub fn plan_drag(
    target: &InputTarget,
    path_points: &[WindowPoint],
    path: PostPath,
) -> Result<Vec<SyntheticEvent>> {
    if path_points.len() < 2 {
        return Err(DriverError::InvalidRequest);
    }
    let mut locals = Vec::with_capacity(path_points.len());
    for point in path_points {
        locals.push(require_point(target, *point)?);
    }
    let mut events = Vec::with_capacity(path_points.len() + 1);
    events.push(pointer(
        target,
        path,
        EventKind::MouseDown,
        locals[0],
        0,
        1,
        true,
    ));
    for local in locals.iter().skip(1).take(locals.len().saturating_sub(2)) {
        events.push(pointer(
            target,
            path,
            EventKind::MouseDrag,
            *local,
            0,
            1,
            true,
        ));
    }
    if let Some(last) = locals.last() {
        events.push(pointer(
            target,
            path,
            EventKind::MouseUp,
            *last,
            0,
            1,
            false,
        ));
    }
    Ok(events)
}

pub(crate) fn key_event(
    target: &InputTarget,
    path: PostPath,
    kind: EventKind,
    keycode: u16,
    flags: u64,
    down: bool,
    unicode: Option<String>,
) -> SyntheticEvent {
    let targeted = path == PostPath::ToPid;
    SyntheticEvent {
        path,
        kind,
        pid: target.pid,
        window_id: target.window_id,
        field_51: targeted.then_some(i64::from(target.window_id)),
        window_local: None,
        screen: None,
        button: None,
        click_count: 0,
        scroll: None,
        keycode: (unicode.is_none()).then_some(keycode),
        flags,
        down,
        unicode,
    }
}
