//! Built-in browser (L1) gates. Pure data only: no WebView, socket, or profile IO.
//!
//! Every current OS platform reports [`PlatformProof::NotRun`]. [`l1_open`]
//! stays false until a platform proves connection-time destination limits.
//! Callers must not treat a `proven: true` test as a native pass.

mod book;
mod display;
mod profile;
mod url;
mod wire;

pub use book::{ApplyFault, Book, ImageFault, NavFault, NavRequest, Plan};
pub use display::{abbreviate_address, display_origin, escape_untrusted};
pub use profile::{is_exact_profile, profile_dir};
pub use url::{classify, UrlClass, UrlRole, URL_MAX_BYTES};
pub use wire::{
    window_valid, BrowserAck, BrowserKind, BrowserNavigate, BrowserNavigated, BrowserSync,
    BrowserWindow, Geometry, OsPlatform,
};

use crate::ErrorCode;

/// Parent window label. Page-supplied names are not this value.
pub const WINDOW_LABEL: &str = "cu-browser";
/// Address-bar webview. The only browser surface that may call chrome commands.
pub const CHROME_LABEL: &str = "cu-browser-chrome";
/// Content webview. It has no Tauri commands, events, or tokens.
pub const CONTENT_LABEL: &str = "cu-browser-content";
/// Fixed profile directory name under the app data directory. Not a parameter.
pub const PROFILE_DIR_NAME: &str = "cu-browser-profile";
/// Commands the chrome webview may call. Content and the parent window may not.
pub const CHROME_COMMANDS: [&str; 5] = [
    "cu_browser_state",
    "cu_browser_navigate",
    "cu_browser_back",
    "cu_browser_forward",
    "cu_browser_reload",
];

/// Why L1 is closed on a platform. Missing evidence is not a pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlatformProof {
    /// Network, IPC, DNS, and worker limits were not measured on this OS.
    NotRun,
}

/// Evidence for one OS. Public WebView APIs do not prove connection-time destinations.
#[must_use]
pub fn platform_proof(platform: OsPlatform) -> PlatformProof {
    match platform {
        OsPlatform::Macos | OsPlatform::Windows | OsPlatform::Linux => PlatformProof::NotRun,
    }
}

/// Whether L1 may create a content webview or dispatch navigation on `platform`.
#[must_use]
pub fn l1_open(platform: OsPlatform) -> bool {
    !matches!(platform_proof(platform), PlatformProof::NotRun)
}

/// OS this process is compiled for. Used only to keep that platform's L1 closed.
#[must_use]
pub fn current_platform() -> OsPlatform {
    #[cfg(target_os = "macos")]
    {
        OsPlatform::Macos
    }
    #[cfg(target_os = "windows")]
    {
        OsPlatform::Windows
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        OsPlatform::Linux
    }
}

/// Fact supplied by Rust after it has verified the native window. Titles are ignored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedWindow {
    /// App session that owns the browser.
    pub session_id: String,
    /// Native window id from the platform driver, not a webview label.
    pub window_id: String,
    /// Non-zero process id of the verified window owner.
    pub pid: u32,
    /// Non-zero process start token. Zero is an unproven fact.
    pub started: u64,
    /// Non-empty Rust or platform generation. AXIdentifier and titles are not accepted.
    pub generation: String,
}

/// Model parameters for the conditional `browser_navigate` tool. Identity is not here.
#[derive(Debug, Clone, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BrowserNavigateParams {
    /// Absolute URL, at most [`URL_MAX_BYTES`] UTF-8 bytes.
    pub url: String,
}

/// Reject a command for a browser label. Other labels return false.
///
/// The parent window and the content webview reject every command. Chrome
/// accepts only [`CHROME_COMMANDS`]. A page-claimed label is not consulted;
/// the caller must pass the label Rust read from the webview.
#[must_use]
pub fn rejects_command(label: &str, command: &str) -> bool {
    match label {
        WINDOW_LABEL | CONTENT_LABEL => true,
        CHROME_LABEL => !CHROME_COMMANDS.contains(&command),
        _ => false,
    }
}

