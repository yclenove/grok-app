//! Named keys to an already focused, observed AX control. No focus stealing,
//! global event tap, modifier chord, clipboard, coordinate or text fallback.
use super::ax_api::{Budget, Element};
use super::*;
use grok_computer_use_core::native_action::NativeActionGuard;

const PRIVATE_SOURCE: u32 = u32::MAX; // kCGEventSourceStatePrivate (-1).
const MODIFIERS: u64 = 0x00ff_0000; // Public alpha/shift/control/option/cmd/pad/help/fn flags.

#[cfg_attr(target_os = "macos", link(name = "CoreGraphics", kind = "framework"))]
extern "C" {
    fn CGEventCreateKeyboardEvent(source: CfTypeRef, key: u16, down: bool) -> CgEventRef;
    fn CGEventSetFlags(event: CgEventRef, flags: u64);
    fn CGEventSourceFlagsState(state: u32) -> u64;
    fn CGEventSourceKeyState(state: u32, key: u16) -> bool;
}

fn application(budget: &Budget<'_>, pid: u32) -> Result<Element, String> {
    Ok(Element(budget.element(
        OwnedCf::new(
            unsafe { ax_api::AXUIElementCreateApplication(pid as i32) },
            "AX application unavailable for keyboard input",
        )?,
        pid,
    )?))
}

fn window_focused(
    budget: &Budget<'_>,
    app: &Element,
    root: &Element,
    pid: u32,
) -> Result<bool, String> {
    if budget.flag(app.raw(), "AXFrontmost")? != Some(true) {
        return Ok(false);
    }
    let Some(window) = budget.optional(app.raw(), "AXFocusedWindow")? else {
        return Ok(false);
    };
    let window = Element(budget.element(window, pid)?);
    Ok(window.same(root))
}

pub(super) fn focused(
    budget: &Budget<'_>,
    root: &Element,
    node: &Element,
    pid: u32,
) -> Result<bool, String> {
    if budget.flag(node.raw(), "AXFocused")? != Some(true) {
        return Ok(false);
    }
    let app = application(budget, pid)?;
    if !window_focused(budget, &app, root, pid)? {
        return Ok(false);
    }
    let Some(focus) = budget.optional(app.raw(), "AXFocusedUIElement")? else {
        return Ok(false);
    };
    let same = Element(budget.element(focus, pid)?).same(node);
    // A focused-element read may itself race a window/frontmost transition.
    Ok(same && window_focused(budget, &app, root, pid)?)
}

fn hardware_clear(key: u16, admission: bool) -> Result<(), String> {
    // Do not borrow a user's held modifier/key, or release it as our own. The
    // private source and explicit zero flags do not authorize ignoring HID state.
    if unsafe { CGEventSourceFlagsState(K_CG_EVENT_SOURCE_STATE_HID) } & MODIFIERS != 0
        || unsafe { CGEventSourceKeyState(K_CG_EVENT_SOURCE_STATE_HID, key) }
        || unsafe { CGEventSourceFlagsState(0) } & MODIFIERS != 0
        || (admission && unsafe { CGEventSourceKeyState(0, key) })
    {
        return Err("macOS keyboard is in use; release held keys before retrying".into());
    }
    Ok(())
}

