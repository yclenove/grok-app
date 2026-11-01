//! Drop delivered images and files when the lease generation is no longer current.
//!
//! Each store starts one owner from `bind_lease`. A caller that already has a
//! Tokio runtime gets a task on that runtime. A synchronous caller gets a
//! dedicated thread with its own current-thread runtime. [`Observer::shutdown`]
//! and `Drop` join that thread within [`OBSERVER_JOIN`].
//!
//! The owner sweeps, then looks. After it sees a terminal lease (`current` is
//! `None`, the epoch watch closes, or `remaining` is zero) it sweeps once more
//! before it exits. A zero remainder is re-read through `current` on this
//! owner, so expiry is published here and not by the other store's watcher.
//! The epoch is not a reliable wake for that case: `remaining` calls `expire`
//! and then samples the clock again, and a deadline that falls between those
//! reads returns zero without a publish.
//!
//! If the owner cannot start, the store does not publish delivered paths or
//! pixels. Disk cleanup runs after the store lock is released. The lease lock
//! is only the one [`crate::lease_view::visible`] already takes, and it is not
//! held across unlink.

use gcu_core::scope::ScopeLease;
use std::io;
#[cfg(test)]
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::watch;

/// How long `close` and `Drop` wait for the synchronous observer thread.
///
/// The thread is signalled first, so this covers an in-flight unlink, not the
/// lease deadline. It is not a Stop heartbeat or driver deadline.
const OBSERVER_JOIN: Duration = Duration::from_secs(5);

/// Invalidation owner for one store.
///
/// Shutdown signals the owner and, when the owner is a thread, joins it within
/// [`OBSERVER_JOIN`]. A Tokio task is only signalled; its runtime drives it.
/// Drop does the same. Joining from the owner thread itself detaches instead,
/// because that thread is inside `drive` and cannot join itself.
#[must_use]
pub(crate) struct Observer {
    stop: watch::Sender<bool>,
    thread: Mutex<Option<std::thread::JoinHandle<()>>>,
    worker: Option<std::thread::ThreadId>,
    watching: bool,
}

impl Default for Observer {
    fn default() -> Self {
        let (stop, _) = watch::channel(false);
        Self {
            stop,
            thread: Mutex::new(None),
            worker: None,
            watching: false,
        }
    }
}

impl Observer {
    /// Whether this owner is running a task or a thread.
    ///
    /// Memory only. No timeout or cancellation.
    pub(crate) fn watching(&self) -> bool {
        self.watching
    }

    /// Stop the owner. A thread is joined within [`OBSERVER_JOIN`].
    ///
    /// Blocking: up to that bound when a thread is still running. Cancellation:
    /// none. A second call is a no-op. The owner thread does not join itself.
    pub(crate) fn shutdown(&self) -> io::Result<()> {
        let _ = self.stop.send(true);
        let handle = self
            .thread
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .take();
        let Some(handle) = handle else {
            return Ok(());
        };
        if self.worker == Some(std::thread::current().id()) {
            drop(handle);
            return Ok(());
        }
        join_bounded(handle, OBSERVER_JOIN)
    }
}

