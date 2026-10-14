//! SelectText checks scalar bounds and provider endpoint semantics before Select.
use super::*;
use gcu_core::model::TextRange;
const TEXT_CAP: i32 = 1_048_576;
pub(super) fn select(
    el: &IUIAutomationElement,
    requested: TextRange,
    r: &Guard<'_>,
) -> Prepared<PatternResult> {
    let end = requested
        .start
        .checked_add(requested.length)
        .filter(|v| *v <= i32::MAX as usize)
        .ok_or(DriverError::InvalidRequest)?;
    let p: IUIAutomationTextPattern = pattern(el, UIA_TextPatternId, r)?;
    if read!(r, p.SupportedTextSelection()) == SupportedTextSelection_None {
        return Err(ActionError::Unavailable);
    }
    let range = read!(r, p.DocumentRange());
    let text = read!(r, range.GetText(TEXT_CAP)).to_string();
    // Refuse truncated document text: bounds and scalar correspondence must be proven.
    if text.encode_utf16().count() >= TEXT_CAP as usize || end > text.chars().count() {
        return Err(DriverError::InvalidRequest.into());
    }
    let prefix: String = text.chars().take(requested.start).collect();
    let expected: String = text
        .chars()
        .skip(requested.start)
        .take(requested.length)
        .collect();
    let prefix_range = read!(r, range.Clone());
    read!(
        r,
        prefix_range.MoveEndpointByRange(
            TextPatternRangeEndpoint_End,
            &range,
            TextPatternRangeEndpoint_Start
        )
    );
    if read!(
        r,
        prefix_range.MoveEndpointByUnit(
            TextPatternRangeEndpoint_End,
            TextUnit_Character,
            requested.start as i32
        )
    ) != requested.start as i32
        || read!(r, prefix_range.GetText(TEXT_CAP)) != prefix
    {
        return Err(DriverError::InvalidRequest.into());
    }
    read!(
        r,
        range.MoveEndpointByRange(
            TextPatternRangeEndpoint_Start,
            &prefix_range,
            TextPatternRangeEndpoint_End
        )
    );
    read!(
        r,
        range.MoveEndpointByRange(
            TextPatternRangeEndpoint_End,
            &range,
            TextPatternRangeEndpoint_Start
        )
    );
    if read!(
        r,
        range.MoveEndpointByUnit(
            TextPatternRangeEndpoint_End,
            TextUnit_Character,
            requested.length as i32
        )
    ) != requested.length as i32
        || read!(r, range.GetText(TEXT_CAP)) != expected
    {
        return Err(DriverError::InvalidRequest.into());
    }
    Ok(mutate!(r, range.Select()))
}
