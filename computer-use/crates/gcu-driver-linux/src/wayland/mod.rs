//! Wayland environment detection, compositor classification, and portal probing.

pub mod capabilities;
pub mod capture;
pub mod doctor;
pub mod gnome;
pub mod kwin;
pub mod toplevel;
pub mod windows;
pub mod portal;
pub mod restore;

pub use capabilities::{
    build_environment_capabilities, CapabilityItem, CapabilityStatus, EnvironmentCapabilities,
    TargetEnvironment,
};
#[cfg(all(target_os = "linux", feature = "pipewire"))]
pub use capture::RealPipeWireFrameReceiver;
pub use capture::{
    CaptureError, FakeFrameReceiver, PipeWireFrameReceiver, PixelRegion, RawCapturedFrame,
    VideoTransform, WaylandCapturePipeline,
};
pub use doctor::build_doctor_report;
#[cfg(target_os = "linux")]
pub use portal::{AshpdPortalClient, AshpdSessionWrapper};
pub use portal::{
    FakePortalClient, FakePortalSession, PortalCursorMode, PortalError, PortalScreenCastClient,
    PortalScreenCastResult, PortalScreenCastSession, PortalSourceKind, PortalStream,
};
pub use restore::{
    OriginBinding, RestoreError, RestoreRecord, RestoreTokenStore, MAX_TOKEN_LIFETIME_SECS,
};

use async_trait::async_trait;
use std::collections::HashMap;

/// Source of environment variables for detection, abstracting `std::env`.
pub trait EnvSource: Send + Sync {
    /// Retrieve an environment variable by name.
    fn var(&self, key: &str) -> Option<String>;
}

/// System environment variable provider backed by `std::env::var`.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemEnv;

impl EnvSource for SystemEnv {
    fn var(&self, key: &str) -> Option<String> {
        std::env::var(key).ok().filter(|s| !s.is_empty())
    }
}

/// In-memory mock environment variable provider for platform-independent testing.
#[derive(Debug, Clone, Default)]
pub struct MockEnv {
    vars: HashMap<String, String>,
}

impl MockEnv {
    /// Create a new empty mock environment.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set an environment variable in this mock.
    pub fn set(mut self, key: impl Into<String>, val: impl Into<String>) -> Self {
        self.vars.insert(key.into(), val.into());
        self
    }

    /// Set an environment variable in this mock (alias for `set`).
    pub fn with(self, key: impl Into<String>, val: impl Into<String>) -> Self {
        self.set(key, val)
    }
}

impl EnvSource for MockEnv {
    fn var(&self, key: &str) -> Option<String> {
        self.vars.get(key).cloned().filter(|s| !s.is_empty())
    }
}

/// Linux display session category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LinuxSessionType {
    /// Pure X11 display session.
    PureX11,
    /// Wayland session with active XWayland X11 server.
    WaylandWithXWayland,
    /// Pure Wayland session without XWayland.
    WaylandPure,
    /// Unknown or headless display session.
    Unknown,
}

impl LinuxSessionType {
    /// Whether the session is running on Wayland.
    pub fn is_wayland(&self) -> bool {
        matches!(self, Self::WaylandWithXWayland | Self::WaylandPure)
    }

    /// Whether XWayland is available within this session.
    pub fn has_xwayland(&self) -> bool {
        matches!(self, Self::WaylandWithXWayland)
    }

    /// Human-readable label for this session type.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::PureX11 => "x11",
            Self::WaylandWithXWayland => "wayland+xwayland",
            Self::WaylandPure => "wayland",
            Self::Unknown => "unknown",
        }
    }
}

/// Wayland compositor family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Compositor {
    /// GNOME Mutter compositor.
    Gnome,
    /// KDE KWin compositor.
    Kde,
    /// wlroots-based compositors (Sway, Hyprland, Wayfire, River, etc.).
    Wlroots,
    /// Known other desktop environments (XFCE, LXQt, COSMIC, etc.).
    Other,
    /// Unknown or unrecognized compositor.
    Unknown,
}

