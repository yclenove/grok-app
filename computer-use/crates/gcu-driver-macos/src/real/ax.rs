//! Accessibility windows and non-activating raise.
//!
//! Tree walks, batch attribute copies, and `RealAxSource` live in [`super::reader`]
//! so this file stays the window-list FFI.
//!
//! Trust is checked with `kAXTrustedCheckOptionPrompt = false`. This module
//! never calls the prompting variant and never clicks a system dialog.
//! `kAXErrorAPIDisabled` is reported as a missing accessibility permission.
//! Any other per-window copy failure is skipped.

use crate::api::AxWindow;
use accessibility_sys::{
    kAXErrorAPIDisabled, kAXErrorSuccess, kAXMainAttribute, kAXMinimizedAttribute,
    kAXPositionAttribute, kAXRaiseAction, kAXSizeAttribute, kAXTitleAttribute,
    kAXTrustedCheckOptionPrompt, kAXValueTypeCGPoint, kAXValueTypeCGSize, kAXWindowsAttribute,
    AXIsProcessTrustedWithOptions, AXUIElementCopyAttributeValue, AXUIElementCreateApplication,
    AXUIElementPerformAction, AXValueGetValue, AXValueRef,
};
use core_foundation::base::CFRelease;
use core_foundation::{
    array::{CFArray, CFArrayRef},
    base::{CFTypeRef, TCFType},
    boolean::CFBoolean,
    dictionary::CFDictionary,
    string::CFString,
};
use gcu_core::driver::{DriverError, Result};
use gcu_protocol::Rect;
use std::ptr;

#[repr(C)]
struct CgPoint {
    x: f64,
    y: f64,
}

#[repr(C)]
struct CgSize {
    width: f64,
    height: f64,
}

pub(super) fn accessibility_trusted() -> Result<bool> {
    // SAFETY: the prompt key is a process-lifetime CFString. Get-rule retains a
    // copy for the dictionary; the static itself is not released.
    let key = unsafe { CFString::wrap_under_get_rule(kAXTrustedCheckOptionPrompt) };
    let value = CFBoolean::false_value();
    let dict = CFDictionary::from_CFType_pairs(&[(key.as_CFType(), value.as_CFType())]);
    // SAFETY: the dictionary lives for this call. The prompt value is false, so
    // the function does not present a system dialog. It does not retain the dict.
    Ok(unsafe {
        crate::diagnostics::ax_call("AXIsProcessTrustedWithOptions", 0, "prompt=false", || {
            AXIsProcessTrustedWithOptions(dict.as_concrete_TypeRef())
        })
    })
}

pub(super) fn ax_windows(pid: u32) -> Result<Vec<AxWindow>> {
    let pid = i32::try_from(pid).map_err(|_| DriverError::InvalidRequest)?;
    // SAFETY: the pid names a process. The create rule returns +1, released by
    // the guard at the end of this function.
    let app = unsafe {
        crate::diagnostics::ax_call("AXUIElementCreateApplication", 0, "", || {
            AXUIElementCreateApplication(pid)
        })
    };
    if app.is_null() {
        return Ok(Vec::new());
    }
    let copied = copy_attribute(app, kAXWindowsAttribute);
    let windows = match copied {
        Ok(value) => read_windows(value),
        Err(error) => {
            release(app as CFTypeRef);
            return Err(error);
        }
    };
    release(app as CFTypeRef);
    windows
}

pub(super) fn ax_raise(pid: u32, index: u32) -> Result<()> {
    let pid = i32::try_from(pid).map_err(|_| DriverError::InvalidRequest)?;
    // SAFETY: same create-rule contract as [`ax_windows`].
    let app = unsafe {
        crate::diagnostics::ax_call("AXUIElementCreateApplication", 0, "", || {
            AXUIElementCreateApplication(pid)
        })
    };
    if app.is_null() {
        return Err(DriverError::WindowGone);
    }
    let result = (|| {
        let value = copy_attribute(app, kAXWindowsAttribute)?;
        let element = window_at(value, index).ok_or(DriverError::WindowGone)?;
        let action = CFString::new(kAXRaiseAction);
        // SAFETY: `element` is an AX window from the array we still hold, and
        // `action` is the AXRaise constant. This does not activate the app.
        let status = unsafe {
            crate::diagnostics::ax_call(
                "AXUIElementPerformAction",
                (element) as usize,
                kAXRaiseAction,
                || AXUIElementPerformAction(element, action.as_concrete_TypeRef()),
            )
        };
        release(value);
        if status == kAXErrorAPIDisabled {
            Err(DriverError::PermissionMissing {
                permission: "accessibility".into(),
            })
        } else if status == kAXErrorSuccess {
            Ok(())
        } else {
            Err(DriverError::WindowMinimized)
        }
    })();
    release(app as CFTypeRef);
    result
}

