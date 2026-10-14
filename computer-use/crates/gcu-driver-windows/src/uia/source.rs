//! Portable source and fake. Each source call is one bounded cache/page job.
use super::cache::{Properties, BATCH_SIZE};
use async_trait::async_trait;
use gcu_core::driver::{DriverError, Result};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::Duration,
};

/// Worker-local element identity. A new root invalidates earlier generations.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct NodeId {
    /// Snapshot generation.
    pub generation: u64,
    /// Element slot (never an HWND or a serialized COM pointer).
    pub slot: u64,
}

/// A bounded child page, preserving native order.
#[derive(Clone, Debug, Default)]
pub struct ChildPage {
    /// At most the requested limit of nodes.
    pub nodes: Vec<NodeId>,
    /// Further children exist. Exact subtree size is intentionally unknown.
    pub more: bool,
}

/// Platform-neutral UIA source. Native calls must stay on a persistent MTA.
/// Futures may be abandoned on timeout/cancel; queued work must then be skipped.
#[async_trait]
pub trait UiaSource: Send + Sync + 'static {
    /// Begin a new read of the exact HWND; no title/name fallback.
    async fn root(&self, hwnd: u64) -> Result<Option<NodeId>>;
    /// Read the entire owned property bag from a single cache request.
    async fn properties(&self, node: NodeId) -> Result<Properties>;
    /// Fetch up to `limit` children after `after`, with bounded lookahead.
    /// `limit` is at most BATCH_SIZE; no unbounded subtree FindAll is allowed.
    async fn children(
        &self,
        node: NodeId,
        after: Option<NodeId>,
        limit: usize,
    ) -> Result<ChildPage>;
}

/// Owned fixture node. Flattened once by FakeUia, so large-tree tests are linear.
#[derive(Clone, Debug, Default)]
pub struct FakeNode {
    /// Cached metadata.
    pub properties: Properties,
    /// Ordered child fixtures.
    pub children: Vec<FakeNode>,
}

#[derive(Default)]
struct FakeState {
    nodes: BTreeMap<NodeId, (Properties, Vec<NodeId>)>,
    root: Option<NodeId>,
    delay: Duration,
    error: Option<(NodeId, DriverError)>,
    child_error: Option<(NodeId, DriverError)>,
    calls: usize,
    max_page: usize,
}
/// Portable source with deterministic errors/delays and cache-page counters.
#[derive(Clone, Default)]
pub struct FakeUia(Arc<Mutex<FakeState>>);
impl FakeUia {
    /// Flatten a fixture without invoking any OS API.
    pub fn new(root: FakeNode) -> Self {
        fn add(node: FakeNode, state: &mut FakeState) -> NodeId {
            let id = NodeId {
                generation: 1,
                slot: state.nodes.len() as u64 + 1,
            };
            state.nodes.insert(id, (node.properties, Vec::new()));
            let children = node.children.into_iter().map(|n| add(n, state)).collect();
            if let Some((_, stored)) = state.nodes.get_mut(&id) {
                *stored = children;
            }
            id
        }
        let mut state = FakeState::default();
        state.root = Some(add(root, &mut state));
        Self(Arc::new(Mutex::new(state)))
    }
    /// Delay every source call; tokio's paused clock can exercise the hard ceiling.
    pub fn set_delay(&self, delay: Duration) {
        if let Ok(mut s) = self.0.lock() {
            s.delay = delay;
        }
    }
    /// Fail one cached property bag, with no effects.
    pub fn fail_properties(&self, slot: u64, error: DriverError) {
        if let Ok(mut s) = self.0.lock() {
            s.error = Some((
                NodeId {
                    generation: 1,
                    slot,
                },
                error,
            ));
        }
    }
    /// Fail a child enumeration, preserving its already-read parent.
    pub fn fail_children(&self, slot: u64, error: DriverError) {
        if let Ok(mut s) = self.0.lock() {
            s.child_error = Some((
                NodeId {
                    generation: 1,
                    slot,
                },
                error,
            ));
        }
    }
    /// Number of source calls and largest requested page.
    pub fn stats(&self) -> (usize, usize) {
        self.0
            .lock()
            .map(|s| (s.calls, s.max_page))
            .unwrap_or_default()
    }
    async fn gate(&self) -> Result<()> {
        let delay = {
            let mut s = self.0.lock().map_err(|_| DriverError::Crashed {
                maybe_executed: false,
            })?;
            s.calls += 1;
            s.delay
        };
        if !delay.is_zero() {
            tokio::time::sleep(delay).await;
        }
        Ok(())
    }
}
#[async_trait]
impl UiaSource for FakeUia {
    async fn root(&self, _: u64) -> Result<Option<NodeId>> {
        self.gate().await?;
        Ok(self
            .0
            .lock()
            .map_err(|_| DriverError::Crashed {
                maybe_executed: false,
            })?
            .root)
    }
    async fn properties(&self, node: NodeId) -> Result<Properties> {
        self.gate().await?;
        let s = self.0.lock().map_err(|_| DriverError::Crashed {
            maybe_executed: false,
        })?;
        if let Some((id, error)) = &s.error {
            if *id == node {
                return Err(error.clone());
            }
        }
        s.nodes
            .get(&node)
            .map(|n| n.0.clone())
            .ok_or(DriverError::StaleElement)
    }
    async fn children(
        &self,
        node: NodeId,
        after: Option<NodeId>,
        limit: usize,
    ) -> Result<ChildPage> {
        self.gate().await?;
        if limit == 0 || limit > BATCH_SIZE {
            return Err(DriverError::InvalidRequest);
        }
        let mut s = self.0.lock().map_err(|_| DriverError::Crashed {
            maybe_executed: false,
        })?;
        s.max_page = s.max_page.max(limit);
        if let Some((id, error)) = &s.child_error {
            if *id == node {
                return Err(error.clone());
            }
        }
        let children = &s.nodes.get(&node).ok_or(DriverError::StaleElement)?.1;
        let start = match after {
            Some(id) => {
                children
                    .iter()
                    .position(|n| *n == id)
                    .ok_or(DriverError::StaleElement)?
                    + 1
            }
            None => 0,
        };
        let end = (start + limit).min(children.len());
        Ok(ChildPage {
            nodes: children[start..end].to_vec(),
            more: end < children.len(),
        })
    }
}
