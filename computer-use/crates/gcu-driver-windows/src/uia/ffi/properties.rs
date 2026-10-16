//! Owned VARIANT decoding; no current property/pattern reads.
use super::super::cache::{
    read::{decode, Fields},
    Properties,
};
use gcu_core::model::Rect;
use windows::Win32::{
    System::Variant::*,
    UI::Accessibility::{IUIAutomationElement, UIA_PROPERTY_ID},
};

pub(super) fn read(element: &IUIAutomationElement) -> Properties {
    let _timing = crate::timing::Timing::provider("uia_cached_properties_decode");
    decode(&mut Reader {
        element,
        failure: None,
    })
}

struct Reader<'a> {
    element: &'a IUIAutomationElement,
    failure: Option<String>,
}
impl Fields for Reader<'_> {
    fn runtime_id(&mut self) -> Option<Vec<i32>> {
        use windows::Win32::System::Ole::{
            SafeArrayGetDim, SafeArrayGetElement, SafeArrayGetLBound, SafeArrayGetUBound,
        };
        let v = self.get(30000, VARENUM(VT_ARRAY.0 | VT_I4.0))?;
        // SAFETY: get checked VT_ARRAY|VT_I4; VARIANT owns the array during access.
        let array = unsafe { v.Anonymous.Anonymous.Anonymous.parray };
        if array.is_null() {
            self.note(
                "RuntimeId SAFEARRAY decode",
                "invalid array",
                tokio::time::Instant::now(),
            );
            return None;
        }
        // SAFETY: live SAFEARRAY; all scalar output pointers are valid. UIA
        // RuntimeIds are bounded here to avoid accepting a provider-sized array.
        let values = unsafe {
            let bounds = (SafeArrayGetLBound(array, 1), SafeArrayGetUBound(array, 1));
            let (Ok(lower), Ok(upper)) = bounds else {
                self.note(
                    "RuntimeId SAFEARRAY decode",
                    "invalid array",
                    tokio::time::Instant::now(),
                );
                return None;
            };
            if SafeArrayGetDim(array) != 1
                || !(1..=256).contains(&(i64::from(upper) - i64::from(lower) + 1))
            {
                self.note(
                    "RuntimeId SAFEARRAY decode",
                    "invalid array",
                    tokio::time::Instant::now(),
                );
                return None;
            }
            let mut result = Vec::new();
            for index in lower..=upper {
                let mut value = 0i32;
                if SafeArrayGetElement(array, &index, &mut value as *mut _ as *mut _).is_err() {
                    self.note(
                        "RuntimeId SAFEARRAY decode",
                        "invalid array",
                        tokio::time::Instant::now(),
                    );
                    return None;
                }
                result.push(value);
            }
            result
        };
        Some(values)
    }

    fn boolean(&mut self, id: i32) -> Option<bool> {
        let v = self.get(id, VT_BOOL)?;
        // SAFETY: `get` checked the discriminant; v owns and outlives this access.
        Some(unsafe { v.Anonymous.Anonymous.Anonymous.boolVal }.0 != 0)
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
            self.note(
                "GetCachedPropertyValueEx",
                "non-finite number",
                tokio::time::Instant::now(),
            );
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
    fn frame(&mut self) -> Option<Rect> {
        let started = tokio::time::Instant::now();
        // SAFETY: cached live interface belongs to this MTA; getter is local.
        match unsafe { self.element.CachedBoundingRectangle() } {
            Ok(rect) => Some(Rect {
                x: rect.left as f64,
                y: rect.top as f64,
                w: (rect.right as i64 - rect.left as i64) as f64,
                h: (rect.bottom as i64 - rect.top as i64) as f64,
            }),
            Err(error) => {
                self.note(
                    "get_CachedBoundingRectangle",
                    &format!("HRESULT {:08x}", error.code().0 as u32),
                    started,
                );
                None
            }
        }
    }
    fn failure(&self) -> Option<String> {
        self.failure.clone()
    }
}
impl Reader<'_> {
    fn note(&mut self, call: &str, detail: &str, started: tokio::time::Instant) {
        if self.failure.is_none() {
            self.failure = Some(format!(
                "{call}: {detail}; elapsed_ms={}",
                started.elapsed().as_millis()
            ));
        }
    }
    fn get(&mut self, id: i32, expected: VARENUM) -> Option<VARIANT> {
        // SAFETY: all IDs were added to this cached element's request. The owned
        // VARIANT releases its data on Drop. No payload enters errors or logging.
        let started = tokio::time::Instant::now();
        let result = unsafe {
            self.element
                .GetCachedPropertyValueEx(UIA_PROPERTY_ID(id), true)
        };
        match result {
            Ok(value) if value.vt() == expected => Some(value),
            // UIA's reserved NotSupported object/empty value is an absent property.
            Ok(value) if value.vt() == VT_UNKNOWN || value.vt() == VT_EMPTY => {
                if id == 30019 {
                    self.note(
                        "GetCachedPropertyValueEx(30019)",
                        "HRESULT 00000000; IsPassword unsupported/empty",
                        started,
                    );
                }
                None
            }
            Ok(value) => {
                self.note(
                    &format!("GetCachedPropertyValueEx({id})"),
                    &format!(
                        "unexpected VARTYPE {} (expected {})",
                        value.vt().0,
                        expected.0
                    ),
                    started,
                );
                None
            }
            Err(error) => {
                self.note(
                    &format!("GetCachedPropertyValueEx({id})"),
                    &format!("HRESULT {:08x}", error.code().0 as u32),
                    started,
                );
                None
            }
        }
    }
}
