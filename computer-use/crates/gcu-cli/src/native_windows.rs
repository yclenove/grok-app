//! Windows product assembly. Compiled on every host; the real worker runs only
//! with `native-windows` on Windows.
//!
//! Self High/System, UIAccess, a secure desktop, or a non-interactive session
//! refuses before any worker. `monitored` is the only launch. Failure is
//! returned once: no fake driver, no `InProcess`, no lower-integrity restart.

#[cfg(any(test, all(windows, feature = "native-windows")))]
use crate::host::activity::WindowsActivity;
#[cfg(any(test, all(windows, feature = "native-windows")))]
use crate::host::LocalHost;
use gcu_core::driver::DriverError;
use gcu_driver_windows::gate::{check_engine, ProcessSecurity};
#[cfg(any(test, all(windows, feature = "native-windows")))]
use gcu_driver_windows::input::hotkey::StopLatch;
use gcu_driver_windows::WindowsDriver;
#[cfg(any(test, all(windows, feature = "native-windows")))]
use gcu_host_api::UserActivity;
#[cfg(any(test, all(windows, feature = "native-windows")))]
use gcu_host_protocol::stop_reason::StopReason;
#[cfg(any(test, all(windows, feature = "native-windows")))]
use gcu_mcp::host::StopMirror;
use gcu_protocol::capabilities::{
    BackgroundOperation, Capabilities, CapabilityLimits, DeliveryCapabilities,
    ForegroundCapability, ObserveCapabilities, Platform,
};
use std::collections::BTreeMap;
#[cfg(any(test, all(windows, feature = "native-windows")))]
use std::sync::Arc;

/// Handshake snapshot. Input stays closed until the Host latch is bound.
#[must_use]
pub(crate) fn conservative_handshake() -> Capabilities {
    Capabilities {
        platform: Platform::Windows,
        version: "Windows; input withheld until the Host latch is attached".into(),
        observe: ObserveCapabilities {
            tree: false,
            screenshot: false,
            events: false,
        },
        delivery: DeliveryCapabilities {
            accessibility: false,
            targeted_pointer: false,
            targeted_keys: false,
            foreground: ForegroundCapability::Unavailable,
        },
        background_operation: BackgroundOperation::None,
        user_activity_guard: false,
        overlay_excluded_from_capture: false,
        private_apis: BTreeMap::new(),
        limits: CapabilityLimits {
            max_tree_nodes: 0,
            max_image_edge: 0,
            type_text_max: 0,
        },
    }
}

/// Run the self gate, then exactly one monitored opener.
///
/// The opener is `WindowsDriver::monitored` in production. A test may supply a
/// failing opener. This function never calls `InProcess` and never builds a fake.
#[cfg_attr(not(all(windows, feature = "native-windows")), allow(dead_code))]
pub(crate) async fn open_monitored<F, Fut>(
    security: &ProcessSecurity,
    open: F,
) -> Result<WindowsDriver, DriverError>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<WindowsDriver, DriverError>>,
{
    check_engine(security)?;
    open().await
}

/// Install the same clock route B passes to the engine.
#[cfg(any(test, all(windows, feature = "native-windows")))]
fn bind_activity(driver: &Arc<WindowsDriver>, host: &LocalHost) {
    host.activity
        .set_physical(Arc::new(WindowsActivity::new(driver)));
}

#[cfg(any(test, all(windows, feature = "native-windows")))]
fn local_latch(host: &LocalHost) -> Arc<dyn StopLatch> {
    Arc::new(LocalStopLatch {
        activity: host.activity.clone(),
    })
}

#[cfg(any(test, all(windows, feature = "native-windows")))]
struct LocalStopLatch {
    activity: Arc<crate::host::Activity>,
}

#[cfg(any(test, all(windows, feature = "native-windows")))]
impl StopLatch for LocalStopLatch {
    fn is_stopped(&self) -> bool {
        self.activity.latched_stop()
    }

