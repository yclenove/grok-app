use crate::Profile;
use gcu_core::driver::Platform;
pub(crate) fn instructions(platform: Platform, profile: Profile, route_note: &str) -> String {
    let platform = match platform {
        Platform::Linux => "Linux",
        Platform::Macos => "macOS",
        Platform::Windows => "Windows",
    };
    let profile = match profile {
        Profile::Minimal => "minimal",
        Profile::Default => "default",
        Profile::Full => "full",
    };
    let base = format!("Grok Computer Use ({platform}, {profile} profile). First call get_app_state, then operate. Prefer element_index. Coordinates are pixels of the latest delivered screenshot for that window; use its screenshot_id. Actions return fresh state, so another observation is unnecessary. Never blindly repeat an unknown outcome: input may already have executed. For handoff_required, login, or CAPTCHA, ask the user to take over. Never operate terminals, shells, execution consoles, or Grok itself. Authorization is enforced by the server, independently of tool annotations. Temporary image/text paths are valid only for this connection; use the image file if the attachment is unavailable.");
    if route_note.is_empty() {
        base
    } else {
        format!("{base} {route_note}")
    }
}
