//! Attribute decoding for one AX element.
//!
//! Copied values are +1 and released when the decode finishes. Child elements
//! that are kept move into the snapshot map; the rest release on drop.

use std::collections::HashMap;

use accessibility_sys::{
    kAXChildrenAttribute, kAXContentsAttribute, kAXDescriptionAttribute, kAXElementBusyAttribute,
    kAXEnabledAttribute, kAXErrorAPIDisabled, kAXErrorSuccess, kAXExpandedAttribute,
    kAXFocusedAttribute, kAXHelpAttribute, kAXIdentifierAttribute, kAXIsEditableAttribute,
    kAXModalAttribute, kAXNumberOfCharactersAttribute, kAXPlaceholderValueAttribute,
    kAXPositionAttribute, kAXRoleAttribute, kAXSelectedAttribute, kAXSelectedTextRangeAttribute,
    kAXSizeAttribute, kAXStringForRangeParameterizedAttribute, kAXSubroleAttribute,
    kAXTitleAttribute, kAXURLAttribute, kAXValueAttribute, kAXValueTypeAXError,
    kAXValueTypeCFRange, kAXValueTypeCGPoint, kAXValueTypeCGSize,
    kAXVisibleCharacterRangeAttribute, kAXVisibleChildrenAttribute, AXUIElementCopyActionNames,
    AXUIElementCopyAttributeValue, AXUIElementCopyMultipleAttributeValues,
    AXUIElementCopyParameterizedAttributeValue, AXUIElementGetTypeID,
    AXUIElementIsAttributeSettable, AXUIElementRef, AXValueCreate, AXValueGetType,
    AXValueGetTypeID, AXValueGetValue, AXValueRef,
};
use core_foundation::array::{CFArray, CFArrayRef};
use core_foundation::base::{kCFNull, CFGetTypeID, CFRange, CFType, CFTypeRef, TCFType};
use core_foundation::boolean::CFBoolean;
use core_foundation::number::CFNumber;
use core_foundation::string::CFString;
use gcu_core::{
    driver::DriverError,
    model::{NodeAction, NodeValue, Rect, TextRange},
};

use super::world::{self, OwnedElement, World};
use crate::ax::tree::{map_ax_action, RENDER_BYTE_CAP};
use gcu_core::driver::Result;

const ATTRS: &[&str] = &[
    kAXRoleAttribute,
    kAXSubroleAttribute,
    kAXTitleAttribute,
    kAXDescriptionAttribute,
    kAXHelpAttribute,
    kAXPlaceholderValueAttribute,
    kAXIdentifierAttribute,
    kAXPositionAttribute,
    kAXSizeAttribute,
    kAXEnabledAttribute,
    kAXFocusedAttribute,
    kAXSelectedAttribute,
    kAXExpandedAttribute,
    kAXModalAttribute,
    kAXElementBusyAttribute,
    kAXIsEditableAttribute,
    kAXURLAttribute,
    kAXNumberOfCharactersAttribute,
    kAXSelectedTextRangeAttribute,
    kAXVisibleCharacterRangeAttribute,
    kAXVisibleChildrenAttribute,
    kAXContentsAttribute,
    kAXChildrenAttribute,
];

/// Fields produced by one batch read.
#[derive(Clone, Debug)]
pub(super) struct Facts {
    pub(super) role: String,
    pub(super) subrole: Option<String>,
    pub(super) title: Option<String>,
    pub(super) value: Option<NodeValue>,
    pub(super) description: Option<String>,
    pub(super) placeholder: Option<String>,
    pub(super) help: Option<String>,
    pub(super) identifier: Option<String>,
    pub(super) frame: Option<Rect>,
    pub(super) states: crate::ax::AxStateBits,
    pub(super) actions: Vec<NodeAction>,
    pub(super) selected: Option<TextRange>,
    pub(super) visible: Option<TextRange>,
    pub(super) chars: Option<usize>,
    pub(super) url: Option<String>,
    pub(super) attributes: HashMap<String, String>,
    pub(super) children: Vec<u64>,
}

