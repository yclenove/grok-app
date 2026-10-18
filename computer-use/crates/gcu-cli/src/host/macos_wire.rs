//! Shared macOS Stop latch and activity adapter for the helper and the engine.
//!
//! The driver does not keep a second latch. `request_stop` only flips the Host
//! bit and wakes a nonblocking channel; disk persistence stays on another task.
//! Activity `None` is unavailable, never an idle guess.

use super::Activity;
#[cfg(target_os = "macos")]
use crate::exit::Failure;
#[cfg(target_os = "macos")]
use gcu_core::driver::Driver;
use gcu_driver_macos::input::hotkey::StopLatch;
use gcu_host_api::UserActivity;
use gcu_mcp::host::PhysicalActivity;
use gcu_protocol::capabilities::{
    BackgroundOperation, Capabilities, CapabilityLimits, DeliveryCapabilities,
    ForegroundCapability, ObserveCapabilities, Platform,
};
use std::collections::BTreeMap;
use std::sync::{Arc, Weak};

/// Exit-2 text for terminal `call`, `doctor`, and HTTP MCP, and for an engine
/// that is not an evidenced authenticated App subprocess.
pub(crate) const TERMINAL_NATIVE_REFUSAL: &str =
    "This entry does not assemble a native driver; use the helper or the App";

/// Short initialize note. Does not claim TCC on unproven OS versions.
pub(crate) fn engine_route_note() -> String {
    "Native route shares the App Stop latch. App TCC responsibility is claimed only for macOS 27 arm64."
        .into()
}

/// ADR 0006 measured the App subprocess chain on macOS 27 arm64 only.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TccEvidence {
    /// `NSProcessInfo` product major, or a value a test supplied.
    pub os_major: u32,
    /// This process is a macOS arm64 binary.
    pub aarch64: bool,
}

impl TccEvidence {
    /// True only for the measured responsibility chain.
    pub(crate) fn allows_claim(self) -> bool {
        self.aarch64 && self.os_major == 27
    }

    /// Evidence for this process. Other versions stay unclaimed.
    pub(crate) fn current() -> Self {
        Self {
            aarch64: cfg!(all(target_os = "macos", target_arch = "aarch64")),
            os_major: current_major(),
        }
    }
}

fn current_major() -> u32 {
    #[cfg(target_os = "macos")]
    {
        gcu_driver_macos::input::product_major()
    }
    #[cfg(not(target_os = "macos"))]
    {
        0
    }
}

/// Authenticated App child plus measured TCC evidence. Either miss refuses.
pub(crate) fn engine_assembly_allowed(
    authenticated: bool,
    evidence: TccEvidence,
) -> Result<(), &'static str> {
    if authenticated && evidence.allows_claim() {
        Ok(())
    } else {
        Err(TERMINAL_NATIVE_REFUSAL)
    }
}

/// Handshake snapshot before the latch is attached. Input stays off.
pub(crate) fn conservative_handshake() -> Capabilities {
    Capabilities {
        platform: Platform::Macos,
        version: "macOS; input withheld until the Host latch is attached".into(),
        observe: ObserveCapabilities {
            tree: false,
            screenshot: false,
            events: false,
        },
        delivery: DeliveryCapabilities {
            accessibility: false,
            targeted_pointer: false,
            targeted_keys: false,
            foreground: ForegroundCapability::Unavailable,
        },
        background_operation: BackgroundOperation::None,
        user_activity_guard: false,
        overlay_excluded_from_capture: false,
        private_apis: BTreeMap::new(),
        limits: CapabilityLimits {
            max_tree_nodes: 0,
            max_image_edge: 0,
            type_text_max: 0,
        },
    }
}

/// LocalHost persistent Stop, seen by the macOS driver.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) struct ActivityLatch {
    activity: Arc<Activity>,
    notify: tokio::sync::mpsc::Sender<()>,
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
impl ActivityLatch {
    /// `notify` is `try_send` only. A full channel still leaves the memory latch set.
    pub(crate) fn new(activity: Arc<Activity>, notify: tokio::sync::mpsc::Sender<()>) -> Self {
        Self { activity, notify }
    }
}

impl StopLatch for ActivityLatch {
    fn is_stopped(&self) -> bool {
        self.activity.latched_stop()
    }

    fn request_stop(&self) {
        self.activity.latch();
        let _ = self.notify.try_send(());
    }
}

/// Reads the same guards the driver uses for routing. Dropping the driver yields `None`.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) struct MacosActivity(Weak<gcu_driver_macos::MacosDriver>);

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
impl MacosActivity {
    pub(crate) fn new(driver: &Arc<gcu_driver_macos::MacosDriver>) -> Self {
        Self(Arc::downgrade(driver))
    }
}

impl PhysicalActivity for MacosActivity {
    fn ms_since_physical_input(&self) -> Option<u64> {
        self.0.upgrade()?.input_guards()?.ms_since_physical_input()
    }
}

