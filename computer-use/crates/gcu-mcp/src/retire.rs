//! Lease invalidation hooks for one store.
//!
//! `bind_lease` registers a [`CallbackSlot`] on that lease. Publish calls it
//! on the thread that revoked, rebound, or read an expired lease, after the
//! lease state lock is released. The callback records a [`Sweep`] ticket and
//! `try_lock`s the store. A busy mutex is left for the holder, which catches
//! up after it unlocks. [`Hold`] means this thread already owns that mutex:
//! same-thread `try_lock` is unspecified, so the ticket waits for [`Hold`]'s
//! drop. There is no watcher task and no retire thread.

use gcu_core::scope::ScopeLease;
use std::cell::RefCell;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// Strong handle for the callback a lease stores weakly.
///
/// Dropping the slot, or [`Self::clear`], drops the `Arc`. The lease's weak
/// handle then fails to upgrade. Memory only. No timeout or cancellation.
pub(crate) struct CallbackSlot {
    hook: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
}

impl CallbackSlot {
    pub(crate) fn new() -> Self {
        Self {
            hook: Mutex::new(None),
        }
    }

    /// Arm `callback` on `lease`, then drop the previous hook.
    ///
    /// The new weak handle is registered before the old `Arc` is dropped, so a
    /// publish cannot land in the gap. Blocking: the lease hook list. Timeout:
    /// none. Cancellation: none.
    pub(crate) fn arm(&self, lease: &ScopeLease, callback: Arc<dyn Fn() + Send + Sync>) {
        lease.on_invalidated(Arc::downgrade(&callback));
        *self.hook.lock().unwrap_or_else(|err| err.into_inner()) = Some(callback);
    }

    /// Drop the strong hook. An in-flight publish that already upgraded keeps
    /// running; a later publish does not. Memory only. No timeout or cancellation.
    pub(crate) fn clear(&self) {
        self.hook
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .take();
    }
}

/// This thread is inside the store lock for `id`.
///
/// The id is per store value, not a thread-global depth, so one thread can
/// hold the image cache and the file ledger at the same time.
pub(crate) struct Hold {
    id: u64,
}

impl Hold {
    /// Remember `id` until this value drops. Memory only. No timeout or cancellation.
    pub(crate) fn enter(id: u64) -> Self {
        HELD.with(|held| held.borrow_mut().push(id));
        Self { id }
    }

    /// Whether this thread already holds store `id`. Memory only.
    pub(crate) fn contains(id: u64) -> bool {
        HELD.with(|held| held.borrow().contains(&id))
    }
}

impl Drop for Hold {
    fn drop(&mut self) {
        HELD.with(|held| {
            let mut held = held.borrow_mut();
            if let Some(index) = held.iter().rposition(|slot| *slot == self.id) {
                held.remove(index);
            }
        });
    }
}

thread_local! {
    static HELD: RefCell<Vec<u64>> = const { RefCell::new(Vec::new()) };
}

/// Id for [`Hold`]. Unique for the life of the process. Memory only.
pub(crate) fn next_sweep_id() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

/// Tickets for a deferred store sweep.
///
/// [`Self::request`] runs before `try_lock`. When that fails, the holder has
/// not finished its unlock path yet, so the ticket is still visible there.
/// [`Self::mark`] runs under the store mutex after rows for `seen` are gone.
/// Tickets issued after `seen` stay open. Memory only. No timeout or
/// cancellation. The counters wrap with the process; a wrap is not handled.
pub(crate) struct Sweep {
    requested: AtomicU64,
    finished: AtomicU64,
}

impl Sweep {
    pub(crate) fn new() -> Self {
        Self {
            requested: AtomicU64::new(0),
            finished: AtomicU64::new(0),
        }
    }

    /// Note one sweep. Returns the ticket. Memory only.
    pub(crate) fn request(&self) -> u64 {
        self.requested
            .fetch_add(1, Ordering::SeqCst)
            .wrapping_add(1)
    }

    /// Latest ticket issued. Memory only.
    pub(crate) fn requested(&self) -> u64 {
        self.requested.load(Ordering::SeqCst)
    }

    /// Highest ticket a sweep has already applied. Memory only.
    pub(crate) fn finished(&self) -> u64 {
        self.finished.load(Ordering::SeqCst)
    }

    /// Record that every row invisible at `seen` has been removed.
    ///
    /// Does not move the mark backwards. Call under the store mutex, after
    /// the lease read. Memory only.
    pub(crate) fn mark(&self, seen: u64) {
        let mut current = self.finished.load(Ordering::SeqCst);
        while current < seen {
            match self.finished.compare_exchange_weak(
                current,
                seen,
                Ordering::SeqCst,
                Ordering::SeqCst,
            ) {
                Ok(_) => break,
                Err(observed) => {
                    std::hint::spin_loop();
                    current = observed;
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "retire_tests.rs"]
mod tests;
