//! Portable driver boundary. No policy, rendering, routing upgrades, or retries.

use crate::model::{Geometry, NativeHandle, NodeAction, RawTree, TextRange, WindowPoint};
use async_trait::async_trait;
use futures_core::Stream;
pub use gcu_protocol::capabilities::Platform;
pub use gcu_protocol::tools::{Direction as Dir, MouseButton as Button};
pub use gcu_protocol::{AppInfo, Capabilities, KeyChord, WindowInfo};
use std::{
    pin::Pin,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use thiserror::Error;
use tokio::sync::Notify;

/// Default hard native-call deadline; mutation timeouts must report uncertainty.
pub const NATIVE_TIMEOUT: Duration = Duration::from_secs(5);
/// Inner deadline a driver enforces on its own native calls.
///
/// It is deliberately shorter than [`NATIVE_TIMEOUT`], the router's outer deadline. A
/// driver that stops first can prove the call had no effect (`maybe_executed == false`);
/// if both limits were equal the outer one could win and report an uncertain outcome for
/// a call that never started. Real drivers must use this value for their own timeouts.
pub const DRIVER_DEADLINE: Duration = Duration::from_millis(4_750);
/// Driver operation result, including whether input might have been delivered.
pub type Result<T, E = DriverError> = std::result::Result<T, E>;
/// Optional event stream, terminated when the native subscription is dropped.
pub type BoxStream<'a, T> = Pin<Box<dyn Stream<Item = T> + Send + 'a>>;

/// Resolved app identity; drivers must validate it against their own app registry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppRef {
    /// Resolved discovery record, including provenance when available.
    pub info: AppInfo,
}
/// Exact window identity; never substitutes another window after it disappears.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowRef {
    /// Owning app.
    pub app: AppRef,
    /// Native window identifier returned by discovery.
    pub id: String,
}
/// Discovery query accepting a name, platform ID, executable name, or absolute path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppQuery(pub String);
impl From<&str> for AppQuery {
    fn from(value: &str) -> Self {
        Self(value.into())
    }
}

/// Tree read options. Core performs tree budgeting and text rendering afterwards.
#[derive(Debug, Clone)]
pub struct TreeOpts {
    /// Hard read deadline, including native waits.
    pub timeout: Duration,
    /// Maximum extended-text Unicode scalars; None requests native maximum.
    pub text_limit: Option<usize>,
    /// Core node budget, including the root. Zero yields an empty observation.
    pub max_tree_nodes: usize,
    /// Core depth budget in levels, counting the window as level one.
    pub max_tree_depth: usize,
}
impl Default for TreeOpts {
    fn default() -> Self {
        Self {
            timeout: NATIVE_TIMEOUT,
            text_limit: Some(500),
            max_tree_nodes: 1200,
            max_tree_depth: 64,
        }
    }
}
/// Native window capture options. Scaling and format conversion belong to core.
#[derive(Debug, Clone)]
pub struct CaptureOpts {
    /// Hard capture deadline.
    pub timeout: Duration,
    /// Whether the native cursor should be included.
    pub include_cursor: bool,
}
impl Default for CaptureOpts {
    fn default() -> Self {
        Self {
            timeout: NATIVE_TIMEOUT,
            include_cursor: false,
        }
    }
}
/// Encoded window capture, using the same physical-pixel origin as node frames.
///
/// Debug is metadata only. Raw bytes are never formatted.
#[derive(Clone)]
pub struct Capture {
    /// Encoded image bytes.
    pub bytes: Vec<u8>,
    /// Physical width.
    pub width: u32,
    /// Physical height.
    pub height: u32,
    /// Encoding MIME type.
    pub mime: String,
    /// Geometry sampled for this capture.
    pub geometry: Geometry,
}
impl std::fmt::Debug for Capture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Capture")
            .field("byte_count", &self.bytes.len())
            .field("width", &self.width)
            .field("height", &self.height)
            .field("mime", &self.mime)
            .field("geometry", &self.geometry)
            .finish()
    }
}
/// Permission/dependency probe with no account access or automatic grants.
#[derive(Debug, Clone)]
pub struct DoctorReport {
    /// Same capabilities exposed by the driver.
    pub capabilities: Capabilities,
    /// Ready for supported operations.
    pub ready: bool,
    /// Missing dependencies or permissions, without payloads.
    pub issues: Vec<String>,
}

