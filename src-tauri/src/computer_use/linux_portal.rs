//! App-owned native Wayland selection. Same registry backs Broker dispatch and
//! the explicit UI consent transaction. No model-call or XWayland fallback.
use grok_computer_use_core::session_grants::AuthorizationTicket;
use grok_computer_use_wayland::{
    PortalInputPolicy, PortalOptions, PortalRegistry, PortalSelection, PortalSelectionState,
    SourceKind,
};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

fn native_session(session_type: Option<&str>, display: Option<&str>) -> bool {
    session_type.is_some_and(|s| s.eq_ignore_ascii_case("wayland"))
        || display.is_some_and(|s| !s.trim().is_empty())
}

pub(crate) fn selected() -> bool {
    static SELECTED: OnceLock<bool> = OnceLock::new();
    *SELECTED.get_or_init(|| {
        cfg!(feature = "computer-use-wayland-preview")
            && native_session(
                std::env::var("XDG_SESSION_TYPE").ok().as_deref(),
                std::env::var("WAYLAND_DISPLAY").ok().as_deref(),
            )
    })
}

struct HostPolicy;
impl PortalInputPolicy for HostPolicy {
    fn input_available(&self) -> bool {
        super::feature::feature_enabled()
    }
    // Only the product switch. The mandatory GNOME/logind monitor composed by
    // select_for_authorization independently fences physical input and locks.
    fn user_input_active(&self) -> bool {
        false
    }
}

pub(crate) fn registry() -> Arc<PortalRegistry> {
    static REGISTRY: OnceLock<Arc<PortalRegistry>> = OnceLock::new();
    REGISTRY
        .get_or_init(|| Arc::new(PortalRegistry::new(Arc::new(HostPolicy))))
        .clone()
}

/// The command owns cancellation until the SessionGrants transaction commits.
/// A dropped UI waiter can never leave an uncommitted native grant running.
pub(crate) struct PreparedTarget {
    registry: Arc<PortalRegistry>,
    selection: PortalSelection,
    target: String,
    committed: bool,
}
impl PreparedTarget {
    pub(crate) fn target(&self) -> &str {
        &self.target
    }
    pub(crate) fn commit(&mut self) {
        self.committed = true;
    }
}
impl Drop for PreparedTarget {
    fn drop(&mut self) {
        if !self.committed {
            let _ = self.registry.cancel(&self.selection);
        }
        retire_when_joined(self.registry.clone(), self.selection.clone());
    }
}

fn retire_when_joined(registry: Arc<PortalRegistry>, selection: PortalSelection) {
    // Metadata housekeeping, not an owner of native completion. Start only
    // after the command drops its guard, so it can first read the close reason.
    // A failed/unjoined owner remains retained and blocks replacement.
    tauri::async_runtime::spawn(async move {
        loop {
            match registry.state(&selection) {
                Ok(PortalSelectionState::Closed { .. }) => {
                    if let Err(error) = registry.forget(&selection) {
                        tracing::warn!(%error, "native Wayland selection could not be forgotten");
                    }
                    break;
                }
                Ok(PortalSelectionState::Failed { error }) => {
                    tracing::warn!(%error, "native Wayland selection owners remain unproven");
                    break;
                }
                Err(_) => break,
                _ => tokio::time::sleep(Duration::from_millis(25)).await,
            }
        }
    });
}

pub(crate) async fn prepare<R: tauri::Runtime>(
    window: tauri::Window<R>,
    ticket: &AuthorizationTicket,
) -> Result<PreparedTarget, String> {
    if !selected() {
        return Err("native Wayland selection is not enabled for this build/session".into());
    }
    ticket.check_preparation()?;
    let registry = registry();
    let mut options = PortalOptions::new(ticket.run_id.clone(), SourceKind::Monitor);
    options.timeout = Duration::from_secs(90);
    let selection =
        super::linux_parent::select_for_authorization(window, &registry, options, 1, 1, ticket)?;
    let mut pending = PreparedTarget {
        registry: registry.clone(),
        selection: selection.clone(),
        target: String::new(),
        committed: false,
    };
    loop {
        ticket.check_preparation()?;
        match pending.registry.state(&pending.selection)? {
            PortalSelectionState::Ready { target_id } => {
                ticket.check_preparation()?;
                pending.target = target_id;
                return Ok(pending);
            }
            PortalSelectionState::Pending => {}
            PortalSelectionState::Closed { reason } => {
                return Err(reason.unwrap_or_else(|| "native Wayland selection cancelled".into()))
            }
            PortalSelectionState::Closing => {
                return Err("native Wayland selection revoked or cancelled".into())
            }
            PortalSelectionState::Failed { error } => return Err(error),
        }
        tokio::select! {
            _ = ticket.preparation_cancelled() => return Err("target authorization was cancelled".into()),
            _ = tokio::time::sleep(Duration::from_millis(10)) => {},
        }
    }
}

#[cfg(test)]
#[path = "linux_portal_tests.rs"]
mod tests;
