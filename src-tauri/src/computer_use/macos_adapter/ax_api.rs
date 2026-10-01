//! Bounded public AX calls. All retained element access is serialized by the
//! window-binding lock; these are remote CF proxies, never AppKit UI objects.
use super::*;
use grok_computer_use_core::execution::ActionCancellation;
use std::time::{Duration, Instant};

#[cfg_attr(
    target_os = "macos",
    link(name = "ApplicationServices", kind = "framework")
)]
extern "C" {
    pub(super) fn AXUIElementCreateApplication(pid: i32) -> CfTypeRef;
    pub(super) fn AXUIElementCopyElementAtPosition(
        app: CfTypeRef,
        x: f32,
        y: f32,
        out: *mut CfTypeRef,
    ) -> i32;
    fn AXUIElementGetTypeID() -> usize;
    fn AXUIElementGetPid(element: CfTypeRef, pid: *mut i32) -> i32;
    fn AXUIElementSetMessagingTimeout(element: CfTypeRef, seconds: f32) -> i32;
    fn AXUIElementCopyAttributeValue(
        element: CfTypeRef,
        attr: CfTypeRef,
        out: *mut CfTypeRef,
    ) -> i32;
    fn AXUIElementGetAttributeValueCount(
        element: CfTypeRef,
        attr: CfTypeRef,
        out: *mut isize,
    ) -> i32;
    fn AXUIElementCopyAttributeValues(
        element: CfTypeRef,
        attr: CfTypeRef,
        index: isize,
        max: isize,
        out: *mut CfTypeRef,
    ) -> i32;
    fn AXUIElementCopyActionNames(element: CfTypeRef, out: *mut CfTypeRef) -> i32;
    fn AXUIElementPerformAction(element: CfTypeRef, action: CfTypeRef) -> i32;
    fn AXUIElementIsAttributeSettable(
        element: CfTypeRef,
        attribute: CfTypeRef,
        settable: *mut u8,
    ) -> i32;
    fn AXUIElementSetAttributeValue(
        element: CfTypeRef,
        attribute: CfTypeRef,
        value: CfTypeRef,
    ) -> i32;
    pub(super) fn AXValueGetTypeID() -> usize;
    pub(super) fn AXValueGetType(value: CfTypeRef) -> u32;
    pub(super) fn AXValueGetValue(value: CfTypeRef, kind: u32, out: *mut c_void) -> bool;
    pub(super) fn AXValueCreate(kind: u32, value: *const c_void) -> CfTypeRef;
}
#[cfg_attr(target_os = "macos", link(name = "CoreFoundation", kind = "framework"))]
extern "C" {
    pub(super) fn CFGetTypeID(value: CfTypeRef) -> usize;
    pub(super) fn CFNumberGetTypeID() -> usize;
    fn CFStringGetTypeID() -> usize;
    fn CFStringGetLength(value: CfTypeRef) -> isize;
    fn CFArrayGetTypeID() -> usize;
    fn CFBooleanGetTypeID() -> usize;
    fn CFBooleanGetValue(value: CfTypeRef) -> bool;
    pub(super) fn CFEqual(a: CfTypeRef, b: CfTypeRef) -> bool;
    fn CFRetain(value: CfTypeRef) -> CfTypeRef;
}

pub(super) struct Element(pub(super) OwnedCf);
// Owned AX proxy, not Sync. Calls are serialized by WindowBindings; no
// AXObserver/run-loop source or AppKit object is moved between threads.
unsafe impl Send for Element {}

/// Allocate the complete write before revalidating the retained target. Never
/// read AXValue, focus another object, or silently truncate a model string.
pub(super) struct PreparedValue {
    attribute: OwnedCf,
    value: OwnedCf,
}
impl PreparedValue {
    pub(super) fn new(parameters: &serde_json::Value) -> Result<Self, String> {
        let parameters = parameters
            .as_object()
            .ok_or("set_value parameters required")?;
        let text = parameters
            .get("text")
            .and_then(serde_json::Value::as_str)
            .ok_or("set_value text required")?;
        if parameters.len() != 1
            || text.contains('\0')
            || text.chars().count() > super::super::protocol::TEXT_MAX_CHARS
        {
            return Err("invalid set_value parameters or text length".into());
        }
        Ok(Self {
            attribute: OwnedCf::new(cf_str("AXValue"), "AX value key allocation failed")?,
            value: OwnedCf::new(cf_str(text), "AX value string allocation failed")?,
        })
    }
}

