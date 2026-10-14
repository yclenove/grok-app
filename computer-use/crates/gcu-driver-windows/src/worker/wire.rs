//! Private versioned worker messages; no native pointers or arbitrary methods.
use super::{action::Action, tree::Tree};
use gcu_core::{driver::*, model::Geometry};
use gcu_protocol::{AppInfo, Capabilities, WindowInfo};
use serde::{Deserialize, Serialize};
use std::time::Duration;

pub(crate) const VERSION: u32 = 1;
/// Exact app and stamped HWND identity; never a name-based fallback.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Window {
    /// Verified discovery app record.
    pub app: AppInfo,
    /// Opaque stamped window ID from discovery.
    pub id: String,
}
impl From<&WindowRef> for Window {
    fn from(w: &WindowRef) -> Self {
        Self {
            app: w.app.info.clone(),
            id: w.id.clone(),
        }
    }
}
impl From<Window> for WindowRef {
    fn from(w: Window) -> Self {
        Self {
            app: AppRef { info: w.app },
            id: w.id,
        }
    }
}
/// Cross-process element identity. Epoch prevents reuse after restart; HWND,
/// RuntimeId and fingerprint survive unchanged native rereads.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Element {
    /// Exact owning window and app provenance.
    pub window: Window,
    /// Numeric HWND, never a COM pointer.
    pub hwnd: u64,
    /// UIA RuntimeId copied from the native cache.
    pub runtime_id: Vec<i32>,
    /// Native snapshot generation.
    pub generation: u64,
    /// Observed native slot; dispatch resolves the newest slot by RuntimeId.
    pub slot: u64,
    /// Expected native role/name/identifier/path/frame fingerprint.
    pub fingerprint: u64,
    /// Supervisor process incarnation; never valid after worker restart.
    pub epoch: u64,
}
/// Bounded native tree request options.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TreeOptions {
    /// Total caller budget in milliseconds.
    pub timeout_ms: u64,
    /// Maximum extended-text Unicode scalars.
    pub text_limit: Option<usize>,
    /// Node ceiling.
    pub max_tree_nodes: usize,
    /// Depth ceiling.
    pub max_tree_depth: usize,
}
impl From<&TreeOpts> for TreeOptions {
    fn from(o: &TreeOpts) -> Self {
        Self {
            timeout_ms: millis(o.timeout),
            text_limit: o.text_limit,
            max_tree_nodes: o.max_tree_nodes,
            max_tree_depth: o.max_tree_depth,
        }
    }
}
impl From<TreeOptions> for TreeOpts {
    fn from(o: TreeOptions) -> Self {
        Self {
            timeout: Duration::from_millis(o.timeout_ms).min(DRIVER_DEADLINE),
            text_limit: o.text_limit,
            max_tree_nodes: o.max_tree_nodes,
            max_tree_depth: o.max_tree_depth,
        }
    }
}
pub(crate) fn millis(d: Duration) -> u64 {
    d.as_millis().min(u64::MAX as u128) as u64
}
/// Window capture options, without image delivery/redaction permissions.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaptureOptions {
    /// Total capture budget in milliseconds.
    pub timeout_ms: u64,
    /// Include native cursor when supported.
    pub include_cursor: bool,
}
/// Closed operation set. Read-only classification is server-defined, never a
/// caller-supplied flag. KeyWindow may restore a window and is a mutation.
#[derive(Clone, Serialize, Deserialize)]
#[serde(
    tag = "op",
    content = "args",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Request {
    /// Native readiness and capabilities.
    Doctor,
    /// Discovery metadata.
    ListApps,
    /// Resolve exact app provenance.
    ResolveApp(String),
    /// Launch a verified app once.
    LaunchApp(AppInfo),
    /// Enumerate windows.
    ListWindows(Option<AppInfo>),
    /// Resolve/restore the app's key window.
    KeyWindow(AppInfo),
    /// Read raw native tree.
    Snapshot(Window, TreeOptions),
    /// Read raw window PNG. Core still must redact it.
    Capture(Window, CaptureOptions),
    /// Live geometry.
    Geometry(Window),
    /// Exact native focused element.
    Focused(AppInfo),
    /// Deliver one native action; never automatically retry.
    Perform(Action),
}
impl Request {
    /// Whether transport failures can be retried once without side effects.
    pub fn is_read_only(&self) -> bool {
        !matches!(
            self,
            Self::LaunchApp(_) | Self::KeyWindow(_) | Self::Perform(_)
        )
    }
    pub(crate) fn valid_epoch(&self, epoch: u64) -> bool {
        match self {
            Self::Perform(a) => a.valid_epoch(epoch),
            _ => true,
        }
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Call {
    pub version: u32,
    pub id: u64,
    pub epoch: u64,
    pub namespace: String,
    pub request: Request,
}
/// Correlated response. Unknown/malformed variants poison the entire connection.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Response {
    /// Worker protocol version.
    pub version: u32,
    /// Request sequence number.
    pub id: u64,
    /// Process incarnation.
    pub epoch: u64,
    /// Exact reply or native error (contains no provider payload).
    pub result: Result<Reply, Failure>,
}
/// Worker response data. Raw captures are private until core redaction.
#[derive(Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "data",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Reply {
    /// Readiness/capabilities. Events are disabled across this synchronous RPC.
    Doctor {
        /// Actual native capabilities.
        capabilities: Capabilities,
        /// Native readiness.
        ready: bool,
        /// Payload-free diagnostics.
        issues: Vec<String>,
    },
    /// Apps.
    Apps {
        /// In-memory discovery results, possibly incomplete while indexing.
        apps: Vec<AppInfo>,
        /// Native index remains in progress.
        indexing: bool,
    },
    /// Resolved/started app.
    App(AppInfo),
    /// Windows.
    Windows(Vec<WindowInfo>),
    /// Key window.
    Window(WindowInfo),
    /// Flattened tree; parent indices bound JSON nesting depth.
    Tree(Tree),
    /// Raw window capture.
    Capture {
        /// Raw PNG bytes.
        bytes: Vec<u8>,
        /// Physical width.
        width: u32,
        /// Physical height.
        height: u32,
        /// Image MIME.
        mime: String,
        /// Exact geometry.
        geometry: Geometry,
    },
    /// Geometry.
    Geometry(Geometry),
    /// Exact focus.
    Focused(Option<Element>),
    /// Successful route.
    Delivery(WireDelivery),
}
/// Native error adapter preserving certainty and refusal kinds.
#[derive(Serialize, Deserialize)]
pub struct Failure(#[serde(with = "ErrorDef")] pub DriverError);
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// Actual successful delivery route.
pub enum WireDelivery {
    /// Accessibility delivery.
    Accessibility,
    /// TargetedEvent delivery.
    TargetedEvent,
    /// Foreground delivery.
    Foreground,
    /// Clipboard delivery.
    Clipboard,
}
#[derive(Serialize, Deserialize)]
#[serde(remote = "DriverError", rename_all = "snake_case")]
enum ErrorDef {
    Timeout {
        maybe_executed: bool,
    },
    Crashed {
        maybe_executed: bool,
    },
    Cancelled {
        maybe_executed: bool,
    },
    AppNotFound,
    AmbiguousApp,
    WindowGone,
    StaleElement,
    CapabilityUnavailable,
    ElementNotActionable,
    NotSettable,
    OutOfBounds,
    InvalidRequest,
    ElevationRequired,
    PermissionMissing {
        permission: String,
    },
    SecureDesktop,
    WindowMinimized,
    Native {
        message: String,
        maybe_executed: bool,
    },
}

impl From<Delivery> for WireDelivery {
    fn from(value: Delivery) -> Self {
        match value {
            Delivery::Accessibility => Self::Accessibility,
            Delivery::TargetedEvent => Self::TargetedEvent,
            Delivery::Foreground => Self::Foreground,
            Delivery::Clipboard => Self::Clipboard,
        }
    }
}
impl From<WireDelivery> for Delivery {
    fn from(value: WireDelivery) -> Self {
        match value {
            WireDelivery::Accessibility => Self::Accessibility,
            WireDelivery::TargetedEvent => Self::TargetedEvent,
            WireDelivery::Foreground => Self::Foreground,
            WireDelivery::Clipboard => Self::Clipboard,
        }
    }
}
