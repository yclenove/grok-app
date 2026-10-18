//! Pure L2 admission policy for the private native-host broker.
//!
//! This is not a Host or ACP method and it does not read a tab. No current
//! platform can prove which OS window contains the shared top document, so the
//! policy never admits L2. Callers keep the already declared L0 route; a
//! refusal here is not an L0 grant and not a second delivery.
//!
//! Blocking: none. Timeout: none. Cancellation: none.

use super::wire;
use super::ExtensionIdentity;

/// Desktop whose native-messaging evidence was reviewed. Not a runtime probe.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProofPlatform {
    /// X11, including XWayland windows that expose an XID.
    LinuxX11,
    /// Native Wayland, with no XID for the target window.
    LinuxWayland,
    /// Win32.
    Windows,
    /// macOS.
    Macos,
}

/// Browser the launch chain claims to be. Other Chromium products stay on L0.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProofBrowser {
    /// Google Chrome or Chromium's official executable names.
    Chrome,
    /// Microsoft Edge.
    Edge,
}

/// Why this connection must not use L2.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum L2Refusal {
    /// No live share for this document.
    Unshared,
    /// Key, origin, or embedded identity did not match.
    Identity,
    /// Parent is not a Chrome or Edge native-messaging launch.
    FakeRelay,
    /// Host image or build is not the expected gcu.
    BuildMismatch,
    /// Claimed window is not the shared document's window.
    WrongWindow,
    /// Claimed tab is not the shared tab.
    WrongTab,
    /// Claimed document or frame is not the shared top document.
    WrongDocument,
    /// More than one engine could own the connection.
    AmbiguousEngine,
    /// No single engine endpoint is listening.
    NoEngine,
    /// More than one authorized local session matches.
    AmbiguousSession,
    /// No authorized session matches this engine and epoch.
    UnauthorizedSession,
    /// Request or claim belongs to an older connection generation.
    StaleEpoch,
    /// Response has no live request, or arrived after that request was retired.
    LateResponse,
    /// The pipe is gone. Pending mutations stay unknown.
    Disconnected,
    /// The user withdrew the share. Pending mutations stay unknown.
    Revoked,
    /// The platform cannot bind the top document to one OS window.
    WindowUnproven,
}

/// L2 decision. Admission is not representable: every platform verdict is unproven.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum L2Decision {
    /// Stay off L2. This does not authorize an L0 action.
    Unavailable {
        /// Specific refusal.
        reason: L2Refusal,
    },
}

/// Why top-document mapping cannot be used as a security proof.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnprovenReason {
    /// Windows `--parent-window` is the foreground HWND at launch, not the tab.
    WindowsForegroundHwnd,
    /// Linux native messaging passes no XID. One browser process owns many windows.
    X11WindowUnbound,
    /// No authenticated Wayland toplevel handle is supplied.
    WaylandWindowUnbound,
    /// macOS native messaging passes no CGWindowID. Extension ids are not OS ids.
    MacosWindowUnbound,
}

/// Result of the platform mapping review.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MappingVerdict {
    /// Mapping is not reproducible. L2 stays closed.
    Unproven(UnprovenReason),
}

impl MappingVerdict {
    /// Whether this verdict could admit L2. Always false for the reviewed set.
    pub const fn admits_l2(self) -> bool {
        match self {
            Self::Unproven(_) => false,
        }
    }
}

/// Real Chrome/Edge launch and window-mapping evidence. Every entry is `not_run`.
///
/// Unit fixtures and Xvfb do not change these rows.
pub const REAL_MACHINE_EVIDENCE: &[(&str, &str)] = &[
    ("linux-x11-chrome-launch-chain", "not_run"),
    ("linux-x11-edge-launch-chain", "not_run"),
    ("linux-wayland-chrome-launch-chain", "not_run"),
    ("linux-wayland-edge-launch-chain", "not_run"),
    ("windows-chrome-launch-chain", "not_run"),
    ("windows-edge-launch-chain", "not_run"),
    ("macos-chrome-launch-chain", "not_run"),
    ("macos-edge-launch-chain", "not_run"),
    ("linux-x11-top-document-window", "not_run"),
    ("linux-wayland-top-document-window", "not_run"),
    ("windows-top-document-window", "not_run"),
    ("macos-top-document-window", "not_run"),
];

