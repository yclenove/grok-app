//! Attach a proven Wayland ScreenCast source to [`LinuxDriver`].
//!
//! [`PortalInfo`] remains a probe. Capture, geometry, and the published
//! screenshot bit follow the bound source. Detecting GlobalShortcuts does not
//! register Stop.

use super::LinuxDriver;
use crate::wayland::{
    assembly::{self, FrameSource, ProvenCapture, WindowConsent},
    capture::{CaptureError, WaylandCapturePipeline},
    doctor::run_driver_doctor,
    input_control::StopControl,
    PortalChecker,
};
use gcu_core::{
    driver::{Cancel, Capture, DoctorReport, Driver, DriverError, Result, TreeOpts, WindowRef},
    model::{Geometry, WindowPoint},
};
use gcu_protocol::WindowInfo;
use std::sync::Arc;

impl LinuxDriver {
    /// Wayland driver with no listener until the Host latch is bound.
    ///
    /// A missing AT-SPI bus or portal does not substitute a fake source. An X
    /// server is opened only when the session reports XWayland, and only for
    /// windows that resolve to an X id.
    pub async fn open_wayland_unbound() -> Self {
        let env = crate::wayland::detect_environment(&crate::wayland::SystemEnv);
        let (connection, subscriber) = match crate::atspi::connect_atspi().await {
            Ok(connection) => {
                let subscriber: Arc<dyn crate::atspi::events::AtspiEventSubscriber> = Arc::new(
                    crate::atspi::events::LiveEventSubscriber::new(connection.clone()),
                );
                (Some(connection), Some(subscriber))
            }
            Err(error) => {
                tracing::warn!(
                    "AT-SPI unavailable; native Wayland observation stays limited: {error}"
                );
                (None, None)
            }
        };
        let server = if env.session_type.has_xwayland() {
            crate::x11::RealX::connect()
                .ok()
                .map(|server| Arc::new(server) as Arc<dyn crate::x11::XServer>)
        } else {
            None
        };
        let mut driver = Self::new_internal(connection, subscriber, server, false);
        driver.env = env;
        let portal = tokio::time::timeout(
            gcu_core::driver::DRIVER_DEADLINE,
            crate::wayland::LivePortalChecker::new().probe(),
        )
        .await
        .unwrap_or_default();
        driver.portal_info = portal;
        driver.recompute_capabilities();
        driver
    }

    /// In-memory Wayland driver. `server` is the verified XWayland connection, if any.
    #[must_use]
    pub fn offline_wayland(
        env: crate::wayland::LinuxEnvironment,
        server: Option<Arc<dyn crate::x11::XServer>>,
    ) -> Self {
        let mut driver = Self::new_internal(None, None, server, false);
        driver.env = env;
        driver.recompute_capabilities();
        driver
    }

    /// Record a Stop listener that has already registered. Interface probes cannot call this.
    #[must_use]
    pub fn bind_registered_stop(mut self, stop: Arc<StopControl>) -> Self {
        self.wayland_stop = Some(stop);
        self
    }

    /// Whether a registered Stop listener is still inside its lease.
    #[must_use]
    pub fn wayland_stop_registered(&self) -> bool {
        self.wayland_stop
            .as_ref()
            .is_some_and(|stop| stop.available())
    }

    /// Consent to one window, store the token, and use that stream for capture.
    pub async fn consent_screencast(
        &mut self,
        client: &dyn crate::wayland::PortalScreenCastClient,
        frames: &FrameSource,
        request: &WindowConsent,
        tokens: Arc<crate::wayland::RestoreTokenStore>,
    ) -> Result<()> {
        if !assembly::compositor_allows_native(self.env.compositor)
            || self.env.compositor != request.compositor
        {
            return Err(DriverError::CapabilityUnavailable);
        }
        let proven =
            assembly::consent_window(client, frames, request, tokens, self.stop_latch.as_ref())
                .await?;
        self.bound_capture = Some(Arc::new(proven));
        self.recompute_capabilities();
        Ok(())
    }

    /// Move the capture clock and refresh the published screenshot bit.
    pub fn advance_capture_clock(&mut self, now_secs: u64) {
        if let Some(bound) = &self.bound_capture {
            bound.set_now(now_secs);
        }
        self.recompute_capabilities();
    }

