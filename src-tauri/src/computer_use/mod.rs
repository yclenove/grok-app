//! Computer Use host. The engine is a child process, not a linked driver.

pub(crate) mod approval;
pub(crate) mod browser;
mod codesign;
pub(crate) mod commands;
mod events;
mod gateway;
mod host_server;
pub(crate) mod inject;
pub(crate) mod lifecycle;
pub(crate) mod overlay;
mod panel;
mod policy_store;
mod supervisor;

use std::path::PathBuf;
use std::time::{Duration, Instant};

use gcu_host_protocol::approval::Office;
use gcu_host_protocol::lifecycle::Lifecycle;
use gcu_host_protocol::panel::{Panel, View};
use gcu_host_protocol::supervisor::{Error, Phase, Signal, Status};
use tauri::{AppHandle, Manager};
use tokio::sync::Mutex;

/// Settings key. The settings page that persists it is P5-08.
pub const SETTING_ENABLED: &str = "computer_use_enabled";
/// Developer external engine path. The settings page that persists it is P5-08.
pub const SETTING_EXTERNAL_ENGINE: &str = "computer_use_external_engine";

struct Inner {
    external: Option<PathBuf>,
    dev_grant: bool,
    stop_latched: bool,
    control:
        Option<gcu_host_protocol::supervisor::Control<gcu_host_protocol::supervisor::OsLaunch>>,
}

/// In-memory engine owner. Default is off. No settings row is registered here.
pub struct ComputerUseHost {
    inner: Mutex<Inner>,
    office: Office,
    shots: events::Shots,
    lifecycle: Lifecycle,
    panel: Panel,
    overlay: overlay::Overlay,
}

impl ComputerUseHost {
    fn disabled(office: Office, overlay: overlay::Overlay) -> Self {
        let shots = events::Shots::open();
        let lifecycle = Lifecycle::new(office.clone(), shots.gallery());
        Self {
            inner: Mutex::new(Inner {
                external: None,
                dev_grant: false,
                stop_latched: false,
                control: None,
            }),
            office,
            shots,
            lifecycle,
            panel: Panel::open(),
            overlay,
        }
    }

    pub(crate) fn panel(&self) -> Panel {
        self.panel.clone()
    }

    /// Timeline for one session. An unsafe id is `rejected`.
    pub(crate) fn timeline(&self, session_id: &str) -> Result<View, String> {
        self.panel
            .view(session_id)
            .ok_or_else(|| "rejected".to_owned())
    }

    /// Shared approval desk. Clone is cheap.
    pub(crate) fn office(&self) -> Office {
        self.office.clone()
    }

    pub(crate) fn overlay(&self) -> overlay::Overlay {
        self.overlay.clone()
    }

    /// Current phase. Stopped when the feature has never been turned on.
    pub async fn phase(&self) -> Phase {
        self.status().await.phase
    }

    /// Latch, session count, and phase.
    pub async fn status(&self) -> Status {
        let inner = self.inner.lock().await;
        let mut status = inner
            .control
            .as_ref()
            .map(gcu_host_protocol::supervisor::Control::status)
            .unwrap_or(Status {
                phase: Phase::Stopped,
                enabled: false,
                stop_latched: inner.stop_latched,
                sessions: 0,
            });
        status.stop_latched = self.overlay.safety.stopped();
        status
    }

    /// Start or stop by the feature switch. The second call in the same phase is a no-op.
    pub async fn set_enabled(&self, enabled: bool) -> Result<(), Error> {
        let mut inner = self.inner.lock().await;
        if !enabled {
            // Turning off a running host is a Stop and must survive restart. The
            // startup call on a host that never ran must not latch one.
            if inner.control.is_some() {
                self.overlay.safety.latch();
                self.overlay
                    .safety
                    .persist()
                    .await
                    .map_err(Error::Handshake)?;
            }
            self.office.deny_pending();
            gateway::revoke_all_locked(&mut inner, &self.lifecycle).await;
        }
        if enabled && inner.control.is_none() {
            let mut control = supervisor::open_control(
                inner.external.clone(),
                inner.dev_grant,
                self.office.clone(),
                self.shots.gallery(),
                self.panel.clone(),
                self.overlay.clone(),
                self.lifecycle.clone(),
            )?;
            if inner.stop_latched {
                control.handle(Signal::UserStop, now_ms()).await?;
            }
            inner.control = Some(control);
        }
        let Some(control) = inner.control.as_mut() else {
            return Ok(());
        };
        control.handle(Signal::SetEnabled(enabled), now_ms()).await
    }