pub(super) fn open_root(world: &mut World, pid: u32, index: u32) -> Result<u64> {
    world.reset_snapshot();
    let pid = i32::try_from(pid).map_err(|_| DriverError::InvalidRequest)?;
    let app = world.application(pid)?;
    world::set_messaging_timeout(app)?;
    let windows = named_elements(app, World::windows_attribute())?;
    let Some(window) = windows.into_iter().nth(index as usize) else {
        return Err(DriverError::WindowGone);
    };
    Ok(world.insert(window.into_inner()))
}

pub(super) fn read_facts(world: &mut World, id: u64, text_limit: Option<usize>) -> Result<Facts> {
    let Some(element) = world.element(id) else {
        return Err(DriverError::Native {
            message: "accessibility element is not in this snapshot".into(),
            maybe_executed: false,
        });
    };
    world::set_messaging_timeout(element)?;
    let slots = copy_many(element)?;
    let role = slot_string(&slots, 0).unwrap_or_else(|| "AXUnknown".into());
    let subrole = slot_string(&slots, 1);
    let secure = role == "AXSecureTextField" || subrole.as_deref() == Some("AXSecureTextField");
    let chars = if secure { None } else { slot_usize(&slots, 17) };
    let selected = if secure { None } else { slot_range(&slots, 18) };
    let visible = if secure { None } else { slot_range(&slots, 19) };
    let value = if secure {
        Some(NodeValue::Text("•••".into()))
    } else {
        let limit = text_limit.unwrap_or(RENDER_BYTE_CAP).min(RENDER_BYTE_CAP);
        read_value(element, chars, limit)
    };
    let identifier = slot_string(&slots, 6);
    let mut attributes = HashMap::new();
    if let Some(id) = identifier.clone() {
        attributes.insert("AXIdentifier".into(), id);
    }
    Ok(Facts {
        role,
        subrole,
        title: slot_string(&slots, 2),
        value,
        description: slot_string(&slots, 3),
        placeholder: slot_string(&slots, 5),
        help: slot_string(&slots, 4),
        identifier,
        frame: slot_frame(&slots),
        states: crate::ax::AxStateBits {
            enabled: slot_bool(&slots, 9),
            focused: slot_bool(&slots, 10),
            selected: slot_bool(&slots, 11),
            expanded: slot_bool(&slots, 12),
            modal: slot_bool(&slots, 13).unwrap_or(false),
            busy: slot_bool(&slots, 14).unwrap_or(false),
            editable: slot_bool(&slots, 15).unwrap_or(false),
            settable: value_is_settable(element),
            ..crate::ax::AxStateBits::default()
        },
        actions: copy_actions(element),
        selected,
        visible,
        chars,
        url: slot_string(&slots, 16),
        attributes,
        children: take_children(world, &slots),
    })
}

struct Slots(Vec<CFTypeRef>);

impl Drop for Slots {
    fn drop(&mut self) {
        for ptr in &self.0 {
            world::release(*ptr);
        }
    }
}

