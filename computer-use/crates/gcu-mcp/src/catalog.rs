mod profiles;
pub use profiles::Profile;

use gcu_protocol::ToolName;
use rmcp::model::Tool;

/// Executable default tools. Optional scripts require a Unix script build.
#[cfg(not(all(unix, feature = "script")))]
pub const IMPLEMENTED_TOOLS: [ToolName; 15] = profiles::DEFAULT_TOOLS;
/// Executable tools in a Unix script build. Exposure still requires a runner
/// and the full profile. No browser executor is assembled yet (ADR 0010).
#[cfg(all(unix, feature = "script"))]
pub const IMPLEMENTED_TOOLS: [ToolName; 16] = profiles::FULL_TOOLS;

pub(crate) fn tools(profile: Profile, script_ready: bool) -> Result<Vec<Tool>, serde_json::Error> {
    serde_json::from_value(profiles::tools_json(
        profile,
        script_ready,
        &IMPLEMENTED_TOOLS,
    ))
}