/// Build identity compiled into this gcu image.
///
/// Blocking: none. Timeout: none. Cancellation: none.
#[must_use]
pub fn build_id() -> String {
    format!("gcu/{}", env!("CARGO_PKG_VERSION"))
}

/// Reviewed mapping for one platform and browser. Chrome and Edge share the limit.
///
/// Blocking: none. Timeout: none. Cancellation: none.
#[must_use]
pub fn mapping_verdict(platform: ProofPlatform, browser: ProofBrowser) -> MappingVerdict {
    let reason = match (platform, browser) {
        (ProofPlatform::Windows, ProofBrowser::Chrome | ProofBrowser::Edge) => {
            UnprovenReason::WindowsForegroundHwnd
        }
        (ProofPlatform::LinuxX11, ProofBrowser::Chrome | ProofBrowser::Edge) => {
            UnprovenReason::X11WindowUnbound
        }
        (ProofPlatform::LinuxWayland, ProofBrowser::Chrome | ProofBrowser::Edge) => {
            UnprovenReason::WaylandWindowUnbound
        }
        (ProofPlatform::Macos, ProofBrowser::Chrome | ProofBrowser::Edge) => {
            UnprovenReason::MacosWindowUnbound
        }
    };
    MappingVerdict::Unproven(reason)
}

/// OS process facts for one native-host invocation. Tests inject these.
/// `record_current` fills them from the running process.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LaunchSnapshot {
    /// Native-host pid. Zero is not an instance.
    pub host_pid: u32,
    /// Native-host start token. Zero is not an instance. Pid reuse must not match.
    pub host_instance: u64,
    /// Absolute path of the gcu image that was started.
    pub host_exe: String,
    /// Build id of that image.
    pub host_build_id: String,
    /// Parent pid. The parent must be the browser, not this process.
    pub parent_pid: u32,
    /// Parent start token. Zero is not an instance.
    pub parent_instance: u64,
    /// File name of the parent executable, not a window title.
    pub parent_exe_name: String,
    /// Arguments after argv0, as the browser passed them.
    pub argv: Vec<String>,
}

/// gcu image the engine expects the browser to have launched.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExpectedBuild {
    /// Absolute path registered for the host.
    pub exe: String,
    /// [`build_id`] of that image.
    pub build_id: String,
}

/// One local engine endpoint. Discovery alone grants nothing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EngineEndpoint {
    /// Engine id. Not taken from the page.
    pub engine_id: String,
    /// Process instance token. Zero does not identify an engine.
    pub instance: u64,
}

/// Browser-owned top document from the extension. Ids are not OS window ids.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SharedDocument {
    /// Consent id.
    pub share_id: String,
    /// Browser tab id.
    pub tab_id: u32,
    /// Frame id. Only the top frame (zero) can be considered.
    pub frame_id: u32,
    /// Browser document id.
    pub document_id: String,
    /// `chrome.windows` id. This is not an HWND, XID, or Wayland handle.
    pub browser_window_id: u32,
}

/// OS window a caller hopes to bind. Supplying one does not prove the document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OsWindow {
    /// Win32 HWND.
    Win32 {
        /// Window handle value.
        hwnd: u64,
    },
    /// X11 window id.
    X11 {
        /// XID.
        xid: u64,
    },
    /// Wayland toplevel handle.
    Wayland {
        /// Handle token. Not a title.
        handle: String,
    },
    /// macOS window id.
    Macos {
        /// CGWindowID or AppKit window number.
        window_id: u64,
    },
}

