//! Computer Use host. The engine is a child process, not a linked driver.

pub(crate) mod approval;
pub(crate) mod commands;
mod events;
mod gateway;
mod host_server;
pub(crate) mod inject;
mod policy_store;
mod supervisor;

use std::path::PathBuf;
use std::time::{Duration, Instant};

use gcu_host_protocol::approval::Office;
use gcu_host_protocol::inject::TokenBook;
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
    book: TokenBook,
}

/// In-memory engine owner. Default is off. No settings row is registered here.
pub struct ComputerUseHost {
    inner: Mutex<Inner>,
    office: Office,
    shots: events::Shots,
}

impl ComputerUseHost {
    fn disabled(office: Office) -> Self {
        Self {
            inner: Mutex::new(Inner {
                external: None,
                dev_grant: false,
                stop_latched: false,
                control: None,
                book: TokenBook::new(),
            }),
            office,
            shots: events::Shots::open(),
        }
    }

    /// Shared approval desk. Clone is cheap.
    pub(crate) fn office(&self) -> Office {
        self.office.clone()
    }

    /// Current phase. Stopped when the feature has never been turned on.
    pub async fn phase(&self) -> Phase {
        self.status().await.phase
    }

    /// Latch, session count, and phase.
    pub async fn status(&self) -> Status {
        let inner = self.inner.lock().await;
        inner
            .control
            .as_ref()
            .map(gcu_host_protocol::supervisor::Control::status)
            .unwrap_or(Status {
                phase: Phase::Stopped,
                enabled: false,
                stop_latched: inner.stop_latched,
                sessions: 0,
            })
    }

    /// Start or stop by the feature switch. The second call in the same phase is a no-op.
    pub async fn set_enabled(&self, enabled: bool) -> Result<(), Error> {
        let mut inner = self.inner.lock().await;
        if !enabled {
            self.office.deny_pending();
            let ids = inner.book.live_ids();
            gateway::revoke_all_locked(&mut inner).await;
            drop(inner);
            let shots = self.shots.clone();
            let _ = tauri::async_runtime::spawn_blocking(move || {
                for id in ids {
                    shots.drop_session(&id);
                }
            })
            .await;
            inner = self.inner.lock().await;
        }
        if enabled && inner.control.is_none() {
            let mut control = supervisor::open_control(
                inner.external.clone(),
                inner.dev_grant,
                self.office.clone(),
                self.shots.gallery(),
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

    /// User stop. Restart copies the latch into the next handshake.
    ///
    /// P5-06 calls this from the panel. The latch is already honored by spawn.
    #[allow(dead_code)]
    pub async fn user_stop(&self) -> Result<(), Error> {
        let mut inner = self.inner.lock().await;
        inner.stop_latched = true;
        if let Some(control) = inner.control.as_mut() {
            control.handle(Signal::UserStop, now_ms()).await?;
        }
        Ok(())
    }

    /// Explicit user resume. Crash recovery never calls this.
    ///
    /// P5-06 calls this from the panel.
    #[allow(dead_code)]
    pub async fn user_resume(&self) -> Result<(), Error> {
        let mut inner = self.inner.lock().await;
        inner.stop_latched = false;
        if let Some(control) = inner.control.as_mut() {
            control.handle(Signal::UserResume, now_ms()).await?;
        }
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
                let _ = control.poll(now_ms()).await;
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
    app.manage(ComputerUseHost::disabled(office.clone()));
    approval::forward_notices(app.clone(), &office);
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let host = app.state::<ComputerUseHost>();
        let _ = host.set_enabled(false).await;
        let _ = host.phase().await;
        host.poll_forever().await;
    });
}

fn now_ms() -> u64 {
    static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    let elapsed = START.get_or_init(Instant::now).elapsed().as_millis();
    u64::try_from(elapsed).unwrap_or(u64::MAX)
}