/// Engine-global Stop. Session-scoped stops stay on the router.
#[cfg(target_os = "macos")]
pub(crate) struct MirrorLatch {
    stop: gcu_mcp::host::StopMirror,
    notify: tokio::sync::mpsc::Sender<gcu_host_protocol::stop_reason::StopReason>,
}

#[cfg(target_os = "macos")]
impl StopLatch for MirrorLatch {
    fn is_stopped(&self) -> bool {
        self.stop.globally_stopped()
    }

    fn request_stop(&self) {
        self.stop.latch(None);
        let _ = self
            .notify
            .try_send(gcu_host_protocol::stop_reason::StopReason::Hotkey);
    }
}

/// Build the real driver after App authentication. macOS only.
#[cfg(target_os = "macos")]
pub(crate) async fn attach_engine(
    stop: gcu_mcp::host::StopMirror,
    notify: tokio::sync::mpsc::Sender<gcu_host_protocol::stop_reason::StopReason>,
    idle_ms: u64,
) -> Result<(Arc<dyn Driver>, Arc<dyn PhysicalActivity>), Failure> {
    let latch = Arc::new(MirrorLatch { stop, notify });
    let driver = Arc::new(
        gcu_driver_macos::MacosDriver::with_stop_latch(
            latch,
            gcu_driver_macos::input::hotkey::HotkeyConfig::default(),
            idle_ms,
        )
        .await
        .map_err(|_| Failure::internal("Cannot start the macOS driver"))?,
    );
    let activity = Arc::new(MacosActivity::new(&driver));
    Ok((driver, activity))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::LocalHost;
    use gcu_driver_macos::input::activity::ActivityMonitor;
    use gcu_driver_macos::input::clock::FakeClock;
    use gcu_driver_macos::input::hotkey::Guards;
    use gcu_driver_macos::input::FakeEvents;
    use gcu_driver_macos::MacosDriver;
    use std::time::Duration;

    #[test]
    fn engine_gate_requires_authentication_and_macos_27_arm64() {
        let evidenced = TccEvidence {
            os_major: 27,
            aarch64: true,
        };
        assert!(engine_assembly_allowed(true, evidenced).is_ok());
        for evidence in [
            TccEvidence {
                os_major: 27,
                aarch64: false,
            },
            TccEvidence {
                os_major: 15,
                aarch64: true,
            },
            TccEvidence {
                os_major: 14,
                aarch64: true,
            },
            TccEvidence {
                os_major: 26,
                aarch64: true,
            },
        ] {
            let error = engine_assembly_allowed(true, evidence).unwrap_err();
            assert_eq!(error, TERMINAL_NATIVE_REFUSAL);
        }
        assert_eq!(
            engine_assembly_allowed(false, evidenced).unwrap_err(),
            TERMINAL_NATIVE_REFUSAL
        );
        let current = TccEvidence::current();
        let expect =
            cfg!(all(target_os = "macos", target_arch = "aarch64")) && current.os_major == 27;
        assert_eq!(current.allows_claim(), expect);
        let caps = conservative_handshake();
        assert!(!caps.user_activity_guard);
        assert!(!caps.delivery.targeted_pointer);
        assert!(!caps.observe.tree);
    }

    #[tokio::test]
    async fn injected_latch_and_activity_follow_the_host_and_fail_closed() {
        let directory = tempfile::TempDir::new().unwrap();
        let host = LocalHost::fixture(directory.path());
        let (tx, mut rx) = tokio::sync::mpsc::channel(1);
        let latch = Arc::new(ActivityLatch::new(host.activity.clone(), tx));
        let clock = Arc::new(FakeClock::new());
        let monitor = Arc::new(ActivityMonitor::new(clock.clone()));
        monitor.connected(true);
        monitor.heartbeat();
        monitor.observe(0);
        let guards = Arc::new(Guards::new(clock.clone(), monitor.clone(), latch, 3_000));
        let mut driver =
            MacosDriver::with_api(Arc::new(gcu_driver_macos::testing::FakeMacApi::default()))
                .await
                .unwrap();
        let events = Arc::new(FakeEvents::ready());
        events.set_guards(guards);
        driver.inject_events(events);
        let driver = Arc::new(driver);
        host.activity
            .set_physical(Arc::new(MacosActivity::new(&driver)));
        assert_eq!(host.activity.ms_since_physical_input(), Some(0));
        driver.input_guards().unwrap().request_stop();
        assert!(host.activity.latched_stop());
        assert!(driver.input_guards().unwrap().is_stopped());
        assert!(rx.try_recv().is_ok());
        clock.advance(Duration::from_millis(250));
        assert_eq!(host.activity.ms_since_physical_input(), None);
        assert!(host.activity.latched_stop());
        assert!(host.activity.resume().is_ok());
        assert!(!driver.input_guards().unwrap().is_stopped());
        assert!(!host.activity.latched_stop());
    }
}
