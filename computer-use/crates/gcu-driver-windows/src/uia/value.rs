//! SetValue permits only proven writable, nonsensitive native patterns.
use super::{
    actions::{finish, Budget},
    cache::Properties,
    patterns::*,
    source::{NodeId, UiaSource},
};
use gcu_core::driver::{Delivery, DriverError, Result};

pub(super) async fn set(
    source: &dyn UiaSource,
    node: NodeId,
    hwnd: u64,
    props: &Properties,
    value: &str,
    budget: &Budget<'_>,
) -> Result<Delivery> {
    if props.password
        || props.partial
        || !props.enabled
        || super::sensitive::is_execution_surface(props)
        || props.read_only != Some(false)
    {
        return Err(DriverError::NotSettable);
    }
    // Native Value and RangeValue each recheck their own CurrentIsReadOnly.
    // Only a proven no-effect absence permits trying the numeric alternative.
    if budget
        .step(
            source,
            node,
            hwnd,
            props,
            PatternAction::SetValue(value.into()),
        )
        .await?
        == PatternResult::Performed
    {
        return Ok(Delivery::Accessibility);
    }
    if !props.patterns.range_value {
        return Err(DriverError::NotSettable);
    }
    let number = value
        .parse::<f64>()
        .ok()
        .filter(|v| v.is_finite())
        .ok_or(DriverError::NotSettable)?;
    finish(
        budget
            .step(
                source,
                node,
                hwnd,
                props,
                PatternAction::SetRangeValue(number),
            )
            .await?,
        DriverError::NotSettable,
    )
}

#[cfg(any(windows, test))]
pub(super) fn range_number(
    action: &PatternAction,
    min: f64,
    max: f64,
    current_step: Option<(f64, f64)>,
) -> Result<f64> {
    if !min.is_finite() || !max.is_finite() || min > max {
        return Err(DriverError::NotSettable);
    }
    let number = match action {
        PatternAction::SetRangeValue(number) => *number,
        PatternAction::AdjustRange(increment) => {
            let (current, step) = current_step.ok_or(DriverError::NotSettable)?;
            if !current.is_finite() || !step.is_finite() || step <= 0. {
                return Err(DriverError::NotSettable);
            }
            (current + if *increment { step } else { -step }).clamp(min, max)
        }
        _ => return Err(DriverError::InvalidRequest),
    };
    if !number.is_finite() || number < min || number > max {
        return Err(DriverError::NotSettable);
    }
    Ok(number)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn range_values_validate_bounds_and_step_before_dispatch() {
        for number in [f64::NAN, f64::INFINITY, -1., 11.] {
            assert_eq!(
                range_number(&PatternAction::SetRangeValue(number), 0., 10., None),
                Err(DriverError::NotSettable)
            );
        }
        assert_eq!(
            range_number(&PatternAction::SetRangeValue(7.), 0., 10., None),
            Ok(7.)
        );
        assert_eq!(
            range_number(&PatternAction::AdjustRange(true), 0., 10., Some((9., 2.))),
            Ok(10.)
        );
        assert_eq!(
            range_number(&PatternAction::AdjustRange(false), 0., 10., Some((1., 2.))),
            Ok(0.)
        );
        for step in [f64::NAN, f64::INFINITY, 0., -1.] {
            assert!(
                range_number(&PatternAction::AdjustRange(true), 0., 10., Some((5., step))).is_err()
            );
        }
    }
}
