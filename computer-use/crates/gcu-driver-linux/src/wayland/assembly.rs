//! Product binding of one consented ScreenCast window.
//!
//! Probing `PortalInfo` does not create this value. A monitor source, an
//! unproven node, a missing mapping, or an unsupported compositor never does
//! either. Real PipeWire frames exist only when the `pipewire` feature built
//! this crate; the virtual receiver is a separate path.

use super::{
    capture::PipeWireFrameReceiver,
    ei::InputRegion,
    portal::{
        PortalCursorMode, PortalError, PortalScreenCastClient, PortalScreenCastSession,
        PortalSourceKind, PortalStream,
    },
    restore::{
        OriginBinding, RestoreError, RestoreRecord, RestoreTokenStore, MAX_TOKEN_LIFETIME_SECS,
    },
    Compositor,
};
use gcu_core::{
    driver::{DriverError, WindowRef},
    model::{Geometry, Rect, WindowPoint},
};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

/// Whether this build can open a real PipeWire receiver.
#[must_use]
pub fn pipewire_frames_compiled() -> bool {
    cfg!(all(target_os = "linux", feature = "pipewire"))
}

/// GNOME, KDE, and wlroots are the only native Wayland families this route enables.
#[must_use]
pub fn compositor_allows_native(compositor: Compositor) -> bool {
    matches!(
        compositor,
        Compositor::Gnome | Compositor::Kde | Compositor::Wlroots
    )
}

/// Frame supplier. Only [`Self::pipewire`] clears the virtual-path flag.
pub struct FrameSource {
    receiver: Arc<dyn PipeWireFrameReceiver>,
    virtual_receiver: bool,
}

impl FrameSource {
    /// In-memory frames. This is not a PipeWire device.
    #[must_use]
    pub fn virtual_frames(receiver: Arc<dyn PipeWireFrameReceiver>) -> Self {
        Self {
            receiver,
            virtual_receiver: true,
        }
    }

    /// Real PipeWire receiver from a portal FD. Compiled only with `pipewire`.
    #[cfg(all(target_os = "linux", feature = "pipewire"))]
    pub fn pipewire(fd: std::os::fd::OwnedFd, node_id: u32) -> Result<Self, DriverError> {
        let receiver = super::capture::RealPipeWireFrameReceiver::new(fd, node_id)
            .map_err(|_| DriverError::CapabilityUnavailable)?;
        Ok(Self {
            receiver: Arc::new(receiver),
            virtual_receiver: false,
        })
    }

    fn parts(&self) -> (&Arc<dyn PipeWireFrameReceiver>, bool) {
        (&self.receiver, self.virtual_receiver)
    }
}

/// Trusted consent inputs. None of these fields come from the model.
pub struct WindowConsent {
    /// Native window id the portal source must name.
    pub window_id: String,
    /// Compositor family. Other and unknown families are refused.
    pub compositor: Compositor,
    /// User, image, process, source, and authorization binding.
    pub origin: OriginBinding,
    /// Permit start, in seconds on the Host clock.
    pub permit_issued_at_secs: u64,
    /// Permit end. Must be within 24h and within the authorization deadline.
    pub permit_expires_at_secs: u64,
    /// Current Host time.
    pub now_secs: u64,
    /// Mapping id the granted stream must repeat.
    pub expected_mapping_id: String,
    /// Non-zero PipeWire serial the granted stream must repeat.
    pub expected_serial: u64,
    /// Restore-token key, scoped to this session and window.
    pub token_key: String,
}

/// One proven single-window source. Dropping the session closes the portal.
pub struct ProvenCapture {
    /// Window this source is allowed to paint.
    pub window_id: String,
    /// Validated window stream.
    pub stream: PortalStream,
    receiver: Arc<dyn PipeWireFrameReceiver>,
    /// Live portal session.
    pub session: Arc<dyn PortalScreenCastSession>,
    /// Permit start.
    pub permit_issued_at_secs: u64,
    /// Permit end, already capped at 24h and the authorization deadline.
    pub permit_expires_at_secs: u64,
    /// Binding stored with the restore token.
    pub origin: OriginBinding,
    /// Geometry taken from the granted stream size, not from a title or focus check.
    pub geometry: Geometry,
    virtual_receiver: bool,
    /// Compositor that was allowed at consent.
    pub compositor: Compositor,
    /// Token store that received the portal restore token.
    pub tokens: Arc<RestoreTokenStore>,
    /// Key passed to [`RestoreTokenStore`].
    pub token_key: String,
    now_secs: AtomicU64,
}

