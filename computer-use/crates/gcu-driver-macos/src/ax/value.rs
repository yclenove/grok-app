//! `SetValue` for a settable `AXValue`. No macOS calls.
//!
//! Password roles and terminal-like controls are refused before any write.
//! A performed write is read back inside a settle window. A value that still
//! does not match is [`DriverError::NotSettable`] with `maybe_executed = true`:
//! the write was submitted and may already have landed. Numeric controls may
//! apply on the next frame, so the readback polls a fresh value. The typed
//! text is not logged.

use std::time::Duration;

use gcu_core::driver::{Delivery, DriverError, Result};
use gcu_core::model::NodeValue;

use crate::input::clock::Clock;

use super::actions::{guarded, Budget};
use super::roles::map_ax_role;
use super::sensitive::is_terminal_like;
use super::source::{AxCall, AxSource};

/// One frame at 60 Hz. The first read is immediate; later reads wait this long.
pub const READBACK_GAP: Duration = Duration::from_millis(16);
/// Settle budget. Twenty frames, and always inside [`gcu_core::driver::DRIVER_DEADLINE`].
pub const READBACK_BUDGET: Duration = Duration::from_millis(320);
const NUMBER_EPSILON: f64 = 1e-3;

/// Replace `AXValue` when the attribute is settable.
///
/// A refusal or a failed numeric parse happens before the write, with
/// `maybe_executed = false`. An uncertain write is not repeated.
pub(super) async fn set_value<S: AxSource>(
    source: &S,
    node: &S::Node,
    value: &str,
    budget: &Budget<'_>,
    clock: &dyn Clock,
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
        return Err(DriverError::NotSettable {
            maybe_executed: false,
        });
    }
    if guarded(budget, source.attribute_settable(node, "AXValue")).await? != Some(true) {
        source.note("refuse:not-settable");
        return Err(DriverError::NotSettable {
            maybe_executed: false,
        });
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
                return Err(DriverError::NotSettable {
                    maybe_executed: false,
                });
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
            match confirm_readback(source, node, value, number, budget, clock).await {
                Ok(()) => {
                    budget.finish(true);
                    Ok(Delivery::Accessibility)
                }
                Err(error) => {
                    budget.finish(error.maybe_executed());
                    Err(error)
                }
            }
        }
        Ok(AxCall::Unsupported | AxCall::Unavailable) => {
            budget.finish(false);
            Err(DriverError::NotSettable {
                maybe_executed: false,
            })
        }
        Err(error) => {
            budget.finish(error.maybe_executed());
            Err(error)
        }
    }
}

/// Poll a fresh value until it matches, the settle window ends, or Stop fires.
///
/// Cancel during the window is [`DriverError::Cancelled`] with
/// `maybe_executed = true`. A mismatch after the window is
/// [`DriverError::NotSettable`] with `maybe_executed = true`.
async fn confirm_readback<S: AxSource>(
    source: &S,
    node: &S::Node,
    value: &str,
    number: Option<f64>,
    budget: &Budget<'_>,
    clock: &dyn Clock,
) -> Result<()> {
    let settle_end = clock.now() + READBACK_BUDGET.min(budget.remaining());
    loop {
        if budget.is_cancelled() {
            return Err(DriverError::Cancelled {
                maybe_executed: true,
            });
        }
        match guarded(budget, source.fresh_value(node)).await {
            Ok(now) if value_matches(&now, value, number) => return Ok(()),
            Ok(_) => {}
            Err(error) => return Err(mark_executed(error)),
        }
        let now = clock.now();
        if now >= settle_end || budget.remaining().is_zero() {
            source.note("reject:readback");
            return Err(DriverError::NotSettable {
                maybe_executed: true,
            });
        }
        let wake = (now + READBACK_GAP).min(settle_end);
        tokio::select! {
            biased;
            _ = budget.until_cancel() => {
                return Err(DriverError::Cancelled {
                    maybe_executed: true,
                });
            }
            _ = clock.wait_until(wake) => {}
        }
    }
}

fn value_matches(now: &Option<NodeValue>, text: &str, number: Option<f64>) -> bool {
    if let Some(want) = number {
        let got = match now {
            Some(NodeValue::Number(value)) => *value,
            Some(NodeValue::Range { value, .. }) => *value,
            _ => return false,
        };
        return (got - want).abs() <= NUMBER_EPSILON;
    }
    matches!(now, Some(NodeValue::Text(got)) if got == text)
}

fn mark_executed(error: DriverError) -> DriverError {
    if error.maybe_executed() {
        error
    } else {
        DriverError::Native {
            message: "AXValue readback failed".into(),
            maybe_executed: true,
        }
    }
}
