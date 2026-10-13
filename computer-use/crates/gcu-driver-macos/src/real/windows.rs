//! `CGWindowListCopyWindowInfo` for on-screen layer-0 windows.
//!
//! Space membership is not available from this public call. The field stays
//! `None` instead of guessing. Scale stays `1.0` until display capture (P1-04).

use crate::api::CgWindow;
use gcu_core::driver::Result;
use gcu_protocol::Rect;
use objc2_core_foundation::{CFDictionary, CFNumber, CFString, CFType, CGPoint, CGRect, CGSize};
use objc2_core_graphics::{
    kCGWindowAlpha, kCGWindowBounds, kCGWindowLayer, kCGWindowName, kCGWindowNumber,
    kCGWindowOwnerName, kCGWindowOwnerPID, CGRectMakeWithDictionaryRepresentation,
    CGWindowListCopyWindowInfo, CGWindowListOption,
};
use std::ptr;

pub(super) fn windows() -> Result<Vec<CgWindow>> {
    let option =
        CGWindowListOption::OptionOnScreenOnly | CGWindowListOption::ExcludeDesktopElements;
    let Some(list) = CGWindowListCopyWindowInfo(option, 0) else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for index in 0..list.len() {
        // SAFETY: `index` is below `len`. The returned pointer is +0 and stays
        // valid while `list` is alive. A null slot is skipped.
        let value = unsafe { list.value_at_index(index as isize) };
        if value.is_null() {
            continue;
        }
        // SAFETY: window-list values are CFDictionaries. The list keeps the
        // dictionary alive for this iteration.
        let dict = unsafe { &*value.cast::<CFDictionary>() };
        if let Some(window) = one_window(dict) {
            out.push(window);
        }
    }
    Ok(out)
}

fn one_window(dict: &CFDictionary) -> Option<CgWindow> {
    let window_id = u32::try_from(dict_i64(dict, unsafe { kCGWindowNumber })?).ok()?;
    if window_id == 0 {
        return None;
    }
    let pid = u32::try_from(dict_i64(dict, unsafe { kCGWindowOwnerPID })?).ok()?;
    if pid == 0 {
        return None;
    }
    let layer = dict_i64(dict, unsafe { kCGWindowLayer })
        .and_then(|value| i32::try_from(value).ok())
        .unwrap_or(0);
    let alpha = dict_f64(dict, unsafe { kCGWindowAlpha }).unwrap_or(1.0);
    let title = dict_string(dict, unsafe { kCGWindowName });
    let owner_name = dict_string(dict, unsafe { kCGWindowOwnerName });
    let frame = dict_bounds(dict).unwrap_or(Rect {
        x: 0.0,
        y: 0.0,
        w: 0.0,
        h: 0.0,
    });
    Some(CgWindow {
        window_id,
        pid,
        layer,
        title,
        owner_name,
        frame,
        alpha,
        scale: 1.0,
        on_current_desktop: None,
    })
}

fn dict_string(dict: &CFDictionary, key: &'static CFString) -> String {
    lookup(dict, key)
        .and_then(|value| value.downcast_ref::<CFString>())
        .map(|text| text.to_string())
        .unwrap_or_default()
}

fn dict_i64(dict: &CFDictionary, key: &'static CFString) -> Option<i64> {
    lookup(dict, key)?.downcast_ref::<CFNumber>()?.as_i64()
}

fn dict_f64(dict: &CFDictionary, key: &'static CFString) -> Option<f64> {
    lookup(dict, key)?.downcast_ref::<CFNumber>()?.as_f64()
}

fn dict_bounds(dict: &CFDictionary) -> Option<Rect> {
    let bounds = lookup(dict, unsafe { kCGWindowBounds })?.downcast_ref::<CFDictionary>()?;
    let mut rect = CGRect {
        origin: CGPoint { x: 0.0, y: 0.0 },
        size: CGSize {
            width: 0.0,
            height: 0.0,
        },
    };
    // SAFETY: `bounds` is a live dictionary and `rect` is a live out-parameter.
    let ok =
        unsafe { CGRectMakeWithDictionaryRepresentation(Some(bounds), ptr::addr_of_mut!(rect)) };
    if !ok {
        return None;
    }
    Some(Rect {
        x: rect.origin.x,
        y: rect.origin.y,
        w: rect.size.width,
        h: rect.size.height,
    })
}

fn lookup<'a>(dict: &'a CFDictionary, key: &CFString) -> Option<&'a CFType> {
    // SAFETY: `key` is a process-lifetime CoreGraphics string. `value` returns
    // +0; the dictionary keeps the object alive for `dict`'s borrow.
    let value = unsafe { dict.value(std::ptr::from_ref(key).cast()) };
    if value.is_null() {
        None
    } else {
        Some(unsafe { &*value.cast::<CFType>() })
    }
}
