//! Owned decoding of cached metadata; never requests a pattern object/current value.
use super::super::cache::{Patterns, Properties};
use gcu_core::model::{NodeValue, Rect};
use windows::Win32::{
    System::Variant::*,
    UI::Accessibility::{IUIAutomationElement, UIA_PROPERTY_ID},
};

pub(super) fn read(element: &IUIAutomationElement) -> Properties {
    let mut r = Reader {
        element,
        partial: false,
    };
    let password = r.boolean(30019);
    // Unknown IsPassword is privacy-critical: do not extract any value.
    if password.is_none() {
        r.partial = true;
    }
    let control_type = r.integer(30003);
    if control_type.is_none() {
        r.partial = true;
    }
    let patterns = Patterns {
        invoke: r.boolean(30031).unwrap_or(false),
        toggle: r.boolean(30041).unwrap_or(false),
        selection_item: r.boolean(30036).unwrap_or(false),
        expand_collapse: r.boolean(30028).unwrap_or(false),
        value: r.boolean(30043).unwrap_or(false),
        range_value: r.boolean(30033).unwrap_or(false),
        scroll: r.boolean(30034).unwrap_or(false),
        scroll_item: r.boolean(30035).unwrap_or(false),
        text: r.boolean(30040).unwrap_or(false),
        legacy: r.boolean(30090).unwrap_or(false),
    };
    let name = r.string(30005);
    let identifier = r.string(30011);
    let class_name = r.string(30012);
    let framework = r.string(30024);
    let aria_role = r.string(30101);
    let help = r.string(30013);
    let enabled = r.required_boolean(30010).unwrap_or(false);
    let focusable = r.required_boolean(30009).unwrap_or(false);
    let focused = r.required_boolean(30008).unwrap_or(false);
    let offscreen = r.required_boolean(30022).unwrap_or(false);
    let required = r.boolean(30025).unwrap_or(false);
    let modal = r.boolean(30077).unwrap_or(false);
    let selected = r.boolean(30079).unwrap_or(false);
    let toggle_state = if patterns.toggle {
        r.integer(30086)
    } else {
        None
    };
    let expand_state = if patterns.expand_collapse {
        r.integer(30070)
    } else {
        None
    };
    let read_only = if patterns.value {
        r.boolean(30046)
    } else if patterns.range_value {
        r.boolean(30048)
    } else {
        None
    };
    let value = if password != Some(false) {
        None
    } else if patterns.value {
        r.string(30045).map(NodeValue::Text)
    } else if patterns.range_value {
        match (r.number(30047), r.number(30049), r.number(30050)) {
            (Some(value), Some(min), Some(max)) => Some(NodeValue::Range { value, min, max }),
            _ => {
                r.partial = true;
                None
            }
        }
    } else {
        None
    };
    // SAFETY: element is a cached live interface owned by this MTA; getter is local.
    let frame = match unsafe { element.CachedBoundingRectangle() } {
        Ok(rect) => Some(Rect {
            x: rect.left as f64,
            y: rect.top as f64,
            w: (rect.right as i64 - rect.left as i64) as f64,
            h: (rect.bottom as i64 - rect.top as i64) as f64,
        }),
        Err(_) => {
            r.partial = true;
            None
        }
    };
    let runtime_id = r.runtime_id().unwrap_or_default();
    Properties {
        runtime_id,
        control_type: control_type.unwrap_or(0),
        aria_role,
        name,
        identifier,
        class_name,
        framework,
        help,
        frame,
        enabled,
        focusable,
        focused,
        offscreen,
        password: password.unwrap_or(true),
        required,
        modal,
        selected,
        toggle_state,
        expand_state,
        read_only,
        value,
        patterns,
        partial: r.partial,
    }
}
struct Reader<'a> {
    element: &'a IUIAutomationElement,
    partial: bool,
}
impl Reader<'_> {
    fn runtime_id(&mut self) -> Option<Vec<i32>> {
        use windows::Win32::System::Ole::{
            SafeArrayGetDim, SafeArrayGetElement, SafeArrayGetLBound, SafeArrayGetUBound,
        };
        let v = self.get(30000, VARENUM(VT_ARRAY.0 | VT_I4.0))?;
        // SAFETY: get checked VT_ARRAY|VT_I4; VARIANT owns the array during access.
        let array = unsafe { v.Anonymous.Anonymous.Anonymous.parray };
        if array.is_null() {
            self.partial = true;
            return None;
        }
        // SAFETY: live SAFEARRAY; all scalar output pointers are valid. UIA
        // RuntimeIds are bounded here to avoid accepting a provider-sized array.
        let values = unsafe {
            let bounds = (SafeArrayGetLBound(array, 1), SafeArrayGetUBound(array, 1));
            let (Ok(lower), Ok(upper)) = bounds else {
                self.partial = true;
                return None;
            };
            if SafeArrayGetDim(array) != 1
                || !(1..=256).contains(&(i64::from(upper) - i64::from(lower) + 1))
            {
                self.partial = true;
                return None;
            }
            let mut result = Vec::new();
            for index in lower..=upper {
                let mut value = 0i32;
                if SafeArrayGetElement(array, &index, &mut value as *mut _ as *mut _).is_err() {
                    self.partial = true;
                    return None;
                }
                result.push(value);
            }
            result
        };
        Some(values)
    }

    fn get(&mut self, id: i32, expected: VARENUM) -> Option<VARIANT> {
        // SAFETY: all IDs were added to this cached element's request. The owned
        // VARIANT releases its data on Drop. No payload enters errors or logging.
        let result = unsafe {
            self.element
                .GetCachedPropertyValueEx(UIA_PROPERTY_ID(id), true)
        };
        match result {
            Ok(value) if value.vt() == expected => Some(value),
            // UIA's reserved NotSupported object/empty value is an absent property.
            Ok(value) if value.vt() == VT_UNKNOWN || value.vt() == VT_EMPTY => None,
            _ => {
                self.partial = true;
                None
            }
        }
    }
    fn boolean(&mut self, id: i32) -> Option<bool> {
        let v = self.get(id, VT_BOOL)?;
        // SAFETY: `get` checked the discriminant; v owns and outlives this access.
        Some(unsafe { v.Anonymous.Anonymous.Anonymous.boolVal }.0 != 0)
    }
    fn required_boolean(&mut self, id: i32) -> Option<bool> {
        let value = self.boolean(id);
        if value.is_none() {
            self.partial = true;
        }
        value
    }
    fn integer(&mut self, id: i32) -> Option<i32> {
        let v = self.get(id, VT_I4)?;
        // SAFETY: VT_I4 was verified; the variant owns this scalar.
        Some(unsafe { v.Anonymous.Anonymous.Anonymous.lVal })
    }
    fn number(&mut self, id: i32) -> Option<f64> {
        let v = self.get(id, VT_R8)?;
        // SAFETY: VT_R8 was verified; the variant owns this scalar.
        let value = unsafe { v.Anonymous.Anonymous.Anonymous.dblVal };
        if value.is_finite() {
            Some(value)
        } else {
            self.partial = true;
            None
        }
    }
    fn string(&mut self, id: i32) -> Option<String> {
        let v = self.get(id, VT_BSTR)?;
        // SAFETY: VT_BSTR was verified. Copy before the owned VARIANT is dropped;
        // the nested ManuallyDrop BSTR must not be separately released.
        let text = unsafe { &v.Anonymous.Anonymous.Anonymous.bstrVal }.to_string();
        if text.is_empty() {
            None
        } else {
            Some(text)
        }
    }
}
