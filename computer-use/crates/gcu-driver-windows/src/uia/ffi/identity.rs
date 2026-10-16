//! Worker-local observed RuntimeIds and action-time HWND/metadata revalidation.
use super::super::{cache::Properties, source::NodeId};
use super::{native, properties, Request, State};
use crate::{
    api::{NativeWindow, WinApi},
    desktop::HwndStamp,
    gate::WinGate,
};
use gcu_core::driver::{DriverError, Result};
use windows::Win32::{
    System::{Com::SAFEARRAY, Ole::*, Variant::VT_I4},
    UI::Accessibility::IUIAutomationElement,
};

pub(super) struct WindowIdentity {
    pub(super) window: NativeWindow,
    target_pid: u32,
    target_created: u64,
}
pub(super) struct Observed {
    properties: Properties,
    runtime: Vec<i32>,
    hwnd: u64,
}
impl State {
    pub(super) fn observe(&mut self, node: NodeId, request: &Request) -> Result<Properties> {
        let element = self.element(node)?;
        let properties = properties::read(&element);
        let runtime = runtime(&element, request).inspect_err(|_| {
            super::super::diagnostics::identity(&properties);
        })?;
        let hwnd = self
            .window
            .as_ref()
            .ok_or(DriverError::WindowGone)?
            .window
            .hwnd;
        self.observed.insert(
            node,
            Observed {
                properties: properties.clone(),
                runtime,
                hwnd,
            },
        );
        Ok(properties)
    }
    pub(super) fn validate(
        &self,
        node: NodeId,
        hwnd: u64,
        request: &Request,
    ) -> Result<(IUIAutomationElement, Properties)> {
        request.check()?;
        let old = self.observed.get(&node).ok_or(DriverError::StaleElement)?;
        if old.hwnd != hwnd {
            return Err(DriverError::StaleElement);
        }
        let observed_window = self.window.as_ref().ok_or(DriverError::StaleElement)?;
        let current = window(&self.api, hwnd, request)?;
        HwndStamp::from(&observed_window.window).validate(&current.window)?;
        if observed_window.target_pid != current.target_pid
            || observed_window.target_created != current.target_created
        {
            return Err(DriverError::WindowGone);
        }
        let element = self.element(node)?;
        if runtime(&element, request)? != old.runtime {
            return Err(DriverError::StaleElement);
        }
        request.check()?;
        // SAFETY: element/cache are live MTA-local interfaces. This refreshes only
        // the exact retained element; it never searches by name or other hints.
        let fresh = unsafe { element.BuildUpdatedCache(&self.cache) }.map_err(read_error)?;
        request.check()?;
        let props = properties::read(&fresh);
        if !super::super::actions::same_identity(&old.properties, &props) {
            return Err(DriverError::StaleElement);
        }
        // Walk the exact ancestry, including non-HWND provider fragments. UIA
        // child HWNDs are allowed; only the observed top-level HWND ends the walk.
        let mut ancestor = element.clone();
        for depth in 0..=super::super::tree::DEPTH_CAP {
            request.check()?;
            // SAFETY: refresh only the retained ancestor on its owning MTA.
            let cached = unsafe { ancestor.BuildUpdatedCache(&self.cache) }.map_err(read_error)?;
            let ancestor_props = properties::read(&cached);
            if depth != 0 && super::super::sensitive::is_execution_surface(&ancestor_props) {
                return Err(DriverError::ElementNotActionable);
            }
            request.check()?;
            // SAFETY: live exact ancestor interface on its owning MTA.
            let native_hwnd =
                unsafe { ancestor.CurrentNativeWindowHandle() }.map_err(read_error)?;
            if native_hwnd.0 as usize as u64 == hwnd {
                return Ok((element, props));
            }
            request.check()?;
            // SAFETY: walker/element belong to this MTA; never used after Drop.
            ancestor = unsafe { self.walker.GetParentElement(&ancestor) }.map_err(|e| {
                if e.code().0 as u32 == 0x80004003 {
                    DriverError::StaleElement
                } else {
                    read_error(e)
                }
            })?;
        }
        Err(DriverError::StaleElement)
    }
}
pub(super) fn window(
    api: &crate::RealWinApi,
    hwnd: u64,
    request: &Request,
) -> Result<WindowIdentity> {
    request.check()?;
    let window = api
        .enum_windows()?
        .into_iter()
        .find(|w| w.hwnd == hwnd)
        .ok_or(DriverError::WindowGone)?;
    request.check()?;
    let pid = crate::desktop::window_process(api, &window)?;
    WinGate.check(api, window.pid, false, None)?;
    let target = WinGate.check(api, pid, false, None)?;
    Ok(WindowIdentity {
        window,
        target_pid: pid,
        target_created: target.created,
    })
}
pub(super) fn read_error(error: windows::core::Error) -> DriverError {
    if error.code().0 as u32 == windows::Win32::UI::Accessibility::UIA_E_ELEMENTNOTAVAILABLE {
        DriverError::StaleElement
    } else {
        native(error)
    }
}
struct Array(*mut SAFEARRAY);
impl Drop for Array {
    fn drop(&mut self) {
        // SAFETY: GetRuntimeId transferred ownership of this SAFEARRAY to us.
        // It is never locked or handed out, so destruction cannot leave users.
        let _ = unsafe { SafeArrayDestroy(self.0) };
    }
}
fn runtime(element: &IUIAutomationElement, request: &Request) -> Result<Vec<i32>> {
    request.check()?;
    let started = tokio::time::Instant::now();
    // SAFETY: live MTA-local element; successful result is an owned SAFEARRAY.
    let array = Array(unsafe { element.GetRuntimeId() }.map_err(|e| {
        if e.code().0 as u32 == windows::Win32::UI::Accessibility::UIA_E_ELEMENTNOTAVAILABLE {
            read_error(e)
        } else {
            super::super::diagnostics::native_call(
                "GetRuntimeId",
                started,
                e,
                &properties::read(element),
            )
        }
    })?);
    if array.0.is_null() {
        return Err(DriverError::StaleElement);
    }
    // SAFETY: owned non-null array. Dimensions/type checked before scalar reads.
    let (dim, size, kind, low, high) = unsafe {
        (
            SafeArrayGetDim(array.0),
            SafeArrayGetElemsize(array.0),
            SafeArrayGetVartype(array.0),
            SafeArrayGetLBound(array.0, 1),
            SafeArrayGetUBound(array.0, 1),
        )
    };
    if dim != 1 || size != 4 || kind.map_err(native)? != VT_I4 {
        return Err(DriverError::StaleElement);
    }
    let (low, high) = (low.map_err(native)?, high.map_err(native)?);
    if !(1..=256).contains(&(i64::from(high) - i64::from(low) + 1)) {
        return Err(DriverError::StaleElement);
    }
    let mut values = Vec::new();
    for index in low..=high {
        request.check()?;
        let mut value = 0i32;
        // SAFETY: index is within verified bounds, array is VT_I4, output is i32.
        unsafe { SafeArrayGetElement(array.0, &index, (&mut value as *mut i32).cast()) }
            .map_err(native)?;
        values.push(value);
    }
    Ok(values)
}
