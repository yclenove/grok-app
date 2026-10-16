//! Driver-owned observation coordination; no COM interfaces cross this boundary.
use super::{
    events::UiaEventSource,
    source::{CachedElement, NodeId, UiaSource},
    tree,
};
use gcu_core::{
    driver::{Cancel, DriverError, Result, TreeOpts, WindowRef},
    model::{NativeHandle, RawNode, RawTree},
};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

/// Exact stamped window plus an observed worker-local UIA action slot.
/// Carries no COM pointer; a new read invalidates the generation.
#[derive(Clone, Debug)]
pub struct UiaElement {
    /// Owning discovery identity, never model-provided HWND text.
    pub window: WindowRef,
    /// Worker-local snapshot identity.
    pub node: NodeId,
    /// Owning HWND, copied from validated discovery.
    pub hwnd: u64,
    /// Cached UIA RuntimeId (never a provider pointer).
    pub runtime_id: Vec<i32>,
    /// Native tree identity, excluding volatile state/value.
    pub fingerprint: u64,
}
pub(crate) struct Observer {
    source: Arc<dyn UiaSource>,
    events: Arc<dyn UiaEventSource>,
    reading: tokio::sync::Mutex<()>,
    windows: Mutex<BTreeMap<String, (WindowRef, u64)>>,
}
impl Observer {
    pub(crate) fn new(source: Arc<dyn UiaSource>, events: Arc<dyn UiaEventSource>) -> Self {
        Self {
            source,
            events,
            reading: tokio::sync::Mutex::new(()),
            windows: Mutex::new(BTreeMap::new()),
        }
    }
    pub(crate) async fn snapshot(
        &self,
        hwnd: u64,
        win: &WindowRef,
        opts: &TreeOpts,
        cancel: &Cancel,
    ) -> Result<RawTree> {
        // Serializing generations prevents concurrent observations invalidating
        // each other's element maps. Waiting for this lock is cancellable too.
        let deadline = tokio::time::Instant::now() + opts.timeout.min(tree::TREE_TIMEOUT);
        let _guard = tokio::select! { biased;
            _ = cancel.cancelled() => return Err(DriverError::Cancelled { maybe_executed: false }),
            guard = tokio::time::timeout_at(deadline, self.reading.lock()) => guard.map_err(|_| DriverError::Timeout { maybe_executed: false })?,
        };
        let mut remaining = opts.clone();
        remaining.timeout = deadline.saturating_duration_since(tokio::time::Instant::now());
        let mut raw = tree::build_raw_tree(self.source.as_ref(), hwnd, &remaining, cancel).await?;
        fn bind(node: &mut RawNode, win: &WindowRef, hwnd: u64) {
            if let Some(id) = node.handle.downcast_ref::<CachedElement>().cloned() {
                node.handle = NativeHandle::new(UiaElement {
                    window: win.clone(),
                    node: id.node,
                    hwnd,
                    runtime_id: id.runtime_id,
                    fingerprint: node.fingerprint.0,
                });
            }
            for child in &mut node.children {
                bind(child, win, hwnd);
            }
        }
        if let Some(root) = &mut raw.root {
            bind(root, win, hwnd);
        }
        Ok(raw)
    }
    pub(crate) async fn perform(
        &self,
        window: crate::api::NativeWindow,
        input: Option<&crate::input::InputBackend>,
        req: &gcu_core::driver::DriverAction,
        cancel: &Cancel,
        deadline: tokio::time::Instant,
    ) -> Result<gcu_core::driver::Delivery> {
        // Actions and observations share the generation lock. Never let a new root
        // invalidate the selected slot between preflight and worker dispatch.
        let _guard = tokio::select! { biased;
            _ = cancel.cancelled() => return Err(DriverError::Cancelled { maybe_executed: false }),
            guard = tokio::time::timeout_at(deadline, self.reading.lock()) => guard.map_err(|_| DriverError::Timeout { maybe_executed: false })?,
        };
        let hwnd = window.hwnd;
        let press = matches!(
            req,
            gcu_core::driver::DriverAction::Press { .. }
                | gcu_core::driver::DriverAction::SecondaryAction {
                    action: gcu_core::model::NodeAction::Press
                        | gcu_core::model::NodeAction::Confirm,
                    ..
                }
        );
        if press && self.source.press_uses_targeted_messages() {
            let el = super::actions::target(req)
                .and_then(|h| h.downcast_ref::<UiaElement>())
                .ok_or(DriverError::StaleElement)?;
            let props = tokio::select! { biased;
                _ = cancel.cancelled() => return Err(DriverError::Cancelled { maybe_executed: false }),
                result = tokio::time::timeout_at(deadline, self.source.validate(el.node, hwnd)) => result.map_err(|_| DriverError::Timeout { maybe_executed: false })??,
            };
            if let Some(action) = super::press::targeted(&props, &window, &el.window)? {
                let check = super::press::Check {
                    source: self.source.clone(),
                    node: el.node,
                    hwnd,
                };
                return input
                    .ok_or(DriverError::CapabilityUnavailable)?
                    .perform_checked(window, &action, cancel, deadline, Some(&check))
                    .await;
            }
        }
        super::actions::perform_until(self.source.as_ref(), hwnd, req, cancel, deadline).await
    }
    pub(crate) fn remember(&self, win: &WindowRef, hwnd: u64) {
        if let Ok(mut windows) = self.windows.lock() {
            windows.insert(win.id.clone(), (win.clone(), hwnd));
        }
    }
    pub(crate) fn subscription(&self, win: &WindowRef) -> Option<(u64, Arc<dyn UiaEventSource>)> {
        let windows = self.windows.lock().ok()?;
        let (expected, hwnd) = windows.get(&win.id)?;
        if expected != win {
            return None;
        }
        Some((*hwnd, self.events.clone()))
    }
}
