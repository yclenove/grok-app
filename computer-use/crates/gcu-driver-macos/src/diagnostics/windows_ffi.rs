//! Raw window facts for one disposable fixture PID, without changing discovery.

use crate::{api::MacApi, MacosDriver, RealMacApi};
use accessibility_sys::{kAXWindowsAttribute, AXUIElementCopyAttributeValue};
use core_foundation::{
    array::CFArray,
    base::{CFType, CFTypeRef, TCFType},
    string::CFString,
};
use gcu_core::driver::{DriverError, Result};
use objc2_core_foundation::{CFDictionary, CFNumber, CFString as ObjString, CGRect};
use objc2_core_graphics::{
    kCGWindowAlpha, kCGWindowBounds, kCGWindowIsOnscreen, kCGWindowLayer, kCGWindowName,
    kCGWindowNumber, kCGWindowOwnerPID, CGRectMakeWithDictionaryRepresentation,
    CGWindowListCopyWindowInfo, CGWindowListOption,
};
use serde_json::{json, Value};
use std::{ptr, time::Instant};

/// Compare all CG rows (including hidden/offscreen) with AXWindows for `pid`.
/// Reads only; AX runs on the driver's worker with 1s messaging / 1.5s waiter
/// deadlines. Never invents window ids or changes the driver's window gate.
pub async fn fixture_window_report(driver: &MacosDriver, pid: u32) -> Result<Value> {
    let started = Instant::now();
    let cg = tokio::time::timeout(
        gcu_core::driver::DRIVER_DEADLINE,
        tokio::task::spawn_blocking(move || {
            let api = RealMacApi::new();
            json!({"all": all_cg(pid), "frontmost": match api.frontmost_pid() {
                Ok(pid) => json!({"pid": pid}), Err(error) => json!({"error": error.to_string()}) },
            "running": match api.running_apps() { Ok(apps) => json!(apps.into_iter()
                .filter(|a| a.pid == pid).map(|a| json!({"pid": a.pid,
                    "bundle_path": a.bundle_path, "hidden": a.hidden})).collect::<Vec<_>>()),
                Err(error) => json!({"error": error.to_string()}) }})
        }),
    )
    .await
    .map_err(|_| DriverError::Timeout {
        maybe_executed: false,
    })?
    .map_err(|_| DriverError::Crashed {
        maybe_executed: false,
    })?;
    let thread = driver
        .ax
        .as_ref()
        .ok_or(DriverError::CapabilityUnavailable)?;
    let ax = thread
        .call(crate::real::AX_CALL_TIMEOUT, false, move |world| {
            let app =
                world.application(i32::try_from(pid).map_err(|_| DriverError::InvalidRequest)?)?;
            let name = CFString::new(kAXWindowsAttribute);
            let mut value: CFTypeRef = ptr::null();
            // SAFETY: app is owned by this AX worker; name and the out-pointer live
            // through the call. The copied object is +1 and consumed below.
            let status = unsafe {
                super::ax_call(
                    "AXUIElementCopyAttributeValue",
                    app as usize,
                    kAXWindowsAttribute,
                    || AXUIElementCopyAttributeValue(app, name.as_concrete_TypeRef(), &mut value),
                )
            };
            let count = if value.is_null() {
                None
            } else {
                // SAFETY: take exactly the Copy retain; type-check before casting.
                let owned = unsafe { CFType::wrap_under_create_rule(value) };
                if owned.instance_of::<CFArray>() {
                    // SAFETY: type id is CFArray; get-rule retains for this scope.
                    Some(unsafe { CFArray::<CFType>::wrap_under_get_rule(value.cast()) }.len())
                } else {
                    None
                }
            };
            Ok(json!({"AXWindows_status": status, "AXWindows_count": count}))
        })
        .await;
    Ok(json!({"kind": "WINDOW_NATIVE", "pid": pid, "cg": cg,
        "ax": match ax { Ok(value) => value, Err(error) => json!({"error": error.to_string()}) },
        "elapsed_ms": started.elapsed().as_secs_f64() * 1000.0,
        "driver_rule": "CG on-screen, layer 0, alpha > .01, size >= 2; AXMain then largest; no frontmost requirement"}))
}

fn all_cg(pid: u32) -> Value {
    let Some(list) = CGWindowListCopyWindowInfo(CGWindowListOption::ExcludeDesktopElements, 0)
    else {
        return json!({"error": "CGWindowListCopyWindowInfo returned null"});
    };
    let mut rows = Vec::new();
    for index in 0..list.len() {
        // SAFETY: index is in bounds; the list owns each dictionary until return.
        let raw = unsafe { list.value_at_index(index as isize) };
        if raw.is_null() {
            continue;
        }
        // SAFETY: CGWindowListCopyWindowInfo returns an array of dictionaries.
        let dict = unsafe { &*raw.cast::<CFDictionary>() };
        // SAFETY: CG window keys are process-lifetime CFStrings.
        let owner = number(dict, unsafe { kCGWindowOwnerPID });
        if owner != Some(i64::from(pid)) {
            continue;
        }
        // SAFETY: all keys below are process-lifetime CG constants.
        let (id, layer, alpha, onscreen, title, bounds) = unsafe {
            (
                number(dict, kCGWindowNumber),
                number(dict, kCGWindowLayer),
                get(dict, kCGWindowAlpha)
                    .and_then(|v| v.downcast_ref::<CFNumber>())
                    .and_then(|v| v.as_f64()),
                get(dict, kCGWindowIsOnscreen)
                    .and_then(|v| v.downcast_ref::<objc2_core_foundation::CFBoolean>())
                    .map(|v| v.value()),
                get(dict, kCGWindowName)
                    .and_then(|v| v.downcast_ref::<ObjString>())
                    .map(|v| v.to_string()),
                get(dict, kCGWindowBounds).and_then(|v| v.downcast_ref::<CFDictionary>()),
            )
        };
        let mut rect = CGRect::default();
        // SAFETY: bounds is borrowed from the live dictionary; rect is writable.
        let frame = bounds
            .filter(|b| unsafe { CGRectMakeWithDictionaryRepresentation(Some(b), &mut rect) })
            .map(|_| {
                json!({"x": rect.origin.x, "y": rect.origin.y,
                "w": rect.size.width, "h": rect.size.height})
            });
        rows.push(
            json!({"cg_id": id, "pid": pid, "layer": layer, "alpha": alpha,
            "onscreen": onscreen, "title": title, "frame": frame}),
        );
    }
    json!(rows)
}

fn number(dict: &CFDictionary, key: &ObjString) -> Option<i64> {
    get(dict, key)?.downcast_ref::<CFNumber>()?.as_i64()
}

fn get<'a>(dict: &'a CFDictionary, key: &ObjString) -> Option<&'a objc2_core_foundation::CFType> {
    // SAFETY: key is a CFString; +0 value is kept alive by dict for 'a.
    let value = unsafe { dict.value(ptr::from_ref(key).cast()) };
    // SAFETY: a nonnull dictionary value is a CFType owned by dict.
    (!value.is_null()).then(|| unsafe { &*value.cast::<objc2_core_foundation::CFType>() })
}
