//! Owned lifetime of the in-process fake worker.
//!
//! Stopping the worker is confirmed only by `join`. The handle is never
//! detached and never forgotten. When the join budget runs out, the handle is
//! parked on this process and the attempt is unconfirmed: the caller must not
//! treat that attempt, or anything after it, as evidence.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// How long a cancelled worker may take to finish before the attempt is unusable.
pub(crate) const JOIN_BUDGET: Duration = Duration::from_millis(500);

static PARKED: Mutex<Vec<JoinHandle<()>>> = Mutex::new(Vec::new());

/// Join handle that stays owned until the thread has stopped.
///
/// Drop sets the cancel flag before the join attempt, so a dropped `execute`
/// future asks the worker to stop at its next check.
pub(crate) struct OwnedWorker {
    handle: Option<JoinHandle<()>>,
    budget: Duration,
    cancel: Arc<AtomicBool>,
}

impl OwnedWorker {
    pub(crate) fn new(handle: JoinHandle<()>, budget: Duration, cancel: Arc<AtomicBool>) -> Self {
        Self {
            handle: Some(handle),
            budget,
            cancel,
        }
    }

    /// `true` when the thread has stopped. The handle stays in `self` until then,
    /// so cancelling this future cannot detach the thread.
    pub(crate) async fn confirm(&mut self) -> bool {
        let deadline = Instant::now() + self.budget;
        loop {
            if self.handle.is_none() {
                return true;
            }
            if self.handle.as_ref().is_some_and(JoinHandle::is_finished) {
                if let Some(handle) = self.handle.take() {
                    let _ = handle.join();
                }
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            let left = deadline.saturating_duration_since(Instant::now());
            let slice = Duration::from_millis(5).min(left);
            if slice.is_zero() {
                return false;
            }
            tokio::time::sleep(slice).await;
        }
    }

    /// Keep a still-running thread owned by this process.
    pub(crate) fn park(&mut self) {
        let Some(handle) = self.handle.take() else {
            return;
        };
        park(handle);
    }
}

impl Drop for OwnedWorker {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        let Some(handle) = self.handle.take() else {
            return;
        };
        if let Err(handle) = confirm_sync(handle, self.budget) {
            park(handle);
        }
    }
}

/// Budget for the next join. Tests may shorten it; production stays at 500ms.
pub(crate) fn join_budget() -> Duration {
    #[cfg(test)]
    {
        test_hook::configured_budget()
    }
    #[cfg(not(test))]
    {
        JOIN_BUDGET
    }
}

fn park(handle: JoinHandle<()>) {
    match PARKED.lock() {
        Ok(mut parked) => parked.push(handle),
        Err(poisoned) => poisoned.into_inner().push(handle),
    }
}

#[cfg(test)]
fn parked_len() -> usize {
    match PARKED.lock() {
        Ok(parked) => parked.len(),
        Err(poisoned) => poisoned.into_inner().len(),
    }
}

#[cfg(test)]
fn take_parked() -> Vec<JoinHandle<()>> {
    let mut parked = match PARKED.lock() {
        Ok(parked) => parked,
        Err(poisoned) => poisoned.into_inner(),
    };
    std::mem::take(&mut *parked)
}

fn confirm_sync(handle: JoinHandle<()>, budget: Duration) -> Result<(), JoinHandle<()>> {
    let deadline = Instant::now() + budget;
    loop {
        if handle.is_finished() {
            let _ = handle.join();
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(handle);
        }
        let left = deadline.saturating_duration_since(Instant::now());
        let slice = Duration::from_millis(5).min(left);
        if slice.is_zero() {
            return Err(handle);
        }
        std::thread::sleep(slice);
    }
}

#[cfg(test)]
pub(crate) mod test_hook {
    use super::{parked_len, take_parked, JOIN_BUDGET};
    use std::cell::{Cell, RefCell};
    use std::path::{Path, PathBuf};
    use std::sync::{Condvar, Mutex, MutexGuard};
    use std::time::Duration;

    struct Gate {
        armed: bool,
        arrived: bool,
        release: bool,
        late: Vec<PathBuf>,
    }

