//! Read-only KWin script reports, authenticated against KWin's unique bus owner.
//!
//! The public scripting API does not prove whether `Window.pid` came from native
//! Wayland credentials or an X11 property. Reports are deliberately observation-only.

use super::{
    gnome::{decode_snapshot, CompositorPeer, WireWindow, PATH},
    windows::*,
    Compositor,
};
use crate::identity::inspection::INSPECTION_TIMEOUT;
use async_trait::async_trait;
use std::sync::{Arc, Mutex};
use tokio::time::Instant;

/// Injectable monotonic clock for script heartbeat expiry; never uses wall time.
pub trait ReportClock: Send + Sync {
    /// Time since an arbitrary stable origin, in milliseconds.
    fn now_ms(&self) -> u64;
}

/// Tokio monotonic time, compatible with paused-time tests.
pub struct TokioReportClock(Instant);
impl Default for TokioReportClock {
    fn default() -> Self {
        Self(Instant::now())
    }
}
impl ReportClock for TokioReportClock {
    fn now_ms(&self) -> u64 {
        self.0.elapsed().as_millis().min(u64::MAX as u128) as u64
    }
}

struct Report {
    snapshot: WindowSnapshot,
    received_ms: u64,
}

/// Authenticated D-Bus receiver exported on the engine's existing unique connection.
#[derive(Clone)]
pub struct KwinReports {
    owner: String,
    clock: Arc<dyn ReportClock>,
    report: Arc<Mutex<Option<Report>>>,
}

impl KwinReports {
    /// Create a receiver for a pinned KWin unique owner; no ambient service names.
    pub fn new(owner: String, clock: Arc<dyn ReportClock>) -> Result<Self, IdentityError> {
        if !owner.starts_with(':') {
            return Err(IdentityError::Handoff);
        }
        Ok(Self {
            owner,
            clock,
            report: Arc::new(Mutex::new(None)),
        })
    }

    /// Validate and retain a coherent report. Sender must come from the D-Bus header,
    /// never a script argument. Public to permit deterministic transport fakes.
    pub fn receive(
        &self,
        sender: &str,
        epoch: String,
        records: Vec<(String, u64, String, String, u32, bool)>,
    ) -> Result<(), IdentityError> {
        if sender != self.owner || records.iter().any(|r| r.5) {
            return Err(IdentityError::Handoff);
        }
        let snapshot = decode_snapshot(Compositor::Kde, sender, epoch, records)?;
        *self.report.lock().map_err(|_| IdentityError::Handoff)? = Some(Report {
            snapshot,
            received_ms: self.clock.now_ms(),
        });
        Ok(())
    }

    /// Read only fresh reports (two seconds); an expired script never grants identity.
    pub fn latest(&self) -> Result<WindowSnapshot, IdentityError> {
        let report = self.report.lock().map_err(|_| IdentityError::Handoff)?;
        let report = report.as_ref().ok_or(IdentityError::Handoff)?;
        let age = self
            .clock
            .now_ms()
            .checked_sub(report.received_ms)
            .ok_or(IdentityError::Handoff)?;
        if age >= 2000 {
            return Err(IdentityError::Handoff);
        }
        Ok(report.snapshot.clone())
    }
}

#[zbus::interface(name = "org.grok.ComputerUse.Windows1")]
impl KwinReports {
    /// Accept only the pinned KWin sender. No activation, policy mutation or input RPC.
    fn publish(
        &self,
        payload: String,
        #[zbus(header)] header: zbus::message::Header<'_>,
    ) -> zbus::fdo::Result<()> {
        let sender = header
            .sender()
            .ok_or_else(|| zbus::fdo::Error::AccessDenied("missing sender".into()))?;
        if sender.as_str() != self.owner || payload.len() > 8 * 1024 * 1024 {
            return Err(zbus::fdo::Error::AccessDenied("untrusted report".into()));
        }
        // KWin callDBus converts JS arrays to QVariantList (`av`), not typed D-Bus
        // structs. A single bounded JSON string is portable across Qt versions.
        let (version, epoch, records): (u32, String, Vec<WireWindow>) =
            serde_json::from_str(&payload)
                .map_err(|_| zbus::fdo::Error::InvalidArgs("malformed report".into()))?;
        if version != 1 {
            return Err(zbus::fdo::Error::NotSupported("version".into()));
        }
        self.receive(sender.as_str(), epoch, records)
            .map_err(|_| zbus::fdo::Error::AccessDenied("untrusted report".into()))
    }
}

/// KWin inventory backend. Activation is explicitly unsupported through scripting.
pub struct KwinBackend {
    peer: CompositorPeer,
    reports: KwinReports,
}

impl KwinBackend {
    /// Export the report receiver. The user must explicitly configure the installed
    /// script's `engineBusName` to this connection's unique name. Does not load scripts.
    /// Drop the future to cancel setup; a five-second deadline bounds D-Bus work.
    pub async fn install_receiver(
        peer: CompositorPeer,
        clock: Arc<dyn ReportClock>,
    ) -> Result<Self, IdentityError> {
        if !peer.is_kwin() {
            return Err(IdentityError::Unsupported);
        }
        tokio::time::timeout(INSPECTION_TIMEOUT, peer.check())
            .await
            .map_err(|_| IdentityError::Timeout {
                maybe_executed: false,
            })??;
        tokio::time::timeout(IDENTITY_TIMEOUT, async {
            let reports = KwinReports::new(peer.owner().into(), clock)?;
            let added = peer
                .connection()
                .ok_or(IdentityError::Unsupported)?
                .object_server()
                .at(PATH, reports.clone())
                .await
                .map_err(super::gnome::transport)?;
            if !added {
                return Err(IdentityError::Handoff);
            }
            Ok(Self { peer, reports })
        })
        .await
        .map_err(|_| IdentityError::Timeout {
            maybe_executed: false,
        })?
    }

    /// Remove the receiver on session end; reports cease immediately. Bounded to five seconds.
    pub async fn close(self) -> Result<(), IdentityError> {
        tokio::time::timeout(
            IDENTITY_TIMEOUT,
            self.peer
                .connection()
                .ok_or(IdentityError::Unsupported)?
                .object_server()
                .remove::<KwinReports, _>(PATH),
        )
        .await
        .map_err(|_| IdentityError::Timeout {
            maybe_executed: false,
        })?
        .map_err(super::gnome::transport)?;
        Ok(())
    }
}

#[async_trait]
impl WindowBackend for KwinBackend {
    fn compositor(&self) -> Compositor {
        Compositor::Kde
    }
    async fn snapshot(&self) -> Result<WindowSnapshot, IdentityError> {
        tokio::time::timeout(INSPECTION_TIMEOUT, async {
            self.peer.check().await?;
            self.reports.latest()
        })
        .await
        .map_err(|_| IdentityError::Timeout {
            maybe_executed: false,
        })?
    }
}