fn read_windows(value: CFTypeRef) -> Result<Vec<AxWindow>> {
    let array = as_array(value).ok_or(DriverError::Native {
        message: "accessibility windows were not an array".into(),
        maybe_executed: false,
    })?;
    let mut out = Vec::new();
    for index in 0..array.len() {
        let Some(element) = array.get(index) else {
            continue;
        };
        let element = element.as_CFTypeRef() as accessibility_sys::AXUIElementRef;
        out.push(AxWindow {
            index: index as u32,
            title: copy_string(element, kAXTitleAttribute),
            frame: copy_frame(element),
            minimized: copy_bool(element, kAXMinimizedAttribute),
            main: copy_bool(element, kAXMainAttribute),
        });
    }
    release(value);
    Ok(out)
}

fn window_at(value: CFTypeRef, index: u32) -> Option<accessibility_sys::AXUIElementRef> {
    let array = as_array(value)?;
    let item = array.get(isize::try_from(index).ok()?)?;
    Some(item.as_CFTypeRef() as accessibility_sys::AXUIElementRef)
}

fn copy_frame(element: accessibility_sys::AXUIElementRef) -> Rect {
    let point = copy_value(element, kAXPositionAttribute, kAXValueTypeCGPoint);
    let size = copy_value(element, kAXSizeAttribute, kAXValueTypeCGSize);
    let mut origin = CgPoint { x: 0.0, y: 0.0 };
    let mut extent = CgSize {
        width: 0.0,
        height: 0.0,
    };
    if let Some(value) = point {
        // SAFETY: `value` is an AXValue of point type, and `origin` matches CGPoint.
        unsafe {
            crate::diagnostics::ax_call("AXValueGetValue", (value) as usize, "", || {
                AXValueGetValue(value, kAXValueTypeCGPoint, ptr::addr_of_mut!(origin).cast())
            })
        };
        release(value as CFTypeRef);
    }
    if let Some(value) = size {
        // SAFETY: `value` is an AXValue of size type, and `extent` matches CGSize.
        unsafe {
            crate::diagnostics::ax_call("AXValueGetValue", (value) as usize, "", || {
                AXValueGetValue(value, kAXValueTypeCGSize, ptr::addr_of_mut!(extent).cast())
            })
        };
        release(value as CFTypeRef);
    }
    Rect {
        x: origin.x,
        y: origin.y,
        w: extent.width,
        h: extent.height,
    }
}

fn copy_string(element: accessibility_sys::AXUIElementRef, name: &str) -> String {
    let Ok(value) = copy_attribute(element, name) else {
        return String::new();
    };
    // SAFETY: `copy_attribute` returns a +1 object. Create-rule takes that retain.
    let typed = unsafe { core_foundation::base::CFType::wrap_under_create_rule(value) };
    if !typed.instance_of::<CFString>() {
        return String::new();
    }
    // SAFETY: the type id matches CFString. Get-rule retains; `typed` still owns the original.
    unsafe { CFString::wrap_under_get_rule(typed.as_CFTypeRef() as _) }.to_string()
}

fn copy_bool(element: accessibility_sys::AXUIElementRef, name: &str) -> bool {
    let Ok(value) = copy_attribute(element, name) else {
        return false;
    };
    // SAFETY: `copy_attribute` returns a +1 object.
    let typed = unsafe { core_foundation::base::CFType::wrap_under_create_rule(value) };
    if !typed.instance_of::<CFBoolean>() {
        return false;
    }
    // SAFETY: the type id matches CFBoolean.
    bool::from(unsafe { CFBoolean::wrap_under_get_rule(typed.as_CFTypeRef() as _) })
}

fn copy_value(
    element: accessibility_sys::AXUIElementRef,
    name: &str,
    _kind: u32,
) -> Option<AXValueRef> {
    let value = copy_attribute(element, name).ok()?;
    Some(value as AXValueRef)
}

fn copy_attribute(element: accessibility_sys::AXUIElementRef, name: &str) -> Result<CFTypeRef> {
    let attribute = CFString::new(name);
    let mut value: CFTypeRef = ptr::null();
    // SAFETY: `element` is a live AX object. `value` is an out-parameter. On
    // success it is +1 and the caller releases it.
    let status = unsafe {
        crate::diagnostics::ax_call(
            "AXUIElementCopyAttributeValue",
            (element) as usize,
            name,
            || AXUIElementCopyAttributeValue(element, attribute.as_concrete_TypeRef(), &mut value),
        )
    };
    if status == kAXErrorAPIDisabled {
        return Err(DriverError::PermissionMissing {
            permission: "accessibility".into(),
        });
    }
    if status != kAXErrorSuccess || value.is_null() {
        if !value.is_null() {
            release(value);
        }
        return Err(DriverError::Native {
            message: "accessibility attribute copy failed".into(),
            maybe_executed: false,
        });
    }
    Ok(value)
}

fn as_array(value: CFTypeRef) -> Option<CFArray<core_foundation::base::CFType>> {
    if value.is_null() {
        return None;
    }
    // SAFETY: the caller owns `value` and keeps it alive while the array wrapper
    // is borrowed. Get-rule retains, so the wrapper does not free the caller's copy.
    Some(unsafe {
        CFArray::<core_foundation::base::CFType>::wrap_under_get_rule(value as CFArrayRef)
    })
}

fn release(value: CFTypeRef) {
    if !value.is_null() {
        crate::diagnostics::forget_reference(value as usize);
        // SAFETY: `value` is a +1 CoreFoundation object owned by the caller.
        unsafe { CFRelease(value) };
    }
}