/// Attempt to bind a share to a core window. Similarity fields are discarded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MapClaim {
    /// Share the caller wants to bind.
    pub share_id: String,
    /// Claimed tab.
    pub tab_id: u32,
    /// Claimed document.
    pub document_id: String,
    /// Claimed `chrome.windows` id.
    pub browser_window_id: u32,
    /// Connection epoch the claim was issued for.
    pub epoch: u64,
    /// Core window id the caller wants.
    pub core_window_id: String,
    /// Optional OS window. Mismatch refuses; a match is still not a proof.
    pub os_window: Option<OsWindow>,
    /// Title similarity in percent. Never read as authorization.
    pub title_similarity: u8,
    /// Geometry similarity in percent. Never read as authorization.
    pub geometry_similarity: u8,
}

impl MapClaim {
    /// Return the similarity scores so callers can see they were not applied.
    ///
    /// Blocking: none. Timeout: none. Cancellation: none.
    #[must_use]
    pub const fn similarity_discarded(&self) -> (u8, u8) {
        (self.title_similarity, self.geometry_similarity)
    }
}

/// Authorized-or-not local session. Window id does not break a tie.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionCandidate {
    /// Session id.
    pub session_id: String,
    /// Engine that owns the session.
    pub engine_id: String,
    /// Whether App authorization already allows this browser session.
    pub authorized: bool,
    /// Epoch the authorization was checked against.
    pub epoch: u64,
    /// Core window id. Not used to pick among several authorized sessions.
    pub window_id: String,
}

/// Checked launch. The HWND, when present, is only the Windows foreground window.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckedLaunch {
    /// Parsed `--parent-window`, Windows only.
    pub foreground_hwnd: Option<u64>,
}

/// Check origin, browser parent, argv shape, and expected gcu build.
///
/// A matching parent proves the launching process image only. It does not prove
/// a tab or an OS window. Blocking: none. Timeout: none. Cancellation: none.
pub fn check_launch(
    identity: &ExtensionIdentity,
    origin: &str,
    platform: ProofPlatform,
    browser: ProofBrowser,
    launch: &LaunchSnapshot,
    expected: &ExpectedBuild,
) -> Result<CheckedLaunch, L2Refusal> {
    if origin != identity.origin() {
        return Err(L2Refusal::Identity);
    }
    let foreground_hwnd = parse_launch(platform, origin, launch)?;
    if !parent_allowed(platform, browser, &launch.parent_exe_name) {
        return Err(L2Refusal::FakeRelay);
    }
    if launch.host_exe != expected.exe
        || launch.host_build_id != expected.build_id
        || launch.host_build_id != build_id()
        || !absolute_path(platform, &expected.exe)
    {
        return Err(L2Refusal::BuildMismatch);
    }
    Ok(CheckedLaunch { foreground_hwnd })
}

/// Choose the single authorized session for this engine and epoch.
///
/// Several authorized sessions stay ambiguous even when their windows differ.
/// Title and geometry are not inputs. Blocking: none. Timeout: none.
/// Cancellation: none.
pub fn select_session<'a>(
    engine_id: &str,
    epoch: u64,
    sessions: &'a [SessionCandidate],
) -> Result<&'a SessionCandidate, L2Refusal> {
    if engine_id.is_empty() || !wire::identifier(engine_id) || epoch == 0 {
        return Err(L2Refusal::AmbiguousEngine);
    }
    let mut foreign = false;
    let mut matched: Vec<&SessionCandidate> = Vec::new();
    for session in sessions {
        if !session.authorized || session.epoch != epoch {
            continue;
        }
        if !wire::identifier(&session.session_id) || session.window_id.is_empty() {
            return Err(L2Refusal::UnauthorizedSession);
        }
        if session.engine_id == engine_id {
            matched.push(session);
        } else if wire::identifier(&session.engine_id) {
            foreign = true;
        } else {
            return Err(L2Refusal::AmbiguousEngine);
        }
    }
    if foreign {
        return Err(L2Refusal::AmbiguousEngine);
    }
    match matched.as_slice() {
        [one] => Ok(*one),
        [] => Err(L2Refusal::UnauthorizedSession),
        _ => Err(L2Refusal::AmbiguousSession),
    }
}

