//! Accessibility actions on the `gcu-macos-ax` thread.
//!
//! Every call sets `AXUIElementSetMessagingTimeout` before it touches the
//! element. Reads report `maybe_executed = false`. Writes report
//! `maybe_executed = true` when the outcome is uncertain. `AXRaise`,
//! `AXFocus`, and setting `AXFocused` are refused here as well as in the
//! portable dispatcher.

use accessibility_sys::{
    kAXErrorAPIDisabled, kAXErrorActionUnsupported, kAXErrorAttributeUnsupported,
    kAXErrorCannotComplete, kAXErrorFailure, kAXErrorInvalidUIElement, kAXErrorNoValue,
    kAXErrorNotImplemented, kAXErrorParameterizedAttributeUnsupported, kAXErrorSuccess,
    kAXParentAttribute, kAXPositionAttribute, kAXRoleAttribute, kAXSelectedChildrenAttribute,
    kAXSelectedTextRangeAttribute, kAXSizeAttribute, kAXTitleAttribute, kAXValueTypeCFRange,
    kAXValueTypeCGPoint, kAXValueTypeCGSize, AXUIElementCopyActionNames,
    AXUIElementCopyAttributeValue, AXUIElementGetTypeID, AXUIElementIsAttributeSettable,
    AXUIElementPerformAction, AXUIElementRef, AXUIElementSetAttributeValue, AXValueCreate,
    AXValueGetTypeID, AXValueGetValue, AXValueRef,
};
use core_foundation::array::{CFArray, CFArrayRef};
use core_foundation::base::{CFGetTypeID, CFRange, CFType, CFTypeRef, TCFType};
use core_foundation::number::CFNumber;
use core_foundation::string::CFString;
use gcu_core::driver::{DriverError, Result};
use gcu_core::model::{Rect, TextRange};

use super::world::{self, World};
use crate::ax::source::{AxCall, AxIdentity};

pub(super) fn action_names(world: &mut World, id: u64) -> Result<Vec<String>> {
    let element = prepare(world, id)?;
    let mut names: CFArrayRef = std::ptr::null();
    // SAFETY: element is a live snapshot element. On success `names` is +1.
    let status = unsafe { AXUIElementCopyActionNames(element, &mut names) };
    if status == kAXErrorSuccess {
        if names.is_null() {
            return Ok(Vec::new());
        }
        // SAFETY: CopyActionNames returned +1. Create-rule takes that retain.
        let owned = unsafe { CFArray::<CFType>::wrap_under_create_rule(names) };
        let mut out = Vec::new();
        for index in 0..owned.len() {
            if let Some(item) = owned.get(index) {
                if let Some(name) = string_of(item.as_CFTypeRef()) {
                    out.push(name);
                }
            }
        }
        return Ok(out);
    }
    if !names.is_null() {
        world::release(names as CFTypeRef);
    }
    if unsupported(status) {
        // The element has no action list. That is a definite miss, so the
        // portable fallback may try the next step.
        return Ok(Vec::new());
    }
    Err(read_error(status))
}

pub(super) fn invoke(world: &mut World, id: u64, name: &str) -> Result<AxCall> {
    if focus_substitute(name) {
        return Err(DriverError::ElementNotActionable);
    }
    let element = prepare(world, id)?;
    let action = CFString::new(name);
    // SAFETY: element is live. `action` outlives the call. AX copies the name.
    let status = unsafe { AXUIElementPerformAction(element, action.as_concrete_TypeRef()) };
    classify_mutation(status)
}

pub(super) fn attribute_settable(
    world: &mut World,
    id: u64,
    attribute: &str,
) -> Result<Option<bool>> {
    let element = prepare(world, id)?;
    let mut flag: u8 = 0;
    let name = CFString::new(attribute);
    // SAFETY: element is live. `flag` is an out-parameter.
    let status =
        unsafe { AXUIElementIsAttributeSettable(element, name.as_concrete_TypeRef(), &mut flag) };
    if status == kAXErrorSuccess {
        return Ok(Some(flag != 0));
    }
    if unsupported(status) {
        return Ok(None);
    }
    Err(read_error(status))
}