impl Drop for Observer {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

/// Hooks the owner polls. Production leaves every slot empty.
///
/// Test builds park between the sweep and the terminal read, and again before
/// `remaining` is sampled, and count owners that have exited.
#[derive(Clone, Default)]
pub(crate) struct DriveHooks {
    #[cfg(test)]
    after_sweep: Option<Arc<Park>>,
    #[cfg(test)]
    before_remaining: Option<Arc<Park>>,
    #[cfg(test)]
    exited: Option<Arc<ExitFlag>>,
}

/// Start the invalidation owner for `lease`.
///
/// `retire` runs outside the lease lock. It must not delete files while it
/// holds that lock. Blocking: a synchronous caller waits until the owner
/// thread has built its runtime, not for the lease. Timeout: none on that
/// wait. Cancellation: none. A Tokio caller returns after `spawn`.
/// `watching` is false when no owner could start; the store must then refuse
/// to publish.
pub(crate) fn start(
    lease: ScopeLease,
    retire: Arc<dyn Fn() + Send + Sync>,
    hooks: DriveHooks,
) -> Observer {
    let (stop_tx, stop_rx) = watch::channel(false);
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        handle.spawn(async move {
            drive(lease, retire, stop_rx, hooks).await;
        });
        return Observer {
            stop: stop_tx,
            thread: Mutex::new(None),
            worker: None,
            watching: true,
        };
    }
    let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel(1);
    let spawned = std::thread::Builder::new()
        .name("gcu-lease-retire".into())
        .spawn(move || {
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_time()
                .build()
            {
                Ok(runtime) => runtime,
                Err(_) => {
                    let _ = ready_tx.send(None);
                    return;
                }
            };
            let _ = ready_tx.send(Some(std::thread::current().id()));
            runtime.block_on(drive(lease, retire, stop_rx, hooks));
        });
    let Ok(join) = spawned else {
        return Observer {
            stop: stop_tx,
            thread: Mutex::new(None),
            worker: None,
            watching: false,
        };
    };
    match ready_rx.recv() {
        Ok(Some(worker)) => Observer {
            stop: stop_tx,
            thread: Mutex::new(Some(join)),
            worker: Some(worker),
            watching: true,
        },
        _ => {
            let _ = join_bounded(join, OBSERVER_JOIN);
            Observer {
                stop: stop_tx,
                thread: Mutex::new(None),
                worker: None,
                watching: false,
            }
        }
    }
}

fn join_bounded(handle: std::thread::JoinHandle<()>, bound: Duration) -> io::Result<()> {
    let (tx, rx) = std::sync::mpsc::channel();
    let joiner = std::thread::Builder::new()
        .name("gcu-lease-retire-join".into())
        .spawn(move || {
            let _ = handle.join();
            let _ = tx.send(());
        });
    let Ok(joiner) = joiner else {
        return Err(io::Error::other("retire observer join did not start"));
    };
    match rx.recv_timeout(bound) {
        Ok(()) => {
            let _ = joiner.join();
            Ok(())
        }
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            Err(io::Error::other("retire observer did not stop within 5s"))
        }
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            let _ = joiner.join();
            Err(io::Error::other("retire observer join failed"))
        }
    }
}

enum Wake {
    Stop,
    EpochClosed,
    Continue,
}

async fn drive(
    lease: ScopeLease,
    retire: Arc<dyn Fn() + Send + Sync>,
    mut stop: watch::Receiver<bool>,
    hooks: DriveHooks,
) {
    let _exit = exit_on_drop(&hooks);
    let mut epoch = lease.subscribe();
    loop {
        if *stop.borrow() {
            retire();
            break;
        }
        // Sweep first. A revoke that lands after this sweep and before the
        // terminal read below must still be cleaned up by the post-terminal
        // sweep. Breaking here would leave the artifact in place, and the
        // owner's flag would stay set so a later bind would not retry.
        retire();
        hooks.pause_after_sweep().await;
        if lease.current().is_none() {
            retire();
            break;
        }
        hooks.pause_before_remaining().await;
        let wake = match lease.remaining() {
            // Zero means revoked, or the clock crossed the deadline between
            // the two reads inside `remaining` without a publish. Re-read
            // here. Waiting on the epoch would sleep forever when neither
            // store publishes.
            Some(left) if left.is_zero() => {
                let _ = lease.current();
                continue;
            }
            Some(left) => wait_wake(&mut epoch, &mut stop, Some(left)).await,
            None => wait_wake(&mut epoch, &mut stop, None).await,
        };
        match wake {
            Wake::Stop | Wake::EpochClosed => {
                retire();
                break;
            }
            Wake::Continue => {}
        }
    }
}

async fn wait_wake(
    epoch: &mut watch::Receiver<u64>,
    stop: &mut watch::Receiver<bool>,
    sleep_for: Option<Duration>,
) -> Wake {
    tokio::select! {
        biased;
        changed = stop.changed() => {
            if changed.is_err() || *stop.borrow() {
                Wake::Stop
            } else {
                Wake::Continue
            }
        }
        changed = epoch.changed() => {
            if changed.is_err() {
                Wake::EpochClosed
            } else {
                Wake::Continue
            }
        }
        _ = sleep_or_pending(sleep_for) => Wake::Continue,
    }
}

