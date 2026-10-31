//! Data-plane latch. It stays closed until a scope ACK has been written.
//!
//! A request while closed is the fixed `ScopeDenied` label and does not count
//! as an effect. Revocation closes the latch again.

use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc,
};

#[derive(Clone, Debug)]
pub(crate) struct DataPlane {
    inner: Arc<Inner>,
}

#[derive(Debug)]
struct Inner {
    open: AtomicBool,
    effects: AtomicUsize,
}

impl DataPlane {
    /// Closed latch. Memory only. No timeout or cancellation.
    pub(crate) fn new() -> Self {
        Self {
            inner: Arc::new(Inner {
                open: AtomicBool::new(false),
                effects: AtomicUsize::new(0),
            }),
        }
    }

    /// Accept one data request, or refuse it without counting an effect.
    ///
    /// Memory only. No timeout or cancellation. The label is fixed.
    pub(crate) fn request(&self) -> Result<(), &'static str> {
        if !self.inner.open.load(Ordering::Acquire) {
            return Err("ScopeDenied");
        }
        self.inner.effects.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    /// Open after a flushed scope ACK. Memory only.
    pub(crate) fn open(&self) {
        self.inner.open.store(true, Ordering::Release);
    }

    /// Close on revoke. Later requests refuse. Memory only.
    pub(crate) fn close(&self) {
        self.inner.open.store(false, Ordering::Release);
    }

    /// Accepted requests. Refusals are not included.
    pub(crate) fn effects(&self) -> usize {
        self.inner.effects.load(Ordering::Relaxed)
    }

    /// Whether a flushed ACK has opened the latch and revoke has not closed it.
    ///
    /// Memory only. No timeout or cancellation.
    pub(crate) fn is_open(&self) -> bool {
        self.inner.open.load(Ordering::Acquire)
    }
}
