//! AT-SPI application and window discovery helpers.

use crate::atspi::bus::{AtspiBus, AtspiNodeRef};
use crate::atspi::source::AtspiSource;
use crate::x11::AtspiWindowCandidate;
use gcu_core::driver::{AppQuery, AppRef, DriverError, Result};
use gcu_core::model::Rect;
use gcu_protocol::AppInfo;

/// Query all accessible desktop applications from AT-SPI Registry.
pub async fn list_atspi_apps(conn: &zbus::Connection) -> Result<Vec<AppInfo>> {
    let bus = AtspiBus::new(conn.clone());
    let root = AtspiNodeRef {
        destination: "org.a11y.atspi.Registry".into(),
        path: "/org/a11y/atspi/accessible/root".into(),
    };

    let app_nodes = bus.children(&root).await.unwrap_or_default();
    let mut apps = Vec::new();
    for node in app_nodes {
        let name = bus
            .name(&node)
            .await
            .unwrap_or_default()
            .unwrap_or_default();
        if !name.is_empty() {
            apps.push(AppInfo {
                id: node.destination.clone(),
                name,
                pid: None,
                identity: None,
            });
        }
    }
    Ok(apps)
}

/// Resolve a single application from a list of discovered apps against a query.
pub fn resolve_atspi_app(apps: &[AppInfo], query: &AppQuery) -> Result<AppRef> {
    let query_str = query.0.to_ascii_lowercase();
    let matches: Vec<AppInfo> = apps
        .iter()
        .filter(|app| {
            app.name.to_ascii_lowercase().contains(&query_str)
                || app.id.to_ascii_lowercase().contains(&query_str)
        })
        .cloned()
        .collect();

    if matches.is_empty() {
        return Err(DriverError::AppNotFound);
    }
    if matches.len() > 1 {
        return Err(DriverError::AmbiguousApp);
    }

    let matched = matches.into_iter().next().ok_or(DriverError::AppNotFound)?;
    Ok(AppRef { info: matched })
}

/// Enumerate window candidates exposed via AT-SPI for the given app filter.
pub async fn list_atspi_candidate_windows(
    conn: &zbus::Connection,
    app: Option<&AppRef>,
) -> Result<Vec<AtspiWindowCandidate>> {
    let bus = AtspiBus::new(conn.clone());
    let root = AtspiNodeRef {
        destination: "org.a11y.atspi.Registry".into(),
        path: "/org/a11y/atspi/accessible/root".into(),
    };

    let app_nodes = bus.children(&root).await.unwrap_or_default();
    let mut candidates = Vec::new();

    for a_node in app_nodes {
        let app_name = bus
            .name(&a_node)
            .await
            .unwrap_or_default()
            .unwrap_or_default();
        if let Some(target) = app {
            if a_node.destination != target.info.id
                && !app_name.eq_ignore_ascii_case(&target.info.name)
            {
                continue;
            }
        }

        let win_nodes = bus.children(&a_node).await.unwrap_or_default();
        for w_node in win_nodes {
            let title = bus
                .name(&w_node)
                .await
                .unwrap_or_default()
                .unwrap_or_default();
            let extents = bus
                .extents(&w_node)
                .await
                .unwrap_or_default()
                .unwrap_or(Rect {
                    x: 0.0,
                    y: 0.0,
                    w: 1280.0,
                    h: 860.0,
                });

            candidates.push(AtspiWindowCandidate {
                path: w_node.path.clone(),
                title,
                frame: extents,
                pid: app.and_then(|a| a.info.pid),
            });
        }
    }

    Ok(candidates)
}
