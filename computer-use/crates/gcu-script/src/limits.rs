//! Sandbox ceilings from architecture §11.5. Pure counters; no IO.

use std::sync::Mutex;

/// UTF-8 source ceiling.
pub const CODE_MAX_BYTES: usize = 20_000;
/// QuickJS heap ceiling. Host `set_memory_limit` enforces this only when the
/// engine uses its own allocator.
pub const JS_HEAP_BYTES: usize = 32 * 1024 * 1024;
/// Bytes the host may retain from one driver reply, separate from the JS heap.
pub const HOST_BUFFER_BYTES: usize = 32 * 1024 * 1024;
/// Combined `emit` and return payload.
pub const OUTPUT_MAX_BYTES: usize = 2 * 1024 * 1024;
/// Native calls waiting behind the single in-flight call.
pub const QUEUE_MAX: usize = 32;
/// In-flight native `cu.*` calls.
pub const CONCURRENCY: usize = 1;
/// Driver calls per script. `emit` and local `find` do not count.
pub const DRIVER_CALL_MAX: u32 = 200;
/// `emit` calls per script.
pub const EMIT_MAX: u32 = 5;
/// Default and maximum script wall clock, in milliseconds.
pub const WALL_MS: u64 = 30_000;
/// Address-space ceiling the CLI launcher applies to the worker process.
pub const WORKER_AS_BYTES: u64 = 128 * 1024 * 1024;
/// Watchdog must invoke its kill within this many milliseconds after the deadline.
pub const WATCHDOG_KILL_MS: u64 = 500;
/// QuickJS native stack ceiling.
pub const JS_STACK_BYTES: usize = 256 * 1024;

/// Why a native call was admitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Admit {
    /// This caller holds the single in-flight slot.
    Run,
    /// This caller is waiting behind the in-flight call.
    Queued,
}

/// The queue is already full.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QueueFull;

/// Single-flight admission with a bounded waiter queue.
///
/// The gate itself never blocks. Callers wait outside it.
#[derive(Debug)]
pub struct CallGate {
    inflight: usize,
    queued: usize,
}

impl Default for CallGate {
    fn default() -> Self {
        Self::new()
    }
}

impl CallGate {
    /// Empty gate.
    ///
    /// Blocking: none. Timeout: none. Cancellation: none.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            inflight: 0,
            queued: 0,
        }
    }

    /// Take the in-flight slot, or a queue slot when one call is already running.
    ///
    /// The 33rd waiter is rejected. This does not block, sleep, or cancel.
    ///
    /// # Errors
    /// Returns [`QueueFull`] when [`CONCURRENCY`] and [`QUEUE_MAX`] are both taken.
    pub fn try_admit(&mut self) -> Result<Admit, QueueFull> {
        if self.inflight < CONCURRENCY {
            self.inflight += 1;
            Ok(Admit::Run)
        } else if self.queued < QUEUE_MAX {
            self.queued += 1;
            Ok(Admit::Queued)
        } else {
            Err(QueueFull)
        }
    }

    /// Release one in-flight call and promote the oldest queued waiter.
    ///
    /// Blocking: none. Timeout: none. Cancellation: none.
    pub fn release(&mut self) {
        if self.inflight > 0 {
            self.inflight -= 1;
        }
        if self.queued > 0 && self.inflight < CONCURRENCY {
            self.queued -= 1;
            self.inflight += 1;
        }
    }

    /// Calls holding the in-flight slot.
    ///
    /// Blocking: none. Timeout: none. Cancellation: none.
    #[must_use]
    pub fn inflight(&self) -> usize {
        self.inflight
    }

    /// Waiters that have not been promoted.
    ///
    /// Blocking: none. Timeout: none. Cancellation: none.
    #[must_use]
    pub fn queued(&self) -> usize {
        self.queued
    }
}

pub(crate) fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|err| err.into_inner())
}