    fn request_stop(&self) {
        self.activity.latch();
        // The hook thread is not a Tokio worker. Persistence must not block it.
        let activity = self.activity.clone();
        let _ = std::thread::Builder::new()
            .name("gcu-stop-persist".into())
            .spawn(move || {
                let _ = activity.persist_stop();
            });
    }
}

#[cfg(any(test, all(windows, feature = "native-windows")))]
struct EngineStopLatch {
    stop: StopMirror,
    notify: tokio::sync::mpsc::Sender<StopReason>,
}

#[cfg(any(test, all(windows, feature = "native-windows")))]
impl StopLatch for EngineStopLatch {
    fn is_stopped(&self) -> bool {
        self.stop.stopped("")
    }

    fn request_stop(&self) {
        self.stop.latch(None);
        let _ = self.notify.try_send(StopReason::Hotkey);
    }
}

/// Assemble the selected Windows route and run the command.
#[cfg(all(windows, feature = "native-windows"))]
pub(crate) async fn dispatch(
    cli: crate::args::Cli,
    disclosure: crate::assemble::Disclosure,
    mode: Option<String>,
) -> Result<u8, crate::exit::Failure> {
    if let Some(crate::args::Command::Engine {
        host_fd,
        host_socket,
        host_token_fd,
        stop_fd,
    }) = cli.command
    {
        return run_engine(disclosure, host_fd, host_socket, host_token_fd, stop_fd).await;
    }
    let host = crate::runtime::open_local_host(mode).await?;
    let driver = Arc::new(production_driver(local_latch(&host)).await?);
    bind_activity(&driver, &host);
    crate::runtime::dispatch_bound(cli, driver, disclosure, host, None).await
}

#[cfg(all(windows, feature = "native-windows"))]
async fn run_engine(
    disclosure: crate::assemble::Disclosure,
    host_fd: Option<usize>,
    socket: Option<std::path::PathBuf>,
    token_fd: Option<usize>,
    stop_fd: usize,
) -> Result<u8, crate::exit::Failure> {
    let session = crate::engine::connect_engine(
        crate::engine::ConnectInput::Windows,
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
    let driver = match production_driver(latch).await {
        Ok(driver) => Arc::new(driver),
        Err(error) => {
            session.peer.close();
            session.stop_peer.close();
            return Err(error);
        }
    };
    let activity = Arc::new(WindowsActivity::new(&driver));
    crate::engine::serve_engine(driver, activity, disclosure.note(), session).await
}

#[cfg(all(windows, feature = "native-windows"))]
async fn production_driver(
    latch: Arc<dyn StopLatch>,
) -> Result<WindowsDriver, crate::exit::Failure> {
    let security = self_security().await?;
    let executable = std::env::current_exe()
        .map_err(|_| crate::exit::Failure::internal("Cannot resolve the gcu executable"))?;
    let config = gcu_driver_windows::worker::WorkerConfig::new(executable);
    let hotkey = gcu_driver_windows::input::hotkey::HotkeyConfig::default();
    // `monitored` installs the parent hooks, hotkey, up-only release, job, and
    // worker handshake. There is no second attempt and no in-process fallback.
    open_monitored(&security, || async move {
        WindowsDriver::monitored(config, hotkey, latch).await
    })
    .await
    .map_err(driver_failure)
}

#[cfg(all(windows, feature = "native-windows"))]
async fn self_security() -> Result<ProcessSecurity, crate::exit::Failure> {
    use gcu_driver_windows::api::WinApi;
    let joined = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        tokio::task::spawn_blocking(|| {
            let api = gcu_driver_windows::RealWinApi::new()?;
            api.engine_security()
        }),
    )
    .await
    .map_err(|_| crate::exit::Failure::internal("Windows security precheck timed out"))?
    .map_err(|_| crate::exit::Failure::internal("Windows security precheck failed"))?;
    joined.map_err(driver_failure)
}

#[cfg(all(windows, feature = "native-windows"))]
fn driver_failure(error: DriverError) -> crate::exit::Failure {
    crate::exit::Failure {
        code: crate::exit::error_exit_code(error.error_code()),
        message: error.to_string(),
    }
}

#[cfg(test)]
#[path = "native_windows_tests.rs"]
mod tests;