impl ProvenCapture {
    /// Host clock second stored for expiry checks.
    #[must_use]
    pub fn now(&self) -> u64 {
        self.now_secs.load(Ordering::Acquire)
    }

    /// Advance the Host clock. Past the permit, capture must retire the grant.
    pub fn set_now(&self, now_secs: u64) {
        self.now_secs.store(now_secs, Ordering::Release);
    }

    /// Frame receiver for the pipeline.
    #[must_use]
    pub fn receiver(&self) -> &dyn PipeWireFrameReceiver {
        self.receiver.as_ref()
    }

    /// True when frames come from the in-memory receiver.
    #[must_use]
    pub fn virtual_receiver(&self) -> bool {
        self.virtual_receiver
    }

    /// Source, session, compositor, feature, and permit all still hold.
    #[must_use]
    pub fn admits(&self) -> bool {
        let (width, height) = self.stream.size.unwrap_or((0, 0));
        self.session.is_active()
            && compositor_allows_native(self.compositor)
            && self.stream.source_type == PortalSourceKind::Window
            && self.stream.node_id != 0
            && width > 0
            && height > 0
            && self
                .stream
                .mapping_id
                .as_deref()
                .is_some_and(|id| !id.is_empty())
            && (self.virtual_receiver || pipewire_frames_compiled())
            && self.now() < self.permit_expires_at_secs
    }
}

/// Open one window ScreenCast, persist its token, and return the proven source.
///
/// A failure after the portal returns a session closes that session, deletes
/// the token, and latches `latch`. A refusal before a session exists does not.
pub async fn consent_window(
    client: &dyn PortalScreenCastClient,
    frames: &FrameSource,
    request: &WindowConsent,
    tokens: Arc<RestoreTokenStore>,
    latch: &dyn crate::x11::hotkey::StopLatch,
) -> Result<ProvenCapture, DriverError> {
    if !compositor_allows_native(request.compositor) {
        return Err(DriverError::CapabilityUnavailable);
    }
    let (receiver, virtual_receiver) = frames.parts();
    if !virtual_receiver && !pipewire_frames_compiled() {
        return Err(DriverError::CapabilityUnavailable);
    }
    validate_request(request)?;
    let restore = load_restore(
        tokens.clone(),
        &request.token_key,
        &request.origin,
        request.now_secs,
    )
    .await?;
    let opened = client
        .start_screencast(
            PortalSourceKind::Window,
            restore.as_deref(),
            PortalCursorMode::Hidden,
        )
        .await;
    let (session, result) = match opened {
        Ok(pair) => pair,
        Err(error) => return Err(portal_error(error)),
    };
    match accept_grant(
        session,
        result.stream,
        result.restore_token,
        receiver.clone(),
        virtual_receiver,
        request,
        tokens,
    )
    .await
    {
        Ok(proven) => Ok(proven),
        Err(error) => {
            latch.request_stop();
            Err(error)
        }
    }
}

fn validate_request(request: &WindowConsent) -> Result<(), DriverError> {
    let span = request
        .permit_expires_at_secs
        .saturating_sub(request.permit_issued_at_secs);
    if request.window_id.is_empty()
        || request.token_key.is_empty()
        || request.expected_mapping_id.is_empty()
        || request.expected_serial == 0
        || request.origin.source_kind != "window"
        || !request
            .origin
            .window_identity
            .starts_with(&request.window_id)
        || request.origin.compositor.is_empty()
        || request.origin.process_instance.is_empty()
        || request.origin.auth_version.is_empty()
        || request.now_secs < request.permit_issued_at_secs
        || request.now_secs >= request.permit_expires_at_secs
        || span == 0
        || span > MAX_TOKEN_LIFETIME_SECS
    {
        return Err(DriverError::CapabilityUnavailable);
    }
    Ok(())
}