pub(super) struct PreparedKey {
    _source: OwnedCf,
    down: OwnedCf,
    up: OwnedCf,
    code: u16,
}
impl PreparedKey {
    pub(super) fn new(parameters: &serde_json::Value) -> Result<Self, String> {
        let parameters = parameters.as_object().ok_or("key parameters required")?;
        let name = parameters
            .get("key")
            .and_then(serde_json::Value::as_str)
            .ok_or("key required")?;
        if parameters.len() != 1 || name.len() > super::super::protocol::KEY_MAX_LEN {
            return Err("invalid key parameters".into());
        }
        // HIToolbox/Events.h non-layout-dependent navigation keycodes only.
        let code = match super::super::protocol::normalize_key(name)? {
            "enter" => 36,
            "tab" => 48,
            "space" => 49,
            "backspace" => 51,
            "escape" => 53,
            "home" => 115,
            "pageup" => 116,
            "delete" => 117,
            "end" => 119,
            "pagedown" => 121,
            "left" => 123,
            "right" => 124,
            "down" => 125,
            "up" => 126,
            _ => return Err("unsupported macOS key".into()),
        };
        hardware_clear(code, true)?;
        let source = OwnedCf::new(
            unsafe { CGEventSourceCreate(PRIVATE_SOURCE) },
            "keyboard source allocation failed",
        )?;
        let down = OwnedCf::new(
            unsafe { CGEventCreateKeyboardEvent(source.0, code, true) },
            "key-down allocation failed",
        )?;
        let up = OwnedCf::new(
            unsafe { CGEventCreateKeyboardEvent(source.0, code, false) },
            "key-up allocation failed",
        )?;
        unsafe {
            CGEventSetFlags(down.0 as CgEventRef, 0);
            CGEventSetFlags(up.0 as CgEventRef, 0);
        }
        Ok(Self {
            _source: source,
            down,
            up,
            code,
        })
    }

    pub(super) fn dispatch(
        &self,
        adapter: &MacosAdapter,
        req: &DispatchRequest,
        observation: &Arc<ModelObservation<ax_tree::Tree>>,
        root: &Element,
        owner: &mut NativeActionGuard<'_>,
        before: impl FnOnce() -> Result<(), String>,
    ) -> Result<(), String> {
        let frame = observation.frame();
        hardware_clear(self.code, true)?;
        before()?;
        req.cancellation.check()?;
        // Native getters in the final AX proof may race user input too.
        hardware_clear(self.code, true)?;
        if !adapter.input_available() {
            return Err("macOS keyboard permission revoked before dispatch".into());
        }
        adapter
            .snapshots
            .require_current(&req.run_id, &req.target_id, observation)?;
        req.cancellation.check()?;
        unsafe { CGEventPostToPid(frame.target.pid as i32, self.down.0 as CgEventRef) };
        // Tab/Enter may naturally move focus to another control. Cleanup is
        // allowed after cancellation/retirement, but only inside the ORIGINAL
        // retained window/process. Never redirect key-up to a new dialog/PID.
        if let Err(error) = self.validate_release(adapter, frame, root) {
            owner.retain_until_native_recovery();
            return Err(format!("key release unconfirmed: {error}"));
        }
        unsafe { CGEventPostToPid(frame.target.pid as i32, self.up.0 as CgEventRef) };
        adapter
            .snapshots
            .require_current(&req.run_id, &req.target_id, observation)?;
        req.cancellation.check()
    }

    fn validate_release(
        &self,
        adapter: &MacosAdapter,
        frame: &CapturedFrame,
        root: &Element,
    ) -> Result<(), String> {
        if !adapter.input_available() {
            return Err("macOS keyboard permission revoked".into());
        }
        identity::validate(frame.target)?;
        if window_bounds(frame.target)? != frame.bounds
            || capture::display_revision()? != frame.display_revision
        {
            return Err("keyboard target geometry changed".into());
        }
        let cancel = Default::default(); // Cleanup only; never admits a new action.
        let budget = Budget::new(&cancel);
        let app = application(&budget, frame.target.pid)?;
        if budget.role(root.raw())? != "AXWindow"
            || budget.bounds(root.raw())? != frame.bounds
            || !window_focused(&budget, &app, root, frame.target.pid)?
        {
            return Err("original keyboard window is no longer focused/live".into());
        }
        // The combined-session key table can contain OUR queued down. It is
        // not an ownership oracle and cannot be used to forbid the matching up.
        // Physical user-held state is separate. Another synthetic sender racing
        // this pair cannot be distinguished by Quartz; no atomicity is claimed.
        hardware_clear(self.code, false)?;
        identity::validate(frame.target)?;
        if !adapter.input_available() {
            return Err("macOS keyboard permission revoked".into());
        }
        budget.check()
    }
}
