//! Fixture-scope identity for one Linux process instance.
//!
//! A pid is not an identity. Proof is the pid, the `/proc` start time, and the
//! boot id, plus a backend window token. Missing proof, a recycled pid, a
//! recycled window id, a new owner, or a handle that now sits on another
//! window all become [`DriverError::ScopeDenied`]. The display text is the
//! fixed sentence from core. This module does not read a desktop; callers
//! pass the observation.

use gcu_core::driver::{DriverError, NATIVE_TIMEOUT};
use gcu_core::model::{Geometry, NativeHandle};
#[cfg(test)]
use gcu_core::model::{RawNode, RawTree};
use gcu_core::scope::{BootIdentity, OsStartTime, ProcessInstance, WindowBinding};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// Why a scope proof failed. Callers surface only [`DriverError::ScopeDenied`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeFault {
    /// A required pid, start time, boot id, or window token is missing.
    Unprovable,
    /// This backend cannot prove which window it would touch.
    Closed,
    /// The pid is the same and the start time is new.
    PidReuse,
    /// The window id is the same and the incarnation is new.
    WindowReuse,
    /// The process that owns the window changed.
    OwnerChanged,
    /// The handle now belongs to a different window.
    HandleCrossed,
    /// The process matches, and the window does not.
    WrongWindow,
}

impl ScopeFault {
    /// Fixed scope refusal. The variant stays out of the display text.
    pub(crate) fn deny(self) -> DriverError {
        let _ = self;
        DriverError::ScopeDenied
    }
}

/// Clock and boot id used to re-read a process. `None` is not proof.
pub trait ProcessClock: Send + Sync {
    /// Current boot id, or `None` when the platform cannot show one.
    fn boot_id(&self) -> Option<String>;
    /// Start time of `pid`, or `None` when `/proc` cannot show it.
    fn start_time(&self, pid: u32) -> Option<u64>;
}

/// `/proc` clock. Off Linux, and when the files are missing, every read is `None`.
#[derive(Debug, Default)]
pub struct ProcClock;

impl ProcessClock for ProcClock {
    fn boot_id(&self) -> Option<String> {
        read_boot_id()
    }

    fn start_time(&self, pid: u32) -> Option<u64> {
        read_start_time(pid)
    }
}

fn read_boot_id() -> Option<String> {
    #[cfg(target_os = "linux")]
    {
        let text = std::fs::read_to_string("/proc/sys/kernel/random/boot_id").ok()?;
        let text = text.trim();
        if text.is_empty() || text.len() > 256 {
            None
        } else {
            Some(text.to_string())
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

fn read_start_time(pid: u32) -> Option<u64> {
    #[cfg(target_os = "linux")]
    {
        if pid == 0 {
            return None;
        }
        super::get_process_start_time(pid)
            .ok()
            .filter(|ticks| *ticks > 0)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = pid;
        None
    }
}

/// One observation of a running process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProcessView {
    /// Process id.
    pub pid: u32,
    /// OS start time in the platform's units.
    pub start_time: u64,
    /// Boot id captured with the start time.
    pub boot_id: String,
}

impl ProcessView {
    /// Build a view. Zero pid, zero start, or an empty boot id is `None`.
    pub(crate) fn new(pid: u32, start_time: u64, boot_id: impl Into<String>) -> Option<Self> {
        let boot_id = boot_id.into();
        if pid == 0 || start_time == 0 || boot_id.is_empty() || boot_id.len() > 256 {
            return None;
        }
        Some(Self {
            pid,
            start_time,
            boot_id,
        })
    }
}

/// Read a process view from `clock`. A missing field is `None`.
pub(crate) fn process_view(clock: &dyn ProcessClock, pid: u32) -> Option<ProcessView> {
    ProcessView::new(pid, clock.start_time(pid)?, clock.boot_id()?)
}

/// Admitted fixture process. Pid alone cannot be stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeInstance {
    pid: u32,
    start_time: u64,
    boot_id: String,
}

impl ScopeInstance {
    /// Store a non-zero pid, a non-zero start time, and a non-empty boot id.
    pub fn try_new(
        pid: u32,
        start_time: u64,
        boot_id: impl Into<String>,
    ) -> Result<Self, ScopeFault> {
        let boot_id = boot_id.into();
        if pid == 0 || start_time == 0 || boot_id.is_empty() || boot_id.len() > 256 {
            return Err(ScopeFault::Unprovable);
        }
        Ok(Self {
            pid,
            start_time,
            boot_id,
        })
    }

    /// Process id.
    pub(crate) fn pid(&self) -> u32 {
        self.pid
    }

    /// Start time recorded at registration.
    pub(crate) fn start_time(&self) -> u64 {
        self.start_time
    }

    /// Boot id recorded at registration.
    pub(crate) fn boot_id(&self) -> &str {
        &self.boot_id
    }
}

/// Compare a live process with the admitted instance.
pub(crate) fn check_process(
    expected: &ScopeInstance,
    live: Option<&ProcessView>,
) -> Result<(), ScopeFault> {
    let Some(live) = live else {
        return Err(ScopeFault::Unprovable);
    };
    if live.pid == 0 || live.start_time == 0 || live.boot_id.is_empty() {
        return Err(ScopeFault::Unprovable);
    }
    if live.boot_id != expected.boot_id() {
        return Err(ScopeFault::Unprovable);
    }
    if live.pid == expected.pid && live.start_time != expected.start_time() {
        return Err(ScopeFault::PidReuse);
    }
    if live.pid != expected.pid {
        return Err(ScopeFault::OwnerChanged);
    }
    Ok(())
}

/// One window the supervisor registered on an instance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeWindow {
    native_id: String,
    incarnation: u64,
    owner: ScopeInstance,
    app_id: String,
}

