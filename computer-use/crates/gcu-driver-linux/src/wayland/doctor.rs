//! Diagnostic doctor report generator derived directly from the capability table.

use super::{
    capabilities::{CapabilityStatus, EnvironmentCapabilities},
    Compositor, LinuxEnvironment, LinuxSessionType, PortalInfo,
};
use gcu_core::driver::DoctorReport;

/// Build a comprehensive doctor report from the environment and capability matrix.
pub fn build_doctor_report(
    env: &LinuxEnvironment,
    caps: &EnvironmentCapabilities,
    portal: &PortalInfo,
    atspi_ready: bool,
    hotkey_error: Option<&str>,
) -> DoctorReport {
    let mut issues = Vec::new();

    // 1. AT-SPI connection status
    if !atspi_ready {
        issues.push("AT-SPI bus not connected".into());
        issues.push(
            "Note: Ensure at-spi2-core is running. Electron/Chromium apps require \
             '--force-renderer-accessibility' flag for full accessibility tree exposure."
                .into(),
        );
    }

    // 2. Desktop session and compositor
    issues.push(format!("session_type: {}", env.session_type.as_str()));
    issues.push(format!("compositor: {}", env.compositor.as_str()));

    if env.compositor == Compositor::Unknown {
        issues.push(format!(
            "compositor: unknown ({}) - recommended: GNOME, KDE, or wlroots-based compositors. \
             Window enumeration and geometry mapping may be inaccurate.",
            env.desktop_name.as_deref().unwrap_or("unknown")
        ));
    }

    // 3. Portal status
    if env.session_type.is_wayland() || portal.available {
        if !portal.available {
            issues.push(
                "portal: unavailable (xdg-desktop-portal is not available on D-Bus session bus: native Wayland screenshot \
                 and input are unavailable). Next step: install xdg-desktop-portal and a backend \
                 suitable for your desktop (e.g. xdg-desktop-portal-gnome, xdg-desktop-portal-kde, \
                 or xdg-desktop-portal-wlr)."
                    .into(),
            );
        } else {
            if portal.screencast_version.is_none() {
                issues.push(
                    "portal: screencast_unavailable (org.freedesktop.portal.ScreenCast is missing: native \
                     Wayland capture will be unavailable). Next step: install or configure the compositor \
                     screencast backend."
                        .into(),
                );
            }
            if portal.remote_desktop_version.is_none() {
                issues.push(
                    "portal: remote_desktop_unavailable (org.freedesktop.portal.RemoteDesktop is missing: \
                     native Wayland input will be unavailable). Next step: install or configure the \
                     compositor remote-desktop backend."
                        .into(),
                );
            }
            if portal.global_shortcuts_version.is_none() {
                issues.push(
                    "portal: global_shortcuts_unavailable (org.freedesktop.portal.GlobalShortcuts is missing: \
                     Global Stop hotkey cannot be registered across native Wayland surfaces). Next step: \
                     ensure your desktop environment supports GlobalShortcuts (GNOME 44+, KDE 6+)."
                        .into(),
                );
            }
        }
    }

    // 4. Capability-specific notes and issues
    if caps.screenshot.status != CapabilityStatus::Available {
        issues.push(caps.screenshot.reason.clone());
        if let Some(ref step) = caps.screenshot.next_step {
            issues.push(format!("Next step for screenshot: {step}"));
        }
    }

    let guard_status = if caps.user_activity_guard.status == CapabilityStatus::Available {
        "available"
    } else {
        "unavailable"
    };
    issues.push(format!("user_activity_guard: {guard_status}"));
    if caps.user_activity_guard.status != CapabilityStatus::Available {
        if let Some(ref step) = caps.user_activity_guard.next_step {
            issues.push(format!("Next step for user_activity_guard: {step}"));
        }
    }

    if caps.global_stop_hotkey.status == CapabilityStatus::Available {
        issues.push("stop_hotkey: available".into());
    } else {
        let reason = hotkey_error.unwrap_or(caps.global_stop_hotkey.reason.as_str());
        issues.push(format!(
            "stop_hotkey: unavailable ({reason}); foreground input disabled"
        ));
        if let Some(ref step) = caps.global_stop_hotkey.next_step {
            issues.push(format!("Next step for stop_hotkey: {step}"));
        }
    }

    if caps.foreground_input.status == CapabilityStatus::Unavailable {
        issues.push(format!(
            "foreground_input: unavailable ({})",
            caps.foreground_input.reason
        ));
        if let Some(ref step) = caps.foreground_input.next_step {
            issues.push(format!("Next step for foreground_input: {step}"));
        }
    }

    let ready = atspi_ready && env.session_type != LinuxSessionType::Unknown;

    DoctorReport {
        capabilities: caps.to_protocol_capabilities(),
        ready,
        issues,
    }
}

/// Run doctor diagnosis for `LinuxDriver`, deriving issues and capabilities from the unified capability table.
#[allow(clippy::too_many_arguments)]
pub fn run_driver_doctor(
    env: &LinuxEnvironment,
    portal: &PortalInfo,
    caps: &gcu_protocol::Capabilities,
    composite: bool,
    xtest: bool,
    hotkey_ok: bool,
    guard_ok: bool,
    is_stopped: bool,
    atspi_ready: bool,
    hotkey_error: Option<&str>,
    has_x_server: bool,
) -> DoctorReport {
    let target = if env.session_type.is_wayland() {
        if has_x_server {
            super::capabilities::TargetEnvironment::XWaylandWindow
        } else {
            super::capabilities::TargetEnvironment::NativeWayland
        }
    } else {
        super::capabilities::TargetEnvironment::PureX11
    };

    let caps_table = super::capabilities::build_environment_capabilities(
        env, target, portal, composite, xtest, hotkey_ok, guard_ok, is_stopped,
    );

    let mut report = build_doctor_report(env, &caps_table, portal, atspi_ready, hotkey_error);
    report.capabilities = caps.clone();
    report
}