    /// Check the granted reis region. This does not send input.
    pub fn proven_reis_point(&self, win: &WindowRef, at: WindowPoint) -> Result<()> {
        if !self.wayland_stop_registered() {
            return Err(DriverError::CapabilityUnavailable);
        }
        let Some(bound) = &self.bound_capture else {
            return Err(DriverError::CapabilityUnavailable);
        };
        assembly::map_reis(bound, win, at)
    }

    /// Release input by closing the portal and deleting the token. Stop stays latched.
    pub async fn revoke_wayland_grant(&mut self) -> Result<()> {
        if let Some(bound) = &self.bound_capture {
            assembly::release_grant(bound).await?;
        }
        self.stop_latch.request_stop();
        if let Some(stop) = &self.wayland_stop {
            stop.listener_failed();
        }
        self.recompute_capabilities();
        Ok(())
    }

    /// Close the feature: release the portal, delete every token, and stay stopped.
    pub async fn close_wayland_feature(&mut self) -> Result<()> {
        if let Some(bound) = &self.bound_capture {
            assembly::clear_grant(bound).await?;
        }
        self.stop_latch.request_stop();
        self.recompute_capabilities();
        Ok(())
    }
}

pub(super) fn apply_capture_capability(driver: &mut LinuxDriver) {
    if !(driver.env.session_type.is_wayland() && driver.x_server.is_none()) {
        return;
    }
    let ready = driver
        .bound_capture
        .as_ref()
        .is_some_and(|bound| bound.admits());
    for capabilities in [
        &mut driver.caps_all,
        &mut driver.caps_no_guard,
        &mut driver.caps_no_hotkey,
        &mut driver.caps_none,
    ] {
        capabilities.observe.screenshot = ready;
    }
}

pub(super) fn bound_geometry(driver: &LinuxDriver, win: &WindowRef) -> Option<Geometry> {
    let bound = driver.bound_capture.as_ref()?;
    if bound.window_id == win.id && bound.admits() {
        Some(bound.geometry)
    } else {
        None
    }
}

pub(super) fn paint_frames(driver: &LinuxDriver, mut windows: Vec<WindowInfo>) -> Vec<WindowInfo> {
    if let Some(bound) = driver.bound_capture.as_ref().filter(|bound| bound.admits()) {
        for window in &mut windows {
            if window.id == bound.window_id {
                window.frame = bound.geometry.frame;
                window.scale = bound.geometry.scale;
            }
        }
    }
    windows
}

pub(super) async fn capture_native(
    driver: &LinuxDriver,
    win: &WindowRef,
    cancel: &Cancel,
) -> Result<Capture> {
    let Some(bound) = driver.bound_capture.as_ref() else {
        return Err(DriverError::CapabilityUnavailable);
    };
    if bound.window_id != win.id || !assembly::compositor_allows_native(driver.env.compositor) {
        return Err(DriverError::CapabilityUnavailable);
    }
    let stop_lost = driver.stop_latch.is_stopped()
        || driver
            .wayland_stop
            .as_ref()
            .is_some_and(|stop| !stop.available());
    if stop_lost {
        let _ = assembly::release_grant(bound).await;
        driver.stop_latch.request_stop();
        return Err(DriverError::CapabilityUnavailable);
    }
    if !bound.admits() {
        let expired = bound.now() >= bound.permit_expires_at_secs;
        let _ = assembly::release_grant(bound).await;
        driver.stop_latch.request_stop();
        return Err(if expired {
            DriverError::PermissionMissing {
                permission: "screencast".into(),
            }
        } else {
            DriverError::CapabilityUnavailable
        });
    }
    let tree = Driver::snapshot_tree(driver, win, &TreeOpts::default(), cancel).await?;
    if tree.incomplete.is_some() {
        return Err(DriverError::CapabilityUnavailable);
    }
    let shot = match WaylandCapturePipeline::capture_frame(
        bound.receiver(),
        &bound.stream,
        bound.permit_issued_at_secs,
        bound.permit_expires_at_secs,
        &tree,
        bound.now(),
    )
    .await
    {
        Ok(shot) => shot,
        Err(error) => {
            let mapped = map_capture(error);
            if !matches!(mapped, DriverError::Timeout { .. }) {
                let _ = assembly::release_grant(bound).await;
                driver.stop_latch.request_stop();
            }
            return Err(mapped);
        }
    };
    let Some((width, height)) = bound.stream.size else {
        let _ = assembly::release_grant(bound).await;
        driver.stop_latch.request_stop();
        return Err(DriverError::CapabilityUnavailable);
    };
    if shot.width != u32::try_from(width).unwrap_or(0)
        || shot.height != u32::try_from(height).unwrap_or(0)
    {
        let _ = assembly::release_grant(bound).await;
        driver.stop_latch.request_stop();
        return Err(DriverError::CapabilityUnavailable);
    }
    Ok(Capture {
        geometry: bound.geometry,
        ..shot
    })
}