#[derive(Default)]
struct CancelState {
    cancelled: AtomicBool,
    notify: Notify,
}
/// Shared, irreversible cancellation token. Cancelling wakes every registered waiter.
#[derive(Clone, Default)]
pub struct Cancel(Arc<CancelState>);
impl Cancel {
    /// Cancel once; subsequent calls are harmless and never reset the token.
    pub fn cancel(&self) {
        self.0.cancelled.store(true, Ordering::SeqCst);
        self.0.notify.notify_waiters();
    }
    /// Nonblocking cancellation check.
    pub fn is_cancelled(&self) -> bool {
        self.0.cancelled.load(Ordering::SeqCst)
    }
    /// Wait until cancelled; cancellation before registration is also observed.
    pub async fn cancelled(&self) {
        loop {
            let notified = self.0.notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if self.is_cancelled() {
                return;
            }
            notified.await;
        }
    }
}

/// Actual delivery path; successful delivery does not assert business success.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Delivery {
    /// Native accessibility action.
    Accessibility,
    /// Window/process-targeted event.
    TargetedEvent,
    /// Global input after authorized foreground activation.
    Foreground,
    /// Clipboard paste with restoration.
    Clipboard,
}
/// Pointer route ceiling, selected by core. Auto and TargetedOnly forbid foreground.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointerRoute {
    /// Choose an available background route.
    Auto,
    /// Targeted events only; failure must not activate the foreground.
    TargetedOnly,
    /// Foreground permitted by the caller's policy decision.
    AllowForeground,
}
/// Keyboard route ceiling, with identical foreground restrictions to pointer input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyRoute {
    /// Choose an available background route.
    Auto,
    /// Targeted events only.
    TargetedOnly,
    /// Caller explicitly permits foreground input.
    AllowForeground,
}

/// Fully resolved action; coordinates have already been converted to window pixels.
#[derive(Clone)]
pub enum DriverAction {
    /// Semantic click.
    Press {
        /// Exact node handle.
        el: NativeHandle,
    },
    /// Advertised semantic or native action.
    SecondaryAction {
        /// Exact node handle.
        el: NativeHandle,
        /// Advertised action.
        action: NodeAction,
    },
    /// Replace an editable or numeric value.
    SetValue {
        /// Exact node handle.
        el: NativeHandle,
        /// Replacement string.
        value: String,
    },
    /// Set native text selection in Unicode-scalar units.
    SelectText {
        /// Exact node handle.
        el: NativeHandle,
        /// Selection range.
        range: TextRange,
    },
    /// Scroll the exact element by pages.
    ScrollElement {
        /// Exact node handle.
        el: NativeHandle,
        /// Direction.
        dir: Dir,
        /// Positive number of pages.
        pages: f32,
    },
    /// Click a physical-pixel point.
    PointerClick {
        /// Exact window.
        win: WindowRef,
        /// Point.
        at: WindowPoint,
        /// Mouse button.
        button: Button,
        /// Number of clicks.
        count: u8,
        /// Allowed route.
        route: PointerRoute,
    },
    /// Scroll at a physical-pixel point.
    PointerScroll {
        /// Exact window.
        win: WindowRef,
        /// Point.
        at: WindowPoint,
        /// Horizontal pixels.
        dx: i32,
        /// Vertical pixels.
        dy: i32,
        /// Allowed route.
        route: PointerRoute,
    },
    /// Drag along an interpolated physical-pixel path.
    PointerDrag {
        /// Exact window.
        win: WindowRef,
        /// Ordered points, at least two.
        path: Vec<WindowPoint>,
        /// Allowed route.
        route: PointerRoute,
    },
    /// Append input, optionally focusing an exact element first.
    TypeText {
        /// Exact window.
        win: WindowRef,
        /// Optional focus target.
        focus: Option<NativeHandle>,
        /// Text, preserved byte-for-byte.
        text: String,
        /// Allowed route.
        route: KeyRoute,
    },
    /// Press and release a validated chord.
    PressKeys {
        /// Exact window.
        win: WindowRef,
        /// Parsed chord.
        chord: KeyChord,
        /// Allowed route.
        route: KeyRoute,
    },
    /// Activate only after core's foreground policy approval.
    Activate {
        /// Exact window.
        win: WindowRef,
    },
}

