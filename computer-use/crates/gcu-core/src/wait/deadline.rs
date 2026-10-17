//! Monotonic `wait_for` horizon. Distinct from the 5s native-call ceiling.
//!
//! Architecture §5 caps each blocking driver call at 5s ([`crate::driver::NATIVE_TIMEOUT`]).
//! Architecture §11.1 caps this tool at 30s. A wait is a sequence of snapshots, each
//! still bounded by [`crate::driver::DRIVER_DEADLINE`]; this type never raises either
//! constant.

use gcu_protocol::ErrorCode;
use std::time::Duration;

/// Default horizon when the caller omits `timeout_ms`.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_millis(5_000);
/// Hard maximum. Values above this are refused.
pub const MAX_TIMEOUT: Duration = Duration::from_millis(30_000);
/// Poll interval when the driver has no subscription, or the subscription closes.
pub const POLL_INTERVAL: Duration = Duration::from_millis(150);
pub(crate) const CONTROL_INTERVAL: Duration = Duration::from_millis(10);

/// Absolute monotonic deadline on an injected [`crate::settle::Clock`].
///
/// Construction is pure. It does not sleep, read a driver, or change native timeouts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Deadline {
    start: Duration,
    end: Duration,
}

impl Deadline {
    /// Accept `1ms..=30s` measured from `now`.
    ///
    /// Blocking: none. Timeout: none. Cancellation: none. A longer request returns
    /// [`ErrorCode::CapabilityUnavailable`] and does not clamp upward.
    pub fn try_from_timeout(now: Duration, timeout: Duration) -> Result<Self, ErrorCode> {
        if timeout < Duration::from_millis(1) || timeout > MAX_TIMEOUT {
            return Err(ErrorCode::CapabilityUnavailable);
        }
        Ok(Self {
            start: now,
            end: now.saturating_add(timeout),
        })
    }

    pub(crate) fn end(self) -> Duration {
        self.end
    }

    /// Time left, or zero at the horizon. Nonblocking.
    pub fn remaining(self, now: Duration) -> Duration {
        self.end.saturating_sub(now)
    }

    /// Whether `now` has reached the horizon. Nonblocking.
    pub fn expired(self, now: Duration) -> bool {
        now >= self.end
    }

    pub(crate) fn elapsed(self, now: Duration) -> Duration {
        now.saturating_sub(self.start)
    }
}
