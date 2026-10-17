//! macOS onboarding. The page is one compiled HTML document, not the App's React tree.
//!
//! Permission polling, copy, and helper restart are tested without wry. The
//! WebView module is `cfg(all(target_os = "macos", feature = "macos-webview"))`.
//! wry is optional and built with default features off so Linux does not need
//! WebKitGTK. Linux cross-builds cannot compile its Objective-C helper.

mod compress;
mod document;
mod export_ts;
mod onboarding;
#[cfg(all(target_os = "macos", feature = "macos-webview"))]
mod window;

pub use compress::embed_document;
pub use document::{
    escape_html, js_embed, navigation_allowed, onboarding_page, onboarding_problems, status_script,
};
pub use export_ts::{binding_name, export_module, js_string};
pub use onboarding::{
    apply_ipc, argv_requests_onboarding, open_settings_allowed, perform_restart, relaunch_waiter,
    settings_url, should_open_onboarding, CardStatus, IpcEffect, OnboardingModel, OnboardingView,
    PermissionKind, PermissionSnapshot, RestartError, RestartHost, SettingsOpener,
    ACCESSIBILITY_SETTINGS_URL, SCREEN_RECORDING_SETTINGS_URL,
};

pub(crate) use onboarding::{prepare_agent_onboarding, AgentStart};