/// Require exactly one usable engine endpoint.
///
/// Blocking: none. Timeout: none. Cancellation: none.
pub fn select_engine(endpoints: &[EngineEndpoint]) -> Result<&EngineEndpoint, L2Refusal> {
    let [only] = endpoints else {
        return Err(if endpoints.is_empty() {
            L2Refusal::NoEngine
        } else {
            L2Refusal::AmbiguousEngine
        });
    };
    if only.instance == 0 || !wire::identifier(&only.engine_id) {
        return Err(L2Refusal::AmbiguousEngine);
    }
    Ok(only)
}

fn parse_launch(
    platform: ProofPlatform,
    origin: &str,
    launch: &LaunchSnapshot,
) -> Result<Option<u64>, L2Refusal> {
    if launch.host_pid == 0
        || launch.parent_pid == 0
        || launch.host_pid == launch.parent_pid
        || launch.host_instance == 0
        || launch.parent_instance == 0
        || launch.parent_exe_name.is_empty()
        || launch.parent_exe_name.contains('\0')
        || launch.host_exe.contains('\0')
    {
        return Err(L2Refusal::FakeRelay);
    }
    let (origin_arg, flag) = match launch.argv.as_slice() {
        [command, origin_arg] if command == "native-host" => (origin_arg.as_str(), None),
        [origin_arg] => (origin_arg.as_str(), None),
        [command, origin_arg, flag] if command == "native-host" => {
            (origin_arg.as_str(), Some(flag.as_str()))
        }
        [origin_arg, flag] => (origin_arg.as_str(), Some(flag.as_str())),
        _ => return Err(L2Refusal::FakeRelay),
    };
    if origin_arg != origin {
        return Err(L2Refusal::Identity);
    }
    match flag {
        None => Ok(None),
        Some(flag) if platform == ProofPlatform::Windows => {
            parent_window(flag).map(Some).ok_or(L2Refusal::FakeRelay)
        }
        Some(_) => Err(L2Refusal::FakeRelay),
    }
}

fn parent_window(flag: &str) -> Option<u64> {
    let raw = flag.strip_prefix("--parent-window=")?;
    if raw.is_empty() || (raw.len() > 1 && raw.starts_with('0')) {
        return None;
    }
    if !raw.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    raw.parse::<u64>().ok().filter(|value| *value > 0)
}

fn parent_allowed(platform: ProofPlatform, browser: ProofBrowser, name: &str) -> bool {
    let names: &[&str] = match (platform, browser) {
        (ProofPlatform::LinuxX11 | ProofPlatform::LinuxWayland, ProofBrowser::Chrome) => &[
            "chrome",
            "google-chrome",
            "google-chrome-stable",
            "google-chrome-beta",
            "google-chrome-unstable",
            "chromium",
            "chromium-browser",
        ],
        (ProofPlatform::LinuxX11 | ProofPlatform::LinuxWayland, ProofBrowser::Edge) => &[
            "microsoft-edge",
            "microsoft-edge-stable",
            "microsoft-edge-beta",
            "microsoft-edge-dev",
        ],
        (ProofPlatform::Macos, ProofBrowser::Chrome) => &["Google Chrome", "Chromium"],
        (ProofPlatform::Macos, ProofBrowser::Edge) => &["Microsoft Edge"],
        (ProofPlatform::Windows, ProofBrowser::Chrome) => &["chrome.exe"],
        (ProofPlatform::Windows, ProofBrowser::Edge) => &["msedge.exe"],
    };
    names.iter().any(|allowed| match platform {
        ProofPlatform::Windows => allowed.eq_ignore_ascii_case(name),
        _ => *allowed == name,
    })
}

fn absolute_path(platform: ProofPlatform, path: &str) -> bool {
    if path.is_empty() || path.contains('\0') {
        return false;
    }
    match platform {
        ProofPlatform::Windows => {
            let bytes = path.as_bytes();
            path.starts_with("\\\\")
                || (bytes.len() >= 3
                    && bytes[0].is_ascii_alphabetic()
                    && bytes[1] == b':'
                    && (bytes[2] == b'\\' || bytes[2] == b'/'))
        }
        _ => path.starts_with('/'),
    }
}
