//! Public managed-browser / existing-tab types.

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TabInfo {
    pub tab_id: String,
    pub run_id: String,
    pub session: String,
    pub title: String,
    pub url: String,
    pub user_owned: bool,
    pub borrowed: bool,
    pub closed: bool,
    pub generation: u64,
    pub home_index: Option<u32>,
    #[serde(default)]
    pub document_generation: u64,
    #[serde(default)]
    pub connection_generation: u64,
    #[serde(default)]
    pub focused: bool,
    #[serde(default)]
    pub disconnect_reason: Option<String>,
    #[serde(default)]
    pub preview_generation: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabDisconnect {
    Stop,
    Disconnect,
    ExtensionUpdate,
    TabClose,
    BrowserExit,
    AppExit,
}

impl TabDisconnect {
    pub fn as_str(self) -> &'static str {
        match self {
            TabDisconnect::Stop => "stop",
            TabDisconnect::Disconnect => "disconnect",
            TabDisconnect::ExtensionUpdate => "extension_update",
            TabDisconnect::TabClose => "tab_close",
            TabDisconnect::BrowserExit => "browser_exit",
            TabDisconnect::AppExit => "app_exit",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ManagedPage {
    pub page_id: String,
    pub page_generation: u64,
    pub url: String,
    pub popup: bool,
}

#[derive(Debug, Clone)]
pub struct ManagedProfile {
    pub dir: std::path::PathBuf,
    pub page: ManagedPage,
}

#[derive(Debug, Clone)]
pub struct ManagedDownload {
    pub path: std::path::PathBuf,
    pub page: ManagedPage,
}

#[derive(Debug, Clone, Default)]
pub struct ManagedLocator {
    pub element_ref: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ManagedNode {
    pub element_ref: String,
    pub role: String,
    pub name: String,
    pub disabled: bool,
    pub truncated: bool,
}

#[derive(Debug, Clone)]
pub struct ManagedObservation {
    pub page_id: String,
    pub page_generation: u64,
    pub snapshot_id: String,
    pub url: String,
    pub title: String,
    pub aria: String,
    pub nodes: Vec<ManagedNode>,
    pub truncated: bool,
    pub text_only: bool,
    pub image_width: u32,
    pub image_height: u32,
    pub image_content_id: String,
    pub png_base64: Option<String>,
    pub image_omitted_reason: Option<String>,
}

#[derive(Debug, Clone, Copy)]
pub struct ManagedPageRef<'a> {
    pub page_id: &'a str,
    pub page_generation: u64,
}

#[derive(Debug)]
pub struct ManagedWorkerAction<'a> {
    pub owner: &'a str,
    pub profile: &'a str,
    pub page: ManagedPageRef<'a>,
    pub snapshot_id: &'a str,
    pub action_id: &'a str,
    pub kind: &'a str,
    pub locator: &'a ManagedLocator,
    pub params: &'a serde_json::Value,
}

#[derive(Debug)]
pub struct ManagedWorkerUpload<'a> {
    pub owner: &'a str,
    pub profile: &'a str,
    pub page: ManagedPageRef<'a>,
    pub snapshot_id: &'a str,
    pub action_id: &'a str,
    pub element_ref: &'a str,
    pub source: &'a std::path::Path,
}

#[derive(Debug)]
pub struct ManagedTabAction<'a> {
    pub page_generation: u64,
    pub snapshot_id: &'a str,
    pub action_id: &'a str,
    pub kind: &'a str,
    pub locator: ManagedLocator,
    pub params: serde_json::Value,
}

#[derive(Debug, Clone, Copy)]
pub struct SharedTabOffer<'a> {
    pub pairing_token: &'a str,
    pub origin: &'a str,
    pub extension_id: Option<&'a str>,
    pub tab_id: &'a str,
    pub title: &'a str,
    pub url: &'a str,
    pub browser_id: &'a str,
    pub profile_id: &'a str,
    pub document_generation: u64,
    pub connection_generation: u64,
    pub focused: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkerCompletion {
    NotStarted,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkerError {
    pub status: u16,
    pub code: String,
    pub completion: WorkerCompletion,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_page_generation: Option<u64>,
}

impl WorkerError {
    pub fn not_started(status: u16, code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            status,
            code: code.into(),
            completion: WorkerCompletion::NotStarted,
            message: message.into(),
            current_page_generation: None,
        }
    }

    pub fn unknown(status: u16, code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            status,
            code: code.into(),
            completion: WorkerCompletion::Unknown,
            message: message.into(),
            current_page_generation: None,
        }
    }

    pub fn with_current_page_generation(mut self, generation: Option<u64>) -> Self {
        self.current_page_generation = generation;
        self
    }

    pub fn transport(message: impl Into<String>) -> Self {
        Self::unknown(502, "worker_transport", message)
    }

    pub fn timeout(message: impl Into<String>) -> Self {
        Self::unknown(504, "worker_timeout", message)
    }

    pub fn invalid_response(status: u16) -> Self {
        Self::unknown(
            status,
            "invalid_worker_response",
            "managed browser worker returned an invalid response",
        )
    }

    pub fn is_valid_for_http_status(&self, actual_status: u16) -> bool {
        self.status == actual_status
            && (400..=599).contains(&self.status)
            && !self.code.is_empty()
            && self.code.len() <= 64
            && self
                .code
                .bytes()
                .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == b'_')
            && !self.message.trim().is_empty()
            && self.message.len() <= 1024
            && self
                .current_page_generation
                .is_none_or(|generation| generation > 0)
    }
}

impl std::fmt::Display for WorkerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} (status {}, completion {:?}): {}",
            self.code, self.status, self.completion, self.message
        )
    }
}

impl std::error::Error for WorkerError {}

impl From<String> for WorkerError {
    fn from(message: String) -> Self {
        Self::unknown(500, "worker_error", message)
    }
}

impl From<&str> for WorkerError {
    fn from(message: &str) -> Self {
        Self::unknown(500, "worker_error", message)
    }
}

pub fn parse_worker_error_response(status: u16, value: &serde_json::Value) -> WorkerError {
    if value.get("ok") != Some(&serde_json::Value::Bool(false)) {
        return WorkerError::invalid_response(status);
    }

    let parsed = value
        .get("error")
        .cloned()
        .and_then(|error| serde_json::from_value::<WorkerError>(error).ok());
    match parsed {
        Some(error) if error.is_valid_for_http_status(status) => error,
        _ => WorkerError::invalid_response(status),
    }
}

pub fn forbidden_browser_act(kind: &str, params: &serde_json::Value) -> Option<&'static str> {
    let kind = kind.trim().to_ascii_lowercase();
    if matches!(
        kind.as_str(),
        "evaluate" | "eval" | "cdp" | "js" | "script" | "run_code" | "browser_run_code_unsafe"
    ) {
        return Some("evaluate/cdp is forbidden");
    }
    if let Some(obj) = params.as_object() {
        for key in ["script", "expression", "evaluate", "cdp", "function", "js"] {
            if obj.contains_key(key) {
                return Some("evaluate/cdp is forbidden");
            }
        }
    }
    None
}
