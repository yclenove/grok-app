//! Deadline watchdog that does not call into the JS engine.

use std::{
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::{Duration, Instant},
};

use crate::runtime::Wake;

/// Poll until `deadline` or `done`.
///
/// On the deadline this sets `cancel`, wakes blocked runtimes, then runs
/// `on_deadline`. The callback is the non-cooperative kill (process signal or
/// an in-process flag). It is not a QuickJS interrupt. The gap between the
/// deadline and the callback is one 10ms poll, which is inside the 500ms budget.
///
/// Blocking: this thread sleeps in 10ms slices until the deadline or `done`.
/// Timeout: the deadline itself. Cancellation: `done` makes the function return
/// without calling `on_deadline`.
pub(crate) fn supervise(
    deadline: Instant,
    done: &AtomicBool,
    cancel: &AtomicBool,
    wake: &Wake,
    on_deadline: impl FnOnce(),
) {
    loop {
        if done.load(Ordering::Acquire) {
            return;
        }
        if Instant::now() >= deadline {
            cancel.store(true, Ordering::Release);
            wake.wake();
            on_deadline();
            return;
        }
        wake.wake();
        thread::sleep(Duration::from_millis(10));
    }
}
