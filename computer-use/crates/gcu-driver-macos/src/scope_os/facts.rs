//! OS facts re-read at the moment of use.
//!
//! A missing fact is absence. Callers return `None` or refuse the delivery.
//! Nothing here substitutes a title, a frame, a bundle id, or a cached pid.

use std::collections::HashMap;
#[cfg(target_os = "macos")]
use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(target_os = "macos")]
use std::sync::Arc;
#[cfg(any(test, target_os = "macos"))]
use std::sync::Mutex;

use gcu_protocol::Rect;

/// One window-server row. Geometry is for hit-testing only, never identity.
#[derive(Clone, Debug, PartialEq)]
pub struct OsWindow {
    /// `CGWindowID`. Zero is not an id.
    pub cg_id: u32,
    /// Owner pid from this list read. Zero is not an owner.
    pub owner_pid: u32,
    /// Window layer. Normal app windows are layer 0.
    pub layer: i32,
    /// Window alpha in `0.0..=1.0`.
    pub alpha: f64,
    /// Screen rectangle in points. Top-left origin.
    pub frame: Rect,
    /// The list placed this row on the current screen.
    pub on_screen: bool,
    /// Continuity token for this window object.
    ///
    /// `None` means this read cannot prove the row is the same object as a
    /// previous row with the same `CGWindowID`. Callers must not invent one
    /// from the title or the frame.
    pub token: Option<u64>,
}

/// One observation of tokens, window rows, boot identity, and process starts.
///
/// The fields come from a single read. Callers must not fill them from a
/// later query. A missing boot is absence, not a wall-clock substitute.
#[derive(Clone, Debug, PartialEq)]
pub struct Snapshot {
    /// Front-to-back rows from this read.
    pub rows: Vec<OsWindow>,
    /// Boot session UUID, when the read returned one.
    pub boot: Option<String>,
    /// Process start in microseconds for each owner pid in `rows`. Zero is unproved.
    pub starts: HashMap<u32, u64>,
}

/// Boot session UUID only. A readable boot time is not an identity.
///
/// Memory only. No blocking, timeout, or cancellation. `boot_time` is ignored
/// so a wall-clock reset cannot make two boots compare equal.
#[cfg(any(test, target_os = "macos"))]
pub(crate) fn boot_identity_from(uuid: Option<&str>, boot_time: Option<&str>) -> Option<String> {
    let _boot_time_is_not_identity = boot_time;
    let uuid = uuid.map(str::trim).filter(|value| !value.is_empty())?;
    Some(uuid.to_owned())
}

/// AX element facts. `cg_id` is `None` when the window cannot be proved.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ElementFact {
    /// `AXUIElementGetPid` result.
    pub pid: u32,
    /// Proved `CGWindowID`, when the public or enabled private call returned one.
    pub cg_id: Option<u32>,
}

/// Facts the platform can re-derive now.
///
/// Implementations must not answer from an identity cache. Proof reads
/// [`OsFacts::snapshot`], which is one observation. The default snapshot is
/// unavailable and does not block. A live macOS body may block on the
/// accessibility thread for at most the accessibility call budget. It does
/// not add cancellation.
pub trait OsFacts: Send + Sync {
    /// Boot identity, or `None` when the sysctl cannot be read.
    fn boot_identity(&self) -> Option<String>;
    /// Process start in microseconds, or `None` when the pid is gone or the time is zero.
    fn process_start_us(&self, pid: u32) -> Option<u64>;
    /// Executable path. Evidence only. `None` when `proc_pidpath` fails.
    ///
    /// Scope admission does not consult this path. It stays on the fact source
    /// so a caller can record [`gcu_core::scope::ExecutableProof`] without
    /// treating the string as authorization.
    #[allow(dead_code)]
    fn executable_path(&self, pid: u32) -> Option<String>;
    /// Front-to-back window rows. `None` means the list itself could not be read.
    /// An empty vector is a real empty desktop. Proof uses [`OsFacts::snapshot`].
    #[allow(dead_code)]
    fn windows(&self) -> Option<Vec<OsWindow>>;
    /// Token, rows, boot, and owner starts from one read.
    ///
    /// `None` means the read was inconsistent or unavailable. Callers refuse.
    /// The default is unavailable and does not block. A live body may block on
    /// the accessibility thread for at most the accessibility call budget. It
    /// does not add cancellation.
    fn snapshot(&self) -> Option<Snapshot> {
        None
    }
    /// AX owner for a driver element id. `None` when the id is unknown or unproved.
    fn element(&self, pid: u32, id: u64) -> Option<ElementFact>;
    /// True when the observer knows it dropped a window event.
    fn observation_lost(&self) -> bool {
        false
    }
    /// Shared token book, when this source has one.
    #[cfg(target_os = "macos")]
    fn token_book(&self) -> Option<Arc<TokenBook>> {
        None
    }
}