async fn load_restore(
    tokens: Arc<RestoreTokenStore>,
    key: &str,
    origin: &OriginBinding,
    now_secs: u64,
) -> Result<Option<String>, DriverError> {
    let key = key.to_owned();
    let origin = origin.clone();
    let loaded =
        tokio::task::spawn_blocking(move || tokens.load_and_validate(&key, &origin, now_secs))
            .await
            .map_err(|_| DriverError::Crashed {
                maybe_executed: false,
            })?;
    match loaded {
        Ok(token) => Ok(Some(token)),
        Err(RestoreError::NotFound(_)) | Err(RestoreError::OriginMismatch(_)) => Ok(None),
        Err(RestoreError::Expired { .. } | RestoreError::AuthExpired { .. }) => {
            Err(DriverError::PermissionMissing {
                permission: "screencast".into(),
            })
        }
        Err(_) => Err(DriverError::CapabilityUnavailable),
    }
}

async fn reject_open_session(
    session: Arc<dyn PortalScreenCastSession>,
    tokens: Arc<RestoreTokenStore>,
    key: String,
    error: DriverError,
) -> DriverError {
    let _ = session.close().await;
    let _ = tokio::task::spawn_blocking(move || tokens.revoke(&key)).await;
    error
}

async fn accept_grant(
    session: Arc<dyn PortalScreenCastSession>,
    stream: PortalStream,
    restore_token: Option<String>,
    receiver: Arc<dyn PipeWireFrameReceiver>,
    virtual_receiver: bool,
    request: &WindowConsent,
    tokens: Arc<RestoreTokenStore>,
) -> Result<ProvenCapture, DriverError> {
    let Some((width, height)) = stream
        .size
        .filter(|(width, height)| *width > 0 && *height > 0)
    else {
        return Err(reject_open_session(
            session,
            tokens,
            request.token_key.clone(),
            DriverError::CapabilityUnavailable,
        )
        .await);
    };
    if stream.validate(PortalSourceKind::Window).is_err()
        || stream.source_type != PortalSourceKind::Window
        || stream.mapping_id.as_deref() != Some(request.expected_mapping_id.as_str())
        || stream.pipewire_serial != Some(request.expected_serial)
    {
        return Err(reject_open_session(
            session,
            tokens,
            request.token_key.clone(),
            DriverError::CapabilityUnavailable,
        )
        .await);
    }
    let geometry = Geometry {
        frame: Rect {
            x: 0.0,
            y: 0.0,
            w: f64::from(width),
            h: f64::from(height),
        },
        scale: 1.0,
    };
    if let Some(token) = restore_token {
        let record = RestoreRecord::new(
            token,
            request.origin.clone(),
            request.permit_issued_at_secs,
            request.permit_expires_at_secs,
        );
        if tokens.save(&request.token_key, &record).is_err() {
            return Err(reject_open_session(
                session,
                tokens,
                request.token_key.clone(),
                DriverError::Native {
                    message: "portal credential was not stored".into(),
                    maybe_executed: false,
                },
            )
            .await);
        }
    }
    Ok(ProvenCapture {
        window_id: request.window_id.clone(),
        stream,
        receiver,
        session,
        permit_issued_at_secs: request.permit_issued_at_secs,
        permit_expires_at_secs: request.permit_expires_at_secs,
        origin: request.origin.clone(),
        geometry,
        virtual_receiver,
        compositor: request.compositor,
        tokens,
        token_key: request.token_key.clone(),
        now_secs: AtomicU64::new(request.now_secs),
    })
}

fn portal_error(error: PortalError) -> DriverError {
    match error {
        PortalError::CancelledByUser => DriverError::Cancelled {
            maybe_executed: false,
        },
        PortalError::Dbus(_) => DriverError::PermissionMissing {
            permission: "screencast".into(),
        },
        PortalError::InvalidStreamCount(_)
        | PortalError::InvalidNodeId(_)
        | PortalError::SourceMismatch { .. }
        | PortalError::InvalidPipeWireSerial
        | PortalError::InvalidStreamDimensions(_, _)
        | PortalError::SessionClosed
        | PortalError::Other(_) => DriverError::CapabilityUnavailable,
    }
}