impl ScopeWindow {
    /// Register one window. The incarnation must be non-zero.
    ///
    /// `app_id` is the AT-SPI bus name used to find the owner. An empty id
    /// does not match any app.
    pub fn try_new(
        native_id: impl Into<String>,
        incarnation: u64,
        owner: ScopeInstance,
        app_id: impl Into<String>,
    ) -> Result<Self, ScopeFault> {
        let native_id = native_id.into();
        let app_id = app_id.into();
        if native_id.is_empty() || native_id.len() > 1024 || incarnation == 0 || app_id.len() > 1024
        {
            return Err(ScopeFault::Unprovable);
        }
        Ok(Self {
            native_id,
            incarnation,
            owner,
            app_id,
        })
    }

    /// Native window id.
    pub(crate) fn native_id(&self) -> &str {
        &self.native_id
    }

    /// Window incarnation.
    pub(crate) fn incarnation(&self) -> u64 {
        self.incarnation
    }

    /// Owning process.
    pub(crate) fn owner(&self) -> &ScopeInstance {
        &self.owner
    }

    /// AT-SPI application id. Empty when the window has no bus name.
    pub(crate) fn app_id(&self) -> &str {
        &self.app_id
    }

    /// Core binding from a live process view that already matches `owner`.
    pub(crate) fn binding(&self, live: &ProcessView) -> Option<WindowBinding> {
        check_process(&self.owner, Some(live)).ok()?;
        let start = OsStartTime::new(live.start_time).ok()?;
        let boot = BootIdentity::new(live.boot_id.clone()).ok()?;
        let owner = ProcessInstance::new(live.pid, start, boot).ok()?;
        WindowBinding::new(self.native_id.clone(), self.incarnation, owner).ok()
    }
}

/// X11 observation. `net_wm_pid` is `_NET_WM_PID`. `None` is not proof.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct X11View {
    /// X11 window id.
    pub window_id: u32,
    /// `_NET_WM_PID`, when the server returned one.
    pub net_wm_pid: Option<u32>,
    /// Lifetime token. `None` means reuse cannot be told apart.
    pub incarnation: Option<u64>,
    /// Process view of `net_wm_pid`.
    pub owner: Option<ProcessView>,
}

/// Prove `live` is still `expected`.
pub(crate) fn check_x11(expected: &ScopeWindow, live: &X11View) -> Result<(), ScopeFault> {
    if live.window_id.to_string() != expected.native_id {
        if live.net_wm_pid == Some(expected.owner.pid) {
            return Err(ScopeFault::WrongWindow);
        }
        return Err(ScopeFault::Closed);
    }
    let Some(pid) = live.net_wm_pid else {
        return Err(ScopeFault::Closed);
    };
    let Some(incarnation) = live.incarnation.filter(|value| *value > 0) else {
        return Err(ScopeFault::Closed);
    };
    if incarnation != expected.incarnation() {
        return Err(ScopeFault::WindowReuse);
    }
    let Some(owner) = live.owner.as_ref() else {
        return Err(ScopeFault::Closed);
    };
    if pid != owner.pid {
        return Err(ScopeFault::Closed);
    }
    check_process(&expected.owner, Some(owner))
}

