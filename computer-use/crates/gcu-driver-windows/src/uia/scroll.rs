//! Bounded page scrolling; partial delivery never becomes a safe-to-retry failure.
use super::{
    actions::{finish, Budget},
    cache::Properties,
    patterns::*,
    source::{NodeId, UiaSource},
};
use gcu_core::driver::{Delivery, Dir, DriverError, Result};

pub(super) async fn scroll(
    source: &dyn UiaSource,
    node: NodeId,
    hwnd: u64,
    props: &Properties,
    dir: Dir,
    pages: f32,
    budget: &Budget<'_>,
) -> Result<Delivery> {
    if !pages.is_finite() || pages <= 0. || pages > 100. {
        return Err(DriverError::InvalidRequest);
    }
    let mut left = pages;
    while left > 0. {
        let amount = left.min(1.);
        finish(
            budget
                .step(
                    source,
                    node,
                    hwnd,
                    props,
                    PatternAction::Scroll { dir, pages: amount },
                )
                .await?,
            DriverError::ElementNotActionable,
        )?;
        left -= amount;
    }
    Ok(Delivery::Accessibility)
}

#[cfg(any(windows, test))]
pub(super) fn fraction_target(percent: f64, view: f64, pages: f32, forward: bool) -> Option<f64> {
    if !pages.is_finite()
        || pages <= 0.
        || pages > 1.
        || !percent.is_finite()
        || !(0. ..=100.).contains(&percent)
        || !view.is_finite()
        || view <= 0.
        || view >= 100.
    {
        return None;
    }
    let delta = f64::from(pages) * view * 100. / (100. - view);
    Some((percent + if forward { delta } else { -delta }).clamp(0., 100.))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fractional_scroll_uses_viewport_size_and_clamps_to_native_limits() {
        assert_eq!(fraction_target(20., 20., 0.5, true), Some(32.5));
        assert_eq!(fraction_target(20., 20., 0.5, false), Some(7.5));
        assert_eq!(fraction_target(99., 20., 0.5, true), Some(100.));
        assert_eq!(fraction_target(1., 20., 0.5, false), Some(0.));
        for view in [f64::NAN, 0., 100., 101.] {
            assert_eq!(fraction_target(20., view, 0.5, true), None);
        }
        assert_eq!(fraction_target(-1., 20., 0.5, true), None);
    }
}
