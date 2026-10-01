//! Typed product-surface mapping. Profile, tab, and WebView ids never
//! reach the desktop adapter.

use super::*;
use crate::adapter::SurfaceKind;

pub struct SurfaceRouter;

impl SurfaceRouter {
    pub fn parse_wire(wire: &str) -> Result<SurfaceKind, BrokerError> {
        SurfaceKind::from_wire(wire).map_err(BrokerError::Schema)
    }

    pub fn classify_target_id(target_id: &str) -> SurfaceKind {
        let id = target_id.trim();
        if id.starts_with("managed-profile:") || id.starts_with("managed:") {
            SurfaceKind::ManagedBrowser
        } else if id.starts_with("wv|") || id.starts_with("wv:") || id.starts_with("webview:") {
            SurfaceKind::WebView
        } else if id.starts_with("existing-tab:") || id.starts_with("tab:") {
            SurfaceKind::ExistingTab
        } else {
            SurfaceKind::Desktop
        }
    }

    pub fn authorize(
        broker: &ComputerUseBroker,
        session: &str,
        run_id: &str,
        surface: SurfaceKind,
        target_id: &str,
    ) -> Result<u64, BrokerError> {
        broker.require_owner(session, run_id)?;
        broker.prepare_authorize(run_id)?;
        broker.reject_cross_surface(run_id, surface)?;
        let classified = Self::classify_target_id(target_id);
        let target_matches_surface = match surface {
            SurfaceKind::Desktop => classified == SurfaceKind::Desktop,
            SurfaceKind::ManagedBrowser => classified == SurfaceKind::ManagedBrowser,
            SurfaceKind::ExistingTab => classified == SurfaceKind::ExistingTab,
            SurfaceKind::WebView => classified == SurfaceKind::WebView,
        };
        if !target_matches_surface {
            return Err(BrokerError::DeadTarget);
        }
        // Missing backends fail before a profile is provisioned or a user tab
        // is borrowed.
        broker.require_surface_executor(surface)?;
        match surface {
            SurfaceKind::Desktop => {
                if Self::classify_target_id(target_id) != SurfaceKind::Desktop {
                    return Err(BrokerError::Schema("target is not a desktop window".into()));
                }
                broker.authorize_desktop_window(run_id, target_id)
            }
            SurfaceKind::ManagedBrowser => {
                broker.authorize_managed_target(session, run_id, target_id)
            }
            SurfaceKind::ExistingTab => {
                // Serialize picker transactions, including raw-tab reauthorization.
                // Stop/feature-off do not wait on this lock; commit rechecks them.
                let _guard = broker.existing_authorization.lock();
                if let Some(selector) = target_id.strip_prefix("existing-tab:") {
                    let previous = broker
                        .authorized_target(run_id)
                        .ok()
                        .and_then(|(target, _)| target.strip_prefix("tab:").map(str::to_owned))
                        .and_then(|tab| broker.tabs().existing_target_info(&tab));
                    let (grant, acquired) = broker
                        .tabs()
                        .acquire_picker_tab(session, run_id, selector)?;
                    let result = broker.authorize_non_desktop(
                        run_id,
                        &format!("tab:{}", grant.tab_id),
                        SurfaceKind::ExistingTab,
                    );
                    if result.is_err() && acquired {
                        broker.tabs().release_existing_grant(&grant);
                    } else if result.is_ok() {
                        if let Some(previous) = previous.filter(|old| old.tab_id != grant.tab_id) {
                            broker.tabs().release_existing_grant(&previous);
                        }
                    }
                    result
                } else {
                    broker.authorize_non_desktop(run_id, target_id, SurfaceKind::ExistingTab)
                }
            }
            SurfaceKind::WebView => {
                broker.authorize_non_desktop(run_id, target_id, SurfaceKind::WebView)
            }
        }
    }
}
