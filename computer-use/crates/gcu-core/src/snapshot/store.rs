use super::prepare::{assemble, Assembled, ObservationBaseline};
use super::{Diff, ElementIndex, RenderedState, SnapshotError};
use crate::{
    model::Geometry,
    redact::{PixelHash, RedactedImage},
    render::RenderOpts,
    tree::{IndexedElement, ProcessedTree},
};
use std::{
    collections::{BTreeMap, HashMap, VecDeque},
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

pub(super) fn window_history_charge(window: &WindowKey) -> usize {
    std::mem::size_of::<(WindowKey, WindowHistory)>()
        + window.app_id.capacity()
        + window.window_id.capacity()
        + 48
}
/// Retention work computed with the snapshot mutex, before the lease lock.
pub(crate) struct InstallPlan {
    window: WindowKey,
    history_bytes: usize,
    evictions: Vec<usize>,
}
/// Validated coordinate update; valid while the snapshot mutex stays held.
pub(crate) struct DeliverySize {
    position: usize,
    width: u32,
    height: u32,
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
    /// Drop one snapshot that was recorded and then refused before delivery.
    ///
    /// Memory only. No blocking, timeout, or cancellation. A missing id is
    /// ignored. Window high-water metadata stays; the observation bytes do not.
    pub fn discard(&mut self, window: &WindowKey, id: SnapshotId) {
        if let Some(position) = self
            .snapshots
            .iter()
            .position(|snapshot| snapshot.id == id && &snapshot.window == window)
        {
            self.evict(position);
        }
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
        if let Some(size) = self.prepare_delivery_size(window, id, width, height) {
            self.apply_delivery_size(size);
        }
    }

    pub(crate) fn prepare_delivery_size(
        &self,
        window: &WindowKey,
        id: SnapshotId,
        width: u32,
        height: u32,
    ) -> Option<DeliverySize> {
        let position = self
            .snapshots
            .iter()
            .position(|s| s.id == id && &s.window == window)?;
        let image = self.snapshots[position].screenshot.as_ref()?;
        if width == 0
            || height == 0
            || u64::from(width) * u64::from(height) * 4 > crate::redact::MAX_CAPTURE_BYTES as u64
            || (f64::from(width) / f64::from(image.width()) * f64::from(image.height())
                - f64::from(height))
            .abs()
                > 1.
        {
            return None;
        }
        Some(DeliverySize {
            position,
            width,
            height,
        })
    }

    pub(crate) fn apply_delivery_size(&mut self, size: DeliverySize) {
        if let Some(snapshot) = self.snapshots.get_mut(size.position) {
            snapshot.delivery_size = (size.width, size.height);
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
    /// Copy the counters an off-lock index walk needs.
    ///
    /// Memory only. The caller must not hold this store across the walk, and
    /// must recheck it in [`Self::prepare_install`]. No timeout or cancellation.
    pub(crate) fn baseline(&self, window: &WindowKey) -> ObservationBaseline {
        let history = self
            .windows
            .get(window)
            .map(|item| (item.next_index, item.force_full));
        let previous = self.latest(window);
        ObservationBaseline {
            history,
            previous_index: previous.map(|snapshot| snapshot.index.clone()),
            previous_rendered: previous.map(|snapshot| snapshot.rendered.clone()),
            latest_id: previous.map(|snapshot| snapshot.id),
            memory_limit: self.memory_limit,
            history_bytes: self.history_bytes,
        }
    }

    /// Burn the next snapshot id. The id is not reused when install is refused.
    ///
    /// Memory only. No timeout or cancellation. Does not insert a snapshot.
    pub(crate) fn reserve_id(&mut self) -> Result<SnapshotId, SnapshotError> {
        let next = self
            .next_snapshot
            .checked_add(1)
            .ok_or(SnapshotError::IndexExhausted)?;
        let id = SnapshotId(self.next_snapshot);
        self.next_snapshot = next;
        Ok(id)
    }

    /// Insert a prepared observation when the copied baseline is still current.
    ///
    /// Call `prepare_install` first and retain exclusive store access until
    /// this call. Memory only, infallible, no timeout or cancellation. Eviction
    /// matches [`Self::record`]. Retired snapshots move into `retired` so their
    /// large trees/pixels can be dropped
    /// after leaving the lease critical section. Reserve capacity before calling.
    pub(crate) fn install(
        &mut self,
        id: SnapshotId,
        assembled: Assembled,
        plan: InstallPlan,
        retired: &mut Vec<Snapshot>,
    ) -> Observation {
        let memory_bytes = assembled.memory_bytes;
        let new_window_bytes = assembled.new_window_bytes;
        let next_index = assembled.next_index;
        let text = assembled.text;
        let overflow_text = assembled.overflow_text;
        let mode = assembled.mode;
        let diff = assembled.diff;
        self.windows.insert(
            plan.window,
            WindowHistory {
                next_index,
                force_full: false,
            },
        );
        self.snapshots.push_back(Snapshot {
            delivery_size: assembled.delivery_size,
            id,
            window: assembled.window,
            index: assembled.index,
            rendered: assembled.rendered,
            screenshot: assembled.screenshot,
            target_hashes: assembled.target_hashes,
            geometry: assembled.geometry,
            state_version: assembled.state_version,
            observed_at: assembled.observed_at,
            memory_bytes,
            scope_run: assembled.scope_run,
            scope_generation: assembled.scope_generation,
        });
        self.history_bytes = plan.history_bytes;
        self.memory_bytes += memory_bytes + new_window_bytes;
        // Descending positions keep every removal aligned with the prepared
        // store. No tree walk, budget scan, allocation, or payload drop here.
        for &position in plan.evictions.iter().rev() {
            self.retire(position, retired);
        }
        Observation {
            snapshot_id: id,
            text,
            overflow_text,
            mode,
            diff,
        }
    }

    /// Atomically record the observation being handed to the model. Observers must
    /// call this for delivered observations only, not speculative/settle reads.
    /// Assigns stable indices, computes diff against the last delivery this turn,
    /// hashes safe physical pixels, then evicts oldest snapshots. An oversize or
    /// invalid observation leaves all prior store state/counters unchanged.
    /// Screenshot input is exclusively RedactedImage; raw captures cannot enter.
    pub fn record(
        &mut self,
        tree: ProcessedTree,
        opts: &RenderOpts,
        screenshot: Option<RedactedImage>,
    ) -> Result<Observation, SnapshotError> {
        let window = WindowKey::from_options(opts);
        let baseline = self.baseline(&window);
        let assembled = assemble(tree, opts, screenshot, &baseline)?;
        let plan = self.prepare_install(&assembled)?;
        let id = self.reserve_id()?;
        let mut retired = Vec::with_capacity(self.snapshots.len());
        Ok(self.install(id, assembled, plan, &mut retired))
    }

    /// Validate the copied baseline/budget and reserve registration capacity.
    /// Keep the snapshot mutex from this call through `install`. Run before
    /// entering a lease critical section. Memory only; no IO or cancellation.
    pub(crate) fn prepare_install(
        &mut self,
        assembled: &Assembled,
    ) -> Result<InstallPlan, SnapshotError> {
        let history = self
            .windows
            .get(&assembled.window)
            .map(|item| (item.next_index, item.force_full));
        let latest = self.latest(&assembled.window).map(|snapshot| snapshot.id);
        if history != assembled.baseline_history || latest != assembled.baseline_latest {
            return Err(SnapshotError::StaleDiff);
        }
        let history_bytes = self
            .history_bytes
            .checked_add(assembled.new_window_bytes)
            .ok_or(SnapshotError::MemoryLimit)?;
        if history_bytes
            .checked_add(assembled.memory_bytes)
            .is_none_or(|size| size > self.memory_limit)
        {
            return Err(SnapshotError::MemoryLimit);
        }
        let window_count = self
            .snapshots
            .iter()
            .filter(|snapshot| snapshot.window == assembled.window)
            .count();
        let excess = (window_count + 1).saturating_sub(SNAPSHOTS_PER_WINDOW);
        let mut evictions: Vec<_> = self
            .snapshots
            .iter()
            .enumerate()
            .filter(|(_, snapshot)| snapshot.window == assembled.window)
            .take(excess)
            .map(|(position, _)| position)
            .collect();
        let mut bytes = self.memory_bytes + assembled.memory_bytes + assembled.new_window_bytes;
        for &position in &evictions {
            bytes -= self.snapshots[position].memory_bytes;
        }
        let window_evictions = evictions.len();
        for (position, snapshot) in self.snapshots.iter().enumerate() {
            if bytes <= self.memory_limit {
                break;
            }
            if evictions[..window_evictions]
                .binary_search(&position)
                .is_err()
            {
                bytes -= snapshot.memory_bytes;
                evictions.push(position);
            }
        }
        evictions.sort_unstable();
        self.snapshots.reserve(1);
        self.windows.reserve(1);
        Ok(InstallPlan {
            window: assembled.window.clone(),
            history_bytes,
            evictions,
        })
    }

    fn retire(&mut self, position: usize, retired: &mut Vec<Snapshot>) {
        if let Some(snapshot) = self.snapshots.remove(position) {
            self.memory_bytes -= snapshot.memory_bytes;
            retired.push(snapshot);
        }
    }
    fn evict(&mut self, position: usize) {
        if let Some(snapshot) = self.snapshots.remove(position) {
            self.memory_bytes -= snapshot.memory_bytes;
        }
    }
}