    static GATE: Mutex<Gate> = Mutex::new(Gate {
        armed: false,
        arrived: false,
        release: false,
        late: Vec::new(),
    });
    static CV: Condvar = Condvar::new();
    static ARRIVED: tokio::sync::Notify = tokio::sync::Notify::const_new();
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    thread_local! {
        static EXPIRE_ON_PAUSE: Cell<bool> = const { Cell::new(false) };
        static PAUSE_CALL: Cell<bool> = const { Cell::new(false) };
        static BUDGET: Cell<Option<Duration>> = const { Cell::new(None) };
        static CANCEL: RefCell<Option<std::sync::Arc<std::sync::atomic::AtomicBool>>> =
            const { RefCell::new(None) };
    }

    fn lock() -> MutexGuard<'static, Gate> {
        match GATE.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    fn wait(guard: MutexGuard<'static, Gate>) -> MutexGuard<'static, Gate> {
        match CV.wait(guard) {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    /// One test at a time may arm the pause. Hold the guard for the whole test.
    pub(crate) fn serialize() -> MutexGuard<'static, ()> {
        match TEST_LOCK.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    pub(crate) fn configured_budget() -> Duration {
        BUDGET.with(|cell| cell.get().unwrap_or(JOIN_BUDGET))
    }

    pub(crate) fn pause_this_call() -> bool {
        PAUSE_CALL.with(|cell| cell.get())
    }

    pub(crate) fn expire_when_paused() -> bool {
        EXPIRE_ON_PAUSE.with(|cell| cell.get())
    }

    pub(crate) fn publish_cancel(flag: std::sync::Arc<std::sync::atomic::AtomicBool>) {
        CANCEL.with(|slot| *slot.borrow_mut() = Some(flag));
    }

    pub(crate) fn cancel_is_set() -> bool {
        CANCEL.with(|slot| {
            slot.borrow()
                .as_ref()
                .is_some_and(|flag| flag.load(std::sync::atomic::Ordering::Relaxed))
        })
    }

    /// Block the worker after a cancel check that already succeeded.
    pub(crate) fn pause_after_cancel_check() {
        let mut gate = lock();
        if !gate.armed {
            return;
        }
        gate.arrived = true;
        drop(gate);
        ARRIVED.notify_one();
        let mut gate = lock();
        while gate.armed && !gate.release {
            gate = wait(gate);
        }
    }

    /// Resolves when the worker is parked between the cancel check and the write.
    pub(crate) async fn paused() {
        loop {
            if lock().arrived {
                return;
            }
            ARRIVED.notified().await;
        }
    }

    pub(crate) fn record_late_write(path: &Path) {
        lock().late.push(path.to_path_buf());
    }

    pub(crate) fn late_writes() -> Vec<PathBuf> {
        lock().late.clone()
    }

    pub(crate) fn parked_count() -> usize {
        parked_len()
    }

    pub(crate) fn join_parked() {
        for handle in take_parked() {
            let _ = handle.join();
        }
    }

    fn arm(expire: bool, budget: Duration) {
        {
            let mut gate = lock();
            gate.armed = true;
            gate.arrived = false;
            gate.release = false;
            gate.late.clear();
        }
        EXPIRE_ON_PAUSE.with(|cell| cell.set(expire));
        PAUSE_CALL.with(|cell| cell.set(true));
        BUDGET.with(|cell| cell.set(Some(budget)));
        CANCEL.with(|slot| *slot.borrow_mut() = None);
    }

    fn clear_locals() {
        EXPIRE_ON_PAUSE.with(|cell| cell.set(false));
        PAUSE_CALL.with(|cell| cell.set(false));
        BUDGET.with(|cell| cell.set(None));
        CANCEL.with(|slot| *slot.borrow_mut() = None);
    }

    fn release_worker() {
        let mut gate = lock();
        gate.release = true;
        gate.armed = false;
        CV.notify_all();
    }

    /// Pauses the worker before the probe write and treats that pause as the deadline.
    #[must_use]
    pub(crate) struct Pause {
        released: bool,
    }

    impl Pause {
        pub(crate) fn until_join_budget(budget: Duration) -> Self {
            arm(true, budget);
            Self { released: false }
        }

        /// Pause without expiring `execute`, so the caller can drop the future.
        pub(crate) fn until_dropped(budget: Duration) -> Self {
            arm(false, budget);
            Self { released: false }
        }

        pub(crate) fn release(&mut self) {
            if self.released {
                return;
            }
            self.released = true;
            release_worker();
        }
    }

    impl Drop for Pause {
        fn drop(&mut self) {
            self.release();
            clear_locals();
            join_parked();
        }
    }
}
