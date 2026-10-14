//! ScrollPattern delivery never calls SetFocus or activates a window.
use super::*;
use gcu_core::driver::Dir;
pub(super) fn scroll(
    el: &IUIAutomationElement,
    dir: Dir,
    pages: f32,
    r: &Guard<'_>,
) -> Prepared<PatternResult> {
    if !pages.is_finite() || pages <= 0. || pages > 1. {
        return Err(DriverError::InvalidRequest.into());
    }
    let p: IUIAutomationScrollPattern = pattern(el, UIA_ScrollPatternId, r)?;
    let horizontal = matches!(dir, Dir::Left | Dir::Right);
    let forward = matches!(dir, Dir::Right | Dir::Down);
    let scrollable = if horizontal {
        read!(r, p.CurrentHorizontallyScrollable())
    } else {
        read!(r, p.CurrentVerticallyScrollable())
    };
    if !scrollable.as_bool() {
        return Err(ActionError::Unavailable);
    }
    if pages == 1. {
        let amount = if forward {
            ScrollAmount_LargeIncrement
        } else {
            ScrollAmount_LargeDecrement
        };
        let (x, y) = if horizontal {
            (amount, ScrollAmount_NoAmount)
        } else {
            (ScrollAmount_NoAmount, amount)
        };
        return Ok(mutate!(r, p.Scroll(x, y)));
    }
    let percent = if horizontal {
        read!(r, p.CurrentHorizontalScrollPercent())
    } else {
        read!(r, p.CurrentVerticalScrollPercent())
    };
    let view = if horizontal {
        read!(r, p.CurrentHorizontalViewSize())
    } else {
        read!(r, p.CurrentVerticalViewSize())
    };
    let target = super::super::super::scroll::fraction_target(percent, view, pages, forward)
        .ok_or(ActionError::Unavailable)?;
    let (x, y) = if horizontal {
        (target, -1.)
    } else {
        (-1., target)
    };
    Ok(mutate!(r, p.SetScrollPercent(x, y)))
}