fn map_capture(error: CaptureError) -> DriverError {
    match error {
        CaptureError::Timeout => DriverError::Timeout {
            maybe_executed: false,
        },
        CaptureError::CredentialsExpired => DriverError::PermissionMissing {
            permission: "screencast".into(),
        },
        CaptureError::Stream(_) => DriverError::Native {
            message: "screencast stream failed".into(),
            maybe_executed: false,
        },
        CaptureError::UnknownCaptureBounds
        | CaptureError::StaleFrame { .. }
        | CaptureError::CrossOriginFrame { .. }
        | CaptureError::HandoffRequired(_) => DriverError::CapabilityUnavailable,
    }
}

pub(super) async fn doctor(driver: &LinuxDriver) -> DoctorReport {
    let atspi_ready = driver.fake_source.is_some()
        || driver
            .connection
            .as_ref()
            .is_some_and(|connection| !connection.is_closed());
    if let Some(input) = driver
        .portal_input
        .as_ref()
        .filter(|input| input.available())
    {
        return crate::wayland::doctor::build_doctor_report(
            &driver.env,
            input.environment_capabilities(),
            &driver.portal_info,
            atspi_ready,
            None,
        );
    }
    let has_x = driver.x_server.is_some();
    let composite = if driver.fake_source.is_some() && !has_x {
        true
    } else {
        driver
            .x_server
            .as_ref()
            .and_then(|server| server.is_composite_available().ok())
            .unwrap_or(false)
    };
    let xtest = if driver.fake_source.is_some() && !has_x {
        true
    } else {
        driver
            .x_server
            .as_ref()
            .and_then(|server| server.is_xtest_available().ok())
            .unwrap_or(false)
    };
    let mut report = run_driver_doctor(
        &driver.env,
        &driver.portal_info,
        driver.capabilities(),
        composite,
        xtest,
        driver.hotkey_manager.is_available() && !driver.stop_latch.is_stopped(),
        driver.activity_clock.is_available(),
        driver.stop_latch.is_stopped(),
        atspi_ready,
        driver.hotkey_manager.last_error().as_deref(),
        has_x,
    );
    if driver.fake_source.is_none() && !atspi_ready && !driver.env.session_type.is_wayland() {
        report.issues.push(
            "Coordinate input is not a fallback; native X11 is not ready without a resident AT-SPI connection"
                .into(),
        );
        report.ready = false;
    }
    if driver.env.session_type.is_wayland() {
        report.issues.push(
            "status capsule is unavailable on Wayland, including GNOME; Stop is not a capsule button"
                .into(),
        );
        if !assembly::compositor_allows_native(driver.env.compositor) {
            report.issues.push(format!(
                "compositor {} is disabled for native Wayland capture and input",
                driver.env.compositor.slug()
            ));
        }
        if driver.portal_info.global_shortcuts_version.is_some()
            && !driver.wayland_stop_registered()
        {
            report
                .issues
                .push("GlobalShortcuts interface detection is not Stop registration".into());
        }
        if !has_x && !driver.capabilities().observe.screenshot {
            report.issues.push(
                "screencast source is not proven; capture stays unavailable until a single window is consented"
                    .into(),
            );
        }
        if !has_x && !assembly::pipewire_frames_compiled() {
            report.issues.push(
                "pipewire feature is not compiled; real PipeWire frames are unavailable".into(),
            );
        }
    }
    report
}

pub(super) fn retire_on_drop(bound: &ProvenCapture) {
    let session = bound.session.clone();
    let tokens = bound.tokens.clone();
    let key = bound.token_key.clone();
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        handle.spawn(async move {
            let _ = session.close().await;
            let _ = tokio::task::spawn_blocking(move || tokens.revoke(&key)).await;
        });
    }
}