/// Host JSON-RPC code and fixed message for a navigation fault.
#[must_use]
pub fn host_error(fault: NavFault) -> (i32, &'static str) {
    match fault {
        NavFault::Unavailable => (-32010, "browser_unavailable"),
        NavFault::Owner => (-32011, "browser_owner_mismatch"),
        NavFault::Gone => (-32012, "browser_gone"),
        NavFault::Stale => (-32013, "browser_state_stale"),
        NavFault::UrlBlocked => (-32014, "browser_url_blocked"),
        NavFault::Stopped => (-32015, "browser_stopped"),
        NavFault::Unknown => (-32016, "browser_navigation_unknown"),
        NavFault::Invalid => (-32602, "invalid parameters"),
        NavFault::Busy => (-32002, "busy"),
        NavFault::Internal => (-32603, "internal error"),
    }
}

/// Model error for a definite refusal. [`NavFault::Unknown`] is an outcome, not an error code.
pub fn model_error(fault: NavFault) -> Option<ErrorCode> {
    match fault {
        NavFault::Unavailable | NavFault::Invalid => Some(ErrorCode::CapabilityUnavailable),
        NavFault::Owner => Some(ErrorCode::AppDenied),
        NavFault::Gone => Some(ErrorCode::WindowGone),
        NavFault::Stale => Some(ErrorCode::StaleElement),
        NavFault::UrlBlocked => Some(ErrorCode::KeyBlocked),
        NavFault::Stopped => Some(ErrorCode::StoppedByUser),
        NavFault::Unknown => None,
        NavFault::Busy => Some(ErrorCode::Busy),
        NavFault::Internal => Some(ErrorCode::Internal),
    }
}

/// Classify a successful `host.browser.navigate` body.
///
/// `dispatched: true` means the URL was handed to native navigation. That is
/// delivery, so the caller must not record [`NavFault::Unknown`].
/// `dispatched: false` proves the handoff did not happen. A body that does not
/// say whether it dispatched is uncertain.
pub fn classify_navigated(value: &serde_json::Value) -> Result<(), NavFault> {
    match value.get("dispatched").and_then(serde_json::Value::as_bool) {
        Some(true) => Ok(()),
        Some(false) => Err(NavFault::Internal),
        None => Err(NavFault::Unknown),
    }
}

/// Answer `host.browser.navigate` while L1 is closed.
///
/// Dangerous and non-public URLs return `browser_url_blocked` and do not
/// dispatch. Every other URL returns `browser_unavailable` on an unproven
/// platform. `dispatched: true` is returned only when [`l1_open`] is true.
pub fn navigate_response(
    params: serde_json::Value,
) -> Result<serde_json::Value, (i32, &'static str)> {
    let request: BrowserNavigate =
        serde_json::from_value(params).map_err(|_| (-32602, "invalid parameters"))?;
    if !matches!(
        classify(&request.url, UrlRole::Navigation),
        UrlClass::Public(_)
    ) {
        return Err(host_error(NavFault::UrlBlocked));
    }
    if l1_open(current_platform()) {
        Ok(serde_json::json!({
            "dispatched": true,
            "instance_id": request.instance_id
        }))
    } else {
        Err(host_error(NavFault::Unavailable))
    }
}

/// Shared engine binding. Memory only. A null sync is the production state.
pub fn with_shared<T>(update: impl FnOnce(&mut Book) -> T) -> T {
    static SHARED: std::sync::Mutex<Book> = std::sync::Mutex::new(Book::empty());
    let mut book = SHARED.lock().unwrap_or_else(|error| error.into_inner());
    update(&mut book)
}

/// Replace the process book with an empty map. Integration tests use this so
/// one process can start again at revision 0. Production sessions do not.
pub fn reset_shared_for_tests() {
    with_shared(|book| *book = Book::empty());
}
