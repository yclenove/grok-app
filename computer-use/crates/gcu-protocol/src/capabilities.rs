//! Serializable capability declarations (architecture §5.4), not runtime probes.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Driver platform; version and display-server detail are declared separately.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Platform {
    /// macOS.
    Macos,
    /// Windows.
    Windows,
    /// Linux (X11 or Wayland).
    Linux,
}

/// Observation mechanisms available at runtime.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ObserveCapabilities {
    /// Accessibility tree.
    pub tree: bool,
    /// Window screenshot.
    pub screenshot: bool,
    /// Event subscriptions.
    pub events: bool,
}

/// Foreground input availability; policy is still enforced at action time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ForegroundCapability {
    /// Available subject to policy.
    Policy,
    /// Not supported by this driver.
    Unavailable,
}

/// Available delivery mechanisms, without granting authorization.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DeliveryCapabilities {
    /// Semantic accessibility actions.
    pub accessibility: bool,
    /// Window/process-targeted pointer input.
    pub targeted_pointer: bool,
    /// Window/process-targeted keyboard input.
    pub targeted_keys: bool,
    /// Foreground input capability.
    pub foreground: ForegroundCapability,
}

/// Extent of operation without foreground activation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BackgroundOperation {
    /// All primary routes work in the background.
    Full,
    /// Only some routes work in the background.
    Partial,
    /// No background operation is available.
    None,
}

/// Runtime availability of a named private API.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PrivateApiStatus {
    /// Resolved and enabled.
    Available,
    /// Explicitly disabled.
    Disabled,
    /// Could not be resolved.
    Missing,
}

/// Hard capability limits, distinct from per-request defaults.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CapabilityLimits {
    /// Maximum tree-node request.
    pub max_tree_nodes: u32,
    /// Maximum image long edge.
    pub max_image_edge: u32,
    /// Maximum text input length.
    pub type_text_max: u32,
}

/// Static plus runtime-probed declaration shared by capabilities and doctor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Capabilities {
    /// Driver platform.
    pub platform: Platform,
    /// OS version.
    pub version: String,
    /// Observation capabilities.
    pub observe: ObserveCapabilities,
    /// Delivery capabilities.
    pub delivery: DeliveryCapabilities,
    /// Honest extent of background operation.
    pub background_operation: BackgroundOperation,
    /// Physical user input can be detected.
    pub user_activity_guard: bool,
    /// Overlay can be excluded from captures.
    pub overlay_excluded_from_capture: bool,
    /// Named private APIs and runtime availability (for example skylight).
    pub private_apis: BTreeMap<String, PrivateApiStatus>,
    /// Hard limits.
    pub limits: CapabilityLimits,
}
