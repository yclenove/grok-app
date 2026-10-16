//! Conservative display-session gating shared with capabilities and doctor.
use crate::CopyKey;

/// Session classification follows P7-01: WAYLAND_DISPLAY wins over DISPLAY.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DisplaySession {
    /// Native AppKit desktop.
    MacOS,
    /// Interactive Windows desktop.
    Windows,
    /// Pure X11 session.
    X11,
    /// Native Wayland session.
    Wayland,
    /// XWayland within a Wayland session; not a pure X11 desktop.
    XWayland,
    /// No verifiable desktop.
    Headless,
}
/// Static eligibility. Native creation and input-health checks may still fail.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Availability {
    /// Whether this session is eligible for a native overlay.
    pub available: bool,
    /// Whether the native capture path excludes the overlay (X11 hides instead).
    pub excluded_from_capture: bool,
    /// Localizable reason for disabling overlay.
    pub reason: Option<CopyKey>,
}
impl Availability {
    /// No IO; useful for doctor/capabilities before opening any windows.
    pub const fn for_session(session: DisplaySession) -> Self {
        match session {
            DisplaySession::MacOS | DisplaySession::Windows => Self {
                available: true,
                excluded_from_capture: true,
                reason: None,
            },
            DisplaySession::X11 => Self {
                available: true,
                excluded_from_capture: false,
                reason: None,
            },
            DisplaySession::Wayland | DisplaySession::XWayland => Self {
                available: false,
                excluded_from_capture: false,
                reason: Some(CopyKey::WaylandUnavailable),
            },
            DisplaySession::Headless => Self {
                available: false,
                excluded_from_capture: false,
                reason: Some(CopyKey::Unavailable),
            },
        }
    }
}
/// Detect session eligibility without connecting to a display server.
pub fn detect() -> DisplaySession {
    if cfg!(target_os = "macos") {
        return DisplaySession::MacOS;
    }
    if cfg!(windows) {
        return DisplaySession::Windows;
    }
    let present = |key| std::env::var(key).is_ok_and(|value| !value.is_empty());
    let wayland = present("WAYLAND_DISPLAY")
        || std::env::var("XDG_SESSION_TYPE").is_ok_and(|v| v.eq_ignore_ascii_case("wayland"));
    match (wayland, present("DISPLAY")) {
        (true, true) => DisplaySession::XWayland,
        (true, false) => DisplaySession::Wayland,
        (false, true) => DisplaySession::X11,
        _ => DisplaySession::Headless,
    }
}