/// AT-SPI owner observation for one accessible.
///
/// Test-only. Scoped AT-SPI reads do not build this view.
#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AtspiView {
    /// Bus-owner pid. `None` is not proof.
    pub pid: Option<u32>,
    /// Process view of `pid`.
    pub owner: Option<ProcessView>,
    /// Window the accessible was read under.
    pub window_id: String,
    /// Incarnation of that window.
    pub incarnation: u64,
}

/// Prove the accessible owner is still the admitted window's process.
#[cfg(test)]
pub(crate) fn check_atspi(expected: &ScopeWindow, live: &AtspiView) -> Result<(), ScopeFault> {
    if live.window_id != expected.native_id {
        return Err(if live.pid == Some(expected.owner.pid) {
            ScopeFault::WrongWindow
        } else {
            ScopeFault::Closed
        });
    }
    if live.incarnation == 0 {
        return Err(ScopeFault::Unprovable);
    }
    if live.incarnation != expected.incarnation() {
        return Err(ScopeFault::WindowReuse);
    }
    let Some(pid) = live.pid else {
        return Err(ScopeFault::Closed);
    };
    let Some(owner) = live.owner.as_ref() else {
        return Err(ScopeFault::Closed);
    };
    if pid != owner.pid {
        return Err(ScopeFault::Closed);
    }
    check_process(&expected.owner, Some(owner))
}

/// Where a handle sits now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HandleView {
    /// Window id the handle currently belongs to.
    pub window_id: String,
    /// Incarnation of that window.
    pub incarnation: u64,
    /// Process that owns the handle.
    pub owner: Option<ProcessView>,
}

/// Prove a handle is still on `expected` and still owned by that process.
pub(crate) fn check_handle(expected: &ScopeWindow, live: &HandleView) -> Result<(), ScopeFault> {
    if live.window_id != expected.native_id {
        return Err(ScopeFault::HandleCrossed);
    }
    if live.incarnation == 0 {
        return Err(ScopeFault::Unprovable);
    }
    if live.incarnation != expected.incarnation() {
        return Err(ScopeFault::WindowReuse);
    }
    check_process(&expected.owner, live.owner.as_ref())
}

/// How a Wayland or portal source was proved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WaylandOrigin {
    /// Peer credential for one window, and the stream names that window.
    PeerWindow,
    /// Monitor, output, or the whole desktop.
    Desktop,
    /// A window label without a peer credential.
    UnprovedWindow,
}

/// One Wayland or portal observation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WaylandView {
    /// How the source was proved.
    pub origin: WaylandOrigin,
    /// Compositor window id.
    pub window_id: String,
    /// Compositor generation. Zero is not proof.
    pub generation: u64,
    /// Peer pid. A portal-reported pid is not stored here.
    pub peer_pid: Option<u32>,
    /// Process view of `peer_pid`.
    pub owner: Option<ProcessView>,
    /// Stream source id. It must equal `window_id` for a window source.
    pub stream_source: String,
}

/// Prove a Wayland or portal source. Anything other than a peer window is closed.
pub(crate) fn check_wayland(expected: &ScopeWindow, live: &WaylandView) -> Result<(), ScopeFault> {
    if live.origin != WaylandOrigin::PeerWindow {
        return Err(ScopeFault::Closed);
    }
    if live.stream_source.is_empty() || live.stream_source != live.window_id {
        return Err(ScopeFault::Closed);
    }
    if live.window_id != expected.native_id {
        if live.peer_pid == Some(expected.owner.pid) {
            return Err(ScopeFault::WrongWindow);
        }
        return Err(ScopeFault::Closed);
    }
    if live.generation == 0 {
        return Err(ScopeFault::Closed);
    }
    if live.generation != expected.incarnation() {
        return Err(ScopeFault::WindowReuse);
    }
    let Some(pid) = live.peer_pid else {
        return Err(ScopeFault::Closed);
    };
    let Some(owner) = live.owner.as_ref() else {
        return Err(ScopeFault::Closed);
    };
    if pid != owner.pid {
        return Err(ScopeFault::Closed);
    }
    check_process(&expected.owner, Some(owner))
}

/// A view that closes the Wayland backend for `window_id`.
pub(crate) fn wayland_closed(window_id: &str) -> WaylandView {
    WaylandView {
        origin: WaylandOrigin::UnprovedWindow,
        window_id: window_id.to_string(),
        generation: 0,
        peer_pid: None,
        owner: None,
        stream_source: String::new(),
    }
}

/// Admitted windows for one engine. The list is fixed for the binding.
#[derive(Debug, Clone)]
pub struct ScopeGate {
    windows: Vec<ScopeWindow>,
}

