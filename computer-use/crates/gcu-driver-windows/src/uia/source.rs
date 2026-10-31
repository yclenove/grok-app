//! Portable source and fake. Each call is one bounded read or semantic worker job.
use super::cache::{Properties, BATCH_SIZE};
use async_trait::async_trait;
use gcu_core::driver::{DriverError, Result};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::Duration,
};

/// Window identity copied at admission and carried into one scoped UIA read.
///
/// It is not a search key. A later enumeration must not replace it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AdmittedWindow {
    /// Top-level HWND.
    pub hwnd: u64,
    /// Window creation generation.
    pub incarnation: u64,
    /// Owner process id.
    pub pid: u32,
    /// Owner process creation time.
    pub created: u64,
    /// Boot-session identity. An empty value is unproved.
    pub boot: String,
}

impl From<&crate::scope_identity::WindowClaim> for AdmittedWindow {
    fn from(claim: &crate::scope_identity::WindowClaim) -> Self {
        Self {
            hwnd: claim.hwnd,
            incarnation: claim.incarnation,
            pid: claim.owner.pid,
            created: claim.owner.created,
            boot: claim.owner.boot.clone(),
        }
    }
}

impl From<&AdmittedWindow> for crate::scope_identity::WindowClaim {
    fn from(admitted: &AdmittedWindow) -> Self {
        Self {
            hwnd: admitted.hwnd,
            incarnation: admitted.incarnation,
            owner: crate::scope_identity::ProcessClaim {
                pid: admitted.pid,
                created: admitted.created,
                boot: admitted.boot.clone(),
            },
        }
    }
}

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
    /// Re-read the exact observed slot, RuntimeId, HWND ancestry and metadata.
    /// A mismatch is StaleElement; no name lookup or replacement is permitted.
    async fn validate(&self, node: NodeId, hwnd: u64) -> Result<Properties>;
    /// Process id of this element, from the element itself and not from a window title.
    ///
    /// `Ok(None)` means this source has no owner probe. Real UIA must return
    /// `Ok(Some(pid))` or an error, never `Ok(None)`. A zero pid is not evidence.
    async fn owner_pid(&self, node: NodeId) -> Result<Option<u32>>;
    /// Revalidate on the MTA immediately before dispatching one pattern command.
    /// Set the context marker only at mutation dispatch. Abandoned jobs cannot
    /// start later. Only explicit Unsupported/Unavailable permit a fallback.
    async fn action(
        &self,
        node: NodeId,
        hwnd: u64,
        action: super::patterns::PatternAction,
        context: super::patterns::ActionContext,
    ) -> Result<super::patterns::PatternResult>;
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
    /// Begin a read of `hwnd` for one admitted window.
    ///
    /// `admitted` is the claim copied at admission. The default ignores it and
    /// calls [`UiaSource::root`], which is the unscoped route. A scoped source
    /// proves the claim before it builds a content cache.
    async fn open_root(
        &self,
        hwnd: u64,
        admitted: Option<AdmittedWindow>,
    ) -> Result<Option<NodeId>> {
        let _ = admitted;
        self.root(hwnd).await
    }
    /// Owner pid and top-level HWND of `node`, without its name or value.
    ///
    /// `Ok(None)` means this node was not proved. The default is unproved.
    async fn fragment(&self, node: NodeId) -> Result<Option<(u32, u64)>> {
        Ok(self.fragment_now(node))
    }
    /// Synchronous `(owner pid, top-level HWND)` recorded by a real query.
    ///
    /// No content read. `None` means this process has no source record for
    /// `node`. Callers must not substitute the requested window's owner.
    fn fragment_now(&self, _node: NodeId) -> Option<(u32, u64)> {
        None
    }
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
pub(super) struct FakeState {
    pub(super) nodes: BTreeMap<NodeId, (Properties, Vec<NodeId>)>,
    root: Option<NodeId>,
    root_reads: usize,
    delay: Duration,
    error: Option<(NodeId, DriverError)>,
    child_error: Option<(NodeId, DriverError)>,
    calls: usize,
    max_page: usize,
    pub(super) actions: super::fake_actions::FakeActions,
    /// Reported owner. `None` means this fake has no owner probe.
    pub(super) owner_pid: Option<u32>,
    /// Per-node owner and top-level window. Absent means the node is unproved.
    fragments: BTreeMap<u64, crate::scope_identity::FragmentFact>,
    /// `properties` calls per slot. Fragment queries do not increment this.
    content_reads: BTreeMap<u64, usize>,
}
impl FakeState {
    pub(super) fn node(&self, slot: u64) -> NodeId {
        NodeId {
            generation: self.root.map_or(1, |id| id.generation),
            slot,
        }
    }
}
/// Portable source with deterministic errors/delays and cache-page counters.
#[derive(Clone, Default)]
pub struct FakeUia(pub(super) Arc<Mutex<FakeState>>);
impl FakeUia {
    /// Flatten a fixture without invoking any OS API.
    pub fn new(root: FakeNode) -> Self {
        fn add(mut node: FakeNode, state: &mut FakeState) -> NodeId {
            let id = NodeId {
                generation: 1,
                slot: state.nodes.len() as u64 + 1,
            };
            if node.properties.runtime_id.is_empty() {
                node.properties.runtime_id = vec![42, id.slot as i32];
            }
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
            s.error = Some((s.node(slot), error));
        }
    }
    /// Fail a child enumeration, preserving its already-read parent.
    pub fn fail_children(&self, slot: u64, error: DriverError) {
        if let Ok(mut s) = self.0.lock() {
            s.child_error = Some((s.node(slot), error));
        }
    }
    /// Record the fragment's own process and top-level window.
    ///
    /// This does not read a name or value. Slot numbers match [`FakeUia::new`]
    /// insertion order, starting at 1 for the root.
    pub fn set_fragment(&self, slot: u64, owner_pid: u32, top_hwnd: u64) {
        if let Ok(mut state) = self.0.lock() {
            state.fragments.insert(
                slot,
                crate::scope_identity::FragmentFact {
                    owner_pid,
                    top_hwnd,
                },
            );
        }
    }
    /// Times [`UiaSource::properties`] read this slot. Fragment checks stay at zero.
    pub fn content_reads(&self, slot: u64) -> usize {
        self.0
            .lock()
            .ok()
            .and_then(|state| state.content_reads.get(&slot).copied())
            .unwrap_or(0)
    }
    /// Clear injected read failures so a new observation can prove recovery.
    pub fn clear_read_errors(&self) {
        if let Ok(mut s) = self.0.lock() {
            s.error = None;
            s.child_error = None;
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
    fn fragment_now(&self, node: NodeId) -> Option<(u32, u64)> {
        let fact = self.0.lock().ok()?.fragments.get(&node.slot).copied()?;
        Some((fact.owner_pid, fact.top_hwnd))
    }
    async fn owner_pid(&self, _node: NodeId) -> Result<Option<u32>> {
        self.0
            .lock()
            .map(|state| state.owner_pid)
            .map_err(|_| DriverError::Crashed {
                maybe_executed: false,
            })
    }
    async fn validate(&self, node: NodeId, hwnd: u64) -> Result<Properties> {
        self.gate().await?;
        self.validate_action(node, hwnd)
    }
    async fn action(
        &self,
        node: NodeId,
        hwnd: u64,
        action: super::patterns::PatternAction,
        context: super::patterns::ActionContext,
    ) -> Result<super::patterns::PatternResult> {
        self.perform_action(node, hwnd, action, context).await
    }
    async fn root(&self, hwnd: u64) -> Result<Option<NodeId>> {
        self.gate().await?;
        let mut s = self.0.lock().map_err(|_| DriverError::Crashed {
            maybe_executed: false,
        })?;
        s.actions.hwnd = hwnd;
        if s.root_reads != 0 {
            let generation = s.root.map_or(1, |id| id.generation + 1);
            let remap = |id: NodeId| NodeId {
                generation,
                slot: id.slot,
            };
            s.nodes = std::mem::take(&mut s.nodes)
                .into_iter()
                .map(|(id, (props, children))| {
                    (
                        remap(id),
                        (props, children.into_iter().map(remap).collect()),
                    )
                })
                .collect();
            s.root = s.root.map(remap);
            if let Some((id, _)) = &mut s.error {
                *id = remap(*id);
            }
            if let Some((id, _)) = &mut s.child_error {
                *id = remap(*id);
            }
            s.actions.new_generation(generation);
        }
        s.root_reads += 1;
        Ok(s.root)
    }
    async fn properties(&self, node: NodeId) -> Result<Properties> {
        self.gate().await?;
        let mut s = self.0.lock().map_err(|_| DriverError::Crashed {
            maybe_executed: false,
        })?;
        if let Some((id, error)) = &s.error {
            if *id == node {
                return Err(error.clone());
            }
        }
        let props = s
            .nodes
            .get(&node)
            .map(|n| n.0.clone())
            .ok_or(DriverError::StaleElement)?;
        *s.content_reads.entry(node.slot).or_insert(0) += 1;
        let runtime = s.actions.runtime(node);
        let hwnd = s.actions.hwnd;
        s.actions
            .observed
            .insert(node, (props.clone(), runtime, hwnd));
        Ok(props)
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

/// Cached native identity retained while the portable tree is bound to a window.
#[derive(Clone)]
pub(crate) struct CachedElement {
    pub node: NodeId,
    pub runtime_id: Vec<i32>,
}
