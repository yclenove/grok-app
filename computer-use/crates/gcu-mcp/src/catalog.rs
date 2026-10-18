use gcu_protocol::ToolName;
use rmcp::model::Tool;
/// Executable default tools. P6 adds tools only after their executors ship.
pub const IMPLEMENTED_TOOLS: [ToolName; 15] = [
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
/// Tool exposure profile, always intersected with actual implementations.
#[derive(Clone, Copy, Debug, Default)]
pub enum Profile {
    /// Nine core tools, as defined in architecture §11.1.
    Minimal,
    /// All currently implemented default tools.
    #[default]
    Default,
    /// Default plus enabled conditional tools (none in P0).
    Full,
}
pub(crate) fn tools(profile: Profile) -> Result<Vec<Tool>, serde_json::Error> {
    gcu_protocol::tools_list_json()
        .as_array()
        .into_iter()
        .flatten()
        .filter(|tool| {
            let name = tool["name"].as_str().unwrap_or("");
            IMPLEMENTED_TOOLS.iter().any(|t| t.as_str() == name)
                && (!matches!(profile, Profile::Minimal)
                    || ![
                        "launch_app",
                        "select_text",
                        "drag",
                        "capabilities",
                        "wait_for",
                        "computer_batch",
                    ]
                    .contains(&name))
        })
        .map(|tool| serde_json::from_value(tool.clone()))
        .collect()
}
