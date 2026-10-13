//! Per-session serial queue and the cross-session window lock.
//!
//! One session runs calls one at a time. Callers may arrive together and wait
//! on the queue. Different sessions may run at once, but mutating actions on
//! the same window take a process-wide lock. The wait is
//! [`WINDOW_LOCK_WAIT`]. Past that the caller gets [`SessionError::Busy`].
//!
//! The window map is not garbage-collected. Stop does not use this queue.
//! The lock is not reentrant: `enqueue_window` must be the only acquisition
//! on the task, and it acquires the session queue first so one session cannot
//! deadlock itself.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use thiserror::Error;
use tokio::sync::OwnedMutexGuard;

/// How long two sessions will wait for the same window.
pub const WINDOW_LOCK_WAIT: Duration = Duration::from_secs(5);

/// Queue or window-lock failure, mapped to a protocol code by [`SessionError::code`].
#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum SessionError {
    /// The session is closed. The queued work was not started.
    #[error("session is closed")]
    Closed,
    /// The window lock was not free within [`WINDOW_LOCK_WAIT`].
    #[error("window is busy")]
    Busy,
}

impl SessionError {
    /// Protocol code a router should return.
    ///
    /// Blocking: none. Timeout: none. Cancellation: none. Thread safety: pure.
    #[must_use]
    pub fn code(self) -> gcu_protocol::ErrorCode {
        match self {
            Self::Closed => gcu_protocol::ErrorCode::ApprovalDenied,
            Self::Busy => gcu_protocol::ErrorCode::Busy,
        }
    }
}

/// Process-wide window locks. Clone shares the map.
#[derive(Clone, Default)]
pub struct WindowLocks {
    inner: Arc<Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>>,
}

impl WindowLocks {
    /// Empty map.
    ///
    /// Blocking: none. Timeout: none. Cancellation: none. Thread safety: the
    /// value is `Send + Sync`.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Acquire `window_id`, waiting up to [`WINDOW_LOCK_WAIT`].
    ///
    /// Blocking: waits until the lock is free or the deadline passes.
    /// Timeout: [`WINDOW_LOCK_WAIT`], then [`SessionError::Busy`].
    /// Cancellation: dropping the future gives up the wait and does not hold
    /// the lock. Thread safety: the map mutex is poisoned-recovered.
    pub async fn acquire(&self, window_id: &str) -> Result<WindowGuard, SessionError> {
        let mutex = {
            let mut map = lock(&self.inner);
            map.entry(window_id.to_string())
                .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
                .clone()
        };
        match tokio::time::timeout(WINDOW_LOCK_WAIT, mutex.lock_owned()).await {
            Ok(guard) => Ok(WindowGuard { _guard: guard }),
            Err(_elapsed) => Err(SessionError::Busy),
        }
    }
}

/// Held window lock. Drop releases it.
#[derive(Debug)]
pub struct WindowGuard {
    _guard: OwnedMutexGuard<()>,
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
