//! Data-plane latch. A generation is admitted only after its ACK is flushed.
//!
//! Generation 0 is never admitted. A request while nothing is admitted is the
//! fixed `ScopeDenied` label and does not count as an effect. Rebind suspends
//! the latch before the new ACK is written. Revocation closes it again.

use std::sync::{
    atomic::{AtomicU64, AtomicUsize, Ordering},
    Arc,
};

#[derive(Clone, Debug)]
pub(crate) struct DataPlane {
    inner: Arc<Inner>,
}

#[derive(Debug)]
struct Inner {
    /// Admitted generation. Zero means no generation may serve data.
    admitted: AtomicU64,
    effects: AtomicUsize,
}

impl DataPlane {
    /// Closed latch. Memory only. No timeout or cancellation.
    pub(crate) fn new() -> Self {
        Self {
            inner: Arc::new(Inner {
                admitted: AtomicU64::new(0),
                effects: AtomicUsize::new(0),
            }),
        }
    }

    /// Accept one data request for the admitted generation.
    ///
    /// A closed latch refuses without counting an effect. Memory only. No
    /// timeout or cancellation. The label is fixed. Fresh connections go
    /// through `admit_data`, which also checks the generation.
    pub(crate) fn request(&self) -> Result<(), &'static str> {
        if self.inner.admitted.load(Ordering::Acquire) == 0 {
            return Err("ScopeDenied");
        }
        self.inner.effects.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    /// Admit `generation` after its ACK has been flushed and the peer matches.
    ///
    /// Generation 0 is ignored and the latch stays closed. Memory only. No
    /// timeout or cancellation.
    pub(crate) fn open_generation(&self, generation: u64) {
        if generation != 0 {
            self.inner.admitted.store(generation, Ordering::Release);
        }
    }

    /// Drop data authority for the rebind gap. The published lease generation
    /// stays where the state machine put it. Memory only.
    pub(crate) fn suspend(&self) {
        self.inner.admitted.store(0, Ordering::Release);
    }

    /// Close on revoke. Later requests refuse. Memory only.
    pub(crate) fn close(&self) {
        self.suspend();
    }

    /// True when `generation` is the one whose ACK has been flushed.
    ///
    /// Memory only. No timeout or cancellation. Generation 0 never matches.
    pub(crate) fn allows(&self, generation: u64) -> bool {
        generation != 0 && self.inner.admitted.load(Ordering::Acquire) == generation
    }

    /// Accepted requests. Refusals are not included.
    pub(crate) fn effects(&self) -> usize {
        self.inner.effects.load(Ordering::Relaxed)
    }

    /// Whether some generation is admitted and revoke has not closed it.
    ///
    /// Memory only. No timeout or cancellation.
    pub(crate) fn is_open(&self) -> bool {
        self.inner.admitted.load(Ordering::Acquire) != 0
    }
}
