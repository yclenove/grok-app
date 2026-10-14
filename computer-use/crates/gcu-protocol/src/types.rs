//! Serializable observations and geometry; no native handles or driver internals.

use crate::result::ActionResult;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Window-relative physical-pixel rectangle.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Rect {
    /// Horizontal origin.
    pub x: f64,
    /// Vertical origin.
    pub y: f64,
    /// Width.
    pub w: f64,
    /// Height.
    pub h: f64,
}

/// Serializable identity used to bind app authorization to executable provenance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AppIdentity {
    /// Bundle ID, canonical executable path, or desktop file ID.
    pub id: String,
    /// Canonical executable path, when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub executable: Option<String>,
    /// macOS signing team.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team_id: Option<String>,
    /// Windows Authenticode publisher.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub publisher: Option<String>,
    /// Hash for unsigned executable identities.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code_hash: Option<String>,
}

/// A discoverable app; recently used apps may not have a running process.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AppInfo {
    /// Platform app identifier.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Running process ID, when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    /// Authorization identity, when probed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity: Option<AppIdentity>,
}

/// Window information shared by observations and discovery.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WindowInfo {
    /// Opaque string returned by list_windows.
    pub id: String,
    /// Window title.
    pub title: String,
    /// Physical-pixel frame.
    pub frame: Rect,
    /// Display scale factor.
    pub scale: f64,
    /// Whether the window is on the user's current virtual desktop/Space; `None` when the
    /// platform cannot tell. Foreground actions on a window that is not current are refused.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_current_desktop: Option<bool>,
}

/// An image reference, without embedded image bytes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ScreenshotInfo {
    /// Session-scoped image ID.
    pub id: String,
    /// Image width in pixels.
    pub width: u32,
    /// Image height in pixels.
    pub height: u32,
    /// Image MIME type.
    pub mime: String,
    /// Multiplier from screenshot pixels to window physical pixels.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale_to_window: Option<f64>,
}

/// Why a native accessibility traversal left part of the tree unread.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum IncompleteReason {
    /// The traversal or a native read exhausted its deadline.
    Deadline,
    /// The native node ceiling or caller's node limit prevented further reads.
    NodeCap,
    /// A native property or child enumeration could not be read reliably.
    PartialError,
    /// The native traversal depth limit prevented further reads.
    DepthCap,
}

/// Payload-free facts about an incomplete native accessibility traversal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Incomplete {
    /// First cause encountered; a later deadline takes precedence to stop the walk.
    pub reason: IncompleteReason,
    /// Nodes retained from native reads, before any core transformation.
    pub read_nodes: u32,
    /// Exact unread node count, only when known; never a guessed subtree size.
    pub unread_nodes: Option<u32>,
}

/// Core rendering omissions and, independently, native traversal incompleteness.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Truncation {
    /// Number of nodes read but omitted by core visibility/rendering budgets.
    pub omitted_nodes: u32,
    /// Native unread regions, independent of the core's known omission count.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_incomplete: Option<Incomplete>,
}

/// A serialized indexed element, without raw-tree or native-driver types.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ElementInfo {
    /// Snapshot element index.
    pub index: u32,
    /// Normalized snake_case role.
    pub role: String,
    /// Accessible name, if any.
    pub name: Option<String>,
    /// Normalized state names.
    pub states: Vec<String>,
    /// Available secondary actions.
    pub actions: Vec<String>,
    /// Window-relative frame, if available.
    pub frame: Option<Rect>,
}

/// Architecture §4.3 structuredContent, also attached to stale-state errors.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AppState {
    /// Observed app.
    pub app: AppInfo,
    /// Observed window.
    pub window: WindowInfo,
    /// Server-held snapshot ID; callers need not echo it.
    pub snapshot_id: String,
    /// Screenshot metadata or null.
    pub screenshot: Option<ScreenshotInfo>,
    /// Focused element index or null.
    pub focused: Option<u32>,
    /// Budget truncation or null.
    pub truncated: Option<Truncation>,
    /// Full-detail elements; omitted by default to avoid duplicating the text tree.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub elements: Option<Vec<ElementInfo>>,
    /// Action result or null.
    pub action: Option<ActionResult>,
}
