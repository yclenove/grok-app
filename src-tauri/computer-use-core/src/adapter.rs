//! Platform adapter contract. Capabilities describe implemented operations.

use super::protocol::{ActionKind, ActionTarget, Observation, PROTOCOL_VERSION};
mod scope_identity;
pub use scope_identity::NativeScopeIdentity;
mod webview_selection;
pub use webview_selection::{WebViewScopeReceipt, WebViewSelectedIdentity};
mod upload;
pub use upload::{
    HostSelectedUploadFile, StagedUpload, UploadError, UploadRequest, MAX_UPLOAD_BYTES,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionScope {
    Directed,
    /// Forbidden as a fallback when a directed target is dead.
    Desktop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SurfaceKind {
    Desktop,
    ManagedBrowser,
    ExistingTab,
    WebView,
}

impl SurfaceKind {
    pub fn from_wire(value: &str) -> Result<Self, String> {
        match value.trim() {
            "desktop" => Ok(Self::Desktop),
            "managed-browser" | "browser" => Ok(Self::ManagedBrowser),
            "existing-tabs" | "existing-tab" => Ok(Self::ExistingTab),
            "app-webview" | "webview" => Ok(Self::WebView),
            other => Err(format!("unsupported_surface:{other}")),
        }
    }

    pub fn as_wire(self) -> &'static str {
        match self {
            Self::Desktop => "desktop",
            Self::ManagedBrowser => "managed-browser",
            Self::ExistingTab => "existing-tabs",
            Self::WebView => "app-webview",
        }
    }
}

/// Per-action input modes. A single backend-wide boolean is not a capability map.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActionSupport {
    pub semantic: bool,
    pub coordinate: bool,
}

impl ActionSupport {
    pub const NONE: Self = Self {
        semantic: false,
        coordinate: false,
    };
    pub const BOTH: Self = Self {
        semantic: true,
        coordinate: true,
    };
    pub const SEMANTIC: Self = Self {
        semantic: true,
        coordinate: false,
    };
    pub const COORDINATE: Self = Self {
        semantic: false,
        coordinate: true,
    };

    pub fn allows(self, target: &ActionTarget) -> bool {
        match target {
            ActionTarget::Element { .. } => self.semantic,
            ActionTarget::Coord { .. } => self.coordinate,
        }
    }
}

#[derive(Debug, Clone)]
pub struct TargetInfo {
    pub target_id: String,
    pub title: String,
    pub app_name: String,
    pub kind: String,
    pub pid: Option<u32>,
    pub backend: String,
    pub execution_mode: String,
    pub replay_policy: String,
    pub lifecycle_stamp: u64,
    pub display_id: String,
    pub coordinate_space: String,
    pub scope_label: String,
}

/// Explicit Host picker metadata paired with the same native instance. No
/// screenshot, AX/DOM tree or input capability is acquired by listing this.
pub struct ScopeCandidate {
    pub info: TargetInfo,
    pub identity: NativeScopeIdentity,
}

pub struct ScopeCandidatePage {
    pub candidates: Vec<ScopeCandidate>,
    pub truncated: bool,
}

/// Overlay / IME / other-agent chrome is not an operable Computer Use target.
pub fn skip_desktop_target(title: &str, app_name: &str) -> bool {
    let title = title.trim();
    if title.is_empty() {
        return true;
    }
    let t = title.to_ascii_lowercase();
    let app = app_name.trim().to_ascii_lowercase();
    t.contains("geforce overlay")
        || t.contains("nvidia overlay")
        || t.contains("using your computer")
        || t.contains("cursor overlay")
        || t.contains("windows input experience")
        || title.contains("输入体验")
        || t == "statusbarwnd"
        || t == "program manager"
        || t == "default ime"
        || t.contains("msctfime")
        || t == "olemainthreadwndname"
        || app.contains("textinputhost")
        || app.contains("nvidia share")
        || app.contains("grok-app")
        || t == "grok"
        || t == "grok dev"
        || t.contains("user account control")
        || title.contains("用户账户控制")
        || app == "consent"
}

