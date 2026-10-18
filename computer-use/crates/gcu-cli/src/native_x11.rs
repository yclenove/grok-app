//! Linux X11 product assembly. Compiled only with `native-x11` on Linux.
//!
//! A missing AT-SPI bus or X server stays not ready. This path never builds a
//! fake driver, never widens the input route, and never elevates.

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

/// Assemble the selected X11 route and run the command.
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
    let driver = bind_local(LinuxDriver::open_x11_unbound().await, &host);
    crate::runtime::dispatch_bound(cli, Arc::new(driver), disclosure, host, None).await
}

async fn run_engine(
    disclosure: Disclosure,
    host_fd: Option<usize>,
    socket: Option<std::path::PathBuf>,
    token_fd: Option<usize>,
    stop_fd: usize,
) -> Result<u8, Failure> {
    let driver = LinuxDriver::open_x11_unbound().await;
    let session = crate::engine::connect_engine(
        &driver,
        &gcu_mcp::host::UnavailableActivity,
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
    use gcu_driver_linux::x11::{
        fake::FakeWindow, server::XEventKind, FakeX, XRootGeometry, XWindowGeometry,
    };
    use std::time::Duration;

    fn server() -> Arc<FakeX> {
        let server = Arc::new(FakeX::new(
            XRootGeometry {
                root_id: 1,
                width: 800,
                height: 600,
                scale: 1.0,
            },
            true,
        ));
        server.add_window(FakeWindow {
            id: 100,
            name: Some("win".into()),
            pid: Some(10),
            geometry: XWindowGeometry {
                x: 0,
                y: 0,
                width: 100,
                height: 80,
                border_width: 0,
                depth: 24,
            },
            is_mapped: true,
            image: None,
        });
        server.set_active_window(Some(100));
        server
    }

    #[tokio::test]
    async fn stop_through_host_latch_reaches_activity_and_shares_the_clock() {
        let directory = tempfile::TempDir::new().unwrap();
        let host = LocalHost::fixture(directory.path());
        let server = server();
        let driver = bind_local(LinuxDriver::offline_x11(server.clone()), &host);
        driver.activity_clock().record_physical_input();
        assert!(host.activity.ms_since_physical_input().is_some());
        assert!(!host.activity.latched_stop());

        let keycode = driver.hotkey_manager().resolved_keycode().unwrap();
        let modifiers = driver.hotkey_manager().config().modifiers;
        server.push_event(XEventKind::KeyPress(keycode, modifiers));
        let mut stopped = false;
        for _ in 0..50 {
            if host.activity.latched_stop() {
                stopped = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(stopped);
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