/// Continuity tokens published by the accessibility thread.
///
/// Held pointers are `AXUIElementRef` values. Only the accessibility thread
/// may dereference or release them. Other threads read [`TokenBook::published`].
#[cfg(target_os = "macos")]
pub struct TokenBook {
    published: Mutex<HashMap<u32, u64>>,
    held: Mutex<HashMap<u32, usize>>,
    next: AtomicU64,
}

#[cfg(target_os = "macos")]
impl TokenBook {
    /// Empty book. The first minted token is 1.
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self {
            published: Mutex::new(HashMap::new()),
            held: Mutex::new(HashMap::new()),
            next: AtomicU64::new(1),
        })
    }

    /// Tokens from the last refresh. Missing keys are unproved.
    pub(crate) fn published(&self) -> HashMap<u32, u64> {
        self.published
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clone()
    }

    /// Drop published tokens. Held pointers stay until the accessibility thread takes them.
    pub(crate) fn clear_published(&self) {
        self.published
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clear();
    }

    /// Remove held pointers. The caller releases them on the accessibility thread.
    pub(crate) fn take_held(&self) -> HashMap<u32, usize> {
        std::mem::take(&mut *self.held.lock().unwrap_or_else(|err| err.into_inner()))
    }

    /// Next non-zero token, or `None` when the counter wraps.
    pub(crate) fn mint(&self) -> Option<u64> {
        let value = self.next.fetch_add(1, Ordering::Relaxed);
        if value == 0 {
            None
        } else {
            Some(value)
        }
    }

    /// Replace held pointers and the published map.
    ///
    /// The caller has already released the previous pointers.
    pub(crate) fn store(&self, held: HashMap<u32, usize>, tokens: HashMap<u32, u64>) {
        *self.held.lock().unwrap_or_else(|err| err.into_inner()) = held;
        *self.published.lock().unwrap_or_else(|err| err.into_inner()) = tokens;
    }
}

/// Facts that prove nothing. Missing permissions stay on this path.
#[derive(Clone, Copy, Debug, Default)]
pub struct ClosedFacts;

impl OsFacts for ClosedFacts {
    fn boot_identity(&self) -> Option<String> {
        None
    }
    fn process_start_us(&self, _pid: u32) -> Option<u64> {
        None
    }
    fn executable_path(&self, _pid: u32) -> Option<String> {
        None
    }
    fn windows(&self) -> Option<Vec<OsWindow>> {
        None
    }
    fn element(&self, _pid: u32, _id: u64) -> Option<ElementFact> {
        None
    }
}

#[cfg(test)]
#[derive(Default)]
struct FakeInner {
    boot: Option<String>,
    boot_time: Option<String>,
    procs: HashMap<u32, (u64, Option<String>)>,
    windows: Option<Vec<OsWindow>>,
    lost: bool,
    elements: HashMap<(u32, u64), ElementFact>,
    /// Token map and the window list disagree. `windows` stays readable.
    token_cg_gap: bool,
}

/// Scripted facts for tests. No process, window, or accessibility call.
#[cfg(test)]
#[derive(Default)]
pub struct FakeOsFacts {
    inner: Mutex<FakeInner>,
}

#[cfg(test)]
impl FakeOsFacts {
    /// Empty facts. Every read is `None` until a setter runs.
    pub fn new() -> Self {
        Self::default()
    }

