//! Foreground activation must be observed, including immediately before posts.

use super::{EventApi, InputTarget};
use gcu_core::driver::{DriverError, Result};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// Activate once, then confirm the actual frontmost process. An accepted
/// request is already a possible side effect, even if verification fails.
/// The enclosing watchdog bounds both calls; activation is never retried.
pub async fn activate_verified(
    api: &dyn EventApi,
    target: &InputTarget,
    sent: &AtomicBool,
) -> Result<()> {
    // The request can change focus even if its reply is lost or times out.
    sent.store(true, Ordering::Release);
    if let Err(error) = api.activate(target.pid).await {
        if !error.maybe_executed() {
            sent.store(false, Ordering::Release);
        }
        return Err(error);
    }
    let guards = api.guards().ok_or(DriverError::CapabilityUnavailable)?;
    let deadline = guards.clock.now() + Duration::from_millis(250);
    loop {
        guards.check(true).map_err(uncertain)?;
        if api.frontmost_pid().await.map_err(uncertain)? == Some(target.pid) {
            return Ok(());
        }
        if guards.clock.now() >= deadline {
            return Err(not_frontmost());
        }
        guards
            .clock
            .wait_until(deadline.min(guards.clock.now() + Duration::from_millis(10)))
            .await;
    }
}

/// Refuse input if activation was deferred, refused, or focus was stolen.
pub async fn verify(api: &dyn EventApi, pid: i32) -> Result<()> {
    if api.frontmost_pid().await.map_err(uncertain)? != Some(pid) {
        return Err(not_frontmost());
    }
    Ok(())
}

pub(crate) fn uncertain(error: DriverError) -> DriverError {
    match error {
        DriverError::Cancelled { .. } => DriverError::Cancelled {
            maybe_executed: true,
        },
        DriverError::Timeout { .. } => DriverError::Timeout {
            maybe_executed: true,
        },
        DriverError::Crashed { .. } => DriverError::Crashed {
            maybe_executed: true,
        },
        DriverError::Native { message, .. } => DriverError::Native {
            message,
            maybe_executed: true,
        },
        _ => DriverError::Native {
            message: "input safety changed after delivery began".into(),
            maybe_executed: true,
        },
    }
}

fn not_frontmost() -> DriverError {
    DriverError::Native {
        message: "target did not become or remain frontmost".into(),
        maybe_executed: true,
    }
}