impl Element {
    pub(super) fn raw(&self) -> CfTypeRef {
        self.0 .0
    }
    pub(super) fn retained(&self) -> Result<Self, String> {
        Ok(Self(OwnedCf::new(
            unsafe { CFRetain(self.raw()) },
            "AX retain failed",
        )?))
    }
    pub(super) fn same(&self, other: &Self) -> bool {
        unsafe { CFEqual(self.raw(), other.raw()) }
    }
}

pub(super) struct Budget<'a> {
    cancellation: &'a ActionCancellation,
    deadline: Instant,
}
impl<'a> Budget<'a> {
    pub(super) fn new(cancellation: &'a ActionCancellation) -> Self {
        Self::with_deadline(cancellation, Instant::now() + Duration::from_secs(2))
    }
    pub(super) fn with_deadline(cancellation: &'a ActionCancellation, deadline: Instant) -> Self {
        Self {
            cancellation,
            deadline: deadline.min(Instant::now() + Duration::from_secs(2)),
        }
    }
    pub(super) fn check(&self) -> Result<(), String> {
        self.cancellation.check()?;
        if Instant::now() >= self.deadline {
            return Err("macOS AX query exceeded its deadline".into());
        }
        Ok(())
    }
    pub(super) fn element(&self, owned: OwnedCf, pid: u32) -> Result<OwnedCf, String> {
        self.check()?;
        if unsafe { CFGetTypeID(owned.0) != AXUIElementGetTypeID() } {
            return Err("AX returned a non-element identity".into());
        }
        if unsafe { AXUIElementSetMessagingTimeout(owned.0, 0.2) } != 0 {
            return Err("cannot bound macOS AX messaging".into());
        }
        let mut actual = 0;
        self.check()?;
        if unsafe { AXUIElementGetPid(owned.0, &mut actual) } != 0
            || actual <= 0
            || actual as u32 != pid
        {
            return Err("AX element belongs to another process".into());
        }
        self.check()?;
        Ok(owned)
    }
    pub(super) fn optional(
        &self,
        element: CfTypeRef,
        name: &str,
    ) -> Result<Option<OwnedCf>, String> {
        self.check()?;
        let key = OwnedCf::new(cf_str(name), "AX attribute allocation failed")?;
        let mut out = std::ptr::null();
        let status = unsafe { AXUIElementCopyAttributeValue(element, key.0, &mut out) };
        let value = (!out.is_null()).then(|| OwnedCf(out));
        self.check()?;
        match status {
            0 => value
                .map(Some)
                .ok_or_else(|| "AX success omitted its value".into()),
            -25205 | -25212 => Ok(None), // Unsupported attribute / no value only.
            _ => Err(format!("AX attribute {name} failed ({status})")),
        }
    }
    pub(super) fn attribute(&self, element: CfTypeRef, name: &str) -> Result<OwnedCf, String> {
        self.optional(element, name)?
            .ok_or_else(|| format!("AX attribute {name} is unavailable"))
    }
    fn string(&self, value: CfTypeRef) -> Result<String, String> {
        if unsafe { CFGetTypeID(value) != CFStringGetTypeID() } {
            return Err("AX string has an unexpected type".into());
        }
        let units = unsafe { CFStringGetLength(value) };
        if !(0..=8192).contains(&units) {
            return Err("AX string exceeds read budget".into());
        }
        let mut buffer = vec![0i8; (units as usize) * 4 + 1];
        if !unsafe {
            CFStringGetCString(
                value,
                buffer.as_mut_ptr(),
                buffer.len() as isize,
                K_CF_STRING_ENCODING_UTF8,
            )
        } {
            return Err("AX string decoding failed".into());
        }
        let value = unsafe { CStr::from_ptr(buffer.as_ptr()) }
            .to_str()
            .map_err(|_| "AX string is not UTF-8")?
            .to_owned();
        self.check()?;
        Ok(value)
    }
    pub(super) fn text(&self, element: CfTypeRef, name: &str) -> Result<Option<String>, String> {
        self.optional(element, name)?
            .map(|v| self.string(v.0))
            .transpose()
    }
    pub(super) fn role(&self, element: CfTypeRef) -> Result<String, String> {
        self.text(element, "AXRole")?
            .ok_or_else(|| "AX role missing".into())
    }
    pub(super) fn flag(&self, element: CfTypeRef, name: &str) -> Result<Option<bool>, String> {
        let Some(value) = self.optional(element, name)? else {
            return Ok(None);
        };
        if unsafe { CFGetTypeID(value.0) != CFBooleanGetTypeID() } {
            return Err("AX flag has an unexpected type".into());
        }
        Ok(Some(unsafe { CFBooleanGetValue(value.0) }))
    }
    pub(super) fn optional_bounds(
        &self,
        element: CfTypeRef,
    ) -> Result<Option<WindowBounds>, String> {
        let Some(position) = self.optional(element, "AXPosition")? else {
            return Ok(None);
        };
        let Some(size) = self.optional(element, "AXSize")? else {
            return Ok(None);
        };
        if unsafe {
            CFGetTypeID(position.0) != AXValueGetTypeID()
                || AXValueGetType(position.0) != 1
                || CFGetTypeID(size.0) != AXValueGetTypeID()
                || AXValueGetType(size.0) != 2
        } {
            return Err("AX geometry has an unexpected type".into());
        }
        let mut p = CgPoint { x: 0.0, y: 0.0 };
        let mut s = CgSize {
            width: 0.0,
            height: 0.0,
        };
        if !unsafe { AXValueGetValue(position.0, 1, std::ptr::from_mut(&mut p).cast()) }
            || !unsafe { AXValueGetValue(size.0, 2, std::ptr::from_mut(&mut s).cast()) }
        {
            return Err("cannot read AX geometry".into());
        }
        self.check()?;
        // Layout containers and collapsed controls may have a legitimate zero
        // size. Keep their semantic description, without clickable geometry.
        if p.x.is_finite()
            && p.y.is_finite()
            && s.width.is_finite()
            && s.height.is_finite()
            && s.width >= 0.0
            && s.height >= 0.0
            && (s.width == 0.0 || s.height == 0.0)
        {
            return Ok(None);
        }
        WindowBounds::new(p.x, p.y, s.width, s.height).map(Some)
    }
    pub(super) fn bounds(&self, element: CfTypeRef) -> Result<WindowBounds, String> {
        self.optional_bounds(element)?
            .ok_or_else(|| "AX window bounds missing".into())
    }
    pub(super) fn related(
        &self,
        element: &Element,
        name: &str,
        pid: u32,
    ) -> Result<Element, String> {
        Ok(Element(
            self.element(self.attribute(element.raw(), name)?, pid)?,
        ))
    }
    fn array_len(&self, array: CfTypeRef, max: usize) -> Result<usize, String> {
        if unsafe { CFGetTypeID(array) != CFArrayGetTypeID() } {
            return Err("AX array has an unexpected type".into());
        }
        let len = unsafe { CFArrayGetCount(array) };
        if len < 0 || len as usize > max {
            return Err("AX array exceeds read budget".into());
        }
        Ok(len as usize)
    }
    pub(super) fn children(
        &self,
        element: &Element,
        pid: u32,
        limit: usize,
    ) -> Result<(Vec<Element>, bool), String> {
        self.check()?;
        let key = OwnedCf::new(cf_str("AXChildren"), "AX child key allocation failed")?;
        let mut count = 0;
        let status = unsafe { AXUIElementGetAttributeValueCount(element.raw(), key.0, &mut count) };
        self.check()?;
        if matches!(status, -25205 | -25212) {
            return Ok((vec![], false));
        }
        if status != 0 || count < 0 {
            return Err("AX child count failed".into());
        }
        let take = (count as usize).min(limit);
        if take == 0 {
            return Ok((vec![], count > 0));
        }
        let mut out = std::ptr::null();
        let status = unsafe {
            AXUIElementCopyAttributeValues(element.raw(), key.0, 0, take as isize, &mut out)
        };
        let array = OwnedCf::new(out, "AX child array unavailable")?;
        if status != 0 {
            return Err("AX child read failed".into());
        }
        self.check()?;
        let len = self.array_len(array.0, take)?;
        let mut result = Vec::with_capacity(len);
        for i in 0..len {
            let raw = unsafe { CFArrayGetValueAtIndex(array.0, i as isize) };
            if raw.is_null() {
                return Err("AX child is null".into());
            }
            let owned = OwnedCf::new(unsafe { CFRetain(raw) }, "AX child retain failed")?;
            result.push(Element(self.element(owned, pid)?));
        }
        Ok((result, count as usize > len))
    }
    pub(super) fn can_press(&self, element: &Element) -> Result<bool, String> {
        self.check()?;
        let mut out = std::ptr::null();
        let status = unsafe { AXUIElementCopyActionNames(element.raw(), &mut out) };
        let value = (!out.is_null()).then(|| OwnedCf(out));
        self.check()?;
        if matches!(status, -25206 | -25208) {
            return Ok(false);
        }
        if status != 0 {
            return Err("AX action list failed".into());
        }
        let array = value.ok_or("AX action list missing")?;
        let mut presses = 0;
        for i in 0..self.array_len(array.0, 64)? {
            let raw = unsafe { CFArrayGetValueAtIndex(array.0, i as isize) };
            if raw.is_null() {
                return Err("AX action is null".into());
            }
            if self.string(raw)? == "AXPress" {
                presses += 1;
            }
        }
        Ok(presses == 1)
    }
    pub(super) fn press(
        &self,
        element: &Element,
        before: impl FnOnce() -> Result<(), String>,
    ) -> Result<i32, String> {
        let action = OwnedCf::new(cf_str("AXPress"), "AX action allocation failed")?;
        self.check()?;
        before()?;
        self.check()?;
        // Caller classifies every nonzero result as potentially applied/unknown.
        Ok(unsafe { AXUIElementPerformAction(element.raw(), action.0) })
    }

