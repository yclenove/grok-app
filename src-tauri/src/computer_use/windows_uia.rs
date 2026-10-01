//! UI Automation tree. Semantic elementRef is RuntimeId + process + window stamp.
//! Win32 titles and child HWNDs are compatibility only and cannot be elementRef.

#![cfg(target_os = "windows")]

use super::protocol::{
    click_button, click_count, normalize_key, ActionKind, ActionTarget, ObservationNode,
};
use windows::core::BSTR;
use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::Graphics::Gdi::ClientToScreen;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
};
use windows::Win32::System::Ole::{
    SafeArrayAccessData, SafeArrayDestroy, SafeArrayGetLBound, SafeArrayGetUBound,
    SafeArrayUnaccessData,
};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationElement, IUIAutomationInvokePattern,
    IUIAutomationLegacyIAccessiblePattern, IUIAutomationScrollPattern, IUIAutomationValuePattern,
    ScrollAmount_LargeDecrement, ScrollAmount_LargeIncrement, ScrollAmount_NoAmount,
    TreeScope_Subtree, UIA_ButtonControlTypeId, UIA_CheckBoxControlTypeId,
    UIA_ComboBoxControlTypeId, UIA_EditControlTypeId, UIA_HyperlinkControlTypeId,
    UIA_InvokePatternId, UIA_LegacyIAccessiblePatternId, UIA_ListControlTypeId,
    UIA_ListItemControlTypeId, UIA_MenuItemControlTypeId, UIA_RadioButtonControlTypeId,
    UIA_ScrollBarControlTypeId, UIA_ScrollPatternId, UIA_SeparatorControlTypeId,
    UIA_SliderControlTypeId, UIA_StatusBarControlTypeId, UIA_TabItemControlTypeId,
    UIA_ThumbControlTypeId, UIA_TitleBarControlTypeId, UIA_ToolTipControlTypeId,
    UIA_ValuePatternId, UIA_WindowControlTypeId, UIA_CONTROLTYPE_ID,
};
pub const ELEMENT_REF_PREFIX: &str = "uia:";
const NODE_CAP: usize = 64;
mod value;

pub fn ensure_com() {
    thread_local! {
        static INIT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    }
    INIT.with(|cell| {
        if !cell.get() {
            unsafe {
                let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            }
            cell.set(true);
        }
    });
}

pub fn automation() -> Result<IUIAutomation, String> {
    ensure_com();
    unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) }
        .map_err(|e| format!("CUIAutomation: {e}"))
}

pub fn parse_element_ref(s: &str) -> Option<(u32, u64, Vec<i32>)> {
    let rest = s.strip_prefix(ELEMENT_REF_PREFIX)?;
    let mut parts = rest.splitn(3, ':');
    let pid: u32 = parts.next()?.parse().ok()?;
    let stamp: u64 = parts.next()?.parse().ok()?;
    let runtime = parts.next()?;
    if runtime.trim().is_empty() {
        return None;
    }
    let ids = runtime
        .split('-')
        .map(|h| u32::from_str_radix(h, 16).map(|v| v as i32))
        .collect::<Result<Vec<i32>, _>>()
        .ok()?;
    if ids.is_empty() {
        return None;
    }
    Some((pid, stamp, ids))
}

pub fn format_element_ref(pid: u32, stamp: u64, runtime: &[i32]) -> String {
    let encoded = runtime
        .iter()
        .map(|n| format!("{:x}", *n as u32))
        .collect::<Vec<_>>()
        .join("-");
    format!("uia:{pid}:{stamp}:{encoded}")
}

fn runtime_id_of(el: &IUIAutomationElement) -> Result<Vec<i32>, String> {
    unsafe {
        let psa = el
            .GetRuntimeId()
            .map_err(|e| format!("GetRuntimeId: {e}"))?;
        if psa.is_null() {
            return Err("empty runtime id".into());
        }
        let lo = SafeArrayGetLBound(psa, 1).map_err(|e| e.to_string())?;
        let hi = SafeArrayGetUBound(psa, 1).map_err(|e| e.to_string())?;
        let mut data = std::ptr::null_mut();
        SafeArrayAccessData(psa, &mut data).map_err(|e| e.to_string())?;
        let n = (hi - lo + 1).max(0) as usize;
        let out = std::slice::from_raw_parts(data as *const i32, n).to_vec();
        let _ = SafeArrayUnaccessData(psa);
        let _ = SafeArrayDestroy(psa);
        if out.is_empty() {
            Err("empty runtime id".into())
        } else {
            Ok(out)
        }
    }
}