impl ScopeGate {
    /// Store the windows. Duplicate ids, or one app id on two owners, are refused.
    pub fn try_new(windows: Vec<ScopeWindow>) -> Result<Self, ScopeFault> {
        let mut ids = HashSet::new();
        let mut apps = HashMap::<String, u32>::new();
        for window in &windows {
            if !ids.insert(window.native_id.clone()) {
                return Err(ScopeFault::Unprovable);
            }
            if window.app_id.is_empty() {
                continue;
            }
            if let Some(pid) = apps.insert(window.app_id.clone(), window.owner.pid) {
                if pid != window.owner.pid {
                    return Err(ScopeFault::Unprovable);
                }
            }
        }
        Ok(Self { windows })
    }

    /// Registered windows.
    pub(crate) fn windows(&self) -> &[ScopeWindow] {
        &self.windows
    }

    /// Window with this native id.
    pub(crate) fn by_id(&self, native_id: &str) -> Option<&ScopeWindow> {
        self.windows
            .iter()
            .find(|window| window.native_id == native_id)
    }

    /// Prove an X11 observation against the window it names.
    pub(crate) fn prove_x11(&self, live: &X11View) -> Result<&ScopeWindow, ScopeFault> {
        let id = live.window_id.to_string();
        if let Some(expected) = self.by_id(&id) {
            check_x11(expected, live)?;
            return Ok(expected);
        }
        if self
            .windows
            .iter()
            .any(|window| live.net_wm_pid == Some(window.owner.pid) && window.native_id != id)
        {
            return Err(ScopeFault::WrongWindow);
        }
        Err(ScopeFault::Closed)
    }

    /// Prove an AT-SPI owner observation.
    #[cfg(test)]
    pub(crate) fn prove_atspi(&self, live: &AtspiView) -> Result<&ScopeWindow, ScopeFault> {
        let Some(expected) = self.by_id(&live.window_id) else {
            if self
                .windows
                .iter()
                .any(|window| live.pid == Some(window.owner.pid))
            {
                return Err(ScopeFault::WrongWindow);
            }
            return Err(ScopeFault::Closed);
        };
        check_atspi(expected, live)?;
        Ok(expected)
    }

    /// Prove a Wayland or portal observation.
    pub(crate) fn prove_wayland(&self, live: &WaylandView) -> Result<&ScopeWindow, ScopeFault> {
        if live.origin != WaylandOrigin::PeerWindow {
            return Err(ScopeFault::Closed);
        }
        if let Some(expected) = self.by_id(&live.window_id) {
            check_wayland(expected, live)?;
            return Ok(expected);
        }
        if self.windows.iter().any(|window| {
            live.peer_pid == Some(window.owner.pid) && window.native_id != live.window_id
        }) {
            return Err(ScopeFault::WrongWindow);
        }
        Err(ScopeFault::Closed)
    }
}

/// Identity of a handle this process created.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum HandleKey {
    /// Live AT-SPI object.
    Atspi {
        /// Bus name.
        destination: String,
        /// Object path.
        path: String,
    },
    /// In-memory fake node id.
    Fake(u64),
}

/// Key for a handle this driver created. An unknown payload is `None`.
pub(crate) fn handle_key(handle: &NativeHandle) -> Option<HandleKey> {
    if let Some(node) = crate::atspi::extract_atspi_node(handle) {
        return Some(HandleKey::Atspi {
            destination: node.destination,
            path: node.path,
        });
    }
    crate::atspi::extract_fake_id(handle).map(HandleKey::Fake)
}

/// Run `prove`, produce a value, then `prove` again before the value is returned.
///
/// The second failure drops the value. Use this for reads and captures.
pub(crate) fn confirm_twice<T>(
    prove: impl Fn() -> Result<(), DriverError>,
    produce: impl FnOnce() -> Result<T, DriverError>,
) -> Result<T, DriverError> {
    prove()?;
    let value = produce()?;
    prove()?;
    Ok(value)
}

/// One off-runtime X11 observation. Missing geometry is not a frame.
#[derive(Debug, Clone)]
pub(crate) struct X11Snapshot {
    /// Identity observation. An absent incarnation is not proof.
    pub view: X11View,
    /// Frame read with that identity, when the server returned one.
    pub geometry: Option<Geometry>,
}

struct EvidenceState {
    deadline: Option<Instant>,
    prepared: bool,
    fresh: HashSet<u32>,
    snapshots: HashMap<u32, X11Snapshot>,
}