    /// Enabled local sessions. Zero starts the idle clock.
    pub async fn set_sessions(&self, count: u32) -> Result<(), Error> {
        self.signal(Signal::Sessions(count)).await
    }

    /// Stop bypasses the supervisor mutex, ordinary RPC and pending approvals.
    pub async fn user_stop(&self) -> Result<(), Error> {
        self.overlay.safety.latch();
        self.overlay
            .set_stop_reason(Some(gcu_host_protocol::stop_reason::StopReason::Panel));
        self.office.deny_pending();
        self.panel.user_stop();
        self.overlay
            .safety
            .persist()
            .await
            .map_err(Error::Handshake)
    }

    /// Explicit trusted user resume on the independent channel only.
    pub async fn user_resume(&self) -> Result<(), Error> {
        self.overlay
            .safety
            .resume_by_user()
            .await
            .map_err(Error::Handshake)?;
        self.panel.user_resume();
        Ok(())
    }

    /// Record a developer engine. Refuses while a child is alive.
    ///
    /// P5-08 persists the path. Release mode still refuses it at admission.
    #[allow(dead_code)]
    pub async fn set_external_engine(
        &self,
        path: Option<PathBuf>,
        dev_grant: bool,
    ) -> Result<(), Error> {
        let mut inner = self.inner.lock().await;
        let busy = inner.control.as_ref().is_some_and(|control| {
            matches!(
                control.status().phase,
                Phase::Running | Phase::Starting | Phase::Stopping
            )
        });
        if busy {
            return Err(Error::Busy);
        }
        if let Some(control) = inner.control.as_ref() {
            inner.stop_latched = control.status().stop_latched;
        }
        inner.external = path;
        inner.dev_grant = dev_grant;
        inner.control = None;
        Ok(())
    }

    async fn signal(&self, signal: Signal) -> Result<(), Error> {
        let mut inner = self.inner.lock().await;
        let Some(control) = inner.control.as_mut() else {
            return Ok(());
        };
        control.handle(signal, now_ms()).await
    }

    async fn poll_forever(&self) {
        loop {
            tokio::time::sleep(Duration::from_millis(250)).await;
            let mut inner = self.inner.lock().await;
            if let Some(control) = inner.control.as_mut() {
                let before = control.status().phase;
                let _ = control.poll(now_ms()).await;
                let after = control.status().phase;
                if before == Phase::Running && after == Phase::Starting {
                    self.panel.note_restart();
                }
                if after == Phase::Tripped && before != Phase::Tripped {
                    self.panel.note_tripped();
                }
                if let Some(reason) = control.stop_display() {
                    self.overlay.set_stop_reason(Some(reason));
                }
            }
        }
    }
}

/// Register a disabled host and keep the idle/crash poll on the Tauri runtime.
pub fn install(app: &AppHandle) {
    tracing::debug!(
        enabled = SETTING_ENABLED,
        external = SETTING_EXTERNAL_ENGINE,
        "computer use host registered"
    );
    let office = policy_store::open_office(app);
    let overlay = overlay::Overlay::new(app);
    let host = ComputerUseHost::disabled(office.clone(), overlay);
    panel::forward_notices(app.clone(), &host.panel());
    app.manage(host);
    lifecycle::forward_closed(app);
    approval::forward_notices(app.clone(), &office);
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let host = app.state::<ComputerUseHost>();
        let _ = host.set_enabled(host.office().enabled()).await;
        let _ = host.phase().await;
        host.poll_forever().await;
    });
}

fn now_ms() -> u64 {
    static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    let elapsed = START.get_or_init(Instant::now).elapsed().as_millis();
    u64::try_from(elapsed).unwrap_or(u64::MAX)
}
