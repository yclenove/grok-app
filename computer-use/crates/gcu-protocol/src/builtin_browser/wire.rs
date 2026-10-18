//! Versioned browser RPC bodies. No IO. Unknown fields and unknown enums are rejected.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// OS named on a binding. A string outside this set does not deserialize.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum OsPlatform {
    /// macOS.
    Macos,
    /// Windows.
    Windows,
    /// Linux.
    Linux,
}

/// The only browser kind L1 accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BrowserKind {
    /// Native window owned by the App, not a page-created view.
    BuiltinBrowser,
}

/// Window-relative physical pixels. The rectangle must sit inside the window.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Geometry {
    /// Left edge, finite and at least zero.
    pub x: f64,
    /// Top edge, finite and at least zero.
    pub y: f64,
    /// Width, finite and positive.
    pub width: f64,
    /// Height, finite and positive.
    pub height: f64,
    /// Display scale, finite and positive.
    pub scale: f64,
    /// Owner window width in physical pixels.
    pub window_width: f64,
    /// Owner window height in physical pixels.
    pub window_height: f64,
}

/// One verified browser window. Page title, label, and window name are not fields.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BrowserWindow {
    /// Engine epoch that produced this window.
    pub epoch: u64,
    /// Document generation. Image rows must match it.
    pub document_revision: u64,
    /// Process start token. Zero is unproven.
    pub started: u64,
    /// Non-zero process id.
    pub pid: u32,
    /// OS that would have to prove network limits.
    pub platform: OsPlatform,
    /// Must be the built-in browser.
    pub kind: BrowserKind,
    /// Rust-generated id, 32 to 128 characters from `[A-Za-z0-9_-]`.
    pub instance_id: String,
    /// Owning App session.
    pub session_id: String,
    /// Native window id from the platform driver.
    pub window_id: String,
    /// Rust or platform generation. Empty is unproven.
    pub generation: String,
    /// Content rectangle inside the native window.
    pub geometry: Geometry,
}

/// `engine.browser.sync` parameters. `browser: null` clears the map.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BrowserSync {
    /// Strictly the previous accepted revision plus one.
    pub revision: u64,
    /// Full binding, or null to remove every window.
    pub browser: Option<BrowserWindow>,
}

/// Ack for [`BrowserSync`]. The revision must echo the request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BrowserAck {
    /// Revision that was accepted.
    pub revision: u64,
}

/// `host.browser.navigate` parameters. The engine fills every field except the model's URL.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BrowserNavigate {
    /// Absolute URL, checked again before any native handoff.
    pub url: String,
    /// Binding instance the engine selected.
    pub instance_id: String,
    /// Binding revision the engine observed.
    pub revision: u64,
    /// Document generation the engine observed. A mismatch is stale.
    pub document_revision: u64,
    /// Policy generation the engine observed.
    pub policy_version: u64,
    /// Owning App session.
    pub session_id: String,
}

/// Successful navigate result. `dispatched: false` is not a success.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BrowserNavigated {
    /// True only after the URL was handed to native navigation.
    pub dispatched: bool,
    /// Instance that received it.
    pub instance_id: String,
}

/// Field checks that serde enums do not express. False means invalid parameters.
#[must_use]
pub fn window_valid(window: &BrowserWindow) -> bool {
    window.pid != 0
        && window.started != 0
        && instance_ok(&window.instance_id)
        && text_ok(&window.session_id)
        && text_ok(&window.window_id)
        && text_ok(&window.generation)
        && geometry_ok(window.geometry)
}

fn instance_ok(id: &str) -> bool {
    (32..=128).contains(&id.len())
        && id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
}

fn text_ok(value: &str) -> bool {
    (1..=256).contains(&value.len()) && !value.chars().any(|ch| ch.is_control())
}

fn geometry_ok(frame: Geometry) -> bool {
    frame.x.is_finite()
        && frame.y.is_finite()
        && frame.width.is_finite()
        && frame.height.is_finite()
        && frame.scale.is_finite()
        && frame.window_width.is_finite()
        && frame.window_height.is_finite()
        && frame.x >= 0.0
        && frame.y >= 0.0
        && frame.width > 0.0
        && frame.height > 0.0
        && frame.scale > 0.0
        && frame.x + frame.width <= frame.window_width
        && frame.y + frame.height <= frame.window_height
}
