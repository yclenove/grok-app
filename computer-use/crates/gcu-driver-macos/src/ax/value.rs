//! `SetValue` for a settable `AXValue`. No macOS calls.
//!
//! Password roles and terminal-like controls are refused before any write.
//! The typed text is not logged.

use gcu_core::driver::{Delivery, DriverError, Result};
use gcu_core::model::NodeValue;

use super::actions::{guarded, Budget};
use super::roles::map_ax_role;
use super::sensitive::is_terminal_like;
use super::source::{AxCall, AxSource};

/// Replace `AXValue` when the attribute is settable.
///
/// A refusal or a failed numeric parse happens before the write, with
/// `maybe_executed = false`. An uncertain write is not repeated.
pub(super) async fn set_value<S: AxSource>(
    source: &S,
    node: &S::Node,
    value: &str,
    budget: &Budget<'_>,
) -> Result<Delivery> {
    let role = guarded(budget, source.role(node)).await?;
    let subrole = guarded(budget, source.subrole(node)).await?;
    let title = guarded(budget, source.title(node)).await?;
    let mapped = map_ax_role(&role, subrole.as_deref());
    if mapped.password
        || is_terminal_like(
            mapped.native_role.as_str(),
            mapped.subrole.as_deref(),
            title.as_deref(),
        )
    {
        source.note("refuse:sensitive");
        return Err(DriverError::NotSettable);
    }
    if guarded(budget, source.attribute_settable(node, "AXValue")).await? != Some(true) {
        source.note("refuse:not-settable");
        return Err(DriverError::NotSettable);
    }
    let current = guarded(budget, source.value(node)).await?;
    let numeric = matches!(
        current,
        Some(NodeValue::Number(_)) | Some(NodeValue::Range { .. })
    );
    let number = if numeric {
        match value
            .parse::<f64>()
            .ok()
            .filter(|number| number.is_finite())
        {
            Some(number) => Some(number),
            None => {
                source.note("refuse:not-settable");
                return Err(DriverError::NotSettable);
            }
        }
    } else {
        None
    };
    budget.begin();
    source.note("set:AXValue");
    let result = if let Some(number) = number {
        guarded(budget, source.set_number(node, "AXValue", number)).await
    } else {
        guarded(budget, source.set_string(node, "AXValue", value)).await
    };
    match result {
        Ok(AxCall::Performed) => {
            budget.finish(true);
            Ok(Delivery::Accessibility)
        }
        Ok(AxCall::Unsupported | AxCall::Unavailable) => {
            budget.finish(false);
            Err(DriverError::NotSettable)
        }
        Err(error) => {
            budget.finish(error.maybe_executed());
            Err(error)
        }
    }
}
