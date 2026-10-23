use super::{Diff, ElementIndex, RenderedState, SnapshotError};
use crate::{
    model::{Geometry, NodeValue, States, WindowPoint},
    redact::{PixelHash, RedactedImage, MAX_STATE_SKEW},
    render::RenderOpts,
    tree::{IndexedElement, ProcessedNode, ProcessedTree},
};
use std::{
    collections::{BTreeMap, HashMap, HashSet, VecDeque},
    fmt,
    time::Duration,
};

/// Number of retained observations per window, including the latest delivery.
pub const SNAPSHOTS_PER_WINDOW: usize = 3;
/// Default session cap for rendered text, fingerprint/native tables, and safe pixels.
pub const DEFAULT_MEMORY_LIMIT: usize = 32 * 1024 * 1024;

/// App-qualified window namespace; native window IDs need not be globally unique.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct WindowKey {
    /// Resolved App identity, never the model's ambiguous query string.
    pub app_id: String,
    /// Native window ID within this App.
    pub window_id: String,
}
impl WindowKey {
    /// Extract a key from trusted resolved render options.
    pub fn from_options(opts: &RenderOpts) -> Self {
        Self {
            app_id: opts.app.id.clone(),
            window_id: opts.window.id.clone(),
        }
    }
}
/// Monotonic identifier scoped to this store/session (never accepted cross-session).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SnapshotId(pub u64);
/// Presentation mode for the observation being delivered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObservationMode {
    /// First observation of a window or turn, or after baseline eviction.
    Full,
    /// Small fingerprint-aligned change, including unchanged-only states.
    Diff,
    /// Full rendering marked `[full state: large change]`.
    FullLargeChange,
}
/// Result handed to the model by the observer. Debug reports only metadata.
pub struct Observation {
    /// Store-issued snapshot ID, for trusted action bookkeeping.
    pub snapshot_id: SnapshotId,
    /// Sanitized model text, full or diff.
    pub text: String,
    /// Complete sanitized rendering when transport contraction was necessary.
    /// Store only in a private session file; never append to inline tool output.
    pub overflow_text: Option<String>,
    /// Whether text contains a full state or a small diff.
    pub mode: ObservationMode,
    /// Semantic patch when a prior delivered baseline exists in this turn.
    pub diff: Option<Diff>,
}
impl fmt::Debug for Observation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Observation")
            .field("snapshot_id", &self.snapshot_id)
            .field("mode", &self.mode)
            .finish_non_exhaustive()
    }
}
/// Bounded, immutable observation owned by exactly one session's store.
/// Native handles never enter text; safe screenshot pixels cannot be mutated.
pub struct Snapshot {
    id: SnapshotId,
    window: WindowKey,
    index: ElementIndex,
    rendered: RenderedState,
    screenshot: Option<RedactedImage>,
    target_hashes: BTreeMap<u32, PixelHash>,
    geometry: Geometry,
    state_version: Option<u64>,
    observed_at: Option<Duration>,
    memory_bytes: usize,
    delivery_size: (u32, u32),
    scope_run: Option<String>,
    scope_generation: Option<u64>,
}
impl fmt::Debug for Snapshot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Snapshot")
            .field("id", &self.id)
            .field("elements", &self.index.len())
            .field("image_present", &self.screenshot.is_some())
            .finish_non_exhaustive()
    }
}
impl Snapshot {
    /// Server-issued snapshot identifier.
    pub fn id(&self) -> SnapshotId {
        self.id
    }
    /// Exact fingerprint/index lookup for this retained observation.
    pub fn index(&self) -> &ElementIndex {
        &self.index
    }
    /// Full sanitized rendered baseline, even when the delivery used a diff.
    pub fn rendered(&self) -> &RenderedState {
        &self.rendered
    }
    /// The original redacted physical window image, never an unredacted Capture.
    pub fn screenshot(&self) -> Option<&RedactedImage> {
        self.screenshot.as_ref()
    }
    /// Center 32x32 physical pixel hash for an actionable element. None means no
    /// screenshot or the center was outside the capture; coordinate recheck must
    /// then fail closed. Arbitrary coordinate targets use screenshot.target_hash.
    pub fn target_hash(&self, index: u32) -> Option<PixelHash> {
        self.target_hashes.get(&index).copied()
    }
    /// Pixel dimensions of the image actually delivered to the model. Memory
    /// only; no waits, timeout, or cancellation. Used for coordinate conversion.
    pub fn delivery_size(&self) -> (u32, u32) {
        self.delivery_size
    }
    /// Window geometry observed for stale-geometry checks.
    pub fn geometry(&self) -> Geometry {
        self.geometry
    }
    /// Trusted tree state version; absent for text-only unstamped observations.
    pub fn state_version(&self) -> Option<u64> {
        self.state_version
    }
    /// Timestamp of the tree read on the monotonic session clock.
    pub fn observed_at(&self) -> Option<Duration> {
        self.observed_at
    }
    /// Estimated bytes charged against the session retention cap.
    pub fn memory_bytes(&self) -> usize {
        self.memory_bytes
    }
    /// Fixture-scope run recorded with this observation. `None` outside a scope.
    ///
    /// Memory only. No blocking, timeout, or cancellation.
    pub fn scope_run(&self) -> Option<&str> {
        self.scope_run.as_deref()
    }
    /// Fixture-scope generation recorded with this observation.
    ///
    /// Memory only. No blocking, timeout, or cancellation.
    pub fn scope_generation(&self) -> Option<u64> {
        self.scope_generation
    }
}
#[derive(Default)]
struct WindowHistory {
    next_index: u64,
    force_full: bool,
}
/// Session-owned memory store with per-window retention and global FIFO eviction.
/// High-water counters outlive eviction and turn changes, preventing index reuse.
/// No filesystem/network IO, waits, blocking native work, or automatic retries.
pub struct SnapshotStore {
    snapshots: VecDeque<Snapshot>,
    windows: HashMap<WindowKey, WindowHistory>,
    next_snapshot: u64,
    memory_limit: usize,
    memory_bytes: usize,
    history_bytes: usize,
}
impl Default for SnapshotStore {
    fn default() -> Self {
        Self::with_memory_limit(DEFAULT_MEMORY_LIMIT)
    }
}
impl SnapshotStore {
    /// Construct a session store with a configurable memory cap (default 32 MB).
    pub fn with_memory_limit(memory_limit: usize) -> Self {
        Self {
            snapshots: VecDeque::new(),
            windows: HashMap::new(),
            next_snapshot: 1,
            memory_limit,
            memory_bytes: 0,
            history_bytes: 0,
        }
    }
    /// Force the first subsequent delivery for every window to be full. Does not
    /// clear native indices/high-water counters or implicitly restore user Stop.
    pub fn begin_turn(&mut self) {
        for history in self.windows.values_mut() {
            history.force_full = true;
        }
    }
    /// Retained observation count across the whole session.
    pub fn len(&self) -> usize {
        self.snapshots.len()
    }
    /// Whether no snapshots remain (window high-water metadata can still exist).
    pub fn is_empty(&self) -> bool {
        self.snapshots.is_empty()
    }
    /// Estimated retained bytes, bounded by memory_limit after every successful record.
    pub fn memory_bytes(&self) -> usize {
        self.memory_bytes
    }
    /// Retained exact snapshot only in this session and app-qualified window.
    pub fn get(&self, window: &WindowKey, id: SnapshotId) -> Option<&Snapshot> {
        self.snapshots
            .iter()
            .find(|s| s.id == id && &s.window == window)
    }
    /// Latest delivered state for this exact window; no other-window fallback.
    pub fn latest(&self, window: &WindowKey) -> Option<&Snapshot> {
        self.snapshots.iter().rev().find(|s| &s.window == window)
    }
    /// Latest retained state with an original screenshot, for optional screenshot_id
    /// coordinate actions. Consumers must revalidate its geometry, version, pixels.
    pub fn latest_with_screenshot(&self, window: &WindowKey) -> Option<&Snapshot> {
        self.snapshots
            .iter()
            .rev()
            .find(|s| &s.window == window && s.screenshot.is_some())
    }
    /// Record the dimensions of a safe encoded delivery, including ADR 0001
    /// enlargement. Ignores empty or oversized derivatives. Memory only; no waits,
    /// timeout or cancellation. Must be called before accepting coordinate input.
    pub fn set_delivery_size(
        &mut self,
        window: &WindowKey,
        id: SnapshotId,
        width: u32,
        height: u32,
    ) {
        if let Some(s) = self
            .snapshots
            .iter_mut()
            .find(|s| s.id == id && &s.window == window)
        {
            if let Some(image) = &s.screenshot {
                if width > 0
                    && height > 0
                    && u64::from(width) * u64::from(height) * 4
                        <= crate::redact::MAX_CAPTURE_BYTES as u64
                    && (f64::from(width) / f64::from(image.width()) * f64::from(image.height())
                        - f64::from(height))
                    .abs()
                        <= 1.
                {
                    s.delivery_size = (width, height);
                }
            }
        }
    }
    /// Tag one retained observation with the fixture-scope generation that produced it.
    ///
    /// Charges the run id against the session cap and evicts older snapshots when
    /// that charge crosses the limit. Does not evict `id` itself when it is the
    /// only remaining snapshot. Memory only. No blocking, timeout, or cancellation.
    pub fn set_scope_epoch(
        &mut self,
        window: &WindowKey,
        id: SnapshotId,
        run: &str,
        generation: u64,
    ) {
        let Some(position) = self
            .snapshots
            .iter()
            .position(|snapshot| snapshot.id == id && &snapshot.window == window)
        else {
            return;
        };
        let next = run.len();
        if let Some(snapshot) = self.snapshots.get_mut(position) {
            let previous = snapshot
                .scope_run
                .as_ref()
                .map(|value| value.len())
                .unwrap_or(0);
            Self::apply_scope_charge(&mut snapshot.memory_bytes, previous, next);
            Self::apply_scope_charge(&mut self.memory_bytes, previous, next);
            snapshot.scope_run = Some(run.to_owned());
            snapshot.scope_generation = Some(generation);
        }
        while self.memory_bytes > self.memory_limit && self.snapshots.len() > 1 {
            if self
                .snapshots
                .front()
                .is_some_and(|snapshot| snapshot.id == id)
            {
                break;
            }
            self.evict(0);
        }
    }