#[derive(Debug, Clone)]
pub struct Capabilities {
    pub backend_id: String,
    pub protocol_version: u32,
    pub observe_screenshot: bool,
    pub observe_ax: bool,
    pub semantic_click: bool,
    pub coordinate_click: bool,
    pub unicode_text: bool,
    pub chinese_ime: bool,
    pub native_wayland: bool,
    pub native_x11: bool,
    pub notes: Vec<String>,
    pub click: ActionSupport,
    pub set_value: ActionSupport,
    pub type_text: ActionSupport,
    pub key: ActionSupport,
    pub scroll: ActionSupport,
    pub drag: ActionSupport,
    pub wait: ActionSupport,
}

impl Capabilities {
    pub fn for_surface(kind: SurfaceKind, backend_id: &str) -> Self {
        match kind {
            SurfaceKind::Desktop => Self {
                backend_id: backend_id.into(),
                protocol_version: PROTOCOL_VERSION,
                observe_screenshot: true,
                observe_ax: false,
                semantic_click: true,
                coordinate_click: true,
                unicode_text: true,
                chinese_ime: true,
                native_wayland: false,
                native_x11: false,
                notes: vec![format!("desktop backend {backend_id}")],
                click: ActionSupport::BOTH,
                set_value: ActionSupport::SEMANTIC,
                type_text: ActionSupport::BOTH,
                key: ActionSupport::BOTH,
                scroll: ActionSupport::COORDINATE,
                drag: ActionSupport::COORDINATE,
                wait: ActionSupport::SEMANTIC,
            },
            SurfaceKind::ManagedBrowser => Self {
                backend_id: backend_id.into(),
                protocol_version: PROTOCOL_VERSION,
                observe_screenshot: true,
                observe_ax: true,
                semantic_click: true,
                coordinate_click: true,
                unicode_text: true,
                chinese_ime: true,
                native_wayland: false,
                native_x11: false,
                notes: vec!["managed browser profile owned by Host".into()],
                click: ActionSupport::BOTH,
                set_value: ActionSupport::SEMANTIC,
                type_text: ActionSupport::BOTH,
                key: ActionSupport::BOTH,
                scroll: ActionSupport::BOTH,
                drag: ActionSupport::COORDINATE,
                wait: ActionSupport::SEMANTIC,
            },
            SurfaceKind::ExistingTab => Self {
                backend_id: backend_id.into(),
                protocol_version: PROTOCOL_VERSION,
                observe_screenshot: true,
                observe_ax: true,
                semantic_click: true,
                coordinate_click: true,
                unicode_text: true,
                chinese_ime: true,
                native_wayland: false,
                native_x11: false,
                notes: vec!["existing tab is borrowed; return on stop".into()],
                click: ActionSupport::BOTH,
                set_value: ActionSupport::SEMANTIC,
                type_text: ActionSupport::BOTH,
                key: ActionSupport::BOTH,
                scroll: ActionSupport::BOTH,
                drag: ActionSupport::NONE,
                wait: ActionSupport::SEMANTIC,
            },
            SurfaceKind::WebView => Self {
                backend_id: backend_id.into(),
                protocol_version: PROTOCOL_VERSION,
                observe_screenshot: false,
                observe_ax: false,
                semantic_click: false,
                coordinate_click: false,
                unicode_text: false,
                chinese_ime: false,
                native_wayland: false,
                native_x11: false,
                notes: vec!["typed WebView subset not bound; no eval".into()],
                click: ActionSupport::NONE,
                set_value: ActionSupport::NONE,
                type_text: ActionSupport::NONE,
                key: ActionSupport::NONE,
                scroll: ActionSupport::NONE,
                drag: ActionSupport::NONE,
                wait: ActionSupport::NONE,
            },
        }
    }

    pub fn support_for(&self, action: ActionKind) -> ActionSupport {
        match action {
            ActionKind::Click => self.click,
            ActionKind::SetValue => self.set_value,
            ActionKind::TypeText => self.type_text,
            ActionKind::Key => self.key,
            ActionKind::Scroll => self.scroll,
            ActionKind::Drag => self.drag,
            ActionKind::Wait => self.wait,
        }
    }

    pub fn allows(&self, action: ActionKind, target: &ActionTarget) -> bool {
        self.support_for(action).allows(target)
    }
}

#[derive(Debug, Clone)]
pub struct DispatchRequest {
    pub managed_request: Option<crate::browser::ManagedRequest>,
    /// Adapters check this before each native side effect and during waits.
    pub cancellation: crate::execution::ActionCancellation,
    pub run_id: String,
    pub action_id: String,
    pub generation: u64,
    pub target_id: String,
    pub target_generation: u64,
    pub snapshot_id: String,
    pub geometry_revision: u64,
    pub action: ActionKind,
    pub target: ActionTarget,
    pub parameters: serde_json::Value,
    pub scope: ActionScope,
}

