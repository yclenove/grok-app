//! Screenshot-bound pointer operations. All drag events/releases are allocated
//! before input; revoked authority permits only cleanup at the last posted point.
use super::*;
use grok_computer_use_core::native_action::NativeActionGuard;
use std::time::Duration;

const DRAG_STEPS: usize = 16;
const K_CG_EVENT_LEFT_MOUSE_DRAGGED: u32 = 6;
const K_CG_SCROLL_EVENT_UNIT_PIXEL: u32 = 0;
// kCGEventSourceStatePrivate (-1).
const PRIVATE_SOURCE: u32 = u32::MAX;
// Shift/control/option/command/function affect pointer meaning. Caps Lock and
// numeric-pad event metadata are not held pointer modifiers.
const POINTER_MODIFIERS: u64 = 0x009e_0000;

#[cfg_attr(target_os = "macos", link(name = "CoreGraphics", kind = "framework"))]
extern "C" {
    // Fixed-arity API: no platform-dependent C variadic argument ABI.
    fn CGEventCreateScrollWheelEvent2(
        source: CfTypeRef,
        units: u32,
        wheel_count: u32,
        wheel1: i32,
        wheel2: i32,
        wheel3: i32,
    ) -> CgEventRef;
    fn CGEventSetLocation(event: CgEventRef, position: CgPoint);
    fn CGEventSetFlags(event: CgEventRef, flags: u64);
    fn CGEventSourceFlagsState(state: u32) -> u64;
    fn CGEventSourceButtonState(state: u32, button: u32) -> bool;
}

pub(super) fn source() -> Result<OwnedCf, String> {
    OwnedCf::new(
        unsafe { CGEventSourceCreate(PRIVATE_SOURCE) },
        "private pointer source allocation failed",
    )
}

pub(super) fn neutral(event: &OwnedCf) {
    unsafe { CGEventSetFlags(event.0 as CgEventRef, 0) };
}

pub(super) fn hardware_clear(owned_button: Option<u32>) -> Result<(), String> {
    // Private event state/zero flags do not entitle us to borrow or release a
    // user's held button. Check physical AND combined-session state. During
    // an owned gesture only its own queued down may appear in the latter.
    // Quartz has no atomic ownership oracle for racing synthetic senders.
    let modifiers = unsafe {
        CGEventSourceFlagsState(K_CG_EVENT_SOURCE_STATE_HID) | CGEventSourceFlagsState(0)
    };
    let held = (0..3).any(|button| unsafe {
        CGEventSourceButtonState(K_CG_EVENT_SOURCE_STATE_HID, button)
            || (owned_button != Some(button) && CGEventSourceButtonState(0, button))
    });
    if modifiers & POINTER_MODIFIERS != 0 || held {
        return Err(
            "macOS pointer is in use; release held buttons/modifiers before retrying".into(),
        );
    }
    Ok(())
}

pub(super) fn point(
    adapter: &MacosAdapter,
    req: &DispatchRequest,
    observation: &Arc<ModelObservation<ax_tree::Tree>>,
    x: f64,
    y: f64,
) -> Result<CgPoint, String> {
    req.admit(adapter)?;
    let position = mapped_point(adapter, req, observation.frame(), x, y, &req.cancellation)?;
    // Native query/configuration callbacks may have revoked permission or
    // replaced the model observation. A loaded Arc alone is not authority.
    adapter
        .snapshots
        .require_current(&req.run_id, &req.target_id, observation)?;
    if !adapter.input_available() {
        return Err("macOS pointer permission revoked before dispatch".into());
    }
    req.cancellation.check()?;
    Ok(position)
}

pub(super) fn scroll(
    adapter: &MacosAdapter,
    req: &DispatchRequest,
    observation: &Arc<ModelObservation<ax_tree::Tree>>,
    x: f64,
    y: f64,
) -> Result<bool, String> {
    let delta = super::super::protocol::ScrollDelta::parse(&req.parameters)?;
    hardware_clear(None)?;
    let pos = point(adapter, req, observation, x, y)?;
    if delta.value() == 0 {
        return Ok(false);
    }
    let source = source()?;
    let event = OwnedCf::new(
        unsafe {
            CGEventCreateScrollWheelEvent2(
                source.0,
                K_CG_SCROLL_EVENT_UNIT_PIXEL,
                1,
                delta.native_positive_up(),
                0,
                0,
            )
        },
        "scroll event allocation failed",
    )?;
    unsafe { CGEventSetLocation(event.0 as CgEventRef, pos) };
    neutral(&event);
    point(adapter, req, observation, x, y)?;
    hardware_clear(None)?;
    // Protocol/choice.rs positive means DOWN (browser scrollBy convention),
    // whereas Quartz positive wheel1 means UP. Invert exactly once. No focus
    // click, cursor warp, second wheel source, or replay of an unverified effect.
    unsafe { CGEventPostToPid(observation.frame().target.pid as i32, event.0 as CgEventRef) };
    req.cancellation.check()?;
    Ok(true)
}