    /// Replace the boot identity. An empty string is stored and then rejected.
    pub fn set_boot(&self, boot: impl Into<String>) {
        self.inner
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .boot = Some(boot.into());
    }

    /// Store a readable boot time. It is not accepted as boot identity.
    pub fn set_boot_time(&self, boot_time: impl Into<String>) {
        self.inner
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .boot_time = Some(boot_time.into());
    }

    /// The next snapshot is inconsistent even though [`OsFacts::windows`] still joins by id.
    pub fn arm_token_cg_reuse(&self) {
        self.inner
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .token_cg_gap = true;
    }

    /// Replace one process. `path` is evidence only.
    pub fn set_process(&self, pid: u32, start_us: u64, path: Option<&str>) {
        self.inner
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .procs
            .insert(pid, (start_us, path.map(str::to_owned)));
    }

    /// Remove a process so the next read looks like it exited.
    pub fn drop_process(&self, pid: u32) {
        self.inner
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .procs
            .remove(&pid);
    }

    /// Replace the window list. `None` is an unreadable list.
    pub fn set_windows(&self, windows: Option<Vec<OsWindow>>) {
        self.inner
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .windows = windows;
    }

    /// Mark the observer as having lost window events.
    pub fn set_lost(&self, lost: bool) {
        self.inner
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .lost = lost;
    }

    /// Install an element fact. The key is `(claimed pid, element id)`.
    pub fn set_element(&self, pid: u32, id: u64, fact: ElementFact) {
        self.inner
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .elements
            .insert((pid, id), fact);
    }
}

#[cfg(test)]
impl OsFacts for FakeOsFacts {
    fn boot_identity(&self) -> Option<String> {
        let inner = self.inner.lock().unwrap_or_else(|err| err.into_inner());
        boot_identity_from(inner.boot.as_deref(), inner.boot_time.as_deref())
    }

    fn process_start_us(&self, pid: u32) -> Option<u64> {
        let start = self
            .inner
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .procs
            .get(&pid)?
            .0;
        if start == 0 {
            None
        } else {
            Some(start)
        }
    }

    fn executable_path(&self, pid: u32) -> Option<String> {
        let path = self
            .inner
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .procs
            .get(&pid)?
            .1
            .clone()?;
        if path.is_empty() {
            None
        } else {
            Some(path)
        }
    }

    fn windows(&self) -> Option<Vec<OsWindow>> {
        self.inner
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .windows
            .clone()
    }

    fn element(&self, pid: u32, id: u64) -> Option<ElementFact> {
        self.inner
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .elements
            .get(&(pid, id))
            .cloned()
    }

    fn observation_lost(&self) -> bool {
        self.inner
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .lost
    }

    fn snapshot(&self) -> Option<Snapshot> {
        let inner = self.inner.lock().unwrap_or_else(|err| err.into_inner());
        if inner.token_cg_gap {
            return None;
        }
        let rows = inner.windows.clone()?;
        let boot = boot_identity_from(inner.boot.as_deref(), inner.boot_time.as_deref());
        let mut starts = HashMap::new();
        for row in &rows {
            let start = inner
                .procs
                .get(&row.owner_pid)
                .map(|pair| pair.0)
                .unwrap_or(0);
            starts.insert(row.owner_pid, start);
        }
        Some(Snapshot { rows, boot, starts })
    }
}

/// Visible app window. Higher layers stay in the hit test and out of this check.
pub(crate) fn is_app_window(window: &OsWindow) -> bool {
    window.on_screen
        && window.layer == 0
        && window.alpha > 0.01
        && window.frame.w >= 2.0
        && window.frame.h >= 2.0
        && window.owner_pid > 0
        && window.cg_id > 0
}

/// Half-open frame test. Non-finite coordinates miss.
pub(crate) fn contains(frame: Rect, x: f64, y: f64) -> bool {
    x.is_finite()
        && y.is_finite()
        && x >= frame.x
        && y >= frame.y
        && x < frame.x + frame.w
        && y < frame.y + frame.h
}
