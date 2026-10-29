//! AT-SPI connection management, role mapping, tree building, and toolkit detection.

pub mod actions;
pub mod bus;
pub mod discovery;
pub mod events;
pub mod roles;
pub(crate) mod scope;
pub mod sensitive;
pub mod source;
pub mod tree;

pub use discovery::{list_atspi_apps, list_atspi_candidate_windows, resolve_atspi_app};

pub use actions::{
    execute_atspi_action, extract_atspi_node, extract_fake_id, find_best_press_action,
    find_secondary_action, PRESS_CANDIDATE_PATTERNS,
};
pub use bus::AtspiBus;
pub use events::{AtspiEventSubscriber, FakeEventSubscriber, LiveEventSubscriber};
pub use roles::map_atspi_role;
pub use sensitive::{is_terminal_like, terminal_node_flags};
pub use source::{AtspiSource, FakeNode, FakeSource};
pub use tree::{build_raw_tree, map_atspi_states, MAX_NODES_CEILING, TREE_BUILD_TIMEOUT};

use gcu_core::driver::DriverError;
use std::{collections::HashMap, env, time::Duration};

/// Recognized application GUI toolkit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppToolkit {
    /// GTK 3 toolkit.
    Gtk3,
    /// GTK 4 toolkit.
    Gtk4,
    /// Qt 5 toolkit.
    Qt5,
    /// Qt 6 toolkit.
    Qt6,
    /// Electron framework.
    Electron,
    /// Chromium / Chrome browser.
    Chromium,
    /// Unrecognized or custom toolkit.
    Unknown,
}

impl AppToolkit {
    /// Diagnostic hint for doctor report or troubleshooting.
    pub fn doctor_hint(&self) -> Option<&'static str> {
        match self {
            Self::Electron | Self::Chromium => Some(
                "Chromium and Electron applications require the --force-renderer-accessibility flag to expose their accessibility tree.",
            ),
            Self::Qt5 | Self::Qt6 => Some(
                "Qt applications require QT_ACCESSIBILITY=1 and the at-spi2 bridge plugin to expose their accessibility tree.",
            ),
            Self::Gtk3 | Self::Gtk4 => None,
            Self::Unknown => None,
        }
    }
}

/// Detect application toolkit from AT-SPI accessible node attributes.
pub fn detect_toolkit(attributes: &HashMap<String, String>) -> AppToolkit {
    let toolkit_name = attributes
        .get("toolkit-name")
        .or_else(|| attributes.get("toolkit_name"))
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_default();

    if toolkit_name.contains("gtk") {
        if toolkit_name.contains('4') {
            AppToolkit::Gtk4
        } else {
            AppToolkit::Gtk3
        }
    } else if toolkit_name.contains("qt") {
        if toolkit_name.contains('6') {
            AppToolkit::Qt6
        } else {
            AppToolkit::Qt5
        }
    } else if toolkit_name.contains("electron") {
        AppToolkit::Electron
    } else if toolkit_name.contains("chromium") || toolkit_name.contains("chrome") {
        AppToolkit::Chromium
    } else {
        AppToolkit::Unknown
    }
}

/// Establish an active connection to the AT-SPI accessibility bus.
///
/// Connection strategy:
/// 1. Inspect `AT_SPI_BUS_ADDRESS` environment variable first.
/// 2. If unset, connect to the user session D-Bus and query `org.a11y.Bus.GetAddress`.
/// 3. If unreachable or unready, report `DriverError::PermissionMissing` or `CapabilityUnavailable`.
pub async fn connect_atspi() -> Result<zbus::Connection, DriverError> {
    const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);

    tokio::time::timeout(CONNECT_TIMEOUT, async {
        // Step 1: Check AT_SPI_BUS_ADDRESS environment variable
        if let Ok(addr_str) = env::var("AT_SPI_BUS_ADDRESS") {
            if !addr_str.trim().is_empty() {
                let addr: zbus::Address =
                    addr_str.trim().parse().map_err(|e| DriverError::Native {
                        message: format!("invalid AT_SPI_BUS_ADDRESS '{addr_str}': {e}"),
                        maybe_executed: false,
                    })?;
                let conn = zbus::connection::Builder::address(addr)
                    .map_err(|e| DriverError::Native {
                        message: format!("failed to configure a11y bus connection: {e}"),
                        maybe_executed: false,
                    })?
                    .build()
                    .await
                    .map_err(|e| DriverError::PermissionMissing {
                        permission: format!("accessibility bus connection failed: {e}"),
                    })?;
                return Ok(conn);
            }
        }

        // Step 2: Query session bus org.a11y.Bus.GetAddress
        let session_conn =
            zbus::Connection::session()
                .await
                .map_err(|e| DriverError::PermissionMissing {
                    permission: format!("session D-Bus unavailable: {e}"),
                })?;

        let bus_proxy = atspi::proxy::bus::BusProxy::new(&session_conn)
            .await
            .map_err(|e| DriverError::PermissionMissing {
                permission: format!("org.a11y.Bus service unavailable: {e}"),
            })?;

        let addr_str =
            bus_proxy
                .get_address()
                .await
                .map_err(|e| DriverError::PermissionMissing {
                    permission: format!(
                        "failed to obtain AT-SPI bus address from org.a11y.Bus: {e}"
                    ),
                })?;

        let addr: zbus::Address = addr_str.parse().map_err(|e| DriverError::Native {
            message: format!("invalid address returned by org.a11y.Bus: {e}"),
            maybe_executed: false,
        })?;

        let conn = zbus::connection::Builder::address(addr)
            .map_err(|e| DriverError::Native {
                message: format!("failed to configure a11y bus connection: {e}"),
                maybe_executed: false,
            })?
            .build()
            .await
            .map_err(|e| DriverError::PermissionMissing {
                permission: format!("failed to connect to AT-SPI bus at '{addr_str}': {e}"),
            })?;

        Ok(conn)
    })
    .await
    .map_err(|_| DriverError::Timeout {
        maybe_executed: false,
    })?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toolkit_detection_heuristics() {
        let mut gtk4_attrs = HashMap::new();
        gtk4_attrs.insert("toolkit-name".into(), "gtk-4.0".into());
        assert_eq!(detect_toolkit(&gtk4_attrs), AppToolkit::Gtk4);

        let mut electron_attrs = HashMap::new();
        electron_attrs.insert("toolkit_name".into(), "Electron".into());
        assert_eq!(detect_toolkit(&electron_attrs), AppToolkit::Electron);
        assert!(detect_toolkit(&electron_attrs).doctor_hint().is_some());

        let mut qt6_attrs = HashMap::new();
        qt6_attrs.insert("toolkit-name".into(), "Qt6".into());
        assert_eq!(detect_toolkit(&qt6_attrs), AppToolkit::Qt6);
        assert!(detect_toolkit(&qt6_attrs).doctor_hint().is_some());

        let empty = HashMap::new();
        assert_eq!(detect_toolkit(&empty), AppToolkit::Unknown);
        assert_eq!(detect_toolkit(&empty).doctor_hint(), None);
    }
}
