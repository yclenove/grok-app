//! Tool parameters and the complete default surface, including future-stage tools.

mod actions;
pub use actions::*;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;

macro_rules! tool_names {
    ($($variant:ident => $name:literal: $doc:literal),+ $(,)?) => {
        /// The sixteen default tools in architecture §11.1.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
        #[serde(rename_all = "snake_case")]
        pub enum ToolName { $(#[doc = $doc] $variant),+ }
        impl ToolName {
            /// Complete default catalog in architecture order.
            pub const ALL: [Self; 16] = [$(Self::$variant),+];
            /// Stable MCP name, without allocation or IO.
            pub const fn as_str(self) -> &'static str { match self { $(Self::$variant => $name),+ } }
            /// MCP readOnlyHint from architecture §11.1; observation still needs authorization.
            pub const fn is_read_only(self) -> bool {
                matches!(self, Self::ListApps | Self::ListWindows | Self::GetAppState | Self::WaitFor | Self::Capabilities)
            }
        }
    };
}

tool_names! {
    ListApps => "list_apps": "Discover running and recently used apps.",
    ListWindows => "list_windows": "Discover windows.",
    LaunchApp => "launch_app": "Launch an app.",
    GetAppState => "get_app_state": "Observe an app window.",
    Click => "click": "Click an element or screenshot point.",
    PerformSecondaryAction => "perform_secondary_action": "Perform an advertised secondary action.",
    SetValue => "set_value": "Set an element value.",
    SelectText => "select_text": "Select text or a character range.",
    TypeText => "type_text": "Type text into an app.",
    PressKey => "press_key": "Press a key chord.",
    Scroll => "scroll": "Scroll an element or screenshot point.",
    Drag => "drag": "Drag between screenshot points.",
    WaitFor => "wait_for": "Wait for an observed condition.",
    ComputerBatch => "computer_batch": "Execute a bounded batch.",
    ComputerScript => "computer_script": "Execute a bounded computer-use script.",
    Capabilities => "capabilities": "Read runtime capabilities.",
}

/// A synchronous parameter preflight failure; no action has been attempted.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{0}")]
pub struct ParamError(pub &'static str);

/// No parameters for list_apps.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListAppsParams {}

/// Optional app filter for list_windows.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListWindowsParams {
    /// App name, platform ID, executable name, or absolute path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app: Option<String>,
}

/// Launch an app after authorization.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LaunchAppParams {
    /// App name, platform ID, executable name, or absolute path.
    pub app: String,
    /// One-sentence purpose for the approval UI.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Literal unlimited text marker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MaxTextLimit {
    /// Do not truncate text.
    Max,
}

/// Per-field character budget or the literal string max.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum TextLimit {
    /// Character budget.
    Characters(u32),
    /// Unlimited text.
    Unlimited(MaxTextLimit),
}
impl Default for TextLimit {
    fn default() -> Self {
        Self::Characters(500)
    }
}
fn yes() -> bool {
    true
}
fn tree_nodes() -> u32 {
    1200
}

/// Observe the key window or a specified window (authorization required).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetAppStateParams {
    /// App name, platform ID, executable name, or absolute path.
    pub app: String,
    /// list_windows ID; omitted means key window.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window: Option<String>,
    /// Attach a window screenshot, default true.
    #[serde(default = "yes")]
    pub include_screenshot: bool,
    /// Per-field character limit, default 500, or max.
    #[serde(default)]
    pub text_limit: TextLimit,
    /// Tree-node budget, default 1200.
    #[serde(default = "tree_nodes")]
    #[schemars(range(min = 1, max = 5000))]
    pub max_tree_nodes: u32,
    /// One-sentence purpose for the approval UI.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

macro_rules! app_tool {
    ($name:ident, $args:ty, $doc:literal) => {
        #[doc = $doc]
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
        #[serde(deny_unknown_fields)]
        pub struct $name {
            /// App name, platform ID, executable name, or absolute path.
            pub app: String,
            /// list_windows ID; omitted means key window.
            #[serde(default, skip_serializing_if = "Option::is_none")]
            pub window: Option<String>,
            /// Action payload, serialized at the same level as app/window.
            #[serde(flatten)]
            pub args: $args,
        }
    };
}
app_tool!(
    ClickParams,
    ClickArgs,
    "Click an indexed element or screenshot point."
);
app_tool!(
    PerformSecondaryActionParams,
    PerformSecondaryActionArgs,
    "Perform an advertised element action."
);
app_tool!(
    SetValueParams,
    SetValueArgs,
    "Replace a settable element value."
);
app_tool!(
    SelectTextParams,
    SelectTextArgs,
    "Select literal text or a range."
);
app_tool!(
    TypeTextParams,
    TypeTextArgs,
    "Type text, optionally focusing an element first."
);
app_tool!(
    PressKeyParams,
    PressKeyArgs,
    "Press an xdotool-style key chord."
);
app_tool!(
    ScrollParams,
    ScrollArgs,
    "Scroll an element or screenshot point."
);
app_tool!(
    DragParams,
    DragArgs,
    "Drag between screenshot-pixel points."
);
app_tool!(
    WaitForParams,
    WaitForArgs,
    "Wait for a condition without mutating the app."
);