fn control_role(id: UIA_CONTROLTYPE_ID) -> &'static str {
    match id.0 {
        _ if id == UIA_ButtonControlTypeId => "button",
        _ if id == UIA_EditControlTypeId => "edit",
        _ if id == UIA_ListControlTypeId => "list",
        _ if id == UIA_ListItemControlTypeId => "listitem",
        _ if id == UIA_CheckBoxControlTypeId => "checkbox",
        _ if id == UIA_ComboBoxControlTypeId => "combobox",
        _ if id == UIA_RadioButtonControlTypeId => "radio",
        _ if id == UIA_HyperlinkControlTypeId => "link",
        _ if id == UIA_MenuItemControlTypeId => "menuitem",
        _ if id == UIA_TabItemControlTypeId => "tab",
        _ if id == UIA_SliderControlTypeId => "slider",
        _ if id == UIA_ScrollBarControlTypeId => "scrollbar",
        _ if id == UIA_WindowControlTypeId => "window",
        _ => "other",
    }
}

fn skip_control_type(id: UIA_CONTROLTYPE_ID) -> bool {
    id == UIA_TitleBarControlTypeId
        || id == UIA_StatusBarControlTypeId
        || id == UIA_ThumbControlTypeId
        || id == UIA_SeparatorControlTypeId
        || id == UIA_ToolTipControlTypeId
}

fn client_box(parent: HWND, screen: RECT) -> Option<(f64, f64, f64, f64)> {
    let mut origin = POINT { x: 0, y: 0 };
    unsafe {
        if !ClientToScreen(parent, &mut origin).as_bool() {
            return None;
        }
    }
    let x = f64::from(screen.left - origin.x);
    let y = f64::from(screen.top - origin.y);
    let w = f64::from((screen.right - screen.left).max(0));
    let h = f64::from((screen.bottom - screen.top).max(0));
    if w < 1.0 || h < 1.0 {
        None
    } else {
        Some((x, y, w, h))
    }
}

fn bstr_name(el: &IUIAutomationElement) -> String {
    unsafe { el.CurrentName() }
        .map(|s| s.to_string())
        .unwrap_or_default()
}