#[derive(Debug, Clone)]
pub struct AdapterActResult {
    pub applied: bool,
    /// Optional terminal classification supplied by an adapter after its
    /// transport completed. This is used for explicit pre-side-effect
    /// rejection; an ordinary `Err` remains unknown because the adapter may
    /// have performed a physical side effect before reporting the error.
    pub outcome: Option<crate::protocol::OutcomeKind>,
    /// True only when the adapter actually checked a declared postcondition.
    pub postcondition_ok: bool,
    /// False means the write cannot be verified; broker must return applied.
    pub verifiable: bool,
    pub detail: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavigationKind {
    Url,
    Reload,
    Back,
    Forward,
}

impl NavigationKind {
    pub fn from_wire(value: &str) -> Result<Self, String> {
        match value.trim().to_ascii_lowercase().as_str() {
            "url" | "navigate" => Ok(Self::Url),
            "reload" => Ok(Self::Reload),
            "back" => Ok(Self::Back),
            "forward" => Ok(Self::Forward),
            other => Err(format!("unsupported navigation kind: {other}")),
        }
    }

    pub fn as_wire(self) -> &'static str {
        match self {
            Self::Url => "url",
            Self::Reload => "reload",
            Self::Back => "back",
            Self::Forward => "forward",
        }
    }
}

#[derive(Debug, Clone)]
pub struct NavigationRequest {
    pub run_id: String,
    pub target_id: String,
    pub target_generation: u64,
    pub action_id: String,
    pub kind: NavigationKind,
    pub url: Option<String>,
    pub cancellation: crate::execution::ActionCancellation,
}

/// A Broker-selected App WebView download destination.  The model supplies a
/// filename, never this path; callers must validate the completed file inside
/// the run-owned staging root before publishing it.
#[derive(Debug, Clone)]
pub struct DownloadRequest {
    pub run_id: String,
    pub target_id: String,
    pub target_generation: u64,
    pub action_id: String,
    pub snapshot_id: String,
    pub element_ref: String,
    pub destination: std::path::PathBuf,
    pub cancellation: crate::execution::ActionCancellation,
}