    pub(super) fn value_settable(&self, element: &Element) -> Result<bool, String> {
        self.attribute_settable(element, "AXValue")
    }

    pub(super) fn attribute_settable(&self, element: &Element, name: &str) -> Result<bool, String> {
        self.check()?;
        let attribute = OwnedCf::new(cf_str(name), "AX attribute key allocation failed")?;
        let mut settable = 0u8; // CoreFoundation Boolean, not a CFBooleanRef.
        let status =
            unsafe { AXUIElementIsAttributeSettable(element.raw(), attribute.0, &mut settable) };
        self.check()?;
        match status {
            0 => match settable {
                0 => Ok(false),
                1 => Ok(true),
                _ => Err("AX settable response is not a Boolean".into()),
            },
            -25205 | -25208 | -25212 => Ok(false),
            _ => Err(format!("AX settable query failed ({status})")),
        }
    }

    pub(super) fn set_value(
        &self,
        element: &Element,
        value: &PreparedValue,
        before: impl FnOnce() -> Result<(), String>,
    ) -> Result<i32, String> {
        self.set_attribute(element, &value.attribute, &value.value, before)
    }

    pub(super) fn set_attribute(
        &self,
        element: &Element,
        attribute: &OwnedCf,
        value: &OwnedCf,
        before: impl FnOnce() -> Result<(), String>,
    ) -> Result<i32, String> {
        self.check()?;
        before()?;
        self.check()?;
        // Nonzero results may have applied; the caller retains occupancy.
        Ok(unsafe { AXUIElementSetAttributeValue(element.raw(), attribute.0, value.0) })
    }
}
