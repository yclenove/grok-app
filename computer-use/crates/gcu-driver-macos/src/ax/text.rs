//! `SelectText` via `AXSelectedTextRange`. No macOS calls.

use gcu_core::driver::{Delivery, DriverError, Result};
use gcu_core::model::TextRange;

use super::actions::{guarded, Budget};
use super::source::{AxCall, AxSource};

/// Set the selected text range when that attribute is settable.
///
/// A range that does not fit a native `CFRange` is [`DriverError::InvalidRequest`]
/// and is not sent. The call stops at the budget in `budget`. Cancel before the
/// write reports `maybe_executed = false`.
pub(super) async fn select_text<S: AxSource>(
    source: &S,
    node: &S::Node,
    range: TextRange,
    budget: &Budget<'_>,
) -> Result<Delivery> {
    if !range_fits(range) {
        return Err(DriverError::InvalidRequest);
    }
    if guarded(
        budget,
        source.attribute_settable(node, "AXSelectedTextRange"),
    )
    .await?
        != Some(true)
    {
        return Err(DriverError::ElementNotActionable);
    }
    budget.begin();
    source.note("set:AXSelectedTextRange");
    match guarded(budget, source.set_text_range(node, range)).await {
        Ok(AxCall::Performed) => {
            budget.finish(true);
            Ok(Delivery::Accessibility)
        }
        Ok(AxCall::Unsupported | AxCall::Unavailable) => {
            budget.finish(false);
            Err(DriverError::ElementNotActionable)
        }
        Err(error) => {
            budget.finish(error.maybe_executed());
            Err(error)
        }
    }
}

fn range_fits(range: TextRange) -> bool {
    let Ok(start) = isize::try_from(range.start) else {
        return false;
    };
    let Ok(length) = isize::try_from(range.length) else {
        return false;
    };
    start.checked_add(length).is_some()
}