pub(super) fn set_string(
    world: &mut World,
    id: u64,
    attribute: &str,
    value: &str,
) -> Result<AxCall> {
    if focus_substitute(attribute) {
        return Err(DriverError::ElementNotActionable);
    }
    let element = prepare(world, id)?;
    let text = CFString::new(value);
    set_attribute(element, attribute, text.as_CFTypeRef())
}

pub(super) fn set_number(
    world: &mut World,
    id: u64,
    attribute: &str,
    value: f64,
) -> Result<AxCall> {
    if focus_substitute(attribute) {
        return Err(DriverError::ElementNotActionable);
    }
    let element = prepare(world, id)?;
    let number = CFNumber::from(value);
    set_attribute(element, attribute, number.as_CFTypeRef())
}

pub(super) fn set_text_range(world: &mut World, id: u64, range: TextRange) -> Result<AxCall> {
    let Ok(location) = isize::try_from(range.start) else {
        return Err(DriverError::InvalidRequest);
    };
    let Ok(length) = isize::try_from(range.length) else {
        return Err(DriverError::InvalidRequest);
    };
    if location.checked_add(length).is_none() {
        return Err(DriverError::InvalidRequest);
    }
    let element = prepare(world, id)?;
    let native = CFRange { location, length };
    // SAFETY: AXValueCreate copies the range bytes. The result is +1.
    let created = unsafe { AXValueCreate(kAXValueTypeCFRange, std::ptr::addr_of!(native).cast()) };
    if created.is_null() {
        return Err(DriverError::Native {
            message: "accessibility text range was not created".into(),
            maybe_executed: false,
        });
    }
    let result = set_attribute(element, kAXSelectedTextRangeAttribute, created as CFTypeRef);
    world::release(created as CFTypeRef);
    result
}

pub(super) fn select_child(world: &mut World, parent: u64, child: u64) -> Result<AxCall> {
    let parent = prepare(world, parent)?;
    let Some(child) = world.element(child) else {
        return Err(DriverError::StaleElement);
    };
    // SAFETY: child is a live element in the map. Get-rule retains a wrapper.
    let wrapped = unsafe { CFType::wrap_under_get_rule(child as CFTypeRef) };
    let array = CFArray::from_CFTypes(&[wrapped]);
    set_attribute(parent, kAXSelectedChildrenAttribute, array.as_CFTypeRef())
}

pub(super) fn parent(world: &mut World, id: u64) -> Result<Option<u64>> {
    let element = prepare(world, id)?;
    let Some(copied) = copy_attribute(element, kAXParentAttribute)? else {
        return Ok(None);
    };
    if is_element(copied) {
        return Ok(Some(world.insert(copied as AXUIElementRef)));
    }
    world::release(copied);
    Ok(None)
}

pub(super) fn fresh_identity(world: &mut World, id: u64) -> Result<AxIdentity> {
    let element = prepare(world, id)?;
    let role = match copy_attribute(element, kAXRoleAttribute)? {
        Some(ptr) => {
            let role = string_of(ptr).unwrap_or_default();
            world::release(ptr);
            role
        }
        None => String::new(),
    };
    let title = match copy_attribute(element, kAXTitleAttribute)? {
        Some(ptr) => {
            let title = string_of(ptr);
            world::release(ptr);
            title
        }
        None => None,
    };
    Ok(AxIdentity {
        role,
        title,
        frame: copy_frame(element)?,
    })
}

fn prepare(world: &mut World, id: u64) -> Result<AXUIElementRef> {
    let Some(element) = world.element(id) else {
        return Err(DriverError::StaleElement);
    };
    world::set_messaging_timeout(element)?;
    Ok(element)
}

fn set_attribute(element: AXUIElementRef, attribute: &str, value: CFTypeRef) -> Result<AxCall> {
    let name = CFString::new(attribute);
    // SAFETY: element and value are live. AX copies the value and does not take it.
    let status =
        unsafe { AXUIElementSetAttributeValue(element, name.as_concrete_TypeRef(), value) };
    classify_mutation(status)
}

fn copy_attribute(element: AXUIElementRef, name: &str) -> Result<Option<CFTypeRef>> {
    let mut result: CFTypeRef = std::ptr::null();
    let cf = CFString::new(name);
    // SAFETY: element is live. On success `result` is +1 and the caller releases it.
    let status =
        unsafe { AXUIElementCopyAttributeValue(element, cf.as_concrete_TypeRef(), &mut result) };
    if status == kAXErrorSuccess {
        return Ok((!result.is_null()).then_some(result));
    }
    if !result.is_null() {
        world::release(result);
    }
    if unsupported(status) {
        return Ok(None);
    }
    Err(read_error(status))
}

