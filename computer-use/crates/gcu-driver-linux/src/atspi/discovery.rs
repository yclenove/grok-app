//! AT-SPI application and window discovery helpers.

use crate::atspi::bus::{AtspiBus, AtspiNodeRef};
use crate::atspi::source::AtspiSource;
use crate::x11::AtspiWindowCandidate;
use gcu_core::driver::{AppQuery, AppRef, DriverError, Result, DRIVER_DEADLINE};
use gcu_core::model::Rect;
use gcu_protocol::types::AppIdentity as ProtocolIdentity;
use gcu_protocol::AppInfo;
use std::time::{Duration, Instant};

/// Map a verified image identity onto the protocol authorization key.
///
/// `desktop_id` stays discovery-only and is not copied. A missing inspect
/// must stay `None` rather than inventing a hash.
pub fn protocol_identity(identity: &crate::identity::AppIdentity) -> ProtocolIdentity {
    let path = identity.exe_path.to_string_lossy().into_owned();
    ProtocolIdentity {
        id: path.clone(),
        executable: Some(path),
        team_id: None,
        publisher: None,
        code_hash: Some(identity.sha256.clone()),
    }
}

/// Query all accessible desktop applications from AT-SPI Registry.
pub async fn list_atspi_apps(conn: &zbus::Connection) -> Result<Vec<AppInfo>> {
    let bus = AtspiBus::new(conn.clone());
    let root = registry_root();
    let started = Instant::now();

    let app_nodes = bus.children(&root).await.unwrap_or_default();
    let mut apps = Vec::new();
    for node in app_nodes {
        let name = bus
            .name(&node)
            .await
            .unwrap_or_default()
            .unwrap_or_default();
        if name.is_empty() {
            continue;
        }
        let pid = bus.application_pid(&node).await;
        let identity = match pid {
            Some(pid) => identity_for_pid(pid, started).await,
            None => None,
        };
        apps.push(AppInfo {
            id: node.destination.clone(),
            name,
            pid,
            identity,
        });
    }
    Ok(apps)
}

async fn identity_for_pid(pid: u32, started: Instant) -> Option<ProtocolIdentity> {
    let elapsed = started.elapsed();
    if elapsed >= DRIVER_DEADLINE {
        return None;
    }
    let remaining = DRIVER_DEADLINE.saturating_sub(elapsed);
    let budget = remaining.min(Duration::from_secs(2));
    match tokio::time::timeout(budget, crate::identity::inspection::inspect_async(pid)).await {
        Ok(Ok(identity)) => Some(protocol_identity(&identity)),
        _ => None,
    }
}

fn registry_root() -> AtspiNodeRef {
    AtspiNodeRef {
        destination: "org.a11y.atspi.Registry".into(),
        path: "/org/a11y/atspi/accessible/root".into(),
    }
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
    let root = registry_root();

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
        let pid = bus
            .application_pid(&a_node)
            .await
            .or_else(|| app.and_then(|target| target.info.pid));

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
                pid,
            });
        }
    }

    Ok(candidates)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::AppIdentity;
    use std::path::PathBuf;

    #[test]
    fn protocol_identity_uses_image_path_and_digest_not_desktop_id() {
        let identity = AppIdentity {
            exe_path: PathBuf::from("/usr/bin/gedit"),
            dev: 1,
            inode: 2,
            sha256: "ab".repeat(32),
            pid: 9,
            start_time: 3,
            desktop_id: Some("org.gnome.gedit.desktop".into()),
        };
        let wire = protocol_identity(&identity);
        assert_eq!(wire.id, "/usr/bin/gedit");
        assert_eq!(wire.executable.as_deref(), Some("/usr/bin/gedit"));
        assert_eq!(wire.code_hash.as_deref(), Some(identity.sha256.as_str()));
        assert!(!wire.id.contains("desktop"));
        assert!(wire.team_id.is_none());
        assert!(wire.publisher.is_none());
    }
}
