//! Numeric writes validate live readonly/bounds/change before one SetValue.
use super::*;
pub(super) fn set(
    el: &IUIAutomationElement,
    action: &PatternAction,
    r: &Guard<'_>,
) -> Prepared<PatternResult> {
    let p: IUIAutomationRangeValuePattern = pattern(el, UIA_RangeValuePatternId, r)?;
    if read!(r, p.CurrentIsReadOnly()).as_bool() {
        return Err(ActionError::Unavailable);
    }
    let min = read!(r, p.CurrentMinimum());
    let max = read!(r, p.CurrentMaximum());
    let current_step = if matches!(action, PatternAction::AdjustRange(_)) {
        Some((read!(r, p.CurrentValue()), read!(r, p.CurrentSmallChange())))
    } else {
        None
    };
    let value = super::super::super::value::range_number(action, min, max, current_step)?;
    Ok(mutate!(r, p.SetValue(value)))
}