/// Match a normalized role, optionally with an accessible name.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ElementCondition {
    /// Normalized snake_case role.
    pub role: String,
    /// Optional accessible name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// Externally tagged condition used by wait_for and batch expect.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Condition {
    /// Text occurs in the observed state.
    TextPresent(String),
    /// Text is absent from the observed state.
    TextAbsent(String),
    /// A matching element exists.
    Element(ElementCondition),
    /// The state is stable; payload must be true.
    Stable(#[schemars(extend("const" = true))] bool),
    /// Window title matches.
    WindowTitle(String),
}
impl Condition {
    /// Validate a condition synchronously without observing, waiting, or cancellation.
    pub fn validate(&self) -> Result<(), ParamError> {
        if matches!(self, Self::Stable(false)) {
            return Err(ParamError("stable must be true"));
        }
        Ok(())
    }
}

/// Externally tagged batch step, reusing the standalone action payloads.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum BatchStep {
    /// Click.
    Click(ClickArgs),
    /// Secondary action.
    PerformSecondaryAction(PerformSecondaryActionArgs),
    /// Replace value.
    SetValue(SetValueArgs),
    /// Text selection.
    SelectText(SelectTextArgs),
    /// Type text.
    TypeText(TypeTextArgs),
    /// Key chord.
    PressKey(PressKeyArgs),
    /// Scroll.
    Scroll(ScrollArgs),
    /// Drag.
    Drag(DragArgs),
    /// Wait.
    WaitFor(WaitForArgs),
    /// Check an expectation without waiting.
    Expect(Condition),
}
impl BatchStep {
    /// Preflight one step with no IO, blocking, timeout, or cancellation.
    pub fn validate(&self) -> Result<(), ParamError> {
        match self {
            Self::Click(args) => args.validate(),
            Self::SelectText(args) => args.validate(),
            Self::Scroll(args) => args.validate(),
            Self::Drag(args) => args.validate(),
            Self::WaitFor(args) => args.validate(),
            Self::Expect(condition) => condition.validate(),
            Self::PressKey(args) => args
                .validate()
                .map_err(|_| ParamError("invalid batch key chord")),
            Self::PerformSecondaryAction(_) | Self::SetValue(_) | Self::TypeText(_) => Ok(()),
        }
    }
}

/// Bounded batch; all steps must pass preflight before any action is executed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ComputerBatchParams {
    /// App inherited by every step.
    pub app: String,
    /// Window inherited by every step, omitted means key window.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window: Option<String>,
    /// Between 1 and 32 action/wait/expect steps; execution budget is 60 seconds.
    #[schemars(length(min = 1, max = 32))]
    pub steps: Vec<BatchStep>,
    /// Stop at the first error, default true.
    #[serde(default = "yes")]
    pub stop_on_error: bool,
    /// Return final state, default true.
    #[serde(default = "yes")]
    pub final_state: bool,
}
impl ComputerBatchParams {
    /// Check every step synchronously with zero execution and no IO or waiting.
    pub fn validate(&self) -> Result<(), ParamError> {
        if !(1..=32).contains(&self.steps.len()) {
            return Err(ParamError("batch must contain 1..=32 steps"));
        }
        for step in &self.steps {
            step.validate()?;
        }
        Ok(())
    }
}
fn script_timeout() -> u32 {
    30000
}

/// Feature-gated script parameters; schema remains part of the default catalog.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ComputerScriptParams {
    /// UTF-8 code, at most 20 KB.
    #[schemars(length(max = 20000))]
    pub code: String,
    /// Wall-clock timeout, default and maximum 30000 milliseconds.
    #[serde(default = "script_timeout")]
    #[schemars(range(min = 1, max = 30000))]
    pub timeout_ms: u32,
}
impl ComputerScriptParams {
    /// Check UTF-8 byte and timeout limits without running code or waiting.
    pub fn validate(&self) -> Result<(), ParamError> {
        if self.code.len() > 20000 {
            return Err(ParamError("code exceeds 20000 UTF-8 bytes"));
        }
        if !(1..=30000).contains(&self.timeout_ms) {
            return Err(ParamError("timeout_ms must be 1..=30000"));
        }
        Ok(())
    }
}

/// No parameters for capabilities.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CapabilitiesParams {}
