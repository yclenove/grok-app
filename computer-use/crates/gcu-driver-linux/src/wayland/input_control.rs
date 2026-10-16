//! Independent Stop health, authorization clock and source-proof boundary.

use super::{
    capabilities::{CapabilityItem, CapabilityStatus, EnvironmentCapabilities, TargetEnvironment},
    portal::{PortalScreenCastSession, PortalStream},
    windows::{StreamIdentity, WindowBinding},
};
use crate::x11::hotkey::StopLatch;
use async_trait::async_trait;
use gcu_core::driver::{Cancel, DriverError, Result};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use tokio::time::{Duration, Instant};

/// Watchdog cadence, below both Stop latch and release deadlines.
pub const WATCHDOG_TICK: Duration = Duration::from_millis(25);
/// A lost listener is detected within 75ms plus one watchdog tick.
pub const LISTENER_LEASE: Duration = Duration::from_millis(75);

/// Clock for authorization timestamps. Monotonic deadlines use Tokio's injectable clock.
pub trait InputClock: Send + Sync {
    /// Current authorization time in seconds, supplied by the trusted Host.
    fn now_secs(&self) -> u64;
}
/// Monotonic clock anchored to a trusted Host timestamp, without reading the wall clock.
pub struct AnchoredClock {
    epoch: u64,
    start: Instant,
}
impl AnchoredClock {
    /// Create at the Host's authorization time.
    pub fn new(epoch_secs: u64) -> Self {
        Self {
            epoch: epoch_secs,
            start: Instant::now(),
        }
    }
}
impl InputClock for AnchoredClock {
    fn now_secs(&self) -> u64 {
        self.epoch.saturating_add(self.start.elapsed().as_secs())
    }
}

/// A registered, desktop-wide Stop listener. This is a trusted assembly API, never a tool.
/// XGrabKey on XWayland and merely detecting GlobalShortcuts MUST NOT create this object.
/// The listener renews its lease from its independent event loop, never from an action.
pub struct StopControl {
    latch: Arc<dyn StopLatch>,
    healthy: AtomicBool,
    heartbeat: Mutex<Instant>,
    signal: Cancel,
}
impl StopControl {
    /// Use only after a native global Stop listener has successfully registered.
    /// The Host owns/persists the supplied latch; this type offers no resume operation.
    pub fn registered(latch: Arc<dyn StopLatch>) -> Arc<Self> {
        Arc::new(Self {
            latch,
            healthy: AtomicBool::new(true),
            heartbeat: Mutex::new(Instant::now()),
            signal: Cancel::default(),
        })
    }
    /// Renew from the independent, healthy listener, not the RPC/action worker.
    pub fn heartbeat(&self) {
        *self.heartbeat.lock().unwrap_or_else(|p| p.into_inner()) = Instant::now();
    }
    /// Latch synchronously, then wake the independent cleanup task.
    pub fn stop(&self) {
        self.latch.request_stop();
        self.signal.cancel();
    }
    /// Registration revoked, listener disconnected or permission lost: fail closed.
    pub fn listener_failed(&self) {
        self.healthy.store(false, Ordering::Release);
        self.stop();
    }
    /// Runtime health includes lease expiry and irreversible session Stop.
    pub fn available(&self) -> bool {
        self.healthy.load(Ordering::Acquire)
            && !self.latch.is_stopped()
            && !self.signal.is_cancelled()
            && self
                .heartbeat
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .elapsed()
                < LISTENER_LEASE
    }
    pub(crate) async fn stopped(&self) {
        self.signal.cancelled().await;
    }
}

/// Proof provider owned by trusted compositor/Host adapters, never model input.
/// prove must independently verify the live portal session/node/serial, exact source,
/// window/process/image lifetime (P7-04), and input confinement including keyboard focus.
/// A global keyboard with only a prechecked title/focus is NOT confined: return an error.
/// monitor failure, focus/scope changes and revocation must make confined() false immediately.
#[async_trait]
pub trait InputAuthority: Send + Sync {
    /// At most DRIVER_DEADLINE; caller can drop this future on cancellation or Stop.
    async fn prove(
        &self,
        session: &str,
        stream: &PortalStream,
        window: &WindowBinding,
    ) -> Result<StreamIdentity>;
    /// Nonblocking live confinement/physical-activity guard health check.
    fn confined(&self) -> bool;
}

/// Promote P7-01's native input table only from successfully registered runtime controls.
/// Portal interface/version detection alone cannot enable a route.
pub fn controlled_capabilities(
    mut caps: EnvironmentCapabilities,
    stop: &StopControl,
    authority: &dyn InputAuthority,
) -> EnvironmentCapabilities {
    if caps.target == TargetEnvironment::NativeWayland
        && matches!(
            caps.compositor,
            super::Compositor::Gnome | super::Compositor::Kde | super::Compositor::Wlroots
        )
        && stop.available()
        && authority.confined()
        && caps.screenshot.status.is_available()
    {
        let available = |reason: &str| CapabilityItem {
            status: CapabilityStatus::Available,
            reason: reason.into(),
            next_step: None,
        };
        caps.global_stop_hotkey =
            available("Registered desktop-wide Stop listener with independent watchdog");
        caps.user_activity_guard = available("Trusted compositor confines input to the authorized window and monitors physical activity");
        caps.foreground_input =
            available("User-approved RemoteDesktop session with stream-paired EI devices");
    }
    caps
}

/// Negotiated portal and EI resources; Drop of the native session also closes its portal.
pub struct RemoteDesktopConnection {
    /// Live session, including compositor Closed monitoring.
    pub session: Arc<dyn PortalScreenCastSession>,
    /// Exactly one requested window stream.
    pub stream: PortalStream,
    /// Rotated portal credential, persisted only after fresh source proof.
    pub restore_token: Option<String>,
    /// EI transport obtained exclusively through this session's ConnectToEIS FD.
    pub ei: Arc<dyn super::ei::EiTransport>,
}
/// Portal calls live behind this trait; fake implementations work on any platform.
#[async_trait]
pub trait RemoteDesktopPortal: Send + Sync {
    /// Request keyboard/pointer + single-window ScreenCast consent, with no monitor fallback.
    /// Bounds/cancellation are imposed by PortalInput and native session Drop cleanup.
    async fn open(&self, restore: Option<&str>) -> Result<RemoteDesktopConnection>;
}

pub(crate) fn unavailable() -> DriverError {
    DriverError::CapabilityUnavailable
}

/// Owned native registration; dropping it must fail the Stop lease and close its portal.
#[async_trait]
pub trait RegisteredStop: Send + Sync {
    /// Shared control used by PortalInput's capability gate and watchdog.
    fn control(&self) -> Arc<StopControl>;
    /// Explicitly retire the registration within DRIVER_DEADLINE, without clearing Stop.
    async fn close(&self) -> Result<()>;
}
/// Native Stop registration behind a trait, just like RemoteDesktop negotiation.
#[async_trait]
pub trait StopRegistrar: Send + Sync {
    /// Register after trusted user configuration, never from a model tool.
    /// description is localized by the Host. Refusal/conflict means input remains disabled.
    async fn register(
        &self,
        latch: Arc<dyn StopLatch>,
        description: String,
    ) -> Result<Arc<dyn RegisteredStop>>;
}