/// Walk the UIA subtree. node_ref is never a Win32 HWND or dialog control id.
pub fn collect_nodes(hwnd: HWND, pid: u32, stamp: u64) -> Result<Vec<ObservationNode>, String> {
    let uia = automation()?;
    let root =
        unsafe { uia.ElementFromHandle(hwnd) }.map_err(|e| format!("ElementFromHandle: {e}"))?;
    let cond = unsafe { uia.CreateTrueCondition() }.map_err(|e| e.to_string())?;
    let all =
        unsafe { root.FindAll(TreeScope_Subtree, &cond) }.map_err(|e| format!("FindAll: {e}"))?;
    let len = unsafe { all.Length() }.map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    let mut truncated = false;
    for i in 0..len {
        let el = match unsafe { all.GetElement(i) } {
            Ok(el) => el,
            Err(_) => continue,
        };
        let ct = unsafe { el.CurrentControlType() }.unwrap_or(UIA_CONTROLTYPE_ID(0));
        if skip_control_type(ct) {
            continue;
        }
        if unsafe { el.CurrentIsOffscreen() }
            .map(|b| b.as_bool())
            .unwrap_or(false)
        {
            continue;
        }
        if !unsafe { el.CurrentIsEnabled() }
            .map(|b| b.as_bool())
            .unwrap_or(true)
        {
            continue;
        }
        // A WPF ScrollViewer may be unnamed (and not itself a control-view
        // element). Its actual vertical ScrollPattern, not its role/name, is
        // what grants semantic scroll authority. Never substitute a scrollbar
        // or list item merely because it looks scroll-related.
        let scrollable = unsafe {
            el.GetCurrentPatternAs::<IUIAutomationScrollPattern>(UIA_ScrollPatternId)
                .and_then(|pattern| pattern.CurrentVerticallyScrollable())
        }
        .map(|available| available.as_bool())
        .unwrap_or(false);
        if !scrollable
            && !unsafe { el.CurrentIsControlElement() }
                .map(|b| b.as_bool())
                .unwrap_or(true)
        {
            continue;
        }
        let name = bstr_name(&el);
        let role = control_role(ct);
        if !scrollable && name.trim().is_empty() && matches!(role, "other" | "window") && i != 0 {
            continue;
        }
        let runtime = match runtime_id_of(&el) {
            Ok(id) => id,
            Err(_) => continue,
        };
        let node_ref = format_element_ref(pid, stamp, &runtime);
        if node_ref.len() > 256 {
            truncated = true;
            continue;
        }
        let mut actions = match role {
            "edit" => vec!["click".into(), "set_value".into(), "type_text".into()],
            "list" | "listitem" => vec!["key".into(), "click".into()],
            _ => vec!["click".into()],
        };
        if scrollable {
            actions.push("scroll".into());
        }
        let box_px = unsafe { el.CurrentBoundingRectangle() }
            .ok()
            .and_then(|rc| client_box(hwnd, rc));
        if out.len() >= NODE_CAP {
            truncated = true;
            break;
        }
        out.push(ObservationNode {
            node_ref,
            role: role.into(),
            name,
            actions,
            truncated: false,
            x: box_px.map(|b| b.0),
            y: box_px.map(|b| b.1),
            width: box_px.map(|b| b.2),
            height: box_px.map(|b| b.3),
        });
    }
    if out.is_empty() {
        return Err("UIA tree produced no operable elements".into());
    }
    if truncated {
        if let Some(last) = out.last_mut() {
            last.truncated = true;
        }
    }
    Ok(out)
}

pub struct ResolvedElement {
    pub x: f64,
    pub y: f64,
    pub native: Option<HWND>,
}

fn find_element(
    parent: HWND,
    pid: u32,
    stamp: u64,
    element_ref: &str,
) -> Result<IUIAutomationElement, String> {
    let (ref_pid, ref_stamp, want) = parse_element_ref(element_ref)
        .ok_or_else(|| "elementRef is not a UIA RuntimeId identity".to_string())?;
    if ref_pid != pid || ref_stamp != stamp {
        return Err("elementRef process/window stamp mismatch".into());
    }
    let uia = automation()?;
    let root = unsafe { uia.ElementFromHandle(parent) }.map_err(|e| e.to_string())?;
    let cond = unsafe { uia.CreateTrueCondition() }.map_err(|e| e.to_string())?;
    let all = unsafe { root.FindAll(TreeScope_Subtree, &cond) }.map_err(|e| e.to_string())?;
    let len = unsafe { all.Length() }.map_err(|e| e.to_string())?;
    for i in 0..len {
        let el = match unsafe { all.GetElement(i) } {
            Ok(el) => el,
            Err(_) => continue,
        };
        let Ok(got) = runtime_id_of(&el) else {
            continue;
        };
        if got == want {
            return Ok(el);
        }
    }
    Err(format!("UIA element not found: {element_ref}"))
}

pub fn resolve(
    parent: HWND,
    pid: u32,
    stamp: u64,
    element_ref: &str,
) -> Result<ResolvedElement, String> {
    let el = find_element(parent, pid, stamp, element_ref)?;
    let screen = unsafe { el.CurrentBoundingRectangle() }.map_err(|e| e.to_string())?;
    let (x, y, w, h) =
        client_box(parent, screen).ok_or_else(|| "UIA bounding rectangle is empty".to_string())?;
    let native = unsafe { el.CurrentNativeWindowHandle() }
        .ok()
        .filter(|h| !h.0.is_null());
    Ok(ResolvedElement {
        x: x + w / 2.0,
        y: y + h / 2.0,
        native,
    })
}

