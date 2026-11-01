//! Callbacks run on the publisher's thread, outside the lease lock.
//!
//! `callback_reads_the_lease_without_deadlock` aborts if `publish` calls the
//! hook while the state lock is held: `current` needs that lock. Removing the
//! `fire` call leaves the counter at zero. A dropped hook must not run.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::ScopeLease;
use crate::scope::{Clock, FixtureScope};
use std::panic::AssertUnwindSafe;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::thread;
use std::time::Duration;

fn scope(generation: u64) -> FixtureScope {
    FixtureScope::try_new("run", generation, "engine", "desktop", "control", vec![]).unwrap()
}

fn lease() -> ScopeLease {
    ScopeLease::install(scope(1))
}

fn hook(body: impl Fn() + Send + Sync + 'static) -> Arc<dyn Fn() + Send + Sync> {
    Arc::new(body)
}

fn watch(bound: Duration) -> Watchdog {
    let (tx, rx) = std::sync::mpsc::channel();
    let thread = thread::spawn(move || {
        if rx.recv_timeout(bound).is_err() {
            eprintln!("lease callback watchdog: blocked past {bound:?}");
            std::process::abort();
        }
    });
    Watchdog {
        tx: Some(tx),
        thread: Some(thread),
    }
}

struct Watchdog {
    tx: Option<std::sync::mpsc::Sender<()>>,
    thread: Option<thread::JoinHandle<()>>,
}

impl Drop for Watchdog {
    fn drop(&mut self) {
        if let Some(tx) = self.tx.take() {
            let _ = tx.send(());
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[derive(Debug)]
struct Manual(AtomicU64);

impl Clock for Manual {
    fn now(&self) -> Duration {
        Duration::from_secs(self.0.load(Ordering::SeqCst))
    }
}

#[test]
fn publish_runs_live_callbacks_outside_the_lock() {
    let _watch = watch(Duration::from_secs(8));
    let bound = lease();
    let hits = Arc::new(AtomicUsize::new(0));
    let seen = Arc::new(Mutex::new(None));
    let hits_hook = Arc::clone(&hits);
    let seen_hook = Arc::clone(&seen);
    let bound_hook = bound.clone();
    let callback = hook(move || {
        // Fails closed (same-thread deadlock, then the watchdog) if publish
        // still holds the state lock.
        *seen_hook.lock().unwrap_or_else(|err| err.into_inner()) = bound_hook.current();
        hits_hook.fetch_add(1, Ordering::SeqCst);
    });
    bound.on_invalidated(Arc::downgrade(&callback));
    bound.revoke();
    assert_eq!(hits.load(Ordering::SeqCst), 1);
    assert!(seen.lock().unwrap().is_none());
    assert!(bound.current().is_none());
}

#[test]
fn dropped_callback_is_not_called_and_does_not_panic() {
    let bound = lease();
    let hits = Arc::new(AtomicUsize::new(0));
    let hits_hook = Arc::clone(&hits);
    let callback = hook(move || {
        hits_hook.fetch_add(1, Ordering::SeqCst);
    });
    bound.on_invalidated(Arc::downgrade(&callback));
    drop(callback);
    bound.revoke();
    assert_eq!(hits.load(Ordering::SeqCst), 0);
}

#[test]
fn panicking_callback_does_not_skip_the_next_or_escape() {
    let bound = lease();
    let hits = Arc::new(AtomicUsize::new(0));
    let boom = hook(|| panic!("invalidation callback"));
    let hits_hook = Arc::clone(&hits);
    let after = hook(move || {
        hits_hook.fetch_add(1, Ordering::SeqCst);
    });
    bound.on_invalidated(Arc::downgrade(&boom));
    bound.on_invalidated(Arc::downgrade(&after));
    let revoked = std::panic::catch_unwind(AssertUnwindSafe(|| bound.revoke()));
    assert!(revoked.is_ok(), "callback panic escaped revoke");
    assert_eq!(hits.load(Ordering::SeqCst), 1);
    assert!(bound.current().is_none());
}

#[test]
fn rebind_publishes_and_renew_does_not() {
    let clock = Arc::new(Manual(AtomicU64::new(0)));
    let bound = ScopeLease::install_with_deadline(scope(1), clock.clone(), Duration::from_secs(10));
    let hits = Arc::new(AtomicUsize::new(0));
    let hits_hook = Arc::clone(&hits);
    let callback = hook(move || {
        hits_hook.fetch_add(1, Ordering::SeqCst);
    });
    bound.on_invalidated(Arc::downgrade(&callback));
    bound.renew_deadline(Duration::from_secs(30)).unwrap();
    assert_eq!(
        hits.load(Ordering::SeqCst),
        0,
        "renewal must not retire delivered rows"
    );
    bound.rebind(scope(2)).unwrap();
    assert_eq!(hits.load(Ordering::SeqCst), 1);
    clock.0.store(30, Ordering::SeqCst);
    assert!(
        bound.current().is_none(),
        "reading the lease publishes expiry"
    );
    assert_eq!(hits.load(Ordering::SeqCst), 2);
}

#[test]
fn dead_weak_is_dropped_without_keeping_the_slot() {
    let bound = lease();
    let callback: Arc<dyn Fn() + Send + Sync> = hook(|| {});
    let weak: Weak<dyn Fn() + Send + Sync> = Arc::downgrade(&callback);
    bound.on_invalidated(Weak::clone(&weak));
    drop(callback);
    bound.revoke();
    assert!(weak.upgrade().is_none());
}
