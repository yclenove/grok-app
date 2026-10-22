//! A read-only lifecycle witness; no runtime/store locks and no IO.
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

#[derive(Debug)]
pub(super) struct ConnectionLifetime(Arc<AtomicBool>);

impl Default for ConnectionLifetime {
    fn default() -> Self {
        Self(Arc::new(AtomicBool::new(true)))
    }
}

impl Clone for ConnectionLifetime {
    fn clone(&self) -> Self {
        // Cloning a pure FSM creates an independent lifecycle, never a second
        // owner whose destruction can cancel the original FSM.
        Self(Arc::new(AtomicBool::new(self.0.load(Ordering::Acquire))))
    }
}

impl ConnectionLifetime {
    pub(super) fn lease(&self) -> ConnectionLease {
        ConnectionLease(self.0.clone())
    }
}

impl Drop for ConnectionLifetime {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

#[derive(Clone)]
pub(crate) struct ConnectionLease(Arc<AtomicBool>);

impl ConnectionLease {
    pub(crate) fn check(&self) -> Result<(), String> {
        if self.0.load(Ordering::Acquire) {
            Ok(())
        } else {
            Err("connection publication was cancelled or superseded".into())
        }
    }

    pub(crate) fn cancel(&self) {
        self.0.store(false, Ordering::Release);
    }
}
