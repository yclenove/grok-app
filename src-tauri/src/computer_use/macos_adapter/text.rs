//! Exact focused-control append using public AX selection attributes. Never
//! read/replace AXValue, copy to the clipboard, or synthesize global keystrokes.
use super::ax_api::{self, Budget, Element};
use super::*;
use grok_computer_use_core::native_action::NativeActionGuard;

const RANGE: &str = "AXSelectedTextRange";
const TEXT: &str = "AXSelectedText";
const COUNT: &str = "AXNumberOfCharacters";
const AX_CF_RANGE: u32 = 4;
const CF_INDEX: i32 = 14;

#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq, Eq)]
struct CfRange {
    location: isize,
    length: isize,
}

fn unprotected(budget: &Budget<'_>, element: &Element) -> Result<(), String> {
    let role = budget.role(element.raw())?;
    if !matches!(role.as_str(), "AXTextField" | "AXTextArea" | "AXComboBox")
        || budget.text(element.raw(), "AXSubrole")?.as_deref() == Some("AXSecureTextField")
    {
        return Err("AX append target is no longer an unprotected text control".into());
    }
    Ok(())
}

fn writable(budget: &Budget<'_>, element: &Element) -> Result<bool, String> {
    unprotected(budget, element)?;
    if !budget.attribute_settable(element, RANGE)? {
        return Ok(false);
    }
    unprotected(budget, element)?;
    let supported = budget.attribute_settable(element, TEXT)?;
    unprotected(budget, element)?;
    Ok(supported)
}

fn count(budget: &Budget<'_>, element: &Element) -> Result<Option<isize>, String> {
    unprotected(budget, element)?;
    let Some(value) = budget.optional(element.raw(), COUNT)? else {
        return Ok(None);
    };
    let mut count: isize = 0;
    if unsafe { ax_api::CFGetTypeID(value.0) != ax_api::CFNumberGetTypeID() }
        || !unsafe { CFNumberGetValue(value.0, CF_INDEX, (&mut count as *mut isize).cast()) }
        || count < 0
        || count > isize::MAX - (super::super::protocol::TEXT_MAX_CHARS * 2) as isize
    {
        return Err("AX character count is not a bounded nonnegative CFIndex".into());
    }
    budget.check()?;
    Ok(Some(count))
}

fn selection(budget: &Budget<'_>, element: &Element) -> Result<Option<CfRange>, String> {
    unprotected(budget, element)?;
    let Some(value) = budget.optional(element.raw(), RANGE)? else {
        return Ok(None);
    };
    let mut range = CfRange::default();
    if unsafe {
        ax_api::CFGetTypeID(value.0) != ax_api::AXValueGetTypeID()
            || ax_api::AXValueGetType(value.0) != AX_CF_RANGE
    } || !unsafe {
        ax_api::AXValueGetValue(value.0, AX_CF_RANGE, (&mut range as *mut CfRange).cast())
    } || range.location < 0
        || range.length < 0
        || range.location.checked_add(range.length).is_none()
    {
        return Err("AX selected text range has invalid type or bounds".into());
    }
    budget.check()?;
    Ok(Some(range))
}

pub(super) fn supported(budget: &Budget<'_>, element: &Element) -> Result<bool, String> {
    if !writable(budget, element)? {
        return Ok(false);
    }
    let (Some(count), Some(range)) = (count(budget, element)?, selection(budget, element)?) else {
        return Ok(false);
    };
    Ok(range.location + range.length <= count)
}

pub(super) struct PreparedText {
    text_key: OwnedCf,
    range_key: OwnedCf,
    value: OwnedCf,
    empty: bool,
}
impl PreparedText {
    pub(super) fn new(parameters: &serde_json::Value) -> Result<Self, String> {
        let object = parameters
            .as_object()
            .ok_or("type_text parameters required")?;
        let text = object
            .get("text")
            .and_then(serde_json::Value::as_str)
            .ok_or("type_text text required")?;
        // Explicit clipboard requests are NOT permission to substitute AX input.
        if object.len() != 1
            || text.contains('\0')
            || text.chars().count() > super::super::protocol::TEXT_MAX_CHARS
        {
            return Err("unsupported type_text parameters or text length".into());
        }
        Ok(Self {
            text_key: OwnedCf::new(cf_str(TEXT), "AX text key allocation failed")?,
            range_key: OwnedCf::new(cf_str(RANGE), "AX range key allocation failed")?,
            value: OwnedCf::new(cf_str(text), "AX text allocation failed")?,
            empty: text.is_empty(),
        })
    }

    pub(super) fn append(
        &self,
        budget: &Budget<'_>,
        element: &Element,
        owner: &mut NativeActionGuard<'_>,
        before: impl Fn() -> Result<(), String>,
        authority: impl Fn() -> Result<(), String>,
    ) -> Result<bool, String> {
        before()?;
        if self.empty {
            return Ok(false);
        }
        let end = count(budget, element)?.ok_or("AX character count unavailable")?;
        // Use the control's native index domain, never Rust scalar counts or a
        // copied AXValue. Allocate the complete write before moving selection.
        let range = CfRange {
            location: end,
            length: 0,
        };
        let native = OwnedCf::new(
            unsafe { ax_api::AXValueCreate(AX_CF_RANGE, (&range as *const CfRange).cast()) },
            "AX insertion range allocation failed",
        )?;
        let available = || {
            // Recheck protection/focus after allocation and after the selection
            // call BEFORE querying any writable text attribute or character count.
            before()?;
            if !writable(budget, element)? || count(budget, element)? != Some(end) {
                return Err("AX append endpoint or writable attributes changed".into());
            }
            before()
        };
        let status = budget.set_attribute(element, &self.range_key, &native, available)?;
        if status != 0 {
            owner.retain_until_native_recovery();
            return Err(format!(
                "AX insertion selection completion unknown ({status}); replay forbidden"
            ));
        }
        // A successful selection call can be ignored by a control, or race a
        // user edit. Read back the zero-length endpoint; never replace selection.
        let status = budget.set_attribute(element, &self.text_key, &self.value, || {
            available()?;
            if selection(budget, element)? != Some(range) || count(budget, element)? != Some(end) {
                return Err("AX insertion selection or text changed; append not sent".into());
            }
            // Native reads can run callbacks. Authority must be checked AFTER
            // them, including Stop, snapshot retirement, focus and secure role.
            before()?;
            // Focus/identity validation itself performs remote AX reads. A user
            // can move selection while those calls run, so the insertion witness
            // must be refreshed AFTER them, with only local authority checks
            // following it. AX provides no atomic compare-and-append operation;
            // this narrows, but cannot eliminate, the final external edit race.
            if count(budget, element)? != Some(end) || selection(budget, element)? != Some(range) {
                return Err("AX append endpoint changed during final validation".into());
            }
            authority()
        })?;
        if status != 0 {
            owner.retain_until_native_recovery();
            return Err(format!(
                "AX text append completion unknown ({status}); replay forbidden"
            ));
        }
        before()?;
        // AX success is not an application postcondition; no whole-value read.
        Ok(true)
    }
}