/// Semantic UIA action. `Ok(None)` means no pattern — caller may fall back.
pub fn perform(
    parent: HWND,
    pid: u32,
    stamp: u64,
    action: ActionKind,
    target: &ActionTarget,
    parameters: &serde_json::Value,
    before_effect: &dyn Fn() -> Result<(), String>,
) -> Result<Option<&'static str>, String> {
    before_effect()?;
    let ActionTarget::Element { element_ref } = target else {
        return Ok(None);
    };
    let el = find_element(parent, pid, stamp, element_ref)?;
    before_effect()?;
    match action {
        ActionKind::Click => {
            if click_button(parameters) != "left" || click_count(parameters) != 1 {
                return Ok(None);
            }
            // Use the actual provider pattern. Posting BM_CLICK and declaring
            // the adapter idle left unacknowledged writes in another queue.
            // A slow provider keeps native occupancy until its call returns.
            if let Ok(pattern) =
                unsafe { el.GetCurrentPatternAs::<IUIAutomationInvokePattern>(UIA_InvokePatternId) }
            {
                before_effect()?;
                unsafe { pattern.Invoke() }.map_err(|e| format!("UIA Invoke: {e}"))?;
                return Ok(Some("windows uia invoke"));
            }
            if let Ok(pattern) = unsafe {
                el.GetCurrentPatternAs::<IUIAutomationLegacyIAccessiblePattern>(
                    UIA_LegacyIAccessiblePatternId,
                )
            } {
                before_effect()?;
                unsafe { pattern.DoDefaultAction() }
                    .map_err(|e| format!("UIA default action: {e}"))?;
                return Ok(Some("windows uia invoke"));
            }
            Ok(None)
        }
        ActionKind::SetValue | ActionKind::TypeText => {
            if action == ActionKind::TypeText
                && parameters.get("via").and_then(|v| v.as_str()) == Some("clipboard")
            {
                return Ok(None);
            }
            let Some(text) = parameters
                .get("text")
                .or_else(|| parameters.get("value"))
                .and_then(|v| v.as_str())
            else {
                return Ok(None);
            };
            let Ok(pattern) = (unsafe {
                el.GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId)
            }) else {
                return Ok(None);
            };
            if unsafe { pattern.CurrentIsReadOnly() }
                .map_err(|e| format!("UIA read-only state: {e}"))?
                .as_bool()
                || unsafe { el.CurrentIsPassword() }
                    .map_err(|e| format!("UIA password state: {e}"))?
                    .as_bool()
            {
                return Err("UIA text target is read-only or password-protected".into());
            }
            value::apply(
                action,
                text,
                before_effect,
                &|| {
                    let value = unsafe { pattern.CurrentValue() }
                        .map_err(|e| format!("UIA value readback: {e}"))?;
                    String::from_utf16(&value).map_err(|_| "UIA value is invalid UTF-16".into())
                },
                &|value| {
                    unsafe { pattern.SetValue(&BSTR::from(value)) }
                        .map_err(|e| format!("UIA SetValue: {e}"))
                },
            )?;
            Ok(Some("windows uia value"))
        }
        ActionKind::Scroll => {
            let delta = super::protocol::ScrollDelta::parse(parameters)?;
            let Ok(pattern) = (unsafe {
                el.GetCurrentPatternAs::<IUIAutomationScrollPattern>(UIA_ScrollPatternId)
            }) else {
                return Ok(None);
            };
            let amount = if delta.value() > 0 {
                ScrollAmount_LargeIncrement
            } else if delta.value() < 0 {
                ScrollAmount_LargeDecrement
            } else {
                return Ok(Some("windows uia zero scroll; no native call"));
            };
            before_effect()?;
            unsafe { pattern.Scroll(ScrollAmount_NoAmount, amount) }
                .map_err(|e| format!("UIA Scroll: {e}"))?;
            Ok(Some("windows uia scroll"))
        }
        ActionKind::Key => {
            let key = parameters
                .get("key")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "key required".to_string())?;
            let key = normalize_key(key)?;
            if key != "enter" {
                return Ok(None);
            }
            if let Ok(pattern) =
                unsafe { el.GetCurrentPatternAs::<IUIAutomationInvokePattern>(UIA_InvokePatternId) }
            {
                before_effect()?;
                unsafe { pattern.Invoke() }.map_err(|e| format!("UIA Invoke: {e}"))?;
                return Ok(Some("windows uia invoke"));
            }
            Ok(None)
        }
        ActionKind::Drag | ActionKind::Wait => Ok(None),
    }
}
