//! Dedicated `gcu-macos-ax` thread.
//!
//! Every AX call runs here. Each call sets `AXUIElementSetMessagingTimeout` to
//! one second inside the job. The waiter gives up after [`AX_CALL_TIMEOUT`].
//! A job that is already inside AX is not preempted: the waiter can return
//! `Timeout` or the caller can drop the future, and the thread still finishes
//! the call. Reads pass `mutation = false`, so that timeout reports
//! `maybe_executed = false`. Toggle writes pass `mutation = true`.
//!
//! Jobs must not call back into this thread.

use std::{
    panic::AssertUnwindSafe,
    sync::{mpsc, Mutex},
    thread::JoinHandle,
    time::Duration,
};

use gcu_core::driver::DriverError;

use super::world::World;
use crate::scope_os::{not_proved, JobScope};
use gcu_core::driver::Result;

/// Wait for one AX job. Longer than the 1s messaging timeout so a finished
/// call is observed before this deadline.
pub const AX_CALL_TIMEOUT: Duration = Duration::from_millis(1500);

type JobBody = Box<dyn FnOnce(&mut World, &JobScope) + Send>;

struct Job {
    scope: JobScope,
    body: JobBody,
}

/// Serial AX worker. Drop closes the queue and joins the thread.
pub struct AxThread {
    tx: Mutex<Option<mpsc::SyncSender<Job>>>,
    join: Mutex<Option<JoinHandle<()>>>,
}

impl AxThread {
    /// Spawn `gcu-macos-ax`. The thread owns every `AXUIElementRef`.
    pub fn spawn() -> Result<Self> {
        let (tx, rx) = mpsc::sync_channel::<Job>(8);
        let join = std::thread::Builder::new()
            .name("gcu-macos-ax".into())
            .spawn(move || {
                let mut world = World::new();
                while let Ok(job) = rx.recv() {
                    let Job { scope, body } = job;
                    body(&mut world, &scope);
                }
            })
            .map_err(|_| DriverError::Crashed {
                maybe_executed: false,
            })?;
        Ok(Self {
            tx: Mutex::new(Some(tx)),
            join: Mutex::new(Some(join)),
        })
    }

    /// Run an unscoped `job` and wait asynchronously. See the module note on cancellation.
    pub async fn call<T: Send + 'static>(
        &self,
        timeout: Duration,
        mutation: bool,
        job: impl FnOnce(&mut World) -> Result<T> + Send + 'static,
    ) -> Result<T> {
        self.call_scoped(timeout, mutation, JobScope::unscoped(), move |world, _| {
            job(world)
        })
        .await
    }

    /// Run `job` with the scope captured at admission.
    ///
    /// A scoped job whose binding is missing is refused before `job` runs.
    /// The scope value is the one stored on the job, not a later pin read.
    /// Same timeout and cancellation as [`Self::call`].
    pub(crate) async fn call_scoped<T: Send + 'static>(
        &self,
        timeout: Duration,
        mutation: bool,
        scope: JobScope,
        job: impl FnOnce(&mut World, &JobScope) -> Result<T> + Send + 'static,
    ) -> Result<T> {
        self.call_with_liveness_scoped(timeout, mutation, scope, move |world, scope, _| {
            job(world, scope)
        })
        .await
    }

    /// Run a job that can recheck waiter liveness immediately before mutation.
    ///
    /// `scope` is the admission captured with the job. Dropping or timing out
    /// the future closes the receiver. Already-entered AX writes remain
    /// uncertain; queued or still-preparing writes can stop.
    pub(crate) async fn call_with_liveness_scoped<T: Send + 'static>(
        &self,
        timeout: Duration,
        mutation: bool,
        scope: JobScope,
        job: impl FnOnce(&mut World, &JobScope, &dyn Fn() -> bool) -> Result<T> + Send + 'static,
    ) -> Result<T> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.enqueue(mutation, scope, move |world, scope| {
            if !tx.is_closed() {
                let result = if scope.missing() {
                    Err(not_proved())
                } else {
                    catch_job(
                        mutation,
                        |world| job(world, scope, &|| !tx.is_closed()),
                        world,
                    )
                };
                let _ = tx.send(result);
            }
        })?;
        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(value)) => value,
            Ok(Err(_)) => Err(DriverError::Crashed {
                maybe_executed: mutation,
            }),
            Err(_) => Err(DriverError::Timeout {
                maybe_executed: mutation,
            }),
        }
    }

    /// Run `job` and block. Drop uses this so a panic unwind can still restore
    /// accessibility attributes. The block is the channel wait, not an AX call
    /// on the caller's thread.
    pub fn call_blocking<T: Send + 'static>(
        &self,
        timeout: Duration,
        mutation: bool,
        job: impl FnOnce(&mut World) -> Result<T> + Send + 'static,
    ) -> Result<T> {
        let (tx, rx) = mpsc::sync_channel(1);
        self.enqueue(mutation, JobScope::unscoped(), move |world, _| {
            let _ = tx.send(catch_job(mutation, job, world));
        })?;
        match rx.recv_timeout(timeout) {
            Ok(value) => value,
            Err(mpsc::RecvTimeoutError::Timeout) => Err(DriverError::Timeout {
                maybe_executed: mutation,
            }),
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(DriverError::Crashed {
                maybe_executed: mutation,
            }),
        }
    }

    fn enqueue(
        &self,
        mutation: bool,
        scope: JobScope,
        job: impl FnOnce(&mut World, &JobScope) + Send + 'static,
    ) -> Result<()> {
        let guard = self.tx.lock().map_err(|_| DriverError::Crashed {
            maybe_executed: mutation,
        })?;
        let tx = guard.as_ref().ok_or(DriverError::Crashed {
            maybe_executed: mutation,
        })?;
        tx.try_send(Job {
            scope,
            body: Box::new(job),
        })
        .map_err(|_| DriverError::Crashed {
            maybe_executed: mutation,
        })
    }
}

fn catch_job<T>(
    mutation: bool,
    job: impl FnOnce(&mut World) -> Result<T>,
    world: &mut World,
) -> Result<T> {
    match std::panic::catch_unwind(AssertUnwindSafe(|| job(world))) {
        Ok(value) => value,
        Err(_) => Err(DriverError::Crashed {
            maybe_executed: mutation,
        }),
    }
}

impl Drop for AxThread {
    fn drop(&mut self) {
        if let Ok(mut tx) = self.tx.lock() {
            tx.take();
        }
        if let Ok(mut join) = self.join.lock() {
            if let Some(handle) = join.take() {
                let _ = handle.join();
            }
        }
    }
}