// Debug output is metadata-only so routine diagnostics cannot leak typed secrets.
impl std::fmt::Debug for DriverAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let kind = match self {
            Self::Press { .. } => "Press",
            Self::SecondaryAction { .. } => "SecondaryAction",
            Self::SetValue { .. } => "SetValue",
            Self::SelectText { .. } => "SelectText",
            Self::ScrollElement { .. } => "ScrollElement",
            Self::PointerClick { .. } => "PointerClick",
            Self::PointerScroll { .. } => "PointerScroll",
            Self::PointerDrag { .. } => "PointerDrag",
            Self::TypeText { .. } => "TypeText",
            Self::PressKeys { .. } => "PressKeys",
            Self::Activate { .. } => "Activate",
        };
        f.debug_struct(kind).finish_non_exhaustive()
    }
}

/// Failures with explicit mutation uncertainty. Deterministic refusals have no effects.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DriverError {
    /// Deadline elapsed; never automatically replay a possibly delivered action.
    #[error("native operation timed out (maybe_executed={maybe_executed})")]
    Timeout {
        /// Whether delivery may have started.
        maybe_executed: bool,
    },
    /// Driver/worker crashed, without panicking across the boundary.
    #[error("driver crashed (maybe_executed={maybe_executed})")]
    Crashed {
        /// Whether delivery may have started.
        maybe_executed: bool,
    },
    /// Cancellation before or during delivery.
    #[error("operation cancelled (maybe_executed={maybe_executed})")]
    Cancelled {
        /// Whether delivery may have started.
        maybe_executed: bool,
    },
    /// Exact app no longer exists.
    #[error("app not found")]
    AppNotFound,
    /// Query identifies more than one app; no arbitrary choice.
    #[error("ambiguous app")]
    AmbiguousApp,
    /// Exact window disappeared; no fallback.
    #[error("window gone")]
    WindowGone,
    /// Exact native handle no longer identifies a live element.
    #[error("stale element")]
    StaleElement,
    /// Unsupported route or native operation; no escalation.
    #[error("capability unavailable")]
    CapabilityUnavailable,
    /// Native control does not accept this action.
    #[error("element not actionable")]
    ElementNotActionable,
    /// Native control cannot accept a value, or the readback never matched.
    ///
    /// `maybe_executed` is false when the write was refused. It is true when a
    /// write was submitted and the value may already have been applied.
    #[error("element not settable")]
    NotSettable {
        /// Whether a write may already have landed.
        maybe_executed: bool,
    },
    /// Physical-pixel point is outside the window.
    #[error("point out of bounds")]
    OutOfBounds,
    /// Invalid range, path, or other driver request.
    #[error("invalid driver request")]
    InvalidRequest,
    /// The target runs at a higher integrity level, or this process is elevated; never escalate.
    #[error("elevation required")]
    ElevationRequired,
    /// An OS permission needed for this operation is missing; the Host guides the user.
    #[error("permission missing: {permission}")]
    PermissionMissing {
        /// Permission name, e.g. `accessibility`; metadata only.
        permission: String,
    },
    /// A secure desktop (UAC, lock/login screen) is active; only the user can proceed.
    #[error("secure desktop active")]
    SecureDesktop,
    /// The target window is minimized and was not restored.
    #[error("window minimized")]
    WindowMinimized,
    /// The target is outside the registered evaluation scope, or proving
    /// evidence is missing. The display text is the only sentence shown to the
    /// model. It never includes an app, path, window list, or driver detail.
    #[error("Target unavailable in this evaluation scope.")]
    ScopeDenied,
    /// Native failure; message must omit payloads.
    #[error("native driver failure: {message} (maybe_executed={maybe_executed})")]
    Native {
        /// Metadata-only diagnostic.
        message: String,
        /// Whether delivery may have started.
        maybe_executed: bool,
    },
}
impl DriverError {
    /// The protocol error this failure reports to the model. `SecureDesktop` is a handoff.
    pub fn error_code(&self) -> gcu_protocol::ErrorCode {
        use gcu_protocol::ErrorCode as C;
        match self {
            Self::Timeout { .. } => C::Timeout,
            Self::Crashed { .. } | Self::Native { .. } => C::DriverError,
            Self::Cancelled { .. } => C::StoppedByUser,
            Self::AppNotFound => C::AppNotFound,
            Self::AmbiguousApp => C::AmbiguousApp,
            Self::WindowGone => C::WindowGone,
            Self::StaleElement => C::StaleElement,
            Self::CapabilityUnavailable | Self::InvalidRequest => C::CapabilityUnavailable,
            Self::ElementNotActionable => C::ElementNotActionable,
            Self::NotSettable { .. } => C::NotSettable,
            Self::OutOfBounds => C::OutOfBounds,
            Self::ElevationRequired => C::ElevationRequired,
            Self::PermissionMissing { .. } => C::PermissionMissing,
            Self::SecureDesktop => C::HandoffRequired,
            Self::WindowMinimized => C::WindowMinimized,
            Self::ScopeDenied => C::AppDenied,
        }
    }