struct DragPoint {
    x: f64,
    y: f64,
    event: OwnedCf,
    release: OwnedCf,
}

pub(super) fn release(
    adapter: &MacosAdapter,
    req: &DispatchRequest,
    observation: &Arc<ModelObservation<ax_tree::Tree>>,
    position: (f64, f64),
    event: &OwnedCf,
    button: u32,
    owner: &mut NativeActionGuard<'_>,
) -> Result<(), String> {
    // Cancellation/retirement revokes new input, not cleanup of our own down.
    // Never release at the planned destination when only an earlier point was
    // posted, and never send a release to a replacement process/window.
    let frame = observation.frame();
    let validation: Result<(), String> = (|| {
        hardware_clear(Some(button))?;
        if !adapter.input_available() {
            return Err("macOS pointer permission revoked".into());
        }
        mapped_point(
            adapter,
            req,
            frame,
            position.0,
            position.1,
            &Default::default(),
        )?;
        identity::validate(frame.target)?;
        if !adapter.input_available() {
            return Err("macOS pointer permission revoked".into());
        }
        hardware_clear(Some(button))?;
        Ok(())
    })();
    if let Err(error) = validation {
        owner.retain_until_native_recovery();
        return Err(format!("pointer release unconfirmed: {error}"));
    }
    unsafe { CGEventPostToPid(frame.target.pid as i32, event.0 as CgEventRef) };
    Ok(())
}

pub(super) fn drag(
    adapter: &MacosAdapter,
    req: &DispatchRequest,
    observation: &Arc<ModelObservation<ax_tree::Tree>>,
    x: f64,
    y: f64,
    owner: &mut NativeActionGuard<'_>,
) -> Result<bool, String> {
    let params = req
        .parameters
        .as_object()
        .ok_or("drag parameters must be an object")?;
    if params.len() != 2 {
        return Err("drag requires exactly one destination pair".into());
    }
    let (to_x, to_y) = super::super::protocol::drag_destination(&req.parameters)?;
    hardware_clear(None)?;
    // Validate BOTH endpoints before allocating, then every sampled point.
    point(adapter, req, observation, x, y)?;
    point(adapter, req, observation, to_x, to_y)?;
    let source = source()?;
    let mut path = Vec::with_capacity(DRAG_STEPS + 1);
    for step in 0..=DRAG_STEPS {
        let fraction = step as f64 / DRAG_STEPS as f64;
        let px = x + (to_x - x) * fraction;
        let py = y + (to_y - y) * fraction;
        let pos = point(adapter, req, observation, px, py)?;
        let kind = if step == 0 {
            K_CG_EVENT_LEFT_MOUSE_DOWN
        } else {
            K_CG_EVENT_LEFT_MOUSE_DRAGGED
        };
        let event = OwnedCf::new(
            unsafe { CGEventCreateMouseEvent(source.0, kind, pos, K_CG_MOUSE_BUTTON_LEFT) },
            "drag event allocation failed",
        )?;
        let release = OwnedCf::new(
            unsafe {
                CGEventCreateMouseEvent(
                    source.0,
                    K_CG_EVENT_LEFT_MOUSE_UP,
                    pos,
                    K_CG_MOUSE_BUTTON_LEFT,
                )
            },
            "drag release allocation failed",
        )?;
        unsafe {
            CGEventSetIntegerValueField(event.0 as CgEventRef, 1, 1);
            CGEventSetIntegerValueField(release.0 as CgEventRef, 1, 1);
        }
        neutral(&event);
        neutral(&release);
        path.push(DragPoint {
            x: px,
            y: py,
            event,
            release,
        });
    }
    // Allocation/configuration may have changed visibility along the route.
    // Recheck the entire bounded path BEFORE the first irreversible down.
    for at in &path {
        point(adapter, req, observation, at.x, at.y)?;
    }
    let mut last_posted = None;
    let dispatch: Result<(), String> = (|| {
        for (index, at) in path.iter().enumerate() {
            if index != 0 {
                req.cancellation.check()?;
                std::thread::sleep(Duration::from_millis(8));
            }
            point(adapter, req, observation, at.x, at.y)?;
            hardware_clear(last_posted.map(|_| K_CG_MOUSE_BUTTON_LEFT))?;
            unsafe {
                CGEventPostToPid(
                    observation.frame().target.pid as i32,
                    at.event.0 as CgEventRef,
                )
            };
            last_posted = Some(index);
        }
        // Retirement/cancellation after the final motion is still an aborted
        // gesture, followed by exactly one owned release at that final point.
        point(adapter, req, observation, to_x, to_y)?;
        Ok(())
    })();
    if let Some(index) = last_posted {
        let at = &path[index];
        release(
            adapter,
            req,
            observation,
            (at.x, at.y),
            &at.release,
            K_CG_MOUSE_BUTTON_LEFT,
            owner,
        )?;
    }
    dispatch?;
    req.cancellation.check()?;
    Ok(true)
}
