//! Action-owned proof window; uses the caller's existing Driver deadline.
use super::*;
use gcu_core::driver::Cancel;
use std::{future::Future, pin::Pin};

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum Phase {
    #[default]
    Idle,
    Proving,
    Injecting,
}
#[derive(Default)]
pub(super) struct Window {
    pub generation: u64,
    pub phase: Phase,
    pub refused: bool,
}
type Recovery = Box<dyn FnOnce() -> Pin<Box<dyn Future<Output = ()> + Send>> + Send>;

impl GuardEvents {
    pub(crate) fn injecting(&self) -> bool {
        self.window.lock().unwrap_or_else(|p| p.into_inner()).phase == Phase::Injecting
    }
    #[cfg(windows)]
    pub(crate) fn probes_permitted(&self) -> bool {
        !self.failed.load(Ordering::Acquire) && !self.stopped()
    }
    /// Native listener observes action generations without doing action work.
    #[cfg(windows)]
    pub(crate) fn probe_window(&self) -> (u64, bool) {
        let window = self.window.lock().unwrap_or_else(|p| p.into_inner());
        (
            window.generation,
            window.phase != Phase::Idle
                && !window.refused
                && !self.stopped()
                && !self.failed.load(Ordering::Acquire),
        )
    }
    /// A completed, invalid proof refuses preparation; only in-flight injection
    /// turns that same failure into a permanent Stop and independent release.
    pub(crate) fn refuse_proof(&self, reason: String) {
        let injecting = {
            let mut window = self.window.lock().unwrap_or_else(|p| p.into_inner());
            window.refused = true;
            window.phase == Phase::Injecting
        };
        if injecting {
            self.fail(reason);
        }
        self.changed.notify_one();
    }
    #[cfg(windows)]
    pub(crate) fn probe_dispatch(&self, pending: bool) {
        *self.probe_pending.lock().unwrap_or_else(|p| p.into_inner()) =
            pending.then(|| self.time.now());
    }
    fn end_window(&self, generation: u64) {
        let mut window = self.window.lock().unwrap_or_else(|p| p.into_inner());
        if window.generation == generation {
            window.phase = Phase::Idle;
            self.clock.discard_lease();
        }
    }
}
impl SafetyGuard {
    /// Request at most one preparation cycle after the caller's live target gate
    /// and before action packets/mutation RPC. All share the Driver deadline.
    pub(crate) async fn begin_foreground(
        self: &Arc<Self>,
        cancel: &Cancel,
        deadline: tokio::time::Instant,
    ) -> Result<ForegroundLease> {
        self.check(true)?;
        let serial = tokio::select! { biased;
            _ = cancel.cancelled() => return Err(DriverError::Cancelled { maybe_executed: false }),
            lock = tokio::time::timeout_at(deadline, self.events.serial.clone().lock_owned()) =>
                lock.map_err(|_| DriverError::CapabilityUnavailable)?,
        };
        self.check(true)?;
        if cancel.is_cancelled() || tokio::time::Instant::now() >= deadline {
            return Err(if cancel.is_cancelled() {
                DriverError::Cancelled {
                    maybe_executed: false,
                }
            } else {
                DriverError::CapabilityUnavailable
            });
        }
        let generation = {
            let mut window = self.events.window.lock().unwrap_or_else(|p| p.into_inner());
            window.generation = window.generation.wrapping_add(1);
            window.phase = Phase::Proving;
            window.refused = false;
            window.generation
        };
        let lease = ForegroundLease {
            guard: self.clone(),
            generation,
            serial: Some(serial),
            recovery: None,
            complete: false,
        };
        if !self.available() {
            // Portable listener drives the same callback boundary synchronously;
            // production proves on its existing native pump, with no new timer.
            let request = self
                .events
                .proof_request
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .clone();
            if let Some(request) = request {
                request();
            }
        }
        loop {
            // Register before checking to avoid losing a callback/failure wakeup.
            let changed = self.events.changed.notified();
            self.check(true)?;
            if cancel.is_cancelled() {
                return Err(DriverError::Cancelled {
                    maybe_executed: false,
                });
            }
            if self
                .events
                .window
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .refused
            {
                return Err(DriverError::CapabilityUnavailable);
            }
            if self.available() {
                self.events
                    .window
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .phase = Phase::Injecting;
                // Synchronous watchdog check closes the expiry race at admission.
                self.check(true)?;
                return Ok(lease);
            }
            tokio::select! { biased;
                _ = cancel.cancelled() => return Err(DriverError::Cancelled { maybe_executed: false }),
                result = tokio::time::timeout_at(deadline, changed) => {
                    if result.is_err() { return Err(DriverError::CapabilityUnavailable); }
                }
            }
        }
    }
    /// Inner checks never initiate another proof cycle.
    pub(crate) fn check_injection(&self) -> Result<()> {
        self.check(true)?;
        if !self.events.injecting() || !self.available() {
            return Err(DriverError::CapabilityUnavailable);
        }
        Ok(())
    }
}
/// Keeps renewal enabled until action completion and input recovery, including
/// abandonment. The serial lock remains owned through asynchronous cleanup.
pub(crate) struct ForegroundLease {
    guard: Arc<SafetyGuard>,
    generation: u64,
    serial: Option<tokio::sync::OwnedMutexGuard<()>>,
    recovery: Option<Recovery>,
    complete: bool,
}
impl ForegroundLease {
    pub(crate) fn recover_with(
        &mut self,
        recovery: impl FnOnce() -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + 'static,
    ) {
        self.recovery = Some(Box::new(recovery));
    }
    /// Caller has finished release_all_input and the action. Never clears Stop.
    pub(crate) fn finish(mut self) {
        self.guard.events.check_health();
        self.guard.events.end_window(self.generation);
        self.complete = true;
    }
}
impl Drop for ForegroundLease {
    fn drop(&mut self) {
        if self.complete {
            return;
        }
        if let Some(recovery) = self.recovery.take() {
            let guard = self.guard.clone();
            let generation = self.generation;
            let serial = self.serial.take();
            tokio::spawn(async move {
                let _serial = serial;
                recovery().await;
                guard.events.check_health();
                guard.events.end_window(generation);
            });
        } else {
            // Preparation failed/cancelled before any action input was admitted.
            self.guard.events.end_window(self.generation);
        }
    }
}
