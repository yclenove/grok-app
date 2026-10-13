//! Thin synchronous OS boundary. Calls run on dedicated threads, never a runtime.
//!
//! The five-second caller deadline lives in the driver. Mutation calls report
//! uncertainty once launch or unhide has started. Implementations must not prompt
//! for Accessibility or Screen Recording and must not click system dialogs.

use crate::identity::SignatureInfo;
use gcu_core::driver::Result;
use gcu_protocol::Rect;
use std::time::SystemTime;

/// A process `NSWorkspace` reports as running.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunningApp {
    /// Process id.
    pub pid: u32,
    /// Bundle identifier, when the process is a bundled app.
    pub bundle_id: Option<String>,
    /// Localized display name (`CFBundleDisplayName`).
    pub localized_name: String,
    /// Absolute `.app` bundle path. Empty for unpackaged processes.
    pub bundle_path: String,
    /// Absolute executable path.
    pub executable_path: String,
    /// Hidden with `NSRunningApplication` hide, not merely occluded.
    pub hidden: bool,
}

/// On-disk bundle metadata, hash, and signature facts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BundleFacts {
    /// Bundle identifier from Info.plist.
    pub bundle_id: Option<String>,
    /// Localized display name.
    pub localized_name: String,
    /// `CFBundleName`, which may differ from the localized name.
    pub bundle_name: String,
    /// Absolute bundle path.
    pub bundle_path: String,
    /// Absolute executable path inside the bundle.
    pub executable_path: String,
    /// OS signature check. Subject text alone is not a grant.
    pub signature: SignatureInfo,
    /// Code-directory hash, hex, when the OS reported one.
    pub cdhash: Option<String>,
    /// SHA-256 of the executable bytes.
    pub sha256: String,
    /// PlugIns, Frameworks, or the executable directory was writable.
    /// This is a signal, not a full plugin audit.
    pub writable_load_path: bool,
}

/// One Spotlight recent-application hit. Not an authorization grant.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpotlightHit {
    /// Absolute bundle path.
    pub path: String,
    /// Spotlight display name.
    pub name: String,
    /// Localized name, when the index has one.
    pub localized_name: String,
    /// Bundle identifier, so resolution by id does not stat every hit.
    pub bundle_id: Option<String>,
    /// Last used instant. `None` is not treated as recent.
    pub last_used: Option<SystemTime>,
}

/// One `CGWindowList` row. `on_current_desktop` is `None` when Spaces are unknown.
#[derive(Clone, Debug, PartialEq)]
pub struct CgWindow {
    /// `CGWindowID`.
    pub window_id: u32,
    /// Owning process id.
    pub pid: u32,
    /// Window layer. Normal app windows are layer 0.
    pub layer: i32,
    /// Window title. Not an authorization identity.
    pub title: String,
    /// Owning process name from the window server.
    pub owner_name: String,
    /// Screen rectangle in pixels. Origin is top-left of the main display.
    pub frame: Rect,
    /// Window alpha in `0.0..=1.0`.
    pub alpha: f64,
    /// Display scale. The real adapter reports `1.0` until P1-04.
    pub scale: f64,
    /// Current Space, when the OS can say. Public APIs often cannot.
    pub on_current_desktop: Option<bool>,
}

/// One Accessibility window, later aligned to a `CGWindowID`.
#[derive(Clone, Debug, PartialEq)]
pub struct AxWindow {
    /// Index in this process's `AXWindows` array. Used by [`MacApi::ax_raise`].
    pub index: u32,
    /// Accessibility title.
    pub title: String,
    /// Accessibility frame in screen pixels.
    pub frame: Rect,
    /// `AXMinimized`.
    pub minimized: bool,
    /// `AXMain`.
    pub main: bool,
}

/// A launch the adapter accepted. `activates` is always false for this driver.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LaunchRecord {
    /// Bundle path passed to `NSWorkspace`.
    pub path: String,
    /// Whether the open configuration requested activation.
    pub activates: bool,
}

/// Injectable OS calls. The driver never prompts and never activates the foreground.
pub trait MacApi: Send + Sync + 'static {
    /// Effective user id. Zero is root and is refused at startup.
    fn engine_euid(&self) -> Result<u32>;
    /// Accessibility trust. Must not pass the prompt option.
    fn accessibility_trusted(&self) -> Result<bool>;
    /// Screen Recording preflight. Must not request access.
    fn screen_recording_ok(&self) -> Result<bool>;
    /// Running GUI applications.
    fn running_apps(&self) -> Result<Vec<RunningApp>>;
    /// Frontmost application pid, if any.
    fn frontmost_pid(&self) -> Result<Option<u32>>;
    /// Bundle, hash, and signature facts for an absolute `.app` path.
    fn bundle_facts(&self, path: &str) -> Result<BundleFacts>;
    /// Spotlight applications used recently. May block; the driver calls it off
    /// the discovery worker so the first `list_apps` does not wait.
    fn spotlight_recent(&self) -> Result<Vec<SpotlightHit>>;
    /// Window list from `CGWindowList`.
    fn windows(&self) -> Result<Vec<CgWindow>>;
    /// Accessibility windows for one pid.
    /// `PermissionMissing` when the AX API is disabled. Other failures may be empty.
    fn ax_windows(&self, pid: u32) -> Result<Vec<AxWindow>>;
    /// `AXRaise` one window by the index from [`Self::ax_windows`].
    /// Does not activate the application.
    fn ax_raise(&self, pid: u32, index: u32) -> Result<()>;
    /// Unhide a running app without activating it.
    fn unhide(&self, pid: u32) -> Result<()>;
    /// Open a bundle with activation disabled. Must not change the frontmost app
    /// as part of the driver's own request. Returns after the open completes or
    /// times out; a timeout may already have started the launch.
    fn launch(&self, path: &str) -> Result<()>;
}