impl Compositor {
    /// Human-readable name for the compositor family.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Gnome => "GNOME (Mutter)",
            Self::Kde => "KDE (KWin)",
            Self::Wlroots => "wlroots",
            Self::Other => "other",
            Self::Unknown => "unknown",
        }
    }

    /// Short slug for capability strings.
    pub fn slug(&self) -> &'static str {
        match self {
            Self::Gnome => "gnome",
            Self::Kde => "kde",
            Self::Wlroots => "wlroots",
            Self::Other => "other",
            Self::Unknown => "unknown",
        }
    }
}

/// Result of probing the Linux desktop environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinuxEnvironment {
    /// Detected display session type.
    pub session_type: LinuxSessionType,
    /// Detected compositor family.
    pub compositor: Compositor,
    /// Raw desktop name from environment variables if set.
    pub desktop_name: Option<String>,
    /// `WAYLAND_DISPLAY` variable if present.
    pub wayland_display: Option<String>,
    /// `DISPLAY` variable if present.
    pub x11_display: Option<String>,
}

impl LinuxEnvironment {
    /// Whether the environment is running on Wayland.
    pub fn is_wayland(&self) -> bool {
        self.session_type.is_wayland()
    }

    /// Return a mock pure X11 environment suitable for testing.
    pub fn pure_x11_test() -> Self {
        Self {
            session_type: LinuxSessionType::PureX11,
            compositor: Compositor::Other,
            desktop_name: Some("test".into()),
            wayland_display: None,
            x11_display: Some(":0".into()),
        }
    }
}

/// Detect the Linux desktop environment and compositor family from environment variables.
pub fn detect_environment(env: &dyn EnvSource) -> LinuxEnvironment {
    let xdg_session_type = env.var("XDG_SESSION_TYPE").map(|s| s.to_ascii_lowercase());
    let wayland_display = env.var("WAYLAND_DISPLAY");
    let x11_display = env.var("DISPLAY");

    let is_wayland = wayland_display.is_some() || xdg_session_type.as_deref() == Some("wayland");

    let session_type = if is_wayland {
        if x11_display.is_some() {
            LinuxSessionType::WaylandWithXWayland
        } else {
            LinuxSessionType::WaylandPure
        }
    } else if x11_display.is_some() || xdg_session_type.as_deref() == Some("x11") {
        LinuxSessionType::PureX11
    } else {
        LinuxSessionType::Unknown
    };

    let desktop_raw = env
        .var("XDG_CURRENT_DESKTOP")
        .or_else(|| env.var("DESKTOP_SESSION"))
        .or_else(|| {
            if env.var("SWAYSOCK").is_some() {
                Some("sway".into())
            } else if env.var("HYPRLAND_INSTANCE_SIGNATURE").is_some() {
                Some("hyprland".into())
            } else if env.var("KDE_FULL_SESSION").is_some() {
                Some("KDE".into())
            } else {
                None
            }
        });

    let compositor = match desktop_raw.as_deref() {
        Some(raw) => {
            let lower = raw.to_ascii_lowercase();
            let segments: Vec<&str> = lower.split(':').collect();
            let matches_any = |keywords: &[&str]| {
                keywords
                    .iter()
                    .any(|k| segments.iter().any(|s| s.contains(k)))
            };

            if matches_any(&["gnome", "mutter"]) {
                Compositor::Gnome
            } else if matches_any(&["kde", "plasma", "kwin"]) {
                Compositor::Kde
            } else if matches_any(&["sway", "hyprland", "river", "wayfire", "wlroots", "phosh"]) {
                Compositor::Wlroots
            } else if matches_any(&["xfce", "lxqt", "mate", "cinnamon", "cosmic", "pantheon"]) {
                Compositor::Other
            } else {
                Compositor::Unknown
            }
        }
        None => Compositor::Unknown,
    };

    LinuxEnvironment {
        session_type,
        compositor,
        desktop_name: desktop_raw,
        wayland_display,
        x11_display,
    }
}

