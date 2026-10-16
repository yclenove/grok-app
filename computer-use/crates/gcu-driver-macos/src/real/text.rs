//! Fresh text focus and append on the dedicated AX thread. No focus changes.
use super::{
    act, apps,
    thread::{AxThread, AX_CALL_TIMEOUT},
    windows,
    world::{self, World},
};
use crate::{
    ax::{sensitive::is_terminal_like, AxHandle},
    input::{
        clock::Clock,
        text::{TextApi, TextFocus},
        InputTarget,
    },
};
use accessibility_sys::{AXUIElementGetPid, AXUIElementRef};
use async_trait::async_trait;
use core_foundation::{
    base::{CFType, TCFType},
    boolean::CFBoolean,
};
use gcu_core::driver::{Cancel, DriverError, Result};
use std::{sync::Arc, time::Duration};

pub(crate) struct RealText {
    thread: Arc<AxThread>,
    clock: Arc<dyn Clock>,
}
impl RealText {
    pub(crate) fn new(thread: Arc<AxThread>, clock: Arc<dyn Clock>) -> Self {
        Self { thread, clock }
    }
}
#[async_trait]
impl TextApi for RealText {
    async fn focus(&self, target: &InputTarget) -> Result<TextFocus> {
        let target = target.clone();
        self.thread
            .call(AX_CALL_TIMEOUT, false, move |world| focus(world, &target))
            .await
    }
    async fn append(&self, target: &InputTarget, expected: &TextFocus, text: &str) -> Result<()> {
        let (target, expected, text) = (target.clone(), expected.clone(), text.to_owned());
        self.thread
            .call_with_liveness(AX_CALL_TIMEOUT, true, move |world, live| {
                let fresh = focus(world, &target)?;
                if fresh.handle != expected.handle
                    || !fresh.editable
                    || fresh.sensitive
                    || fresh.plain_value != expected.plain_value
                {
                    return Err(DriverError::StaleElement);
                }
                let mut value = fresh.plain_value.ok_or(DriverError::NotSettable)?;
                value.push_str(&text);
                if !live() {
                    return Err(DriverError::Cancelled {
                        maybe_executed: false,
                    });
                }
                match act::set_string(world, fresh.handle.id, "AXValue", &value)? {
                    crate::ax::AxCall::Performed => Ok(()),
                    _ => Err(DriverError::NotSettable),
                }
            })
            .await
    }
    async fn paste_observed(
        &self,
        target: &InputTarget,
        expected: &TextFocus,
        text: &str,
        cancel: &Cancel,
    ) -> Result<()> {
        loop {
            if cancel.is_cancelled() {
                return Err(DriverError::Cancelled {
                    maybe_executed: true,
                });
            }
            let fresh = self.focus(target).await?;
            if fresh.handle != expected.handle || !fresh.frontmost {
                return Err(DriverError::Native {
                    message: "paste focus changed after post".into(),
                    maybe_executed: true,
                });
            }
            if let Some(value) = fresh.paste_baseline {
                if Some(&value) != expected.paste_baseline.as_ref() && value.contains(text) {
                    return Ok(());
                }
            }
            self.clock
                .wait_until(self.clock.now() + Duration::from_millis(10))
                .await;
        }
    }
}