    /// True only when the caller cannot prove that an operation had zero effects.
    pub fn maybe_executed(&self) -> bool {
        match self {
            Self::Timeout { maybe_executed }
            | Self::Crashed { maybe_executed }
            | Self::Cancelled { maybe_executed }
            | Self::Native { maybe_executed, .. }
            | Self::NotSettable { maybe_executed } => *maybe_executed,
            _ => false,
        }
    }
}

/// State of background discovery work, so a first call can return a partial list honestly.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DiscoveryStatus {
    /// An app index is still being built; `list_apps` results may be incomplete.
    pub indexing: bool,
}

/// Native change notification; payloads stay in subsequent observations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UiEvent {
    /// Content/value/focus changed.
    Changed,
    /// Window moved, resized, or changed DPI.
    GeometryChanged,
    /// Window disappeared.
    WindowGone,
}

/// Driver contract. Discovery/geometry calls have a hard NATIVE_TIMEOUT ceiling.
/// Reads honor opts.timeout and cancellation; mutations honor NATIVE_TIMEOUT and
/// cancellation, reporting maybe_executed once delivery may have started. Native
/// blocking APIs belong on dedicated threads. Drivers never retry or make policy.
#[async_trait]
pub trait Driver: Send + Sync + 'static {
    /// Nonblocking platform declaration.
    fn platform(&self) -> Platform;
    /// Nonblocking static plus runtime-probed capabilities.
    fn capabilities(&self) -> &Capabilities;
    /// Probe permissions/dependencies within NATIVE_TIMEOUT, without granting them.
    async fn doctor(&self) -> DoctorReport;
    /// Running and recently used apps within NATIVE_TIMEOUT.
    async fn list_apps(&self) -> Result<Vec<AppInfo>>;
    /// Whether background discovery is still running. Nonblocking; defaults to "complete".
    fn discovery_status(&self) -> DiscoveryStatus {
        DiscoveryStatus::default()
    }
    /// Resolve an unambiguous query within NATIVE_TIMEOUT.
    async fn resolve_app(&self, query: &AppQuery) -> Result<AppRef>;
    /// Launch or unhide within NATIVE_TIMEOUT, without foreground activation.
    async fn launch_app(&self, app: &AppRef) -> Result<AppRef>;
    /// Exact app's windows (or all), within NATIVE_TIMEOUT.
    async fn list_windows(&self, app: Option<&AppRef>) -> Result<Vec<WindowInfo>>;
    /// Return/restore a key window without activating another app, within NATIVE_TIMEOUT.
    async fn key_window(&self, app: &AppRef) -> Result<WindowInfo>;
    /// Read raw structure within opts.timeout, interruptible by cancel.
    async fn snapshot_tree(
        &self,
        win: &WindowRef,
        opts: &TreeOpts,
        cancel: &Cancel,
    ) -> Result<RawTree>;
    /// Capture only this window within opts.timeout, interruptible by cancel.
    async fn capture(
        &self,
        win: &WindowRef,
        opts: &CaptureOpts,
        cancel: &Cancel,
    ) -> Result<Capture>;
    /// Read cheap current screen geometry within NATIVE_TIMEOUT.
    async fn window_geometry(&self, win: &WindowRef) -> Result<Geometry>;
    /// Return the exact native focused element within NATIVE_TIMEOUT.
    async fn focused_element(&self, app: &AppRef) -> Result<Option<NativeHandle>>;
    /// Deliver once within NATIVE_TIMEOUT, preserving the caller's route ceiling.
    async fn perform(&self, req: &DriverAction, cancel: &Cancel) -> Result<Delivery, DriverError>;
    /// Optional nonblocking subscription; dropping it must release native resources.
    fn subscribe(&self, win: &WindowRef) -> Option<BoxStream<'static, UiEvent>>;
    /// Idempotently release all held synthetic input, including after cancellation.
    /// Cleanup is bounded by NATIVE_TIMEOUT and must not depend on a cancelled token.
    async fn release_all_input(&self);
}