/// Pid cache and handle observations shared by a bound driver.
pub(crate) struct ScopeOracle {
    clock: Arc<dyn ProcessClock>,
    #[cfg(test)]
    atspi_pid: Mutex<HashMap<String, u32>>,
    handles: Mutex<HashMap<HandleKey, HandleView>>,
    evidence: Mutex<EvidenceState>,
}

impl ScopeOracle {
    /// Oracle that reads `clock` and starts with no cached owners.
    pub(crate) fn new(clock: Arc<dyn ProcessClock>) -> Self {
        Self {
            clock: clock.clone(),
            #[cfg(test)]
            atspi_pid: Mutex::new(HashMap::new()),
            handles: Mutex::new(HashMap::new()),
            evidence: Mutex::new(EvidenceState {
                deadline: None,
                prepared: false,
                fresh: HashSet::new(),
                snapshots: HashMap::new(),
            }),
        }
    }

    /// Clock used for process reads.
    pub(crate) fn clock(&self) -> &dyn ProcessClock {
        self.clock.as_ref()
    }

    /// Live process view, or `None` when the clock cannot prove it.
    pub(crate) fn process(&self, pid: u32) -> Option<ProcessView> {
        process_view(self.clock.as_ref(), pid)
    }

    /// Owned clock handle for an off-runtime read.
    pub(crate) fn clock_arc(&self) -> Arc<dyn ProcessClock> {
        Arc::clone(&self.clock)
    }

    fn evidence(&self) -> std::sync::MutexGuard<'_, EvidenceState> {
        self.evidence.lock().unwrap_or_else(|err| err.into_inner())
    }

    /// Open one native-call budget and drop every captured X11 snapshot.
    ///
    /// Later reads in the same action share this deadline. They do not add
    /// another [`NATIVE_TIMEOUT`].
    pub(crate) fn begin_evidence_action(&self) {
        let mut evidence = self.evidence();
        evidence.deadline = Some(Instant::now() + NATIVE_TIMEOUT);
        evidence.prepared = true;
        evidence.fresh.clear();
        evidence.snapshots.clear();
    }

    /// Whether [`Self::begin_evidence_action`] ran and has not been taken.
    pub(crate) fn take_prepared(&self) -> bool {
        let mut evidence = self.evidence();
        let prepared = evidence.prepared;
        evidence.prepared = false;
        prepared
    }

    /// Whether the open budget is still inside [`NATIVE_TIMEOUT`].
    pub(crate) fn budget_is_live(&self) -> bool {
        self.evidence()
            .deadline
            .is_some_and(|deadline| deadline > Instant::now())
    }

    /// Time left in the open budget. Zero when no budget is live.
    pub(crate) fn evidence_remaining(&self) -> std::time::Duration {
        match self.evidence().deadline {
            Some(deadline) => deadline.saturating_duration_since(Instant::now()),
            None => std::time::Duration::ZERO,
        }
    }

    /// Whether `xid` was proved during the open budget.
    pub(crate) fn is_fresh(&self, xid: u32) -> bool {
        self.evidence().fresh.contains(&xid)
    }

    /// Store a proved snapshot and mark `xid` fresh.
    pub(crate) fn store_x11(&self, xid: u32, snapshot: X11Snapshot) {
        let mut evidence = self.evidence();
        evidence.snapshots.insert(xid, snapshot);
        evidence.fresh.insert(xid);
    }

    /// Forget `xid`. The next admission cannot reuse it.
    pub(crate) fn clear_x11(&self, xid: u32) {
        let mut evidence = self.evidence();
        evidence.snapshots.remove(&xid);
        evidence.fresh.remove(&xid);
    }

    /// Captured X11 observation, when this budget stored one.
    pub(crate) fn x11_snapshot(&self, xid: u32) -> Option<X11Snapshot> {
        self.evidence().snapshots.get(&xid).cloned()
    }

    /// Remember a bus pid. `None` clears a stale cache entry.
    ///
    /// Test-only. Production scoped AT-SPI does not consult this cache.
    #[cfg(test)]
    pub(crate) fn set_atspi_pid(&self, app_id: &str, pid: Option<u32>) {
        let mut pids = self.atspi_pid.lock().unwrap_or_else(|err| err.into_inner());
        match pid {
            Some(pid) if pid > 0 => {
                pids.insert(app_id.to_string(), pid);
            }
            _ => {
                pids.remove(app_id);
            }
        }
    }

    /// AT-SPI view for `app_id` under `window_id`.
    ///
    /// Test-only. The incarnation argument is not a live observation.
    #[cfg(test)]
    pub(crate) fn atspi(&self, app_id: &str, window_id: &str, incarnation: u64) -> AtspiView {
        let pid = self
            .atspi_pid
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .get(app_id)
            .copied();
        let owner = pid.and_then(|pid| self.process(pid));
        AtspiView {
            pid,
            owner,
            window_id: window_id.to_string(),
            incarnation,
        }
    }

    /// Live handle observation, when a caller has proved one.
    pub(crate) fn handle(&self, key: &HandleKey) -> Option<HandleView> {
        self.handles
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .get(key)
            .cloned()
    }

    #[cfg(test)]
    pub(crate) fn set_handle(&self, key: HandleKey, view: HandleView) {
        self.handles
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .insert(key, view);
    }
}

