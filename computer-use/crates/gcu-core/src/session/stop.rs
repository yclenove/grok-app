//! Stop latch timing and the native-call ceiling.
//!
//! Stop is not an action and it is not an approval. [`crate::session::Session::handle_stop`]
//! does not take the session queue. The latch is owned by [`gcu_host_api::UserActivity`].
//! Only [`gcu_host_api::UserActivity::clear_stop_by_user`] clears it. Ending a turn,
//! closing a session, and restarting the engine do not.

use async_trait::async_trait;
use std::future::Future;
use std::time::Duration;

/// How quickly a latched stop must be observed.
pub const STOP_LATCH_BUDGET: Duration = Duration::from_millis(100);
/// How quickly `release_all_input` must be issued after the latch is seen.
///
/// This is the time until the release future is created, not until a slow
/// native call returns.
pub const RELEASE_INPUT_BUDGET: Duration = Duration::from_millis(500);
/// Ceiling for one native call, including `release_all_input`.
///
/// Equal to [`crate::driver::NATIVE_TIMEOUT`].
pub const NATIVE_CALL_TIMEOUT: Duration = Duration::from_secs(5);

/// A native call exceeded [`NATIVE_CALL_TIMEOUT`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativeTimeout;

/// Driver capability the session uses to drop held keys and buttons.
///
/// Implementations must return when the future is dropped. The session wraps
/// the call in [`drive_native`].
#[async_trait]
pub trait InputRelease: Send + Sync {
    /// Release every key and button this engine is holding.
    ///
    /// Blocking: may wait on the driver. Timeout: callers apply
    /// [`NATIVE_CALL_TIMEOUT`]. Cancellation: dropping the future abandons the
    /// wait; the driver should still release if it can. Thread safety:
    /// `Send + Sync`.
    async fn release_all_input(&self);
}

/// When the latch was read and when the release future was created.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StopTimings {
    /// Time from the start of the stop handler until the latch was read.
    pub latch: Duration,
    /// Time from the start until `release_all_input` was issued.
    ///
    /// When the latch is clear this equals `latch` and nothing is issued.
    pub release_issued: Duration,
}

/// Run one native call with [`NATIVE_CALL_TIMEOUT`].
///
/// Blocking: waits for `work` or the timeout. Timeout: [`NATIVE_CALL_TIMEOUT`],
/// then [`NativeTimeout`]. The `work` future is dropped on timeout.
/// Cancellation: none beyond that timeout. Thread safety: `work` runs on the
/// caller's runtime.
pub async fn drive_native<T>(work: impl Future<Output = T>) -> Result<T, NativeTimeout> {
    match tokio::time::timeout(NATIVE_CALL_TIMEOUT, work).await {
        Ok(value) => Ok(value),
        Err(_elapsed) => Err(NativeTimeout),
    }
}