fn focus(world: &mut World, target: &InputTarget) -> Result<TextFocus> {
    if !crate::input::ensure_appkit() {
        return Err(DriverError::CapabilityUnavailable);
    }
    let app = world.application(target.pid)?;
    world::set_messaging_timeout(app)?;
    let field = element(app, "AXFocusedUIElement")?.ok_or(DriverError::ElementNotActionable)?;
    let ptr = field.as_CFTypeRef() as AXUIElementRef;
    world::set_messaging_timeout(ptr)?;
    let mut pid = 0;
    // SAFETY: ptr is a live AX element, pid is a writable out-parameter.
    world::ax_status(unsafe { AXUIElementGetPid(ptr, &mut pid) })?;
    if pid != target.pid {
        return Err(DriverError::StaleElement);
    }
    let win = element(ptr, "AXWindow")?.ok_or(DriverError::ElementNotActionable)?;
    let window = win.as_CFTypeRef() as AXUIElementRef;
    world::set_messaging_timeout(window)?;
    let frame = act::copy_frame(window)?.ok_or(DriverError::WindowGone)?;
    // Conservative public AX/CG alignment: reject coincident/ambiguous windows.
    let cg = windows::windows()?;
    let same: Vec<_> = cg
        .iter()
        .filter(|w| w.pid == pid as u32 && w.layer == 0 && close(w.frame, frame))
        .collect();
    if same.len() != 1 || same[0].window_id != target.window_id {
        return Err(DriverError::StaleElement);
    }
    let id = world.intern(world::retain(field.as_CFTypeRef()) as AXUIElementRef);
    let identity = act::fresh_identity(world, id)?;
    let subrole = string(ptr, "AXSubrole")?;
    let sensitive =
        identity.role == "AXSecureTextField" || subrole.as_deref() == Some("AXSecureTextField");
    let mut editable = matches!(
        identity.role.as_str(),
        "AXTextField" | "AXTextArea" | "AXSearchField"
    ) || boolean(ptr, "AXIsEditable")? == Some(true);
    editable &= boolean(ptr, "AXEnabled")? != Some(false);
    let mut high_risk = !matches!(
        identity.role.as_str(),
        "AXTextField" | "AXTextArea" | "AXSearchField"
    );
    let mut ancestor = Some(field.clone());
    let mut reached_window = false;
    for _ in 0..64 {
        let Some(current) = ancestor else {
            break;
        };
        let node = current.as_CFTypeRef() as AXUIElementRef;
        world::set_messaging_timeout(node)?;
        let role = string(node, "AXRole")?.unwrap_or_default();
        let title = string(node, "AXTitle")?;
        let sub = string(node, "AXSubrole")?;
        if is_terminal_like(&role, sub.as_deref(), title.as_deref()) {
            editable = false;
        }
        high_risk |= role == "AXWebArea";
        // SAFETY: current and win own live CF objects.
        if unsafe { core_foundation::base::CFEqual(current.as_CFTypeRef(), win.as_CFTypeRef()) }
            != 0
        {
            reached_window = true;
            break;
        }
        ancestor = element(node, "AXParent")?;
    }
    high_risk |= !reached_window;
    let value = if sensitive {
        None
    } else {
        string(ptr, "AXValue")?.filter(|s| s.len() <= 1024 * 1024)
    };
    let rich = boolean(ptr, "AXIsRichText")?;
    let plain = !sensitive
        && rich != Some(true)
        && !crate::ax::needs_enhanced_accessibility(&target.bundle_id, &target.app_name)
        && (matches!(identity.role.as_str(), "AXTextField" | "AXSearchField")
            || rich == Some(false))
        && act::attribute_settable(world, id, "AXValue")? == Some(true);
    let frontmost_pid = apps::frontmost_pid()?;
    let focused_window = element(app, "AXFocusedWindow")?;
    let frontmost = frontmost_pid == Some(pid as u32)
        && focused_window.is_some_and(|fw| {
            // SAFETY: both wrappers own live AX elements.
            (unsafe { core_foundation::base::CFEqual(fw.as_CFTypeRef(), win.as_CFTypeRef()) }) != 0
        });
    Ok(TextFocus {
        handle: AxHandle {
            pid: pid as u32,
            id,
            identity,
        },
        editable,
        plain_value: if plain { value.clone() } else { None },
        paste_baseline: value,
        sensitive,
        high_risk,
        frontmost,
    })
}

fn read(element: AXUIElementRef, name: &str) -> Result<Option<CFType>> {
    Ok(act::copy_attribute(element, name)?.map(|ptr| {
        // SAFETY: CopyAttributeValue returns +1, consumed by this wrapper.
        unsafe { CFType::wrap_under_create_rule(ptr) }
    }))
}
fn element(parent: AXUIElementRef, name: &str) -> Result<Option<CFType>> {
    Ok(read(parent, name)?.filter(|value| act::is_element(value.as_CFTypeRef())))
}
fn string(element: AXUIElementRef, name: &str) -> Result<Option<String>> {
    Ok(read(element, name)?.and_then(|v| act::string_of(v.as_CFTypeRef())))
}
fn boolean(element: AXUIElementRef, name: &str) -> Result<Option<bool>> {
    Ok(read(element, name)?.and_then(|value| {
        if !value.instance_of::<CFBoolean>() {
            return None;
        }
        // SAFETY: type id matches CFBoolean, get-rule retains the wrapper.
        Some(unsafe { CFBoolean::wrap_under_get_rule(value.as_CFTypeRef() as _) }.into())
    }))
}
fn close(a: gcu_protocol::Rect, b: gcu_protocol::Rect) -> bool {
    [a.x - b.x, a.y - b.y, a.w - b.w, a.h - b.h]
        .iter()
        .all(|v| v.abs() <= 1.0)
}
