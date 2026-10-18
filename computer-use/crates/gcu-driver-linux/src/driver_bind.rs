//! Rebind Host Stop and activity controls onto the live X11 listener.
//!
//! Replacing the latch, clock, or hotkey manager aborts the previous listener,
//! drops its grabs, and spawns a listener that holds the replacement. Writing
//! the struct field alone would leave the old `Arc` installed.

use super::LinuxDriver;
use crate::atspi::{
    connect_atspi,
    events::{AtspiEventSubscriber, LiveEventSubscriber},
};
use crate::wayland::LinuxEnvironment;
use crate::x11::{
    activity::ActivityClock,
    hotkey::{attach_x11_listener, HotkeyManager, StopLatch},
    RealX, XServer,
};
use gcu_protocol::capabilities::ForegroundCapability;
use std::sync::Arc;

impl LinuxDriver {
    /// Replace the activity clock and restart the listener on that clock.
    pub fn with_activity_clock(mut self, clock: Arc<dyn ActivityClock>) -> Self {
        let latch = self.stop_latch.clone();
        let manager = self.hotkey_manager.clone();
        install_controls(&mut self, latch, clock, manager);
        self
    }

    /// Replace the Stop latch and rebind the listener and hotkey manager to it.
    pub fn with_stop_latch(mut self, latch: Arc<dyn StopLatch>) -> Self {
        let available = self.hotkey_manager.is_available();
        let config = self.hotkey_manager.config().clone();
        let manager = Arc::new(HotkeyManager::new(config, latch.clone()));
        if self.x_server.is_none() {
            manager.set_available(available);
        }
        let clock = self.activity_clock.clone();
        install_controls(&mut self, latch, clock, manager);
        self
    }

    /// Adopt a hotkey manager and the latch that manager already holds.
    ///
    /// A manager already marked unavailable stays unavailable. Re-registering
    /// it would turn a simulated or previous grab failure back into success.
    pub fn with_hotkey_manager(mut self, manager: Arc<HotkeyManager>) -> Self {
        let keep_down = !manager.is_available();
        let latch = manager.stop_latch().clone();
        let clock = self.activity_clock.clone();
        install_controls(&mut self, latch, clock, manager);
        if keep_down {
            if let Some(server) = self.x_server.clone() {
                self.hotkey_manager.unregister(server.as_ref());
            } else {
                self.hotkey_manager.set_available(false);
            }
            self.recompute_capabilities();
        }
        self
    }

    /// Open a product X11 driver without attaching the listener.
    ///
    /// A missing AT-SPI bus or X server does not substitute a fake source.
    /// The caller binds the Host latch before the listener starts, so a hotkey
    /// during handshake cannot land on a latch that is about to be replaced.
    pub async fn open_x11_unbound() -> Self {
        let (connection, subscriber) = match connect_atspi().await {
            Ok(connection) => {
                let subscriber: Arc<dyn AtspiEventSubscriber> =
                    Arc::new(LiveEventSubscriber::new(connection.clone()));
                (Some(connection), Some(subscriber))
            }
            Err(error) => {
                tracing::warn!("AT-SPI unavailable; native X11 stays not ready: {error}");
                (None, None)
            }
        };
        let server = RealX::connect()
            .ok()
            .map(|server| Arc::new(server) as Arc<dyn XServer>);
        Self::new_internal(connection, subscriber, server, false)
    }

    /// X11 test driver with no AT-SPI connection and no fake accessibility source.
    pub fn offline_x11(server: Arc<dyn XServer>) -> Self {
        let mut driver = Self::new_internal(None, None, Some(server), false);
        driver.env = LinuxEnvironment::pure_x11_test();
        driver.recompute_capabilities();
        driver
    }
}

fn install_controls(
    driver: &mut LinuxDriver,
    latch: Arc<dyn StopLatch>,
    clock: Arc<dyn ActivityClock>,
    manager: Arc<HotkeyManager>,
) {
    if let Some(handle) = driver._listener_handle.take() {
        handle.abort();
    }
    if let Some(server) = driver.x_server.clone() {
        driver.hotkey_manager.unregister(server.as_ref());
    }
    driver.hotkey_manager = manager;
    driver.stop_latch = latch;
    driver.activity_clock = clock;
    if let Some(server) = driver.x_server.clone() {
        driver._listener_handle = attach_x11_listener(
            server,
            &driver.hotkey_manager,
            &driver.stop_latch,
            &driver.ledger,
            &driver.activity_clock,
        );
    }
    driver.recompute_capabilities();
}

/// Drop tree, screenshot, and coordinate input when no resident AT-SPI connection exists.
///
/// Wayland and the in-memory accessibility stand-in keep the shared capability
/// matrix. Coordinate input is not offered as a substitute for a missing tree.
pub(super) fn degrade_without_resident_tree(driver: &mut LinuxDriver) {
    let connected = driver
        .connection
        .as_ref()
        .is_some_and(|connection| !connection.is_closed());
    if driver.fake_source.is_some() || connected || driver.env.session_type.is_wayland() {
        return;
    }
    for capabilities in [
        &mut driver.caps_all,
        &mut driver.caps_no_guard,
        &mut driver.caps_no_hotkey,
        &mut driver.caps_none,
    ] {
        capabilities.observe.tree = false;
        capabilities.observe.events = false;
        capabilities.observe.screenshot = false;
        capabilities.delivery.accessibility = false;
        capabilities.delivery.foreground = ForegroundCapability::Unavailable;
    }
}