/// Probed status of `xdg-desktop-portal` and its capability interfaces.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PortalInfo {
    /// Whether `xdg-desktop-portal` is running and reachable on D-Bus.
    pub available: bool,
    /// `org.freedesktop.portal.ScreenCast` interface version if present.
    pub screencast_version: Option<u32>,
    /// `org.freedesktop.portal.RemoteDesktop` interface version if present.
    pub remote_desktop_version: Option<u32>,
    /// `org.freedesktop.portal.GlobalShortcuts` interface version if present.
    pub global_shortcuts_version: Option<u32>,
}

/// Abstract checker for `xdg-desktop-portal` D-Bus interfaces.
#[async_trait]
pub trait PortalChecker: Send + Sync {
    /// Probe the session bus for portal availability and interface versions.
    async fn probe(&self) -> PortalInfo;
}

/// In-memory mock portal checker for tests.
#[derive(Debug, Clone, Default)]
pub struct FakePortalChecker {
    /// Pre-configured portal info to return.
    pub info: PortalInfo,
}

impl FakePortalChecker {
    /// Create a new fake portal checker returning the specified info.
    pub fn new(info: PortalInfo) -> Self {
        Self { info }
    }

    /// Create a fake portal checker representing a missing portal service.
    pub fn unavailable() -> Self {
        Self {
            info: PortalInfo {
                available: false,
                screencast_version: None,
                remote_desktop_version: None,
                global_shortcuts_version: None,
            },
        }
    }
}

#[async_trait]
impl PortalChecker for FakePortalChecker {
    async fn probe(&self) -> PortalInfo {
        self.info.clone()
    }
}

/// Live portal checker querying the D-Bus session bus via `zbus`.
pub struct LivePortalChecker {
    connection: Option<zbus::Connection>,
}

impl LivePortalChecker {
    /// Create a checker that will establish a session bus connection when queried.
    pub fn new() -> Self {
        Self { connection: None }
    }

    /// Create a checker using an existing D-Bus connection.
    pub fn from_connection(connection: zbus::Connection) -> Self {
        Self {
            connection: Some(connection),
        }
    }
}

impl Default for LivePortalChecker {
    fn default() -> Self {
        Self::new()
    }
}

async fn query_portal_interface_version(conn: &zbus::Connection, interface: &str) -> Option<u32> {
    let reply = conn
        .call_method(
            Some("org.freedesktop.portal.Desktop"),
            "/org/freedesktop/portal/desktop",
            Some("org.freedesktop.DBus.Properties"),
            "Get",
            &(interface, "version"),
        )
        .await
        .ok()?;

    let body = reply.body();
    let variant: zbus::zvariant::Value = body.deserialize().ok()?;
    match variant {
        zbus::zvariant::Value::U32(v) => Some(v),
        zbus::zvariant::Value::Value(inner) => match *inner {
            zbus::zvariant::Value::U32(v) => Some(v),
            _ => None,
        },
        _ => None,
    }
}

#[async_trait]
impl PortalChecker for LivePortalChecker {
    async fn probe(&self) -> PortalInfo {
        let conn = match self.connection.as_ref() {
            Some(c) => c.clone(),
            None => match zbus::Connection::session().await {
                Ok(c) => c,
                Err(_) => return PortalInfo::default(),
            },
        };

        let screencast_version =
            query_portal_interface_version(&conn, "org.freedesktop.portal.ScreenCast").await;
        let remote_desktop_version =
            query_portal_interface_version(&conn, "org.freedesktop.portal.RemoteDesktop").await;
        let global_shortcuts_version =
            query_portal_interface_version(&conn, "org.freedesktop.portal.GlobalShortcuts").await;

        let desktop_version =
            query_portal_interface_version(&conn, "org.freedesktop.portal.Desktop").await;

        let available = desktop_version.is_some()
            || screencast_version.is_some()
            || remote_desktop_version.is_some()
            || global_shortcuts_version.is_some();

        PortalInfo {
            available,
            screencast_version,
            remote_desktop_version,
            global_shortcuts_version,
        }
    }
}
