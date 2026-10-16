//! Native pattern preparation and mutation classification, on the existing MTA.
use super::super::{patterns::*, source::NodeId};
use super::{Request, State};
use gcu_core::driver::{DriverError, Result};
use windows::{
    core::{Interface, BSTR},
    Win32::UI::Accessibility::*,
};

// Each expansion checks abandonment/deadline before one COM call. The interfaces
// passed here are obtained and destroyed on this same persistent MTA.
macro_rules! read {
    ($request:expr, $call:expr) => {{
        $request.check()?;
        // SAFETY: live UIA interface and owned arguments on their creating MTA.
        unsafe { $call }.map_err(ActionError::from)?
    }};
}
macro_rules! mutate {
    ($request:expr, $call:expr) => {{
        $request.dispatch(|| {
            // SAFETY: live pattern on its MTA; identity/writability were checked.
            unsafe { $call }
                .map(|()| PatternResult::Performed)
                .or_else(mutation_error)
        })?
    }};
}
mod scroll;
mod text;
mod value;

use super::super::patterns::PatternError as ActionError;
impl From<windows::core::Error> for ActionError {
    fn from(error: windows::core::Error) -> Self {
        Self::from_hresult(error.code().0, false)
    }
}
type Prepared<T> = std::result::Result<T, ActionError>;
fn mutation_error(error: windows::core::Error) -> Result<PatternResult> {
    match ActionError::from_hresult(error.code().0, true) {
        ActionError::Unsupported => Ok(PatternResult::Unsupported),
        ActionError::Unavailable => Ok(PatternResult::Unavailable),
        ActionError::Failure(error) => Err(error),
    }
}

fn pattern<T: Interface>(
    el: &IUIAutomationElement,
    id: UIA_PATTERN_ID,
    r: &Guard<'_>,
) -> Prepared<T> {
    Ok(read!(r, el.GetCurrentPatternAs::<T>(id)))
}
pub(super) fn perform(
    state: &State,
    node: NodeId,
    hwnd: u64,
    action: PatternAction,
    r: &Request,
) -> Result<PatternResult> {
    let (el, props) = state.validate(node, hwnd, r)?;
    if !action.pattern().available(&props.patterns) {
        return Ok(PatternResult::Unsupported);
    }
    if props.password
        || props.partial
        || !props.enabled
        || super::super::sensitive::is_execution_surface(&props)
    {
        return Ok(PatternResult::Unavailable);
    }
    let guard = Guard {
        state,
        node,
        hwnd,
        request: r,
        action: &action,
    };
    let result = execute(&el, &action, &guard);
    match result {
        Ok(v) => Ok(v),
        Err(ActionError::Unsupported) => Ok(PatternResult::Unsupported),
        Err(ActionError::Unavailable) => Ok(PatternResult::Unavailable),
        Err(ActionError::Failure(e)) => Err(e),
    }
}
fn execute(
    el: &IUIAutomationElement,
    action: &PatternAction,
    r: &Guard<'_>,
) -> Prepared<PatternResult> {
    let result = match action {
        PatternAction::Invoke => {
            let p: IUIAutomationInvokePattern = pattern(el, UIA_InvokePatternId, r)?;
            mutate!(r, p.Invoke())
        }
        PatternAction::Toggle => {
            let p: IUIAutomationTogglePattern = pattern(el, UIA_TogglePatternId, r)?;
            mutate!(r, p.Toggle())
        }
        PatternAction::Select => {
            let p: IUIAutomationSelectionItemPattern = pattern(el, UIA_SelectionItemPatternId, r)?;
            mutate!(r, p.Select())
        }
        PatternAction::Expand | PatternAction::Collapse => {
            let p: IUIAutomationExpandCollapsePattern =
                pattern(el, UIA_ExpandCollapsePatternId, r)?;
            if read!(r, p.CurrentExpandCollapseState()) == ExpandCollapseState_LeafNode {
                return Err(ActionError::Unavailable);
            }
            if matches!(action, PatternAction::Expand) {
                mutate!(r, p.Expand())
            } else {
                mutate!(r, p.Collapse())
            }
        }
        PatternAction::SetValue(text) => {
            let p: IUIAutomationValuePattern = pattern(el, UIA_ValuePatternId, r)?;
            if read!(r, p.CurrentIsReadOnly()).as_bool() {
                return Err(ActionError::Unavailable);
            }
            mutate!(r, p.SetValue(&BSTR::from(text)))
        }
        PatternAction::SetRangeValue(_) | PatternAction::AdjustRange(_) => {
            return value::set(el, action, r)
        }
        PatternAction::Scroll { dir, pages } => return scroll::scroll(el, *dir, *pages, r),
        PatternAction::ScrollIntoView => {
            let p: IUIAutomationScrollItemPattern = pattern(el, UIA_ScrollItemPatternId, r)?;
            mutate!(r, p.ScrollIntoView())
        }
        PatternAction::SelectText(range) => return text::select(el, *range, r),
        PatternAction::LegacyDefault => {
            let p: IUIAutomationLegacyIAccessiblePattern =
                pattern(el, UIA_LegacyIAccessiblePatternId, r)?;
            mutate!(r, p.DoDefaultAction())
        }
    };
    Ok(result)
}

struct Guard<'a> {
    state: &'a State,
    node: NodeId,
    hwnd: u64,
    request: &'a Request,
    action: &'a PatternAction,
}
impl Guard<'_> {
    fn check(&self) -> Result<()> {
        self.request.check()
    }
    fn dispatch(&self, mutation: impl FnOnce() -> Result<PatternResult>) -> Result<PatternResult> {
        // Pattern acquisition and TextPattern preparation can cross provider calls.
        // Recheck again here, immediately before the one actual mutation.
        let (_, props) = self.state.validate(self.node, self.hwnd, self.request)?;
        let writing = matches!(
            self.action,
            PatternAction::SetValue(_)
                | PatternAction::SetRangeValue(_)
                | PatternAction::AdjustRange(_)
        );
        if props.password
            || props.partial
            || !props.enabled
            || super::super::sensitive::is_execution_surface(&props)
            || (writing && props.read_only != Some(false))
        {
            return Err(if writing {
                DriverError::NotSettable {
                    maybe_executed: false,
                }
            } else {
                DriverError::ElementNotActionable
            });
        }
        self.request.dispatch(mutation)
    }
}
