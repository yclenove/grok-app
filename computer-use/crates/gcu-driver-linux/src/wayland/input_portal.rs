//! Real RemoteDesktop + single-window ScreenCast negotiation and Closed monitoring.
use super::{
    ei::ReisTransport,
    input_control::{RemoteDesktopConnection, RemoteDesktopPortal},
    portal::{PortalError, PortalScreenCastSession, PortalSourceKind, PortalStream},
};
use ashpd::desktop::{
    remote_desktop::{DeviceType, RemoteDesktop, SelectDevicesOptions},
    screencast::{CursorMode, Screencast, SelectSourcesOptions, SourceType},
    CreateSessionOptions, PersistMode, Session,
};
use async_trait::async_trait;
use futures_core::Stream;
use gcu_core::driver::{DriverError, Result, DRIVER_DEADLINE};
use std::{
    future::poll_fn,
    pin::Pin,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};

fn native_error() -> DriverError {
    DriverError::Native {
        message: "RemoteDesktop portal request failed".into(),
        maybe_executed: false,
    }
}

/// ashpd RemoteDesktop adapter. Does not prove source identity itself: PortalInput
/// always requires separate trusted InputAuthority evidence before any input or restore save.
pub struct AshpdRemoteDesktop;

struct NativeSession {
    session: Arc<Session<RemoteDesktop>>,
    handle: String,
    active: Arc<AtomicBool>,
    close_requested: AtomicBool,
    monitor: Mutex<Option<tokio::task::JoinHandle<()>>>,
}
impl std::fmt::Debug for NativeSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RemoteDesktopSession")
            .field("active", &self.active)
            .finish_non_exhaustive()
    }
}
#[async_trait]
impl PortalScreenCastSession for NativeSession {
    async fn close(&self) -> std::result::Result<(), PortalError> {
        self.active.store(false, Ordering::Release);
        self.close_requested.store(true, Ordering::Release);
        let result = tokio::time::timeout(DRIVER_DEADLINE, self.session.close()).await;
        if let Some(monitor) = self
            .monitor
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take()
        {
            monitor.abort();
        }
        match result {
            Ok(Ok(())) => Ok(()),
            _ => Err(PortalError::Other("RemoteDesktop close failed".into())),
        }
    }
    fn is_active(&self) -> bool {
        self.active.load(Ordering::Acquire)
    }
    fn handle(&self) -> &str {
        &self.handle
    }
}
impl Drop for NativeSession {
    fn drop(&mut self) {
        if let Some(monitor) = self
            .monitor
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take()
        {
            monitor.abort();
        }
        // Also covers a dropped SelectDevices/Start/ConnectToEIS future. No detached input.
        self.active.store(false, Ordering::Release);
        if !self.close_requested.swap(true, Ordering::AcqRel) {
            let session = self.session.clone();
            tokio::spawn(async move {
                if !matches!(
                    tokio::time::timeout(DRIVER_DEADLINE, session.close()).await,
                    Ok(Ok(()))
                ) {
                    tracing::warn!("RemoteDesktop portal cleanup failed");
                }
            });
        }
    }
}
#[async_trait]
impl RemoteDesktopPortal for AshpdRemoteDesktop {
    async fn open(&self, restore: Option<&str>) -> Result<RemoteDesktopConnection> {
        // A private D-Bus connection also retires portal requests if CreateSession is
        // cancelled before ashpd can return a session handle (no orphaned consent).
        let bus = zbus::Connection::session()
            .await
            .map_err(|_| native_error())?;
        let remote = RemoteDesktop::with_connection(bus.clone())
            .await
            .map_err(|_| native_error())?;
        if remote.version() < 2 {
            return Err(DriverError::CapabilityUnavailable);
        }
        let session = Arc::new(
            remote
                .create_session(CreateSessionOptions::default())
                .await
                .map_err(|_| native_error())?,
        );
        let handle = serde_json::to_string(session.as_ref())
            .map_err(|_| native_error())?
            .trim_matches('"')
            .to_owned();
        let wrapper = Arc::new(NativeSession {
            session: session.clone(),
            handle,
            active: Arc::new(AtomicBool::new(true)),
            close_requested: AtomicBool::new(false),
            monitor: Mutex::new(None),
        });
        // Subscribe before Start, so a revocation between consent and connection cannot be lost.
        let closed_session = session.clone();
        let active = wrapper.active.clone();
        let (ready, subscribed) = tokio::sync::oneshot::channel();
        let monitor = tokio::spawn(async move {
            match closed_session.receive_closed().await {
                Ok(closed) => {
                    if ready.send(Ok(())).is_err() {
                        return;
                    }
                    tokio::pin!(closed);
                    let _ = poll_fn(|cx| Pin::new(&mut closed).poll_next(cx)).await;
                }
                Err(_) => {
                    let _ = ready.send(Err(native_error()));
                }
            }
            active.store(false, Ordering::Release);
        });
        *wrapper.monitor.lock().unwrap_or_else(|p| p.into_inner()) = Some(monitor);
        subscribed.await.map_err(|_| native_error())??;
        remote
            .select_devices(
                &session,
                SelectDevicesOptions::default()
                    .set_devices(DeviceType::Keyboard | DeviceType::Pointer)
                    .set_persist_mode(PersistMode::ExplicitlyRevoked)
                    .set_restore_token(restore),
            )
            .await
            .map_err(|_| native_error())?
            .response()
            .map_err(|_| DriverError::PermissionMissing {
                permission: "remote_desktop".into(),
            })?;
        let cast = Screencast::with_connection(bus)
            .await
            .map_err(|_| native_error())?;
        cast.select_sources(
            &session,
            SelectSourcesOptions::default()
                .set_sources(ashpd::enumflags2::BitFlags::from_flag(SourceType::Window))
                .set_multiple(false)
                .set_cursor_mode(CursorMode::Hidden),
        )
        .await
        .map_err(|_| native_error())?
        .response()
        .map_err(|_| DriverError::PermissionMissing {
            permission: "screencast".into(),
        })?;
        let response = remote
            .start(&session, None, Default::default())
            .await
            .map_err(|_| native_error())?
            .response()
            .map_err(|_| DriverError::PermissionMissing {
                permission: "remote_desktop".into(),
            })?;
        if response.streams().len() != 1
            || !response
                .devices()
                .contains(DeviceType::Keyboard | DeviceType::Pointer)
        {
            return Err(DriverError::CapabilityUnavailable);
        }
        let granted = &response.streams()[0];
        if granted.source_type() != Some(SourceType::Window) {
            return Err(DriverError::CapabilityUnavailable);
        }
        let stream = PortalStream {
            node_id: granted.pipe_wire_node_id(),
            source_type: PortalSourceKind::Window,
            position: granted.position(),
            size: granted.size(),
            mapping_id: granted.mapping_id().map(str::to_owned),
            pipewire_serial: None,
        };
        stream
            .validate(PortalSourceKind::Window)
            .map_err(|_| DriverError::CapabilityUnavailable)?;
        let mapping = stream
            .mapping_id
            .clone()
            .filter(|s| !s.is_empty())
            .ok_or(DriverError::CapabilityUnavailable)?;
        let fd = remote
            .connect_to_eis(&session, Default::default())
            .await
            .map_err(|_| native_error())?;
        let ei = ReisTransport::connect(fd, mapping).await?;
        if !wrapper.is_active() {
            return Err(DriverError::CapabilityUnavailable);
        }
        Ok(RemoteDesktopConnection {
            session: wrapper,
            stream,
            restore_token: response.restore_token().map(str::to_owned),
            ei,
        })
    }
}