fn copy_frame(element: AXUIElementRef) -> Result<Option<Rect>> {
    let origin = take_pair(
        copy_attribute(element, kAXPositionAttribute)?,
        kAXValueTypeCGPoint,
    )?;
    let size = take_pair(
        copy_attribute(element, kAXSizeAttribute)?,
        kAXValueTypeCGSize,
    )?;
    Ok(match (origin, size) {
        (Some((x, y)), Some((w, h))) => Some(Rect { x, y, w, h }),
        _ => None,
    })
}

fn take_pair(ptr: Option<CFTypeRef>, kind: u32) -> Result<Option<(f64, f64)>> {
    let Some(ptr) = ptr else {
        return Ok(None);
    };
    let pair = ax_pair(ptr, kind);
    world::release(ptr);
    Ok(pair)
}

fn classify_mutation(status: i32) -> Result<AxCall> {
    if status == kAXErrorSuccess {
        return Ok(AxCall::Performed);
    }
    if unsupported(status) {
        return Ok(AxCall::Unsupported);
    }
    if status == kAXErrorCannotComplete || status == kAXErrorFailure {
        return Err(DriverError::Native {
            message: "accessibility action outcome is unknown".into(),
            maybe_executed: true,
        });
    }
    Err(if let Err(error) = classify_read(status) {
        error
    } else {
        DriverError::Native {
            message: "accessibility action failed".into(),
            maybe_executed: true,
        }
    })
}

fn read_error(status: i32) -> DriverError {
    match classify_read(status) {
        Err(error) => error,
        Ok(()) => DriverError::Native {
            message: "accessibility call failed".into(),
            maybe_executed: false,
        },
    }
}

fn classify_read(status: i32) -> Result<()> {
    if status == kAXErrorAPIDisabled {
        Err(DriverError::PermissionMissing {
            permission: "accessibility".into(),
        })
    } else if status == kAXErrorInvalidUIElement {
        Err(DriverError::StaleElement)
    } else if status == kAXErrorSuccess {
        Ok(())
    } else {
        Err(DriverError::Native {
            message: "accessibility call failed".into(),
            maybe_executed: false,
        })
    }
}

fn unsupported(status: i32) -> bool {
    status == kAXErrorActionUnsupported
        || status == kAXErrorAttributeUnsupported
        || status == kAXErrorNotImplemented
        || status == kAXErrorNoValue
        || status == kAXErrorParameterizedAttributeUnsupported
}

fn focus_substitute(name: &str) -> bool {
    matches!(name, "AXRaise" | "AXFocus" | "AXFocused")
}

fn string_of(ptr: CFTypeRef) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    // SAFETY: ptr is live. Get-rule retains a wrapper that Drop releases.
    let typed = unsafe { CFType::wrap_under_get_rule(ptr) };
    if !typed.instance_of::<CFString>() {
        return None;
    }
    // SAFETY: the type id matches CFString.
    Some(unsafe { CFString::wrap_under_get_rule(ptr as _) }.to_string())
}

fn is_element(ptr: CFTypeRef) -> bool {
    // SAFETY: both type-id calls are pure queries.
    !ptr.is_null() && unsafe { CFGetTypeID(ptr) } == unsafe { AXUIElementGetTypeID() }
}

fn ax_pair(ptr: CFTypeRef, kind: u32) -> Option<(f64, f64)> {
    if !is_ax_value(ptr) {
        return None;
    }
    #[repr(C)]
    struct Pair {
        a: f64,
        b: f64,
    }
    let mut pair = Pair { a: 0.0, b: 0.0 };
    // SAFETY: ptr is an AXValue. Pair matches CGPoint and CGSize.
    let ok =
        unsafe { AXValueGetValue(ptr as AXValueRef, kind, std::ptr::addr_of_mut!(pair).cast()) };
    ok.then_some((pair.a, pair.b))
}

fn is_ax_value(ptr: CFTypeRef) -> bool {
    // SAFETY: both type-id calls are pure queries.
    !ptr.is_null() && unsafe { CFGetTypeID(ptr) } == unsafe { AXValueGetTypeID() }
}
