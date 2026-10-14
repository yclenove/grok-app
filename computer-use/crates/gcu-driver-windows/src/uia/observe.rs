//! Driver-owned observation coordination; no COM interfaces cross this boundary.
use super::{
    events::UiaEventSource,
    source::{NodeId, UiaSource},
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

/// Exact stamped window plus a worker-local UIA slot, for future native actions.
/// Carries no COM pointer; a new read invalidates the generation.
#[derive(Clone, Debug)]
pub struct UiaElement {
    /// Owning discovery identity, never model-provided HWND text.
    pub window: WindowRef,
    /// Worker-local snapshot identity.
    pub node: NodeId,
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
        fn bind(node: &mut RawNode, win: &WindowRef) {
            if let Some(id) = node.handle.downcast_ref::<NodeId>().copied() {
                node.handle = NativeHandle::new(UiaElement {
                    window: win.clone(),
                    node: id,
                });
            }
            for child in &mut node.children {
                bind(child, win);
            }
        }
        if let Some(root) = &mut raw.root {
            bind(root, win);
        }
        Ok(raw)
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