async fn sleep_or_pending(sleep_for: Option<Duration>) {
    match sleep_for {
        Some(left) => tokio::time::sleep(left).await,
        None => std::future::pending::<()>().await,
    }
}

fn exit_on_drop(hooks: &DriveHooks) -> ExitOnDrop {
    #[cfg(test)]
    {
        ExitOnDrop {
            flag: hooks.exited.clone(),
        }
    }
    #[cfg(not(test))]
    {
        let _ = hooks;
        ExitOnDrop {}
    }
}

struct ExitOnDrop {
    #[cfg(test)]
    flag: Option<Arc<ExitFlag>>,
}

impl Drop for ExitOnDrop {
    fn drop(&mut self) {
        #[cfg(test)]
        if let Some(flag) = &self.flag {
            flag.hit();
        }
    }
}

impl DriveHooks {
    async fn pause_after_sweep(&self) {
        let _ = self;
        #[cfg(test)]
        if let Some(park) = &self.after_sweep {
            park.wait().await;
            return;
        }
        std::future::ready(()).await;
    }

    async fn pause_before_remaining(&self) {
        let _ = self;
        #[cfg(test)]
        if let Some(park) = &self.before_remaining {
            park.wait().await;
            return;
        }
        std::future::ready(()).await;
    }
}

/// One-shot gathering point. Every armed waiter parks until [`Park::release`].
///
/// Memory only. The wait is a Tokio watch, so it does not block the runtime
/// thread. A disarmed or already released park returns immediately.
#[cfg(test)]
struct Park {
    armed: AtomicBool,
    count: std::sync::atomic::AtomicUsize,
    parked: watch::Sender<usize>,
    release: watch::Sender<bool>,
}

#[cfg(test)]
impl Park {
    fn new() -> Arc<Self> {
        let (parked, _) = watch::channel(0);
        let (release, _) = watch::channel(false);
        Arc::new(Self {
            armed: AtomicBool::new(true),
            count: std::sync::atomic::AtomicUsize::new(0),
            parked,
            release,
        })
    }

    fn subscribe(&self) -> watch::Receiver<usize> {
        self.parked.subscribe()
    }

    async fn wait(&self) {
        if !self.armed.load(Ordering::SeqCst) {
            return;
        }
        let mut release = self.release.subscribe();
        if *release.borrow() {
            return;
        }
        let n = self.count.fetch_add(1, Ordering::SeqCst) + 1;
        let _ = self.parked.send(n);
        let _ = release.wait_for(|open| *open).await;
    }

    fn release(&self) {
        self.armed.store(false, Ordering::SeqCst);
        let _ = self.release.send(true);
    }
}

/// Owners that have left `drive`. Waiters block on a condvar or poll.
///
/// The success path wakes from `hit`. The timeout is only the failure bound.
#[cfg(test)]
struct ExitFlag {
    count: std::sync::atomic::AtomicUsize,
    lock: Mutex<()>,
    cv: std::sync::Condvar,
}

#[cfg(test)]
impl ExitFlag {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            count: std::sync::atomic::AtomicUsize::new(0),
            lock: Mutex::new(()),
            cv: std::sync::Condvar::new(),
        })
    }

    fn hit(&self) {
        self.count.fetch_add(1, Ordering::SeqCst);
        let _guard = self.lock.lock().unwrap_or_else(|err| err.into_inner());
        self.cv.notify_all();
    }

    fn wait_blocking(&self, n: usize, bound: Duration) -> bool {
        let guard = self.lock.lock().unwrap_or_else(|err| err.into_inner());
        let (guard, _) = self
            .cv
            .wait_timeout_while(guard, bound, |_| self.count.load(Ordering::SeqCst) < n)
            .unwrap_or_else(|err| err.into_inner());
        drop(guard);
        self.count.load(Ordering::SeqCst) >= n
    }

    async fn wait_yielding(&self, n: usize, bound: Duration) -> bool {
        let deadline = tokio::time::Instant::now() + bound;
        loop {
            if self.count.load(Ordering::SeqCst) >= n {
                return true;
            }
            if tokio::time::Instant::now() >= deadline {
                return self.count.load(Ordering::SeqCst) >= n;
            }
            tokio::task::yield_now().await;
        }
    }
}

#[cfg(test)]
#[path = "retire_tests.rs"]
mod tests;
