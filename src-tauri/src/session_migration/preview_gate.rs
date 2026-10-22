//! Shared bounded permit for manual and batch preview materialization.

use std::sync::{Condvar, Mutex, OnceLock};

#[cfg(test)]
use std::sync::atomic::{AtomicUsize, Ordering};

static PERMITS: OnceLock<(Mutex<usize>, Condvar)> = OnceLock::new();

#[cfg(test)]
static TEST_MAX_ACTIVE: AtomicUsize = AtomicUsize::new(0);
#[cfg(test)]
static TEST_MANUAL_ACQUIRES: AtomicUsize = AtomicUsize::new(0);
#[cfg(test)]
static TEST_BATCH_ACQUIRES: AtomicUsize = AtomicUsize::new(0);

#[derive(Clone, Copy)]
pub(crate) enum PreviewOrigin {
    Manual,
    Batch,
}

pub(crate) struct PreviewPermit;

pub(crate) fn acquire(origin: PreviewOrigin) -> PreviewPermit {
    let (active, available) = PERMITS.get_or_init(|| (Mutex::new(0), Condvar::new()));
    let mut guard = active
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    while *guard >= super::budget::READ_CONCURRENCY {
        guard = available
            .wait(guard)
            .unwrap_or_else(|poisoned| poisoned.into_inner());
    }
    *guard += 1;
    #[cfg(test)]
    {
        TEST_MAX_ACTIVE.fetch_max(*guard, Ordering::SeqCst);
        match origin {
            PreviewOrigin::Manual => TEST_MANUAL_ACQUIRES.fetch_add(1, Ordering::SeqCst),
            PreviewOrigin::Batch => TEST_BATCH_ACQUIRES.fetch_add(1, Ordering::SeqCst),
        };
    }
    #[cfg(not(test))]
    let _ = origin;
    PreviewPermit
}

impl Drop for PreviewPermit {
    fn drop(&mut self) {
        let Some((active, available)) = PERMITS.get() else {
            return;
        };
        let mut guard = active
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *guard = guard.saturating_sub(1);
        available.notify_one();
    }
}

#[cfg(test)]
pub(crate) fn reset_metrics_for_test() {
    TEST_MAX_ACTIVE.store(0, Ordering::SeqCst);
    TEST_MANUAL_ACQUIRES.store(0, Ordering::SeqCst);
    TEST_BATCH_ACQUIRES.store(0, Ordering::SeqCst);
}

#[cfg(test)]
pub(crate) fn metrics_for_test() -> (usize, usize, usize) {
    (
        TEST_MAX_ACTIVE.load(Ordering::SeqCst),
        TEST_MANUAL_ACQUIRES.load(Ordering::SeqCst),
        TEST_BATCH_ACQUIRES.load(Ordering::SeqCst),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Barrier};

    #[test]
    fn globally_limits_parallel_preview_work() {
        let start = Arc::new(Barrier::new(21));
        let active = Arc::new(AtomicUsize::new(0));
        let max_active = Arc::new(AtomicUsize::new(0));
        let mut threads = Vec::new();

        for index in 0..20 {
            let start = Arc::clone(&start);
            let active = Arc::clone(&active);
            let max_active = Arc::clone(&max_active);
            threads.push(std::thread::spawn(move || {
                start.wait();
                let origin = if index % 2 == 0 {
                    PreviewOrigin::Manual
                } else {
                    PreviewOrigin::Batch
                };
                let _permit = acquire(origin);
                let now = active.fetch_add(1, Ordering::SeqCst) + 1;
                max_active.fetch_max(now, Ordering::SeqCst);
                std::thread::sleep(std::time::Duration::from_millis(10));
                active.fetch_sub(1, Ordering::SeqCst);
            }));
        }

        start.wait();
        for thread in threads {
            thread.join().unwrap();
        }
        assert!(max_active.load(Ordering::SeqCst) <= super::super::budget::READ_CONCURRENCY);
    }
}
