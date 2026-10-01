//! A requested screenshot point is a target selector, never permission to
//! refocus, guess a nearby control, or fall back from a failed semantic action.
use super::ax_api::{
    AXUIElementCopyElementAtPosition, AXUIElementCreateApplication, Budget, Element,
};
use super::*;

pub(super) struct Binding {
    x: f32,
    y: f32,
    requested: (f64, f64),
}
impl Binding {
    pub(super) fn new(point: CgPoint, window: WindowBounds) -> Result<Self, String> {
        let binding = Self {
            x: point.x as f32,
            y: point.y as f32,
            requested: (point.x, point.y),
        };
        // AX takes Float coordinates. Validate the actual rounded point rather
        // than letting a boundary conversion hit the neighbouring window.
        if !binding.contains(Some(window)) {
            return Err("AX coordinate precision escapes the selected window".into());
        }
        Ok(binding)
    }
    pub(super) fn contains(&self, bounds: Option<WindowBounds>) -> bool {
        let Some(bounds) = bounds else { return false };
        [self.requested, (f64::from(self.x), f64::from(self.y))]
            .into_iter()
            .all(|(x, y)| {
                x.is_finite()
                    && y.is_finite()
                    && x >= bounds.x
                    && y >= bounds.y
                    && x < bounds.x + bounds.width
                    && y < bounds.y + bounds.height
            })
    }
    pub(super) fn hit(
        &self,
        budget: &Budget<'_>,
        root: &Element,
        target: WindowKey,
    ) -> Result<Element, String> {
        budget.check()?;
        let visible = || -> Result<(), String> {
            if !point_owned_by(target, f64::from(self.x), f64::from(self.y))? {
                return Err("macOS keyboard coordinate is occluded; input paused".into());
            }
            Ok(())
        };
        visible()?;
        let app = budget.element(
            OwnedCf::new(
                unsafe { AXUIElementCreateApplication(target.pid as i32) },
                "AX application unavailable for coordinate binding",
            )?,
            target.pid,
        )?;
        let mut out = std::ptr::null();
        let status = unsafe { AXUIElementCopyElementAtPosition(app.0, self.x, self.y, &mut out) };
        let hit = OwnedCf::new(out, "AX coordinate has no exact element")?;
        budget.check()?;
        if status != 0 {
            return Err(format!("AX coordinate hit failed ({status})"));
        }
        let hit = Element(budget.element(hit, target.pid)?);
        if !budget.related(&hit, "AXWindow", target.pid)?.same(root) {
            return Err("AX coordinate belongs to another window".into());
        }
        visible()?;
        budget.check()?;
        Ok(hit)
    }
    pub(super) fn verify(
        &self,
        budget: &Budget<'_>,
        root: &Element,
        node: &Element,
        target: WindowKey,
    ) -> Result<(), String> {
        if !self.hit(budget, root, target)?.same(node) {
            return Err("AX coordinate no longer hits the originally selected control".into());
        }
        Ok(())
    }
}
