//! Explicit Host preparation starts only the surface selected by a reservation.
use super::{ComputerUseAdapter, ComputerUseBroker, SurfaceKind};
use std::sync::{Arc, OnceLock};

static DESKTOP: OnceLock<Arc<grok_computer_use_core::owned::HostOwnedAdapter>> = OnceLock::new();

fn desktop_descriptor() -> Arc<grok_computer_use_core::owned::HostOwnedAdapter> {
    DESKTOP
        .get_or_init(|| {
            Arc::new(grok_computer_use_core::owned::HostOwnedAdapter::new(
                super::os_adapter(),
            ))
        })
        .clone()
}

pub(super) fn desktop_adapter() -> Arc<dyn ComputerUseAdapter> {
    desktop_descriptor()
}

/// Explicit selection registers an identity producer only. It must not start a
/// driver, browser, profile, permission prompt, discovery or input.
pub(super) fn prepare_scope_selection(
    broker: &ComputerUseBroker,
    surface: SurfaceKind,
) -> Result<(), String> {
    prepare_scope_selection_with(broker, surface, desktop_adapter, || {
        super::product_webview()
    })
}

fn prepare_scope_selection_with(
    broker: &ComputerUseBroker,
    surface: SurfaceKind,
    desktop: impl FnOnce() -> Arc<dyn ComputerUseAdapter>,
    webview: impl FnOnce() -> Arc<dyn ComputerUseAdapter>,
) -> Result<(), String> {
    match surface {
        SurfaceKind::Desktop => broker
            .register_surface_adapter(surface, desktop())
            .map(|_| ()),
        SurfaceKind::ExistingTab => broker.register_existing_browser_adapter().map(|_| ()),
        SurfaceKind::WebView => broker
            .register_surface_adapter(surface, webview())
            .map(|_| ()),
        _ => return Err("Computer Use scope picker producer unavailable for this surface".into()),
    }
    .map_err(|error| error.to_string())
}

pub(super) fn prepared_desktop() -> Arc<dyn ComputerUseAdapter> {
    static PREPARED: OnceLock<()> = OnceLock::new();
    let desktop = desktop_descriptor();
    PREPARED.get_or_init(|| super::attach_platform_worker(&desktop));
    desktop
}

/// No listener, discovery, profile, or native permission is prepared here for
/// unrelated surfaces. Any selected-surface failure aborts the Host transaction.
pub(super) fn prepare(broker: &ComputerUseBroker, surface: SurfaceKind) -> Result<(), String> {
    prepare_with(
        broker,
        surface,
        prepared_desktop,
        || super::product_webview(),
        super::browser_supervisor::ensure_product_attached,
    )
}

fn prepare_with(
    broker: &ComputerUseBroker,
    surface: SurfaceKind,
    desktop: impl FnOnce() -> Arc<dyn ComputerUseAdapter>,
    webview: impl FnOnce() -> Arc<dyn ComputerUseAdapter>,
    managed: impl FnOnce(&ComputerUseBroker) -> Result<(), String>,
) -> Result<(), String> {
    match surface {
        SurfaceKind::Desktop => broker
            .register_surface_adapter(surface, desktop())
            .map(|_| ()),
        SurfaceKind::ExistingTab => broker.register_existing_browser_adapter().map(|_| ()),
        SurfaceKind::WebView => broker
            .register_surface_adapter(surface, webview())
            .map(|_| ()),
        SurfaceKind::ManagedBrowser => return managed(broker),
    }
    .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests;
