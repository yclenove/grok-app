//! Synthetic event construction and the injectable [`EventApi`].
//!
//! Targeted mouse events always carry undocumented CoreGraphics field 51 (the
//! window number) and a window-local point. The CoreGraphics backend posts
//! those on macOS. [`super::FakeEvents`] records them on every platform.
//! `PointerRoute::TargetedOnly` and `KeyRoute::TargetedOnly` never become a
//! HID-tap event here; the router returns [`DriverError::CapabilityUnavailable`].

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use gcu_core::{
    driver::{Button, DriverError, Result},
    model::WindowPoint,
};
use gcu_protocol::capabilities::PrivateApiStatus;

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
    /// Pointer moved. Not a held button.
    MouseMove,
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
            EventKind::MouseMove => "mouse_move",
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
    /// Raise one admitted window. Not a process-wide activation.
    Raise {
        /// Target pid.
        pid: i32,
        /// Window number.
        window_id: u32,
    },
}

/// Last check before [`EventApi::post`] hands an event to the OS.
///
/// [`EventApi::post_release`] does not call this. Cleanup must still release
/// a button or key after a refused post.
pub trait PostGate: Send + Sync {
    /// `Err(NotSent)` means this event was not posted.
    fn before_post(&self, event: &SyntheticEvent) -> std::result::Result<(), PostFault>;

    /// True when a global HID post must not be selected or sent.
    ///
    /// The default allows HID. Memory only. No blocking, timeout, or cancellation.
    fn blocks_global_hid(&self) -> bool {
        false
    }
}

/// Backend. Real CoreGraphics and SkyLight live behind this trait.
#[async_trait]
pub trait EventApi: Send + Sync {
    /// Injectable safety sources. Missing guards disable foreground input.
    fn guards(&self) -> Option<std::sync::Arc<super::hotkey::Guards>> {
        None
    }
    /// Read the actual foreground process. The enclosing watchdog bounds it.
    /// Unknown foreground state must not be treated as successful activation.
    async fn frontmost_pid(&self) -> Result<Option<i32>> {
        Ok(None)
    }
    /// Current probe. Cheap and non-blocking.
    fn probe(&self) -> Probe;
    /// Remember that a targeted post could not be observed. No retry.
    fn note_arrival_failed(&self);
    /// `false` when this pid/window pair is gone.
    fn window_open(&self, pid: i32, window_id: u32) -> bool;
    /// Bring `pid` to the foreground. Targeted routes must not call this.
    async fn activate(&self, pid: i32) -> Result<()>;
    /// Raise one window. The default does not activate the process.
    ///
    /// Blocking and timeout are the implementor's. The default does not block
    /// and returns [`DriverError::CapabilityUnavailable`]. Cancellation is the
    /// caller's.
    async fn raise_window(&self, pid: i32, window_id: u32) -> Result<()> {
        let _ = (pid, window_id);
        Err(DriverError::CapabilityUnavailable)
    }
    /// True when the armed gate refuses global HID delivery.
    ///
    /// Memory only in the default, which returns false. No timeout or cancellation.
    fn blocks_global_hid(&self) -> bool {
        false
    }
    /// Post one event. Must not retry internally.
    async fn post(&self, event: SyntheticEvent) -> std::result::Result<(), PostFault>;
    /// Post a cleanup up. Skips the identity gate. Must not retry.
    async fn post_release(&self, event: SyntheticEvent) -> std::result::Result<(), PostFault>;
    /// Install the gate [`EventApi::post`] checks immediately before the OS call.
    /// `None` clears it. The default ignores the gate.
    fn arm_post_gate(&self, gate: Option<Arc<dyn PostGate>>) {
        let _ = gate;
    }
    /// Observe whether the targeted event landed. Must not post again.
    async fn verify_arrival(&self, pid: i32, window_id: u32) -> std::result::Result<(), PostFault>;
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
        button: (!matches!(kind, EventKind::Scroll | EventKind::MouseMove)).then_some(button),
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

/// Hover, then one scroll at `at`. Deltas may be negative or zero.
///
/// The move is a separate event so a scroll area can be hovered before the
/// wheel. `scroll` stays the caller's pixel deltas. [`wheel_axes`] is the
/// line-unit conversion the CoreGraphics backend posts.
pub fn plan_scroll(
    target: &InputTarget,
    at: WindowPoint,
    dx: i32,
    dy: i32,
    path: PostPath,
) -> Result<Vec<SyntheticEvent>> {
    let local = require_point(target, at)?;
    let hover = pointer(target, path, EventKind::MouseMove, local, 0, 0, false);
    let mut event = pointer(target, path, EventKind::Scroll, local, 0, 0, true);
    event.scroll = Some((dx, dy));
    Ok(vec![hover, event])
}

/// Pixel wheel deltas as line counts for egui's native line speed (40 points).
///
/// A non-zero axis is at least one line and at most eight, so one event can
/// move a list without posting a burst. Zero stays zero. The CoreGraphics
/// backend is macOS-only; the conversion still has to exist for the planner.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn wheel_axes(dx: i32, dy: i32) -> (i32, i32) {
    (wheel_lines(dx), wheel_lines(dy))
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn wheel_lines(delta: i32) -> i32 {
    if delta == 0 {
        return 0;
    }
    delta.signum() * (delta.abs() / 40).clamp(1, 8)
}

/// Gap between drag samples.
///
/// egui 0.36 ignores a press and release in the same frame. 32ms is longer
/// than one 60 Hz frame and stays inside the driver deadline.
pub const DRAG_SAMPLE_GAP: Duration = Duration::from_millis(32);

/// Drag along `path_points`. Fewer than two points is [`DriverError::InvalidRequest`].
///
/// Every point after the press is a drag sample, including the destination,
/// and then the button goes up. The router waits [`DRAG_SAMPLE_GAP`] between
/// those samples so a same-frame press-release is not the whole gesture.
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
    for local in locals.iter().skip(1) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use gcu_core::model::WindowPoint;

    fn target() -> InputTarget {
        InputTarget {
            pid: 4,
            window_id: 7,
            bundle_id: "com.example.Editor".into(),
            app_name: "Editor".into(),
            origin_x: 0.0,
            origin_y: 0.0,
            width_px: 400.0,
            height_px: 300.0,
            scale: 1.0,
        }
    }

    #[test]
    fn wheel_axes_use_egui_line_speed() {
        assert_eq!(wheel_axes(0, 0), (0, 0));
        assert_eq!(wheel_axes(0, -480), (0, -8));
        assert_eq!(wheel_axes(0, 480), (0, 8));
        assert_eq!(wheel_axes(0, -8), (0, -1));
        assert_eq!(wheel_axes(39, 0), (1, 0));
    }

    #[test]
    fn plan_scroll_hovers_before_the_wheel() {
        let events = plan_scroll(
            &target(),
            WindowPoint { x: 20.0, y: 40.0 },
            0,
            -480,
            PostPath::ToPid,
        )
        .unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].kind, EventKind::MouseMove);
        assert!(!events[0].is_press());
        assert_eq!(events[0].window_local, Some((20.0, 40.0)));
        assert_eq!(events[1].kind, EventKind::Scroll);
        assert_eq!(events[1].scroll, Some((0, -480)));
    }
}
