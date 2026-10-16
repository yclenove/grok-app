//! Compose compositor input confinement evidence with P7-04's exact OS identity checks.
use super::{
    input_control::InputAuthority,
    portal::PortalStream,
    windows::{StreamIdentity, WaylandWindows, WindowBinding},
};
use async_trait::async_trait;
use gcu_core::driver::{Cancel, DriverError, Result};
use std::sync::Arc;

/// Trusted compositor evidence for window-only input, including keyboard confinement.
/// Implementations must reject monitor/global scope, focus-only/title matching, and
/// unverified PipeWire serials. No portable portal API alone proves this property.
#[async_trait]
pub trait InputEvidence: Send + Sync {
    /// Read current exact source/session/node/serial evidence; caller bounds/cancels it.
    async fn stream(&self, session: &str, stream: &PortalStream) -> Result<StreamIdentity>;
    /// True only while the native confinement and physical-activity monitors are healthy.
    /// Source/focus loss, physical input or revocation must immediately return false.
    fn confined(&self) -> bool;
}

/// Production authority that revalidates the OS-inspected image/window before every action.
/// It never manufactures evidence from a portal selection's title, app ID or dimensions.
pub struct VerifiedInputAuthority {
    windows: Arc<WaylandWindows>,
    evidence: Arc<dyn InputEvidence>,
}
impl VerifiedInputAuthority {
    /// Assemble with the same P7-04 identity service used for the original authorization.
    pub fn new(windows: Arc<WaylandWindows>, evidence: Arc<dyn InputEvidence>) -> Self {
        Self { windows, evidence }
    }
}
#[async_trait]
impl InputAuthority for VerifiedInputAuthority {
    async fn prove(
        &self,
        session: &str,
        stream: &PortalStream,
        window: &WindowBinding,
    ) -> Result<StreamIdentity> {
        if !self.confined() {
            return Err(DriverError::CapabilityUnavailable);
        }
        let actual = self.evidence.stream(session, stream).await?;
        if actual.session != session
            || actual.node_id != stream.node_id
            || actual.window != *window
            || stream
                .pipewire_serial
                .is_some_and(|serial| serial != actual.serial)
        {
            return Err(DriverError::CapabilityUnavailable);
        }
        self.windows
            .check_stream(window, &actual, &actual, &Cancel::default())
            .await
            .map_err(|_| DriverError::CapabilityUnavailable)?;
        if !self.confined() {
            return Err(DriverError::CapabilityUnavailable);
        }
        Ok(actual)
    }
    fn confined(&self) -> bool {
        self.evidence.confined()
    }
}