/// Gate plus the fresh observations used before a read returns or an input is sent.
pub struct ScopeBinding {
    gate: ScopeGate,
    oracle: ScopeOracle,
    seals: Mutex<HashMap<HandleKey, String>>,
}

impl ScopeBinding {
    /// Bind `gate` to `clock`.
    pub(crate) fn new(gate: ScopeGate, clock: Arc<dyn ProcessClock>) -> Self {
        Self {
            gate,
            oracle: ScopeOracle::new(clock),
            seals: Mutex::new(HashMap::new()),
        }
    }

    /// Admitted windows.
    pub(crate) fn gate(&self) -> &ScopeGate {
        &self.gate
    }

    /// Observation cache.
    pub(crate) fn oracle(&self) -> &ScopeOracle {
        &self.oracle
    }

    /// Record that `key` was read under `window_id`.
    ///
    /// Test-only. Production scoped trees return before any handle is sealed.
    #[cfg(test)]
    pub(crate) fn note_seal(&self, key: HandleKey, window_id: &str) -> Result<(), DriverError> {
        if self.gate.by_id(window_id).is_none() {
            return Err(DriverError::ScopeDenied);
        }
        self.seals
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .insert(key, window_id.to_string());
        Ok(())
    }

    /// Seal every handle in `tree` to `window_id`. An unkeyed handle refuses the tree.
    #[cfg(test)]
    pub(crate) fn seal_tree(&self, tree: &RawTree, window_id: &str) -> Result<(), DriverError> {
        if let Some(root) = &tree.root {
            self.seal_node(root, window_id)?;
        }
        Ok(())
    }

    #[cfg(test)]
    fn seal_node(&self, node: &RawNode, window_id: &str) -> Result<(), DriverError> {
        let key = handle_key(&node.handle).ok_or(DriverError::ScopeDenied)?;
        self.note_seal(key, window_id)?;
        for child in &node.children {
            self.seal_node(child, window_id)?;
        }
        Ok(())
    }

    /// Re-check a sealed handle. No seal, or no live window, is a refusal.
    pub(crate) fn admit_handle(&self, handle: &NativeHandle) -> Result<(), DriverError> {
        let key = handle_key(handle).ok_or(DriverError::ScopeDenied)?;
        let window_id = self
            .seals
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .get(&key)
            .cloned()
            .ok_or(DriverError::ScopeDenied)?;
        let expected = self
            .gate
            .by_id(&window_id)
            .ok_or(DriverError::ScopeDenied)?;
        let Some(live) = self.oracle.handle(&key) else {
            return Err(DriverError::ScopeDenied);
        };
        check_handle(expected, &live).map_err(ScopeFault::deny)
    }

    /// Element binding for a handle with a live observation.
    ///
    /// A seal alone is not an observation. The owner is the process clock's
    /// current view of the live pid, not the pid stored on the registration.
    pub(crate) fn element_binding(
        &self,
        handle: &NativeHandle,
    ) -> Option<gcu_core::scope::ElementBinding> {
        let key = handle_key(handle)?;
        self.admit_handle(handle).ok()?;
        let live = self.oracle.handle(&key)?;
        let pid = live.owner.as_ref()?.pid;
        let owner = self.oracle.process(pid)?;
        let expected = self.gate.by_id(&live.window_id)?;
        Some(gcu_core::scope::ElementBinding::new(
            expected.binding(&owner)?,
        ))
    }
}

#[cfg(test)]
pub(crate) struct MapClock {
    boot: String,
    start: Mutex<HashMap<u32, u64>>,
}

#[cfg(test)]
impl MapClock {
    pub(crate) fn new(boot: impl Into<String>) -> Self {
        Self {
            boot: boot.into(),
            start: Mutex::new(HashMap::new()),
        }
    }