/// Map a window point through the granted EI region. No event is sent.
pub fn map_reis(
    bound: &ProvenCapture,
    win: &WindowRef,
    at: WindowPoint,
) -> Result<(), DriverError> {
    if bound.window_id != win.id || !bound.admits() {
        return Err(DriverError::CapabilityUnavailable);
    }
    let mapping = bound
        .stream
        .mapping_id
        .clone()
        .filter(|id| !id.is_empty())
        .ok_or(DriverError::CapabilityUnavailable)?;
    let (width, height) = bound
        .stream
        .size
        .ok_or(DriverError::CapabilityUnavailable)?;
    let region = InputRegion {
        mapping_id: mapping,
        x: 0,
        y: 0,
        width: u32::try_from(width).map_err(|_| DriverError::CapabilityUnavailable)?,
        height: u32::try_from(height).map_err(|_| DriverError::CapabilityUnavailable)?,
    };
    region.map((region.width, region.height), at).map(|_| ())
}

/// Close the portal session and delete the restore token.
pub async fn release_grant(bound: &ProvenCapture) -> Result<(), DriverError> {
    let _ = bound.session.close().await;
    let tokens = bound.tokens.clone();
    let key = bound.token_key.clone();
    tokio::task::spawn_blocking(move || tokens.revoke(&key))
        .await
        .map_err(|_| DriverError::Crashed {
            maybe_executed: false,
        })?
        .map_err(|_| DriverError::Native {
            message: "portal credential cleanup failed".into(),
            maybe_executed: false,
        })
}

/// Delete every token in the store after the session is closed.
pub async fn clear_grant(bound: &ProvenCapture) -> Result<(), DriverError> {
    let _ = bound.session.close().await;
    let tokens = bound.tokens.clone();
    tokio::task::spawn_blocking(move || tokens.clear_all())
        .await
        .map_err(|_| DriverError::Crashed {
            maybe_executed: false,
        })?
        .map(|_| ())
        .map_err(|_| DriverError::Native {
            message: "portal credential cleanup failed".into(),
            maybe_executed: false,
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wayland::{FakeFrameReceiver, FakePortalClient, OriginBinding};
    use crate::x11::hotkey::{SharedStopLatch, StopLatch};

    fn sample(now: u64) -> WindowConsent {
        WindowConsent {
            window_id: "wayland:1".into(),
            compositor: Compositor::Gnome,
            origin: OriginBinding {
                compositor: "gnome:epoch".into(),
                source_kind: "window".into(),
                window_identity: "wayland:1:7".into(),
                process_instance: "/usr/bin/app:10".into(),
                auth_version: "auth-1".into(),
            },
            permit_issued_at_secs: now,
            permit_expires_at_secs: now + 60,
            now_secs: now,
            expected_mapping_id: "mapping-42".into(),
            expected_serial: 1001,
            token_key: "win-1".into(),
        }
    }

    #[tokio::test]
    async fn non_virtual_frames_do_not_open_a_portal_without_pipewire() {
        if pipewire_frames_compiled() {
            return;
        }
        let dir = tempfile::TempDir::new().expect("temp dir");
        let tokens = Arc::new(RestoreTokenStore::new(dir.path().join("tokens")));
        let client = FakePortalClient::new();
        let frames = FrameSource {
            receiver: Arc::new(FakeFrameReceiver::new(42, 1_000)),
            virtual_receiver: false,
        };
        let latch = SharedStopLatch::default();
        let error = match consent_window(&client, &frames, &sample(1_000), tokens, &latch).await {
            Ok(_) => panic!("real frames require the pipewire feature"),
            Err(error) => error,
        };
        assert!(matches!(error, DriverError::CapabilityUnavailable));
        assert!(client.session_flags().is_empty());
        assert!(!latch.is_stopped());
    }
}
