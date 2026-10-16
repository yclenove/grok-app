//! Invoke/Legacy activation is not a background guarantee on Windows.
use super::{
    cache::Properties,
    patterns::{press_patterns, Pattern},
};
use crate::api::NativeWindow;
use gcu_core::{driver::*, model::WindowPoint};
use std::sync::Arc;

pub(super) struct Check {
    pub source: Arc<dyn super::source::UiaSource>,
    pub node: super::source::NodeId,
    pub hwnd: u64,
}
#[async_trait::async_trait]
impl crate::input::api::InputPreflight for Check {
    async fn check(&self) -> Result<()> {
        let props = self.source.validate(self.node, self.hwnd).await?;
        actionable(&props)
    }
}
fn actionable(props: &Properties) -> Result<()> {
    if props.partial
        || props.password
        || !props.enabled
        || props.offscreen
        || super::sensitive::is_execution_surface(props)
    {
        Err(DriverError::ElementNotActionable)
    } else {
        Ok(())
    }
}

pub(super) fn targeted(
    props: &Properties,
    window: &NativeWindow,
    win: &WindowRef,
) -> Result<Option<DriverAction>> {
    actionable(props)?;
    let first = press_patterns(props.control_type)
        .iter()
        .find(|p| p.available(&props.patterns));
    if !matches!(first, Some(Pattern::Invoke | Pattern::Legacy)) {
        return Ok(None);
    }
    // Exact native BUTTONs use BM_CLICK on their own HWND in ffi/actions/press.
    if props
        .class_name
        .as_deref()
        .is_some_and(|c| c.eq_ignore_ascii_case("Button"))
    {
        return Ok(None);
    }
    // UIA virtual elements have no child HWND. Use their freshly validated
    // physical bounds with the existing hit-test/ledger/TargetedOnly route.
    let frame = props.frame.ok_or(DriverError::CapabilityUnavailable)?;
    if ![frame.x, frame.y, frame.w, frame.h]
        .into_iter()
        .all(f64::is_finite)
        || frame.w <= 0.
        || frame.h <= 0.
    {
        return Err(DriverError::ElementNotActionable);
    }
    let at = WindowPoint {
        x: frame.x + frame.w / 2. - window.frame.x,
        y: frame.y + frame.h / 2. - window.frame.y,
    };
    if at.x < 0. || at.y < 0. || at.x >= window.frame.w || at.y >= window.frame.h {
        return Err(DriverError::OutOfBounds);
    }
    Ok(Some(DriverAction::PointerClick {
        win: win.clone(),
        at,
        button: Button::Left,
        count: 1,
        route: PointerRoute::TargetedOnly,
    }))
}
