//! Detailed Linux and Wayland capability matrix and mapping to protocol types.

use super::{Compositor, LinuxEnvironment, PortalInfo};
use crate::atspi::tree::MAX_NODES_CEILING;
use gcu_protocol::{
    capabilities::{
        BackgroundOperation, CapabilityLimits, DeliveryCapabilities, ForegroundCapability,
        ObserveCapabilities, Platform, PrivateApiStatus,
    },
    Capabilities,
};
use std::collections::BTreeMap;

/// Target display and window execution environment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TargetEnvironment {
    /// Pure X11 desktop environment.
    PureX11,
    /// X11 client window managed under XWayland.
    XWaylandWindow,
    /// Native Wayland window (xdg_toplevel surface).
    NativeWayland,
}

impl TargetEnvironment {
    /// Human-readable label for the target environment.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::PureX11 => "pure_x11",
            Self::XWaylandWindow => "xwayland_window",
            Self::NativeWayland => "native_wayland",
        }
    }
}

/// Status of an individual capability item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CapabilityStatus {
    /// Completely supported and verified.
    Available,
    /// Partially supported with degradation or constraints.
    Partial,
    /// Unsupported or disabled.
    Unavailable,
}

impl CapabilityStatus {
    /// Whether the capability is fully available.
    pub fn is_available(&self) -> bool {
        matches!(self, Self::Available)
    }

    /// Whether the capability is available or partially available.
    pub fn is_supported(&self) -> bool {
        matches!(self, Self::Available | Self::Partial)
    }
}

/// Description of an individual capability, including status and rationale.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityItem {
    /// Availability status.
    pub status: CapabilityStatus,
    /// Technical reason or degradation detail.
    pub reason: String,
    /// Suggested user action or remediation if degraded or unavailable.
    pub next_step: Option<String>,
}

/// Complete capability matrix across observation, delivery, safety, and guard subsystems.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvironmentCapabilities {
    /// Target environment for which these capabilities apply.
    pub target: TargetEnvironment,
    /// Compositor family.
    pub compositor: Compositor,
    /// Accessibility tree observation capability.
    pub tree_observation: CapabilityItem,
    /// Window screenshot capture capability.
    pub screenshot: CapabilityItem,
    /// Background semantic accessibility actions capability.
    pub semantic_actions: CapabilityItem,
    /// Targeted window/process input capability.
    pub targeted_input: CapabilityItem,
    /// Foreground global pointer and keyboard input capability.
    pub foreground_input: CapabilityItem,
    /// Global emergency Stop hotkey capability.
    pub global_stop_hotkey: CapabilityItem,
    /// Physical user activity guard capability.
    pub user_activity_guard: CapabilityItem,
    /// Probed private APIs.
    pub private_apis: BTreeMap<String, PrivateApiStatus>,
}

impl EnvironmentCapabilities {
    /// Convert this detailed capability matrix into the standard `gcu_protocol::Capabilities`.
    pub fn to_protocol_capabilities(&self) -> Capabilities {
        let observe = ObserveCapabilities {
            tree: self.tree_observation.status.is_available(),
            screenshot: self.screenshot.status.is_available(),
            events: true,
        };

        let delivery = DeliveryCapabilities {
            accessibility: self.semantic_actions.status.is_available(),
            targeted_pointer: false,
            targeted_keys: false,
            foreground: if self.foreground_input.status.is_available() {
                ForegroundCapability::Policy
            } else {
                ForegroundCapability::Unavailable
            },
        };

        let background_operation =
            match (self.semantic_actions.status, self.foreground_input.status) {
                (CapabilityStatus::Available, _) => BackgroundOperation::Partial,
                _ => BackgroundOperation::None,
            };

        let version = match self.target {
            TargetEnvironment::PureX11 => "x11-atspi".to_string(),
            TargetEnvironment::XWaylandWindow => {
                format!("wayland-{}-xwayland", self.compositor.slug())
            }
            TargetEnvironment::NativeWayland => format!("wayland-{}", self.compositor.slug()),
        };

        Capabilities {
            platform: Platform::Linux,
            version,
            observe,
            delivery,
            background_operation,
            user_activity_guard: self.user_activity_guard.status.is_available(),
            overlay_excluded_from_capture: false,
            private_apis: self.private_apis.clone(),
            limits: CapabilityLimits {
                max_tree_nodes: MAX_NODES_CEILING as u32,
                max_image_edge: 2560,
                type_text_max: 8192,
            },
        }
    }
}

