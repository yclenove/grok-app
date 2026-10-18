//! Linux Wayland product assembly. Compiled only with `native-wayland` on Linux.
//!
//! Opening the driver probes the session. It does not start ScreenCast, does
//! not treat a GlobalShortcuts interface as Stop registration, and does not
//! build a fake driver. A missing portal or AT-SPI bus stays not ready.

use crate::args::{Cli, Command};
use crate::assemble::Disclosure;
use crate::exit::Failure;
use crate::host::LocalHost;
use gcu_driver_linux::x11::{ActivityClock, StopLatch};
use gcu_driver_linux::LinuxDriver;
use gcu_host_api::UserActivity;
use gcu_host_protocol::stop_reason::StopReason;
use gcu_mcp::host::{PhysicalActivity, StopMirror};
use std::sync::Arc;

/// Assemble the selected Wayland route and run the command.
pub(crate) async fn dispatch(
    cli: Cli,
    disclosure: Disclosure,
    mode: Option<String>,
) -> Result<u8, Failure> {
    if let Some(Command::Engine {
        host_fd,
        host_socket,
        host_token_fd,
        stop_fd,
    }) = cli.command
    {
        return run_engine(disclosure, host_fd, host_socket, host_token_fd, stop_fd).await;
    }
    let host = crate::runtime::open_local_host(mode).await?;
    let driver = bind_local(LinuxDriver::open_wayland_unbound().await, &host);
    crate::runtime::dispatch_bound(cli, Arc::new(driver), disclosure, host, None).await
}

async fn run_engine(
    disclosure: Disclosure,
    host_fd: Option<usize>,
    socket: Option<std::path::PathBuf>,
    token_fd: Option<usize>,
    stop_fd: usize,
) -> Result<u8, Failure> {
    let driver = LinuxDriver::open_wayland_unbound().await;
    let session = crate::engine::connect_engine(
        crate::engine::ConnectInput::Given(&driver, &gcu_mcp::host::UnavailableActivity),
        host_fd,
        socket,
        token_fd,
        stop_fd,
    )
    .await?;
    let latch = Arc::new(EngineStopLatch {
        stop: session.stop.clone(),
        notify: session.local_stop.clone(),
    });
    let driver = driver.with_stop_latch(latch);
    let activity = Arc::new(ClockBridge(driver.activity_clock().clone()));
    crate::engine::serve_engine(Arc::new(driver), activity, disclosure.note(), session).await
}

fn bind_local(driver: LinuxDriver, host: &LocalHost) -> LinuxDriver {
    let driver = driver.with_stop_latch(Arc::new(LocalStopLatch {
        activity: host.activity.clone(),
    }));
    host.activity
        .set_physical(Arc::new(ClockBridge(driver.activity_clock().clone())));
    driver
}

struct LocalStopLatch {
    activity: Arc<crate::host::Activity>,
}

impl StopLatch for LocalStopLatch {
    fn is_stopped(&self) -> bool {
        self.activity.latched_stop()
    }

    fn request_stop(&self) {
        self.activity.latch();
        let activity = self.activity.clone();
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                let _ = tokio::task::spawn_blocking(move || activity.persist_stop()).await;
            });
        }
    }
}

struct EngineStopLatch {
    stop: StopMirror,
    notify: tokio::sync::mpsc::Sender<StopReason>,
}

impl StopLatch for EngineStopLatch {
    fn is_stopped(&self) -> bool {
        self.stop.stopped("")
    }

    fn request_stop(&self) {
        self.stop.latch(None);
        let _ = self.notify.try_send(StopReason::Hotkey);
    }
}

struct ClockBridge(Arc<dyn ActivityClock>);

impl PhysicalActivity for ClockBridge {
    fn ms_since_physical_input(&self) -> Option<u64> {
        self.0.ms_since_physical_input()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gcu_driver_linux::{Compositor, LinuxEnvironment, LinuxSessionType};

    fn env() -> LinuxEnvironment {
        LinuxEnvironment {
            session_type: LinuxSessionType::WaylandPure,
            compositor: Compositor::Gnome,
            desktop_name: Some("GNOME".into()),
            wayland_display: Some("wayland-0".into()),
            x11_display: None,
        }
    }

    #[tokio::test]
    async fn stop_through_host_latch_reaches_activity_and_shares_the_clock() {
        let directory = tempfile::TempDir::new().unwrap();
        let host = LocalHost::fixture(directory.path());
        let mut driver = bind_local(LinuxDriver::offline_wayland(env(), None), &host);
        driver.activity_clock().set_available(true);
        driver.activity_clock().record_physical_input();
        assert!(host.activity.ms_since_physical_input().is_some());
        assert!(!host.activity.latched_stop());

        driver.revoke_wayland_grant().await.unwrap();
        assert!(host.activity.latched_stop());
        assert!(driver.is_stopped());
    }

    #[tokio::test]
    async fn engine_stop_latch_latches_the_mirror_and_queues_hotkey() {
        let stop = StopMirror::new(true);
        let (notify, mut inbox) = tokio::sync::mpsc::channel(1);
        let latched = EngineStopLatch {
            stop: stop.clone(),
            notify: notify.clone(),
        };
        assert!(latched.is_stopped());

        let open = StopMirror::new(false);
        let fresh = EngineStopLatch {
            stop: open.clone(),
            notify,
        };
        assert!(!fresh.is_stopped());
        fresh.request_stop();
        assert!(open.stopped(""));
        assert!(fresh.is_stopped());
        assert_eq!(inbox.try_recv().unwrap(), StopReason::Hotkey);
    }
}