#[derive(Debug, Clone)]
pub struct DownloadResult {
    pub path: std::path::PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadDisposition {
    NotDispatched,
    DispatchedRejected,
    DispatchedUnknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadError {
    pub disposition: DownloadDisposition,
    pub detail: String,
}

impl DownloadError {
    pub fn not_dispatched(detail: impl Into<String>) -> Self {
        Self {
            disposition: DownloadDisposition::NotDispatched,
            detail: detail.into(),
        }
    }

    pub fn dispatched_unknown(detail: impl Into<String>) -> Self {
        Self {
            disposition: DownloadDisposition::DispatchedUnknown,
            detail: detail.into(),
        }
    }

    pub fn dispatched_rejected(detail: impl Into<String>) -> Self {
        Self {
            disposition: DownloadDisposition::DispatchedRejected,
            detail: detail.into(),
        }
    }
}

/// A completed file remains private until Broker publishes the result while
/// the original run, task grant and native document are still authoritative.
pub trait StagedDownload: Send {
    fn result(&self) -> &DownloadResult;
    fn commit(
        self: Box<Self>,
        commit: &mut dyn FnMut() -> Result<(), crate::error::BrokerError>,
    ) -> Result<(), crate::error::BrokerError>;
}

#[derive(Debug, Clone)]
pub struct NavigationResult {
    pub target: TargetInfo,
    pub identity: NativeScopeIdentity,
    pub kind: NavigationKind,
    pub url: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavigationDisposition {
    NotDispatched,
    DispatchedUnknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NavigationError {
    pub disposition: NavigationDisposition,
    pub detail: String,
}

impl NavigationError {
    pub fn not_dispatched(detail: impl Into<String>) -> Self {
        Self {
            disposition: NavigationDisposition::NotDispatched,
            detail: detail.into(),
        }
    }

    pub fn dispatched_unknown(detail: impl Into<String>) -> Self {
        Self {
            disposition: NavigationDisposition::DispatchedUnknown,
            detail: detail.into(),
        }
    }
}

/// A successor document is private until Broker commits its run and consent
/// identity under the same adapter/native authority guards. Dropping a staged
/// transition must release it without restoring the retired predecessor.
pub trait StagedNavigation: Send {
    fn result(&self) -> &NavigationResult;
    fn commit(
        self: Box<Self>,
        commit: &mut dyn FnMut() -> Result<(), crate::error::BrokerError>,
    ) -> Result<(), crate::error::BrokerError>;
}

pub trait ComputerUseAdapter: Send + Sync {
    fn backend_id(&self) -> &'static str;
    fn capabilities(&self) -> Capabilities;
    fn list_targets(&self) -> Result<Vec<TargetInfo>, String>;
    /// Dedicated, bounded Host picker producer. The callback must guard native
    /// metadata reads and stop enumeration when the original request retires.
    /// Never fall back to ambient/legacy discovery when this is unsupported.
    fn scope_candidates(
        &self,
        _limit: usize,
        _is_current: &(dyn Fn() -> bool + Send + Sync),
    ) -> Result<ScopeCandidatePage, String> {
        Err("bounded task scope picker is unavailable for this backend".into())
    }
    /// An owned producer may queue work on the App main thread. Its original
    /// request fence must remain usable until that actual callback returns.
    fn scope_candidates_for_task(
        &self,
        limit: usize,
        is_current: std::sync::Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> Result<ScopeCandidatePage, String> {
        self.scope_candidates(limit, is_current.as_ref())
    }
    /// Unbound receipts cannot use claim_target/observe or legacy authorization.
    fn bind_scope_candidate(
        &self,
        _identity: &NativeScopeIdentity,
        _session: &str,
        _run: &str,
    ) -> Result<ScopeCandidate, String> {
        Err("task scope candidate binding unavailable".into())
    }
    /// Run-scoped target listing for adapters whose targets are Host grants.
    /// Desktop adapters keep the process-wide discovery behavior by default.
    fn list_targets_for_run(&self, _run_id: &str) -> Result<Vec<TargetInfo>, String> {
        self.list_targets()
    }
    fn target_alive(&self, target_id: &str) -> bool;
    /// Host-selected instance metadata only: no screenshot, DOM or AX read.
    /// Missing native proof must not be replaced with target_alive or a title.
    fn scope_identity(&self, _target_id: &str) -> Result<NativeScopeIdentity, String> {
        Err("exact task scope identity is unavailable for this backend".into())
    }
    /// Retained authority only. Hold the original binding/document guards
    /// across the callback, exactly once after validation. No native query,
    /// script, IO, retry or callback after retirement is allowed here.
    /// Unsupported backends must reject without invoking the callback.
    fn with_scope_authority(
        &self,
        _identities: &[NativeScopeIdentity],
        _commit: &mut dyn FnMut() -> Result<(), crate::error::BrokerError>,
    ) -> Result<(), crate::error::BrokerError> {
        Err(crate::error::BrokerError::TargetUnauthorized)
    }
    /// Metadata for exactly one Host-selected or approved instance. Never
    /// implement this by enumerating other targets or reading DOM/AX/content.
    /// Missing support must fail rather than falling back to discovery.
    fn scope_target_info(&self, _run_id: &str, _target_id: &str) -> Result<TargetInfo, String> {
        Err("exact task target metadata is unavailable for this backend".into())
    }
    fn observe(&self, target_id: &str) -> Result<Observation, String>;
    /// Run-scoped observation avoids a process-wide "current run" in browser
    /// and other Host-owned adapters.
    fn observe_for_run(&self, _run_id: &str, target_id: &str) -> Result<Observation, String> {
        self.observe(target_id)
    }
    fn capture_for_run(
        &self,
        run_id: &str,
        target_id: &str,
        options: CaptureOptions,
    ) -> Result<Observation, String> {
        options.cancellation.check()?;
        let mut observation = self.observe_for_run(run_id, target_id)?;
        options.cancellation.check()?;
        if !options.screenshot {
            observation.image.png_base64 = None;
        }
        Ok(observation)
    }
    fn act(&self, req: &DispatchRequest) -> Result<AdapterActResult, String>;
    fn stage_navigation(
        &self,
        _req: &NavigationRequest,
    ) -> Result<Box<dyn StagedNavigation>, NavigationError> {
        Err(NavigationError::not_dispatched(
            "navigation is unavailable for this backend",
        ))
    }
    fn stage_download(
        &self,
        _req: &DownloadRequest,
    ) -> Result<Box<dyn StagedDownload>, DownloadError> {
        Err(DownloadError::not_dispatched(
            "download is unavailable for this backend",
        ))
    }
    /// Rust-only Host-selected content. This is not a model filesystem API;
    /// Broker must admit the original task and explicit file consent first.
    fn stage_upload(&self, _req: &UploadRequest) -> Result<Box<dyn StagedUpload>, UploadError> {
        Err(UploadError::NotDispatched(
            "upload is unavailable for this backend".into(),
        ))
    }
    /// Trusted Broker generation, never inferred from an agent action. Native
    /// run-owned adapters can bind an observation to the exact generation that
    /// Broker will publish. Existing adapters retain their capture behavior.
    fn capture_for_run_at_generation(
        &self,
        run_id: &str,
        target_id: &str,
        _generation: u64,
        options: CaptureOptions,
    ) -> Result<Observation, String> {
        self.capture_for_run(run_id, target_id, options)
    }
    /// Some desktop observations use per-snapshot opaque refs rather than
    /// stable native IDs. Their read-only Wait must retain and validate the
    /// original native object; recapture + string equality is not identity.
    /// Such adapters implement bounded, cancellation-aware Wait in `act`.
    fn wait_uses_retained_reference(&self) -> bool {
        false
    }
    fn abort(&self, run_id: &str, generation: u64) -> Result<(), String>;
    fn is_idle(&self, run_id: &str) -> bool;
    fn start_periodic_preview(&self, target_id: &str);
    fn stop_periodic_preview(&self);
    fn periodic_preview_active(&self) -> bool;
    /// Lock screen / secure desktop: false means pause, do not dispatch.
    fn input_available(&self) -> bool {
        true
    }
    /// Authorized target is the foreground window (or a child of it).
    fn foreground_input_available(&self, _target_id: &str) -> bool {
        true
    }
    /// Display topology + DPI epoch. Stale geometry must not fall back to the desktop.
    fn current_geometry_revision(&self) -> u64 {
        1
    }
    /// Per-target geometry (window move/size/DPI). Defaults to the display epoch.
    fn current_geometry_revision_for(&self, _target_id: &str) -> u64 {
        self.current_geometry_revision()
    }
    /// User is typing or otherwise taking over input.
    fn user_input_active(&self) -> bool {
        false
    }
    /// One backend owns a target until stop/revoke. Default: no exclusive registry.
    fn claim_target(&self, _target_id: &str) -> Result<(), String> {
        Ok(())
    }
    fn claim_target_for_run(&self, _run_id: &str, target_id: &str) -> Result<(), String> {
        self.claim_target(target_id)
    }
    fn release_target(&self, _target_id: &str) {}
    fn release_target_for_run(&self, _run_id: &str, target_id: &str) {
        self.release_target(target_id);
    }
    fn worker_attached(&self) -> bool {
        false
    }
    fn worker_generation(&self) -> Option<String> {
        None
    }
}

#[derive(Clone, Debug)]
pub struct CaptureOptions {
    pub managed_request: Option<crate::browser::ManagedRequest>,
    pub for_model: bool,
    pub screenshot: bool,
    pub cancellation: crate::execution::ActionCancellation,
}

impl CaptureOptions {
    pub fn model(screenshot: bool) -> Self {
        Self {
            for_model: true,
            managed_request: None,
            screenshot,
            cancellation: Default::default(),
        }
    }
}

impl DispatchRequest {
    /// Adapters re-check Directed scope, cancellation, liveness, and geometry
    /// before any native side effect. Never upgrade a miss to desktop scope.
    pub fn admit(&self, adapter: &dyn ComputerUseAdapter) -> Result<(), String> {
        if self.scope != ActionScope::Directed {
            return Err("desktop fallback is forbidden".into());
        }
        self.cancellation.check()?;
        if !adapter.input_available() {
            return Err("session locked; secure desktop owns input".into());
        }
        if !adapter.target_alive(&self.target_id) {
            return Err("dead target".into());
        }
        let current = adapter.current_geometry_revision_for(&self.target_id);
        if current != 0 && self.geometry_revision != 0 && current != self.geometry_revision {
            return Err("stale geometryRevision".into());
        }
        Ok(())
    }
}