/// Build the detailed capability matrix from environmental probes and driver state.
#[allow(clippy::too_many_arguments)]
pub fn build_environment_capabilities(
    env: &LinuxEnvironment,
    target: TargetEnvironment,
    portal: &PortalInfo,
    composite_available: bool,
    xtest_available: bool,
    hotkey_available: bool,
    guard_available: bool,
    stop_latched: bool,
) -> EnvironmentCapabilities {
    let tree_observation = CapabilityItem {
        status: CapabilityStatus::Available,
        reason: "AT-SPI2 accessibility tree observation via D-Bus".into(),
        next_step: None,
    };

    let semantic_actions = CapabilityItem {
        status: CapabilityStatus::Available,
        reason: "AT-SPI semantic actions (Action, SetValue, SelectText, Text, Value)".into(),
        next_step: None,
    };

    let targeted_input = CapabilityItem {
        status: CapabilityStatus::Unavailable,
        reason: "Linux does not support per-window targeted input without foreground activation"
            .into(),
        next_step: None,
    };

    let screenshot = match target {
        TargetEnvironment::NativeWayland => {
            if portal.available && portal.screencast_version.is_some() {
                CapabilityItem {
                    status: CapabilityStatus::Available,
                    reason: "Native Wayland window capture via xdg-desktop-portal ScreenCast and PipeWire".into(),
                    next_step: None,
                }
            } else {
                let (reason, next_step) = if !portal.available {
                    (
                        "Native Wayland window capture unavailable: xdg-desktop-portal service missing on D-Bus session bus".into(),
                        Some("Install xdg-desktop-portal and a backend suitable for your desktop environment.".into()),
                    )
                } else {
                    (
                        "Native Wayland window capture unavailable: org.freedesktop.portal.ScreenCast backend interface missing".into(),
                        Some("Install or configure the compositor screencast backend (e.g. xdg-desktop-portal-gnome, xdg-desktop-portal-kde, or xdg-desktop-portal-wlr).".into()),
                    )
                };
                CapabilityItem {
                    status: CapabilityStatus::Unavailable,
                    reason,
                    next_step,
                }
            }
        }
        TargetEnvironment::PureX11 | TargetEnvironment::XWaylandWindow => {
            if composite_available {
                let reason = if target == TargetEnvironment::PureX11 {
                    "X11 XComposite + XGetImage window capture".into()
                } else {
                    "XWayland XComposite window capture".into()
                };
                CapabilityItem {
                    status: CapabilityStatus::Available,
                    reason,
                    next_step: None,
                }
            } else {
                CapabilityItem {
                    status: CapabilityStatus::Partial,
                    reason: "XComposite extension missing: window screenshots degrade to root-window crop and will fail with CapabilityUnavailable if window is occluded or off-screen (observe.screenshot=occluded_unsafe)".into(),
                    next_step: Some("Enable XComposite extension in X11 server for reliable occluded window capture.".into()),
                }
            }
        }
    };

    let global_stop_hotkey = match target {
        TargetEnvironment::NativeWayland => {
            let reason = if portal.global_shortcuts_version.is_some() {
                "GlobalShortcuts portal detected but integration is pending (P7-03)"
            } else {
                "Global Stop hotkey is unavailable on Wayland without GlobalShortcuts portal"
            };
            CapabilityItem {
                status: CapabilityStatus::Unavailable,
                reason: reason.into(),
                next_step: Some(
                    "Ensure desktop environment supports org.freedesktop.portal.GlobalShortcuts (GNOME 44+, KDE 6+)."
                        .into(),
                ),
            }
        }
        TargetEnvironment::XWaylandWindow => {
            if hotkey_available {
                CapabilityItem {
                    status: CapabilityStatus::Partial,
                    reason: "XGrabKey on XWayland only intercepts hotkeys when X11 surfaces are active; ungrabbed during native Wayland focus".into(),
                    next_step: Some("Configure compositor-level global shortcuts daemon for desktop-wide stop reliability.".into()),
                }
            } else {
                CapabilityItem {
                    status: CapabilityStatus::Unavailable,
                    reason: "Stop hotkey registration failed or conflicted on XWayland".into(),
                    next_step: Some(
                        "Ensure Ctrl+Alt+Shift+Backspace is not occupied by another app.".into(),
                    ),
                }
            }
        }
        TargetEnvironment::PureX11 => {
            if hotkey_available {
                CapabilityItem {
                    status: CapabilityStatus::Available,
                    reason: "XGrabKey global root hotkey listener (Ctrl+Alt+Shift+Backspace)"
                        .into(),
                    next_step: None,
                }
            } else {
                CapabilityItem {
                    status: CapabilityStatus::Unavailable,
                    reason: "Stop hotkey registration failed or key combination occupied".into(),
                    next_step: Some(
                        "Ensure Ctrl+Alt+Shift+Backspace is not grabbed by another application."
                            .into(),
                    ),
                }
            }
        }
    };

    let user_activity_guard = match target {
        TargetEnvironment::NativeWayland => CapabilityItem {
            status: CapabilityStatus::Unavailable,
            reason: "User activity guard is unavailable on Wayland (no global passive input snooping mechanism)".into(),
            next_step: Some("Wayland compositors intentionally isolate input devices across clients.".into()),
        },
        TargetEnvironment::XWaylandWindow => CapabilityItem {
            status: CapabilityStatus::Unavailable,
            reason: "XInput2 on XWayland does not receive input events outside XWayland surfaces".into(),
            next_step: Some("Wayland does not permit background monitoring of physical user activity.".into()),
        },
        TargetEnvironment::PureX11 => {
            if guard_available {
                CapabilityItem {
                    status: CapabilityStatus::Available,
                    reason: "XInput2 raw motion and key event listener on root window".into(),
                    next_step: None,
                }
            } else {
                CapabilityItem {
                    status: CapabilityStatus::Unavailable,
                    reason: "XInput2 listener inactive or device filter unavailable".into(),
                    next_step: Some("Check XInput2 extension availability and device permissions.".into()),
                }
            }
        }
    };

    let foreground_input = match target {
        TargetEnvironment::NativeWayland => CapabilityItem {
            status: CapabilityStatus::Unavailable,
            reason: "Native Wayland input requires portal RemoteDesktop and libei (P7-03 implementation pending)".into(),
            next_step: Some("Native Wayland window input requires P7-03 portal RemoteDesktop + libei integration.".into()),
        },
        TargetEnvironment::PureX11 | TargetEnvironment::XWaylandWindow => {
            if stop_latched {
                CapabilityItem {
                    status: CapabilityStatus::Unavailable,
                    reason: "Global Stop latch is active (stopped_by_user); foreground input disabled".into(),
                    next_step: Some("Resume execution via trusted UI or CLI (gcu resume).".into()),
                }
            } else if !hotkey_available {
                CapabilityItem {
                    status: CapabilityStatus::Unavailable,
                    reason: "Global Stop hotkey unavailable or conflicted; foreground input disabled for safety".into(),
                    next_step: Some("Ensure Ctrl+Alt+Shift+Backspace is not occupied by another app.".into()),
                }
            } else if !xtest_available {
                CapabilityItem {
                    status: CapabilityStatus::Unavailable,
                    reason: "XTEST extension missing from X server; foreground input disabled".into(),
                    next_step: Some("Enable XTEST extension in X server.".into()),
                }
            } else {
                CapabilityItem {
                    status: CapabilityStatus::Available,
                    reason: "XTEST pointer and keyboard input".into(),
                    next_step: None,
                }
            }
        }
    };

    let mut private_apis = BTreeMap::new();
    let to_status = |ok| {
        if ok {
            PrivateApiStatus::Available
        } else {
            PrivateApiStatus::Missing
        }
    };

    private_apis.insert("x11_composite".into(), to_status(composite_available));
    private_apis.insert("xtest".into(), to_status(xtest_available));
    private_apis.insert(
        "portal_screencast".into(),
        to_status(portal.screencast_version.is_some()),
    );
    private_apis.insert(
        "portal_remote_desktop".into(),
        to_status(portal.remote_desktop_version.is_some()),
    );
    private_apis.insert(
        "portal_global_shortcuts".into(),
        to_status(portal.global_shortcuts_version.is_some()),
    );

    EnvironmentCapabilities {
        target,
        compositor: env.compositor,
        tree_observation,
        screenshot,
        semantic_actions,
        targeted_input,
        foreground_input,
        global_stop_hotkey,
        user_activity_guard,
        private_apis,
    }
}

