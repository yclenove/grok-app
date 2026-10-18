//! Pure profile projection, shared with xtask so goldens use the serving recipe.
//! No IO, blocking, timeout, or cancellation; availability comes from the caller.

use gcu_protocol::ToolName;
use serde_json::Value;

pub(crate) const MINIMAL_TOOLS: [ToolName; 9] = [
    ToolName::ListApps,
    ToolName::ListWindows,
    ToolName::GetAppState,
    ToolName::Click,
    ToolName::PerformSecondaryAction,
    ToolName::SetValue,
    ToolName::TypeText,
    ToolName::PressKey,
    ToolName::Scroll,
];
pub(crate) const DEFAULT_TOOLS: [ToolName; 15] = [
    ToolName::ListApps,
    ToolName::ListWindows,
    ToolName::LaunchApp,
    ToolName::GetAppState,
    ToolName::Click,
    ToolName::PerformSecondaryAction,
    ToolName::SetValue,
    ToolName::SelectText,
    ToolName::TypeText,
    ToolName::PressKey,
    ToolName::Scroll,
    ToolName::Drag,
    ToolName::WaitFor,
    ToolName::ComputerBatch,
    ToolName::Capabilities,
];
// Explicit executor allowlist, not ToolName::ALL: protocol types may precede
// their implementations. Browser navigation has no executor and is excluded.
pub(crate) const FULL_TOOLS: [ToolName; 16] = [
    ToolName::ListApps,
    ToolName::ListWindows,
    ToolName::LaunchApp,
    ToolName::GetAppState,
    ToolName::Click,
    ToolName::PerformSecondaryAction,
    ToolName::SetValue,
    ToolName::SelectText,
    ToolName::TypeText,
    ToolName::PressKey,
    ToolName::Scroll,
    ToolName::Drag,
    ToolName::WaitFor,
    ToolName::ComputerBatch,
    ToolName::ComputerScript,
    ToolName::Capabilities,
];

/// Tool exposure profile, always intersected with actual implementations.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Profile {
    /// Exactly the nine core tools defined in architecture §11.1.
    Minimal,
    /// The fifteen shipped tools; excludes optional script execution.
    #[default]
    Default,
    /// Default plus implemented, enabled conditional tools. Scripts require a
    /// Unix build with the script feature and an assembled runner. Built-in
    /// browser navigation remains unregistered until ADR 0010 is implemented;
    /// it will require an open, verified browser window owned by this session.
    Full,
}

impl Profile {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Minimal => "minimal",
            Self::Default => "default",
            Self::Full => "full",
        }
    }
}

pub(crate) fn tools_json(profile: Profile, script_ready: bool, implemented: &[ToolName]) -> Value {
    let allowed: &[ToolName] = match profile {
        Profile::Minimal => &MINIMAL_TOOLS,
        Profile::Default => &DEFAULT_TOOLS,
        Profile::Full if script_ready => &FULL_TOOLS,
        Profile::Full => &DEFAULT_TOOLS,
    };
    Value::Array(
        gcu_protocol::tools_list_json()
            .as_array()
            .into_iter()
            .flatten()
            .filter(|tool| {
                let name = tool["name"].as_str().unwrap_or("");
                allowed
                    .iter()
                    .any(|tool| tool.as_str() == name && implemented.contains(tool))
            })
            .cloned()
            .collect(),
    )
}
