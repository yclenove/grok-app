//! Retained AX window witnesses; no title/geometry-only identity fallback.
use super::ax_api::{
    AXUIElementCopyElementAtPosition, AXUIElementCreateApplication, Budget, CFEqual, Element,
};
use super::*;
use grok_computer_use_core::execution::ActionCancellation;
use parking_lot::Mutex;
use std::collections::HashMap;

#[derive(Default)]
pub(super) struct WindowBindings {
    windows: Mutex<HashMap<WindowInstance, Element>>,
}
fn resolve_window(
    key: WindowKey,
    bounds: WindowBounds,
    budget: &Budget<'_>,
) -> Result<Element, String> {
    budget.check()?;
    if !MacosAdapter::ax_ok() {
        return Err("Accessibility permission is required to bind this window instance".into());
    }
    identity::validate(key)?;
    let app = budget.element(
        OwnedCf::new(
            unsafe { AXUIElementCreateApplication(key.pid as i32) },
            "AX application unavailable",
        )?,
        key.pid,
    )?;
    // These are read-only hit tests, NEVER semantic click fallback coordinates.
    // A point must independently hit the selected CG window before and after
    // resolving its AXWindow. Fully occluded/unprovable windows fail closed.
    for (fx, fy) in [
        (0.5, 0.5),
        (0.25, 0.25),
        (0.75, 0.25),
        (0.25, 0.75),
        (0.75, 0.75),
    ] {
        budget.check()?;
        let x = (bounds.x + bounds.width * fx) as f32;
        let y = (bounds.y + bounds.height * fy) as f32;
        let (qx, qy) = (f64::from(x), f64::from(y));
        if qx < bounds.x
            || qy < bounds.y
            || qx >= bounds.x + bounds.width
            || qy >= bounds.y + bounds.height
            || !point_owned_by(key, qx, qy)?
        {
            continue;
        }
        let mut out = std::ptr::null();
        let status = unsafe { AXUIElementCopyElementAtPosition(app.0, x, y, &mut out) };
        let hit = OwnedCf::new(out, "AX window hit test unavailable")?;
        if status != 0 {
            return Err("AX window hit test failed".into());
        }
        let hit = budget.element(hit, key.pid)?;
        let window = if budget.role(hit.0)? == "AXWindow" {
            hit
        } else {
            budget.element(budget.attribute(hit.0, "AXWindow")?, key.pid)?
        };
        if budget.role(window.0)? != "AXWindow" || budget.bounds(window.0)? != bounds {
            return Err("AX hit does not identify the selected CG window".into());
        }
        budget.check()?;
        identity::validate(key)?;
        if !point_owned_by(key, qx, qy)? {
            return Err("window changed during AX identity binding".into());
        }
        budget.check()?;
        return Ok(Element(window));
    }
    Err("selected window has no provable exposed AX identity; no title or coordinate guess is permitted".into())
}

impl WindowBindings {
    pub(super) fn discover(
        &self,
        key: WindowKey,
        bounds: WindowBounds,
    ) -> Result<WindowInstance, String> {
        let mut windows = self.windows.try_lock().ok_or("macOS AX identity is busy")?;
        let cancel = ActionCancellation::default();
        let witness = resolve_window(key, bounds, &Budget::new(&cancel))?;
        for (instance, saved) in windows.iter() {
            if instance.window == key && unsafe { CFEqual(saved.0 .0, witness.0 .0) } {
                return Ok(*instance);
            }
        }
        // Same PID/window number with a new birth or AX object cannot inherit
        // an old nonce, even if every human-visible property is identical.
        windows
            .retain(|instance, _| instance.window.pid != key.pid || instance.window.wid != key.wid);
        if windows.len() >= 128 {
            return Err("macOS retained window identity limit reached".into());
        }
        let instance = WindowInstance::fresh(key);
        windows.insert(instance, witness);
        Ok(instance)
    }

    pub(super) fn validate(
        &self,
        target: WindowInstance,
        bounds: WindowBounds,
        cancellation: &ActionCancellation,
    ) -> Result<(), String> {
        self.with_window(target, bounds, cancellation, |_, _| Ok(()))
    }

    pub(super) fn with_window<T>(
        &self,
        target: WindowInstance,
        bounds: WindowBounds,
        cancellation: &ActionCancellation,
        operation: impl FnOnce(&Element, &Budget<'_>) -> Result<T, String>,
    ) -> Result<T, String> {
        self.with_window_budget(target, bounds, &Budget::new(cancellation), operation)
    }

    pub(super) fn with_window_budget<T>(
        &self,
        target: WindowInstance,
        bounds: WindowBounds,
        budget: &Budget<'_>,
        operation: impl FnOnce(&Element, &Budget<'_>) -> Result<T, String>,
    ) -> Result<T, String> {
        budget.check()?;
        let mut windows = self.windows.try_lock().ok_or("macOS AX identity is busy")?;
        let saved = windows
            .get(&target)
            .ok_or("unknown or retired macOS window instance; discover and authorize it again")?;
        let witness = resolve_window(target.window, bounds, budget)?;
        if !unsafe { CFEqual(saved.0 .0, witness.0 .0) } {
            windows.remove(&target);
            return Err("macOS native window instance was replaced".into());
        }
        // Query the retained object itself: equality alone is not a liveness
        // proof if the remote server has already invalidated its old element.
        if budget.role(saved.0 .0)? != "AXWindow" || budget.bounds(saved.0 .0)? != bounds {
            return Err("retained AX window is no longer live".into());
        }
        budget.check()?;
        identity::validate(target.window)?;
        budget.check()?;
        operation(saved, budget)
    }

    pub(super) fn retain_visible(&self, keys: &[WindowKey]) {
        if let Some(mut windows) = self.windows.try_lock() {
            windows.retain(|instance, _| keys.contains(&instance.window));
        }
    }

    /// Once a native read observes a dead/ineligible target, the old nonce
    /// cannot be revived by a later identical CG identifier. This waits only
    /// for an in-flight bounded AX query; abort never calls or takes this lock.
    pub(super) fn retire(&self, target: WindowInstance) {
        self.windows.lock().remove(&target);
    }
}
