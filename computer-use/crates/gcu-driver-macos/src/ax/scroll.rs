//! `ScrollElement` via `AXScroll*ByPage`. No wheel events and no macOS calls.
//!
//! A missing page action is [`DriverError::CapabilityUnavailable`]. Directed
//! wheel input belongs to a later card. A failure after one page has succeeded
//! reports `maybe_executed = true` and does not continue.

use gcu_core::driver::{Delivery, Dir, DriverError, Result};

use super::actions::{guarded, try_named, Budget, Step};
use super::source::AxSource;

/// Scroll by whole pages. `pages` must be finite, positive, and at most 50.
///
/// The count is `pages` rounded up. The call stops at the budget inside
/// `budget`. Cancel before the first page reports `maybe_executed = false`.
pub(super) async fn scroll_element<S: AxSource>(
    source: &S,
    node: &S::Node,
    dir: Dir,
    pages: f32,
    budget: &Budget<'_>,
) -> Result<Delivery> {
    let Some(count) = page_count(pages) else {
        return Err(DriverError::InvalidRequest);
    };
    let name = page_action(dir);
    let names = guarded(budget, source.action_names(node)).await?;
    let states = guarded(budget, source.states(node)).await?;
    let mut completed = false;
    for _ in 0..count {
        match try_named(source, node, &names, &states, name, budget, false).await? {
            Step::Done => completed = true,
            Step::Continue => {
                return Err(if completed {
                    DriverError::Native {
                        message: "accessibility scroll stopped after a page".into(),
                        maybe_executed: true,
                    }
                } else {
                    DriverError::CapabilityUnavailable
                });
            }
        }
    }
    Ok(Delivery::Accessibility)
}

fn page_count(pages: f32) -> Option<u32> {
    if !pages.is_finite() || pages <= 0.0 || pages > 50.0 {
        return None;
    }
    let mut left = pages;
    let mut count = 0u32;
    while left > 0.0 {
        count = count.checked_add(1)?;
        left -= 1.0;
    }
    (1..=50).contains(&count).then_some(count)
}

fn page_action(dir: Dir) -> &'static str {
    match dir {
        Dir::Up => "AXScrollUpByPage",
        Dir::Down => "AXScrollDownByPage",
        Dir::Left => "AXScrollLeftByPage",
        Dir::Right => "AXScrollRightByPage",
    }
}