    pub(crate) fn set(&self, pid: u32, start: u64) {
        self.start
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .insert(pid, start);
    }
}

#[cfg(test)]
impl ProcessClock for MapClock {
    fn boot_id(&self) -> Option<String> {
        if self.boot.is_empty() {
            None
        } else {
            Some(self.boot.clone())
        }
    }

    fn start_time(&self, pid: u32) -> Option<u64> {
        let start = self.start.lock().unwrap_or_else(|err| err.into_inner());
        let value = *start.get(&pid)?;
        (value > 0).then_some(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gcu_core::scope::SCOPE_DENIED_MESSAGE;

    fn instance() -> ScopeInstance {
        ScopeInstance::try_new(42, 1000, "boot-a").unwrap()
    }

    fn window(id: &str, incarnation: u64) -> ScopeWindow {
        ScopeWindow::try_new(id, incarnation, instance(), "app.fixture").unwrap()
    }

    fn view(pid: u32, start: u64, boot: &str) -> ProcessView {
        ProcessView::new(pid, start, boot).unwrap()
    }

    #[test]
    fn refusal_text_is_the_fixed_sentence() {
        for fault in [
            ScopeFault::Unprovable,
            ScopeFault::Closed,
            ScopeFault::PidReuse,
            ScopeFault::WindowReuse,
            ScopeFault::OwnerChanged,
            ScopeFault::HandleCrossed,
            ScopeFault::WrongWindow,
        ] {
            let err = fault.deny();
            assert_eq!(err, DriverError::ScopeDenied);
            assert_eq!(err.to_string(), SCOPE_DENIED_MESSAGE);
            assert!(!err.to_string().contains("42"));
            assert!(!err.to_string().contains("boot"));
        }
    }

    #[test]
    fn process_rules_cover_reuse_owner_and_boot() {
        let expected = instance();
        assert!(check_process(&expected, Some(&view(42, 1000, "boot-a"))).is_ok());
        assert_eq!(
            check_process(&expected, Some(&view(42, 2000, "boot-a"))),
            Err(ScopeFault::PidReuse)
        );
        assert_eq!(
            check_process(&expected, Some(&view(99, 1000, "boot-a"))),
            Err(ScopeFault::OwnerChanged)
        );
        assert_eq!(
            check_process(&expected, Some(&view(42, 1000, "boot-b"))),
            Err(ScopeFault::Unprovable)
        );
        assert_eq!(check_process(&expected, None), Err(ScopeFault::Unprovable));
        assert!(ScopeInstance::try_new(0, 1000, "boot-a").is_err());
        assert!(ScopeInstance::try_new(42, 0, "boot-a").is_err());
        assert!(ProcessView::new(42, 1000, "").is_none());
    }

    #[test]
    fn x11_window_id_and_pid_must_match_the_instance() {
        let gate = ScopeGate::try_new(vec![window("7", 5)]).unwrap();
        let good = X11View {
            window_id: 7,
            net_wm_pid: Some(42),
            incarnation: Some(5),
            owner: Some(view(42, 1000, "boot-a")),
        };
        assert!(gate.prove_x11(&good).is_ok());

        let mut missing_pid = good.clone();
        missing_pid.net_wm_pid = None;
        missing_pid.owner = None;
        assert_eq!(gate.prove_x11(&missing_pid), Err(ScopeFault::Closed));

        let mut missing_life = good.clone();
        missing_life.incarnation = None;
        assert_eq!(gate.prove_x11(&missing_life), Err(ScopeFault::Closed));

        let mut reused = good.clone();
        reused.incarnation = Some(6);
        assert_eq!(gate.prove_x11(&reused), Err(ScopeFault::WindowReuse));

        let mut recycled = good.clone();
        recycled.owner = Some(view(42, 2000, "boot-a"));
        assert_eq!(gate.prove_x11(&recycled), Err(ScopeFault::PidReuse));

        let mut owner = good.clone();
        owner.net_wm_pid = Some(99);
        owner.owner = Some(view(99, 1000, "boot-a"));
        assert_eq!(gate.prove_x11(&owner), Err(ScopeFault::OwnerChanged));

        let sibling = X11View {
            window_id: 8,
            net_wm_pid: Some(42),
            incarnation: Some(5),
            owner: Some(view(42, 1000, "boot-a")),
        };
        assert_eq!(gate.prove_x11(&sibling), Err(ScopeFault::WrongWindow));
    }

    #[test]
    fn atspi_owner_and_handle_window_are_checked() {
        let gate = ScopeGate::try_new(vec![window("7", 5)]).unwrap();
        let owner = AtspiView {
            pid: Some(42),
            owner: Some(view(42, 1000, "boot-a")),
            window_id: "7".into(),
            incarnation: 5,
        };
        assert!(gate.prove_atspi(&owner).is_ok());
        let mut moved = owner.clone();
        moved.pid = Some(99);
        moved.owner = Some(view(99, 1000, "boot-a"));
        assert_eq!(gate.prove_atspi(&moved), Err(ScopeFault::OwnerChanged));

        let handle = HandleView {
            window_id: "7".into(),
            incarnation: 5,
            owner: Some(view(42, 1000, "boot-a")),
        };
        assert!(check_handle(gate.by_id("7").unwrap(), &handle).is_ok());
        let crossed = HandleView {
            window_id: "8".into(),
            incarnation: 5,
            owner: Some(view(42, 1000, "boot-a")),
        };
        assert_eq!(
            check_handle(gate.by_id("7").unwrap(), &crossed),
            Err(ScopeFault::HandleCrossed)
        );
    }

    #[test]
    fn wayland_without_a_peer_window_is_closed() {
        let gate = ScopeGate::try_new(vec![window("wayland:1", 5)]).unwrap();
        assert_eq!(
            gate.prove_wayland(&wayland_closed("wayland:1")),
            Err(ScopeFault::Closed)
        );
        let desktop = WaylandView {
            origin: WaylandOrigin::Desktop,
            window_id: "wayland:1".into(),
            generation: 5,
            peer_pid: Some(42),
            owner: Some(view(42, 1000, "boot-a")),
            stream_source: "wayland:1".into(),
        };
        assert_eq!(gate.prove_wayland(&desktop), Err(ScopeFault::Closed));
        let peer = WaylandView {
            origin: WaylandOrigin::PeerWindow,
            ..desktop
        };
        assert!(gate.prove_wayland(&peer).is_ok());
        let mut reused = peer.clone();
        reused.generation = 9;
        assert_eq!(gate.prove_wayland(&reused), Err(ScopeFault::WindowReuse));
    }

    #[test]
    fn second_proof_drops_the_value() {
        let calls = Mutex::new(0);
        let result: Result<&str, DriverError> = confirm_twice(
            || {
                let mut calls = calls.lock().unwrap_or_else(|err| err.into_inner());
                *calls += 1;
                if *calls > 1 {
                    Err(DriverError::ScopeDenied)
                } else {
                    Ok(())
                }
            },
            || Ok("canary-pixels"),
        );
        assert_eq!(result, Err(DriverError::ScopeDenied));
        assert!(!format!("{result:?}").contains("canary"));
    }

    #[test]
    fn sealed_handle_rechecks_the_process_clock() {
        let clock = Arc::new(MapClock::new("boot-a"));
        clock.set(42, 1000);
        let binding = ScopeBinding::new(
            ScopeGate::try_new(vec![window("7", 5)]).unwrap(),
            clock.clone(),
        );
        let handle = NativeHandle::new(11u64);
        assert!(binding.element_binding(&handle).is_none());
        assert!(binding
            .element_binding(&NativeHandle::new("forged"))
            .is_none());
        binding.note_seal(HandleKey::Fake(11), "7").unwrap();
        assert!(binding.element_binding(&handle).is_none());

        clock.set(42, 0);
        assert!(binding.element_binding(&handle).is_none());
        clock.set(42, 2000);
        assert!(binding.element_binding(&handle).is_none());
        clock.set(42, 1000);

        binding.oracle().set_handle(
            HandleKey::Fake(11),
            HandleView {
                window_id: "8".into(),
                incarnation: 5,
                owner: Some(view(42, 1000, "boot-a")),
            },
        );
        assert!(binding.element_binding(&handle).is_none());
        binding.oracle().set_handle(
            HandleKey::Fake(11),
            HandleView {
                window_id: "7".into(),
                incarnation: 5,
                owner: Some(view(42, 1000, "boot-a")),
            },
        );
        let bound = binding.element_binding(&handle).unwrap();
        assert_eq!(bound.window().native_id(), "7");
        assert_eq!(bound.window().incarnation(), 5);
        assert_eq!(bound.window().owner().pid(), 42);
        assert_eq!(bound.window().owner().start().ticks(), 1000);
        clock.set(42, 2000);
        assert!(binding.element_binding(&handle).is_none());
    }
}