/// Build the four capability permutations based on user_activity_guard and stop_hotkey availability.
pub fn build_caps_quad(
    env: &LinuxEnvironment,
    target: TargetEnvironment,
    portal: &PortalInfo,
    composite: bool,
    xtest: bool,
) -> (Capabilities, Capabilities, Capabilities, Capabilities) {
    let make = |hotkey_ok: bool, guard_ok: bool| {
        build_environment_capabilities(
            env, target, portal, composite, xtest, hotkey_ok, guard_ok, !hotkey_ok,
        )
        .to_protocol_capabilities()
    };

    (
        make(true, true),
        make(true, false),
        make(false, true),
        make(false, false),
    )
}

/// Compute the capability quad for `LinuxDriver` based on server and environment state.
pub fn compute_driver_caps_quad(
    env: &LinuxEnvironment,
    portal: &PortalInfo,
    has_x_server: bool,
    composite: bool,
    xtest: bool,
) -> (Capabilities, Capabilities, Capabilities, Capabilities) {
    let target = if env.session_type.is_wayland() {
        if has_x_server {
            TargetEnvironment::XWaylandWindow
        } else {
            TargetEnvironment::NativeWayland
        }
    } else {
        TargetEnvironment::PureX11
    };

    build_caps_quad(env, target, portal, composite, xtest)
}

/// Default capabilities for the Linux AT-SPI / X11 driver.
pub fn default_capabilities() -> Capabilities {
    Capabilities {
        platform: Platform::Linux,
        version: "x11-atspi".into(),
        observe: ObserveCapabilities {
            tree: true,
            screenshot: true,
            events: true,
        },
        delivery: DeliveryCapabilities {
            accessibility: true,
            targeted_pointer: false,
            targeted_keys: false,
            foreground: ForegroundCapability::Policy,
        },
        background_operation: BackgroundOperation::Partial,
        user_activity_guard: false,
        overlay_excluded_from_capture: false,
        private_apis: BTreeMap::new(),
        limits: CapabilityLimits {
            max_tree_nodes: MAX_NODES_CEILING as u32,
            max_image_edge: 2560,
            type_text_max: 8192,
        },
    }
}