impl std::ops::Deref for Slots {
    type Target = [CFTypeRef];
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

fn copy_many(element: AXUIElementRef) -> Result<Slots> {
    let strings: Vec<CFString> = ATTRS.iter().copied().map(CFString::new).collect();
    let names = CFArray::from_CFTypes(&strings);
    let mut values: CFArrayRef = std::ptr::null();
    // SAFETY: element is live. `names` outlives the call. On success `values` is +1.
    let status = unsafe {
        crate::diagnostics::ax_call(
            "AXUIElementCopyMultipleAttributeValues",
            (element) as usize,
            "batch: roles/names/states/frames/children",
            || {
                AXUIElementCopyMultipleAttributeValues(
                    element,
                    names.as_concrete_TypeRef(),
                    0,
                    &mut values,
                )
            },
        )
    };
    if status == kAXErrorAPIDisabled {
        return Err(DriverError::PermissionMissing {
            permission: "accessibility".into(),
        });
    }
    if status != kAXErrorSuccess || values.is_null() {
        if !values.is_null() {
            world::release(values as CFTypeRef);
        }
        return Err(DriverError::Native {
            message: "accessibility attribute copy failed".into(),
            maybe_executed: false,
        });
    }
    // SAFETY: CopyMultiple returned +1. Create-rule takes that retain.
    let owned = unsafe { CFArray::<CFType>::wrap_under_create_rule(values) };
    let mut retained = Vec::with_capacity(ATTRS.len());
    for index in 0..ATTRS.len() {
        let ptr = owned
            .get(index as isize)
            .map(|item| item.as_CFTypeRef())
            .unwrap_or(std::ptr::null());
        retained.push(world::retain(ptr));
    }
    Ok(Slots(retained))
}

fn slot_string(slots: &[CFTypeRef], index: usize) -> Option<String> {
    string_value(*slots.get(index)?)
}

fn slot_bool(slots: &[CFTypeRef], index: usize) -> Option<bool> {
    bool_value(*slots.get(index)?)
}

fn slot_usize(slots: &[CFTypeRef], index: usize) -> Option<usize> {
    usize::try_from(number_value(*slots.get(index)?)?).ok()
}

fn slot_range(slots: &[CFTypeRef], index: usize) -> Option<TextRange> {
    range_value(*slots.get(index)?)
}

fn slot_frame(slots: &[CFTypeRef]) -> Option<Rect> {
    let origin = point_value(*slots.get(7)?)?;
    let size = size_value(*slots.get(8)?)?;
    Some(Rect {
        x: origin.0,
        y: origin.1,
        w: size.0,
        h: size.1,
    })
}

fn string_value(ptr: CFTypeRef) -> Option<String> {
    if missing(ptr) {
        return None;
    }
    // SAFETY: ptr is a live +1 slot. Get-rule retains a copy Drop will release.
    let typed = unsafe { CFType::wrap_under_get_rule(ptr) };
    if !typed.instance_of::<CFString>() {
        return None;
    }
    // SAFETY: the type id matches CFString.
    Some(unsafe { CFString::wrap_under_get_rule(ptr as _) }.to_string())
}

pub(super) fn bool_value(ptr: CFTypeRef) -> Option<bool> {
    if missing(ptr) {
        return None;
    }
    // SAFETY: get-rule retain of a live object. Drop releases that retain.
    let typed = unsafe { CFType::wrap_under_get_rule(ptr) };
    if !typed.instance_of::<CFBoolean>() {
        return None;
    }
    // SAFETY: the type id matches CFBoolean.
    Some(bool::from(unsafe {
        CFBoolean::wrap_under_get_rule(ptr as _)
    }))
}

fn number_value(ptr: CFTypeRef) -> Option<i64> {
    if missing(ptr) {
        return None;
    }
    let typed = unsafe { CFType::wrap_under_get_rule(ptr) };
    if !typed.instance_of::<CFNumber>() {
        return None;
    }
    // SAFETY: the type id matches CFNumber.
    unsafe { CFNumber::wrap_under_get_rule(ptr as _) }.to_i64()
}

fn point_value(ptr: CFTypeRef) -> Option<(f64, f64)> {
    ax_pair(ptr, kAXValueTypeCGPoint)
}

fn size_value(ptr: CFTypeRef) -> Option<(f64, f64)> {
    ax_pair(ptr, kAXValueTypeCGSize)
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
    // SAFETY: ptr is an AXValue and Pair matches CGPoint and CGSize.
    let ok = unsafe {
        crate::diagnostics::ax_call("AXValueGetValue", (ptr as AXValueRef) as usize, "", || {
            AXValueGetValue(ptr as AXValueRef, kind, std::ptr::addr_of_mut!(pair).cast())
        })
    };
    ok.then_some((pair.a, pair.b))
}

fn range_value(ptr: CFTypeRef) -> Option<TextRange> {
    if !is_ax_value(ptr) {
        return None;
    }
    // SAFETY: ptr was classified as AXValue.
    if unsafe {
        crate::diagnostics::ax_call("AXValueGetType", (ptr as AXValueRef) as usize, "", || {
            AXValueGetType(ptr as AXValueRef)
        })
    } != kAXValueTypeCFRange
    {
        return None;
    }
    let mut range = CFRange {
        location: 0,
        length: 0,
    };
    // SAFETY: the value type is CFRange and `range` has that layout.
    let ok = unsafe {
        crate::diagnostics::ax_call("AXValueGetValue", (ptr as AXValueRef) as usize, "", || {
            AXValueGetValue(
                ptr as AXValueRef,
                kAXValueTypeCFRange,
                std::ptr::addr_of_mut!(range).cast(),
            )
        })
    };
    if !ok || range.location < 0 || range.length < 0 {
        return None;
    }
    Some(TextRange {
        start: range.location as usize,
        length: range.length as usize,
    })
}

fn is_ax_value(ptr: CFTypeRef) -> bool {
    // SAFETY: CFGetTypeID and AXValueGetTypeID accept a live or null-checked pointer.
    !ptr.is_null()
        && unsafe { CFGetTypeID(ptr) }
            == unsafe {
                crate::diagnostics::ax_call("AXValueGetTypeID", 0, "", || AXValueGetTypeID())
            }
}

fn missing(ptr: CFTypeRef) -> bool {
    if ptr.is_null() {
        return true;
    }
    // SAFETY: kCFNull is a process-lifetime CFNull singleton.
    if ptr == unsafe { kCFNull as CFTypeRef } {
        return true;
    }
    is_ax_value(ptr)
        && unsafe {
            crate::diagnostics::ax_call("AXValueGetType", (ptr as AXValueRef) as usize, "", || {
                AXValueGetType(ptr as AXValueRef)
            })
        } == kAXValueTypeAXError
}

fn read_value(element: AXUIElementRef, chars: Option<usize>, limit: usize) -> Option<NodeValue> {
    if chars.is_some_and(|count| count > limit) {
        if let Some(text) = string_for_range(element, limit) {
            return Some(NodeValue::Text(text));
        }
    }
    copy_single_value(element)
}

fn string_for_range(element: AXUIElementRef, len: usize) -> Option<String> {
    let range = CFRange {
        location: 0,
        length: len as isize,
    };
    // SAFETY: AXValueCreate copies the range bytes. The result is +1.
    let parameter = unsafe {
        crate::diagnostics::ax_call("AXValueCreate", 0, "", || {
            AXValueCreate(kAXValueTypeCFRange, std::ptr::addr_of!(range).cast())
        })
    };
    if parameter.is_null() {
        return None;
    }
    let mut result: CFTypeRef = std::ptr::null();
    let name = CFString::new(kAXStringForRangeParameterizedAttribute);
    // SAFETY: element and parameter are live. result is +1 on success.
    let status = unsafe {
        crate::diagnostics::ax_call(
            "AXUIElementCopyParameterizedAttributeValue",
            (element) as usize,
            kAXStringForRangeParameterizedAttribute,
            || {
                AXUIElementCopyParameterizedAttributeValue(
                    element,
                    name.as_concrete_TypeRef(),
                    parameter as CFTypeRef,
                    &mut result,
                )
            },
        )
    };
    world::release(parameter as CFTypeRef);
    if status != kAXErrorSuccess || result.is_null() {
        if !result.is_null() {
            world::release(result);
        }
        return None;
    }
    let text = string_value(result);
    world::release(result);
    text
}

fn copy_single_value(element: AXUIElementRef) -> Option<NodeValue> {
    let Ok(Some(copied)) = copy_attribute(element, kAXValueAttribute) else {
        return None;
    };
    let value = if let Some(text) = string_value(copied) {
        Some(NodeValue::Text(text))
    } else if let Some(number) = float_value(copied) {
        Some(NodeValue::Number(number))
    } else {
        bool_value(copied).map(NodeValue::Bool)
    };
    world::release(copied);
    value
}

fn copy_attribute(element: AXUIElementRef, name: &str) -> Result<Option<CFTypeRef>> {
    let mut result: CFTypeRef = std::ptr::null();
    let cf = CFString::new(name);
    // SAFETY: element is live. result is +1 on success; the caller releases it.
    let status = unsafe {
        crate::diagnostics::ax_call(
            "AXUIElementCopyAttributeValue",
            (element) as usize,
            name,
            || AXUIElementCopyAttributeValue(element, cf.as_concrete_TypeRef(), &mut result),
        )
    };
    if status == kAXErrorAPIDisabled {
        return Err(DriverError::PermissionMissing {
            permission: "accessibility".into(),
        });
    }
    if status != kAXErrorSuccess || result.is_null() {
        if !result.is_null() {
            world::release(result);
        }
        return Ok(None);
    }
    Ok(Some(result))
}

fn float_value(ptr: CFTypeRef) -> Option<f64> {
    if missing(ptr) {
        return None;
    }
    let typed = unsafe { CFType::wrap_under_get_rule(ptr) };
    if !typed.instance_of::<CFNumber>() {
        return None;
    }
    // SAFETY: the type id matches CFNumber.
    unsafe { CFNumber::wrap_under_get_rule(ptr as _) }.to_f64()
}

fn value_is_settable(element: AXUIElementRef) -> bool {
    let mut flag: u8 = 0;
    let name = CFString::new(kAXValueAttribute);
    // SAFETY: element is live. flag is an out-parameter.
    let status = unsafe {
        crate::diagnostics::ax_call(
            "AXUIElementIsAttributeSettable",
            (element) as usize,
            kAXValueAttribute,
            || AXUIElementIsAttributeSettable(element, name.as_concrete_TypeRef(), &mut flag),
        )
    };
    status == kAXErrorSuccess && flag != 0
}

fn copy_actions(element: AXUIElementRef) -> Vec<NodeAction> {
    let mut names: CFArrayRef = std::ptr::null();
    // SAFETY: element is live. names is +1 on success.
    let status = unsafe {
        crate::diagnostics::ax_call("AXUIElementCopyActionNames", (element) as usize, "", || {
            AXUIElementCopyActionNames(element, &mut names)
        })
    };
    if status != kAXErrorSuccess || names.is_null() {
        if !names.is_null() {
            world::release(names as CFTypeRef);
        }
        return Vec::new();
    }
    // SAFETY: CopyActionNames returned +1. Create-rule takes it.
    let owned = unsafe { CFArray::<CFType>::wrap_under_create_rule(names) };
    let mut actions = Vec::new();
    for index in 0..owned.len() {
        if let Some(item) = owned.get(index) {
            if let Some(name) = string_value(item.as_CFTypeRef()) {
                actions.push(map_ax_action(&name));
            }
        }
    }
    actions
}

fn take_children(world: &mut World, slots: &[CFTypeRef]) -> Vec<u64> {
    let chosen = crate::ax::select_children(
        slot_elements(slots, 20),
        slot_elements(slots, 21),
        slot_elements(slots, 22).unwrap_or_default(),
    );
    chosen
        .into_iter()
        .map(|element| world.insert(element.into_inner()))
        .collect()
}

fn slot_elements(slots: &[CFTypeRef], index: usize) -> Option<Vec<OwnedElement>> {
    elements_of(*slots.get(index)?)
}

fn named_elements(element: AXUIElementRef, name: &str) -> Result<Vec<OwnedElement>> {
    let Some(value) = copy_attribute(element, name)? else {
        return Ok(Vec::new());
    };
    let elements = elements_of(value).unwrap_or_default();
    world::release(value);
    Ok(elements)
}

fn elements_of(ptr: CFTypeRef) -> Option<Vec<OwnedElement>> {
    if missing(ptr) {
        return None;
    }
    if is_element(ptr) {
        return Some(vec![OwnedElement::from_retained(
            world::retain(ptr) as AXUIElementRef
        )]);
    }
    // SAFETY: get-rule retain while we inspect the type.
    let typed = unsafe { CFType::wrap_under_get_rule(ptr) };
    if !typed.instance_of::<CFArray>() {
        return None;
    }
    // SAFETY: the type id matches CFArray. Get-rule retains a wrapper only.
    let array = unsafe { CFArray::<CFType>::wrap_under_get_rule(ptr as CFArrayRef) };
    let mut out = Vec::new();
    for index in 0..array.len() {
        if let Some(item) = array.get(index) {
            let child = item.as_CFTypeRef();
            if is_element(child) {
                out.push(OwnedElement::from_retained(
                    world::retain(child) as AXUIElementRef
                ));
            }
        }
    }
    Some(out)
}

fn is_element(ptr: CFTypeRef) -> bool {
    // SAFETY: both type-id calls are pure queries. ptr is non-null.
    !ptr.is_null()
        && unsafe { CFGetTypeID(ptr) }
            == unsafe {
                crate::diagnostics::ax_call("AXUIElementGetTypeID", 0, "", || {
                    AXUIElementGetTypeID()
                })
            }
}