    fn apply_scope_charge(bytes: &mut usize, previous: usize, next: usize) {
        if next >= previous {
            *bytes = bytes.saturating_add(next - previous);
        } else {
            *bytes = bytes.saturating_sub(previous - next);
        }
    }

    /// Resolve a snapshot-scoped reference only if still retained and its native
    /// fingerprint still exists at the same index in the latest window state.
    /// Eviction/removal always returns StaleElement; never match by label.
    pub fn resolve(
        &self,
        window: &WindowKey,
        id: SnapshotId,
        index: u32,
    ) -> Result<&IndexedElement, SnapshotError> {
        let old = self
            .get(window, id)
            .and_then(|s| s.index.get(index))
            .ok_or(SnapshotError::StaleElement)?;
        let latest = self
            .latest(window)
            .and_then(|s| s.index.get(index))
            .ok_or(SnapshotError::StaleElement)?;
        if old.fingerprint != latest.fingerprint {
            return Err(SnapshotError::StaleElement);
        }
        Ok(latest)
    }
    /// Atomically record the observation being handed to the model. Observers must
    /// call this for delivered observations only, not speculative/settle reads.
    /// Assigns stable indices, computes diff against the last delivery this turn,
    /// hashes safe physical pixels, then evicts oldest snapshots. An oversize or
    /// invalid observation leaves all prior store state/counters unchanged.
    /// Screenshot input is exclusively RedactedImage; raw captures cannot enter.
    pub fn record(
        &mut self,
        mut tree: ProcessedTree,
        opts: &RenderOpts,
        screenshot: Option<RedactedImage>,
    ) -> Result<Observation, SnapshotError> {
        if let Some(image) = &screenshot {
            if !image.is_window_capture()
                || tree.state_version != Some(image.state_version())
                || tree
                    .observed_at
                    .is_none_or(|t| t.abs_diff(image.captured_at()) > MAX_STATE_SKEW)
                || image.geometry()
                    != (Geometry {
                        frame: opts.window.frame,
                        scale: opts.window.scale,
                    })
            {
                return Err(SnapshotError::StaleScreenshot);
            }
        }
        sanitize(&mut tree);
        let window = WindowKey::from_options(opts);
        let history = self.windows.get(&window);
        let previous = self.latest(&window);
        let (index, next_index) = ElementIndex::assign(
            &mut tree,
            previous.map(|s| &s.index),
            history.map_or(0, |h| h.next_index),
        )?;
        let full_text = crate::render::text::file_text(&tree, opts);
        let overflow_text =
            (full_text.len() > crate::render::text::FACTS_PREFIX_BYTES).then_some(full_text);
        let rendered = RenderedState::from_tree(&tree, opts);
        let diff = if history.is_some_and(|h| !h.force_full) {
            previous.map(|s| Diff::between(&s.rendered, &rendered))
        } else {
            None
        };
        let mode = match &diff {
            None => ObservationMode::Full,
            Some(d) if d.is_large_change() => ObservationMode::FullLargeChange,
            Some(_) => ObservationMode::Diff,
        };
        let text = diff.as_ref().map_or_else(|| rendered.text(), Diff::text);
        let mut target_hashes = BTreeMap::new();
        if let Some(image) = &screenshot {
            for (_, node) in tree.nodes() {
                if node.interactive() {
                    if let Some(frame) = node.node.frame {
                        let center = WindowPoint {
                            x: frame.x + frame.w / 2.,
                            y: frame.y + frame.h / 2.,
                        };
                        if let Ok(hash) = image.target_hash(center) {
                            target_hashes.insert(node.element_index, hash);
                        }
                    }
                }
            }
        }
        let memory_bytes = std::mem::size_of::<Snapshot>()
            + window.app_id.capacity()
            + window.window_id.capacity()
            + index.memory_bytes()
            + rendered.memory_bytes()
            + screenshot.as_ref().map_or(0, RedactedImage::memory_bytes)
            + target_hashes.len() * (std::mem::size_of::<(u32, PixelHash)>() + 48);
        // Persistent high-water metadata is also charged. A stream of distinct
        // windows cannot grow the session beyond its cap after snapshot eviction.
        let new_window_bytes = if history.is_none() {
            std::mem::size_of::<(WindowKey, WindowHistory)>()
                + window.app_id.capacity()
                + window.window_id.capacity()
                + 48
        } else {
            0
        };
        let history_bytes = self
            .history_bytes
            .checked_add(new_window_bytes)
            .ok_or(SnapshotError::MemoryLimit)?;
        if history_bytes
            .checked_add(memory_bytes)
            .is_none_or(|size| size > self.memory_limit)
        {
            return Err(SnapshotError::MemoryLimit);
        }
        let id = SnapshotId(self.next_snapshot);
        let next_snapshot = self
            .next_snapshot
            .checked_add(1)
            .ok_or(SnapshotError::IndexExhausted)?;
        let delivery_size = screenshot
            .as_ref()
            .map_or((0, 0), |i| (i.width(), i.height()));
        let snapshot = Snapshot {
            delivery_size,
            id,
            window: window.clone(),
            index,
            rendered,
            screenshot,
            target_hashes,
            geometry: Geometry {
                frame: opts.window.frame,
                scale: opts.window.scale,
            },
            state_version: tree.state_version,
            observed_at: tree.observed_at,
            memory_bytes,
            scope_run: None,
            scope_generation: None,
        };
        self.next_snapshot = next_snapshot;
        self.windows.insert(
            window.clone(),
            WindowHistory {
                next_index,
                force_full: false,
            },
        );
        self.snapshots.push_back(snapshot);
        self.history_bytes = history_bytes;
        self.memory_bytes += memory_bytes + new_window_bytes;
        while self.snapshots.iter().filter(|s| s.window == window).count() > SNAPSHOTS_PER_WINDOW {
            if let Some(position) = self.snapshots.iter().position(|s| s.window == window) {
                self.evict(position);
            } else {
                break;
            }
        }
        while self.memory_bytes > self.memory_limit {
            self.evict(0);
        }
        Ok(Observation {
            snapshot_id: id,
            text,
            overflow_text,
            mode,
            diff,
        })
    }
    fn evict(&mut self, position: usize) {
        if let Some(snapshot) = self.snapshots.remove(position) {
            self.memory_bytes -= snapshot.memory_bytes;
        }
    }
}
pub(crate) fn sanitize(tree: &mut ProcessedTree) {
    let sensitive: HashSet<_> = tree
        .sensitive_regions
        .iter()
        .map(|r| r.fingerprint)
        .collect();
    fn walk(
        node: &mut ProcessedNode,
        inherited: bool,
        sensitive: &HashSet<crate::model::Fingerprint>,
    ) {
        let secret = inherited
            || sensitive.contains(&node.node.fingerprint)
            || node.node.states.contains(States::PASSWORD);
        if secret {
            node.node.value = Some(NodeValue::Text("•••".into()));
            node.node.text_extras = None;
            node.node.name = None;
            node.node.description = None;
            node.node.help = None;
            node.node.url = None;
            node.node.identifier = None;
            node.node.placeholder = None;
        }
        for child in &mut node.children {
            walk(child, secret, sensitive);
        }
    }
    if let Some(root) = &mut tree.root {
        walk(root, false, &sensitive);
    }
}
