//! Independent Stop release service, outside action and approval executors.
//!
//! Native posting futures have a driver deadline under 5s. Real blocking calls
//! run off the executor; a timed-out post is uncertain and never replayed.

use super::{hotkey::Guards, EventApi, InputLedger};
use gcu_core::driver::Cancel;
use std::{sync::Arc, time::Duration};

/// Maximum budget to dispatch all ledger releases after Stop.
pub const RELEASE_DEADLINE: Duration = Duration::from_millis(500);

/// Dispatch held-input releases after Stop, bounded by the injected clock.
/// Every up is polled independently, so one blocked release cannot hold the
/// remaining keys. Dropping the future cancels waiting, never replays input.
pub async fn release_stopped(api: &dyn EventApi, ledger: &InputLedger, guards: &Guards) {
    if !guards.is_stopped() {
        return;
    }
    let deadline = guards.clock.now() + RELEASE_DEADLINE;
    tokio::select! {
        biased;
        _ = super::release_held(api, ledger) => {},
        _ = guards.clock.wait_until(deadline) => {
            tracing::warn!("macos Stop release deadline elapsed");
        },
    }
}

/// Owns an independent current-thread executor for releases and listener health.
/// Drop requests shutdown without waiting for a potentially blocked OS call.
pub struct GuardRuntime {
    shutdown: Cancel,
}

impl GuardRuntime {
    /// Start after connecting the backend. Failure leaves foreground disabled.
    /// The service terminates on driver Drop without clearing the Host latch.
    pub fn start(
        api: &Arc<dyn EventApi>,
        ledger: Arc<InputLedger>,
        guards: Arc<Guards>,
    ) -> std::io::Result<Self> {
        let shutdown = Cancel::default();
        let quit = shutdown.clone();
        let api = api.clone();
        std::thread::Builder::new()
            .name("gcu-mac-stop".into())
            .spawn(move || {
                let runtime = match tokio::runtime::Builder::new_current_thread()
                    .enable_time()
                    .build()
                {
                    Ok(runtime) => runtime,
                    Err(_) => {
                        guards.request_stop();
                        return;
                    }
                };
                runtime.block_on(supervise(api, ledger, guards, &quit));
                // Blocking FFI may outlive its async deadline. Never wait for it
                // while stopping the driver; no further input is queued to it.
                runtime.shutdown_background();
            })?;
        Ok(Self { shutdown })
    }
}

/// Run the independent watchdog using injected time. Production gives it its
/// own executor; tests drive this same loop with a FakeClock. Shutdown cancels
/// waits without clearing the Host Stop latch. No action queue is consulted.
pub async fn supervise(
    api: Arc<dyn EventApi>,
    ledger: Arc<InputLedger>,
    guards: Arc<Guards>,
    quit: &Cancel,
) {
    let mut listener_was_ready = false;
    while !quit.is_cancelled() {
        let ready = guards.stop_available();
        if listener_was_ready && !ready {
            guards.request_stop();
        }
        listener_was_ready = ready;
        if guards.is_stopped() && !ledger.is_empty() {
            release_stopped(api.as_ref(), &ledger, &guards).await;
        }
        let deadline = guards.clock.now() + Duration::from_millis(10);
        tokio::select! {
            biased;
            _ = quit.cancelled() => return,
            _ = guards.stopped(), if !guards.is_stopped() => {},
            _ = guards.clock.wait_until(deadline) => {},
        }
    }
}

impl Drop for GuardRuntime {
    fn drop(&mut self) {
        self.shutdown.cancel();
    }
}
