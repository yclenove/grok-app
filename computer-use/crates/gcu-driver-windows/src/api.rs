//! Thin synchronous OS boundary. Calls run on dedicated threads, never a runtime.
use crate::{gate::ProcessSecurity, identity::SignatureInfo};
use gcu_core::driver::Result;
use gcu_protocol::Rect;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Raw HWND and provenance read in one enumeration pass.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeWindow {
    /// Numeric HWND; callers receive an opaque generation-qualified string.
    pub hwnd: u64,
    /// Actual top-level process, before UWP unwrapping.
    pub pid: u32,
    /// Process creation FILETIME; prevents PID reuse.
    pub process_created: u64,
    /// Window creation-generation marker; replaced when an HWND is recreated.
    pub created: u64,
    /// Native class name.
    pub class: String,
    /// Title; never used as authorization identity.
    pub title: String,
    /// Visible according to IsWindowVisible.
    pub visible: bool,
    /// WS_EX_TOOLWINDOW.
    pub tool_window: bool,
    /// WS_EX_APPWINDOW overrides owner filtering.
    pub app_window: bool,
    /// Owner HWND, if any. Owned dialogs are retained with their owner relation.
    pub owner: Option<u64>,
    /// Minimized state.
    pub minimized: bool,
    /// Current foreground HWND.
    pub foreground: bool,
    /// Window physical screen rectangle.
    pub frame: Rect,
    /// DPI divided by 96.
    pub scale: f64,
    /// Child process IDs used to unwrap ApplicationFrameHost.
    pub child_pids: Vec<u32>,
}

/// A verified file read, obtained from a single open file identity.
#[derive(Clone, Debug)]
pub struct ImageInfo {
    /// Final path after resolving junctions and symlinks.
    pub canonical_path: String,
    /// SHA-256 of that executable's bytes, including signed images.
    pub sha256: String,
    /// Authenticode chain result and signer provenance.
    pub signature: SignatureInfo,
    /// A writable DLL/plugin search directory was found (not a complete audit).
    pub writable_load_path: bool,
}

/// Trusted discovery source, never a launch command supplied by a client.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Shortcut {
    /// Local .lnk path used for later re-resolution.
    pub link: String,
    /// Display name.
    pub name: String,
    /// Executable path resolved by IShellLink (never cmd/PowerShell arguments).
    pub target: String,
    /// Packaged-app AUMID, if exposed by the shell property store.
    pub aumid: Option<String>,
}

/// Injectable OS calls. Implementations must not panic across native callbacks.
/// The driver applies a five-second caller deadline; mutation calls must complete
/// preflight before beginning delivery and report uncertainty after that point.
/// Index change waits must return within `timeout`, so dropping a driver stops it.
pub trait WinApi: Send + Sync + 'static {
    /// Enter the dedicated thread's OS context (MTA for Windows). The returned
    /// guard stays on this thread until worker shutdown; no interface crosses it.
    fn enter_thread(&self) -> Result<Box<dyn std::any::Any>> {
        Ok(Box::new(()))
    }
    /// NUL-terminated window-generation property name, when this API stamps one.
    ///
    /// Empty means there is no scalar marker to re-read. The returned units must not
    /// require enumeration, a process handle, or COM.
    fn generation_property(&self) -> &[u16] {
        &[]
    }
    /// Current process security, interactive session and input-desktop state.
    fn engine_security(&self) -> Result<ProcessSecurity>;
    /// Security state of a process, including creation time.
    fn process_security(&self, pid: u32) -> Result<ProcessSecurity>;
    /// Enumerate top-level HWNDs and their child process IDs.
    fn enum_windows(&self) -> Result<Vec<NativeWindow>>;
    /// Query the process image path, not an untrusted window label.
    fn process_path(&self, pid: u32) -> Result<String>;
    /// Running process AppUserModelId, if packaged. Discovery hint only; never
    /// an authorization identity. No installed-package or Start Menu scan.
    fn process_aumid(&self, _pid: u32) -> Result<Option<String>> {
        Ok(None)
    }
    /// Resolve the final file path, hash and verify its signature coherently.
    fn image(&self, path: &str) -> Result<ImageInfo>;
    /// Query IVirtualDesktopManager; None means detection unavailable.
    fn on_current_desktop(&self, hwnd: u64) -> Result<Option<bool>>;
    /// Parse a local .lnk without executing its arguments.
    fn read_link(&self, path: &str) -> Result<Shortcut>;
    /// Recursively enumerate the user's and common Start Menu .lnk files.
    fn start_menu_links(&self) -> Result<Vec<String>>;
    /// Read the app cache, None if not present. Corrupt data is handled by core logic.
    fn read_cache(&self) -> Result<Option<Vec<u8>>>;
    /// Atomically replace the local cache after a completed scan.
    fn write_cache(&self, bytes: &[u8]) -> Result<()>;
    /// File notification wait, bounded even when nothing changes.
    fn links_changed(&self, timeout: Duration) -> Result<bool>;
    /// Launch a verified executable/packaged app; no arbitrary shortcut arguments.
    fn launch(
        &self,
        shortcut: &Shortcut,
        expected: &crate::identity::WindowsIdentity,
    ) -> Result<()>;
    /// Restore an exact HWND without foreground activation.
    fn restore(&self, expected: &NativeWindow) -> Result<()>;
}
