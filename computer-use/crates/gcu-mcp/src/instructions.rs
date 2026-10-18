use crate::Profile;
use gcu_core::driver::Platform;
use rmcp::model::Tool;

pub(crate) fn instructions(platform: Platform, profile: Profile, tools: &[Tool]) -> String {
    let platform = match platform {
        Platform::Linux => "Linux",
        Platform::Macos => "macOS",
        Platform::Windows => "Windows",
    };
    let profile = profile.as_str();
    let mut text = format!("Grok Computer Use ({platform}, {profile} profile). First call `get_app_state`, then operate. Prefer element_index. Coordinates are pixels of the latest delivered screenshot for that window; use its screenshot_id. Inspect fresh state returned by actions before observing again. Never blindly repeat an unknown outcome: input may already have executed. For handoff_required, login, or CAPTCHA, ask the user to take over. Never operate terminals, shells, execution consoles, or Grok itself. Authorization is enforced by the server, independently of tool annotations. Temporary image/text paths are valid only for this connection; use the image file if the attachment is unavailable. L0 input checks do not block browser redirects.");
    let has = |name: &str| tools.iter().any(|tool| tool.name == name);
    if has("computer_batch") {
        text.push_str(" Use `computer_batch` for known multi-step workflows.");
    }
    if has("wait_for") {
        text.push_str(" Use `wait_for` for observed conditions.");
    }
    if has("computer_script") {
        text.push_str(" `computer_script` appears only in full with the script feature and an assembled Unix runner; API: cu.d.ts.");
    }
    text
}
