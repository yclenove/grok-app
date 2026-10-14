//! Persistent native-thread executor. Dropped futures abandon queued/in-flight work.
use gcu_core::driver::{DriverError, Result, DRIVER_DEADLINE};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    time::{Duration, Instant},
};
use tokio::sync::oneshot;

type Task<T> = Box<dyn FnOnce(&mut T) + Send>;
pub(super) struct Request {
    abandoned: AtomicBool,
    deadline: Instant,
}
impl Request {
    fn new() -> Self {
        Self {
            abandoned: AtomicBool::new(false),
            deadline: Instant::now() + DRIVER_DEADLINE,
        }
    }
    pub(super) fn check(&self) -> Result<()> {
        if self.abandoned.load(Ordering::Acquire) || Instant::now() >= self.deadline {
            Err(DriverError::Timeout {
                maybe_executed: false,
            })
        } else {
            Ok(())
        }
    }
}
struct Abandon(Arc<Request>);
impl Drop for Abandon {
    fn drop(&mut self) {
        self.0.abandoned.store(true, Ordering::Release);
    }
}
pub(super) struct Worker<T> {
    tx: mpsc::SyncSender<Task<T>>,
}
impl<T: 'static> Worker<T> {
    pub(super) async fn start(
        factory: impl FnOnce() -> Result<T> + Send + 'static,
        maintain: fn(&mut T),
    ) -> Result<Self> {
        let (tx, rx) = mpsc::sync_channel::<Task<T>>(32);
        let (ready, started) = oneshot::channel();
        std::thread::Builder::new()
            .name("gcu-win-uia-mta".into())
            .spawn(move || {
                let mut state = match factory() {
                    Ok(state) => state,
                    Err(error) => {
                        let _ = ready.send(Err(error));
                        return;
                    }
                };
                if ready.send(Ok(())).is_err() {
                    return;
                }
                loop {
                    maintain(&mut state);
                    match rx.recv_timeout(Duration::from_millis(20)) {
                        Ok(task) => task(&mut state),
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    }
                }
            })
            .map_err(|_| crashed())?;
        tokio::time::timeout(DRIVER_DEADLINE, started)
            .await
            .map_err(|_| timeout())?
            .map_err(|_| crashed())??;
        Ok(Self { tx })
    }
    pub(super) async fn call<R: Send + 'static>(
        &self,
        task: impl FnOnce(&mut T, &Request) -> Result<R> + Send + 'static,
    ) -> Result<R> {
        let (tx, rx) = oneshot::channel();
        let request = Arc::new(Request::new());
        let _abandon = Abandon(request.clone());
        self.tx
            .try_send(Box::new(move |state| {
                if tx.is_closed() {
                    return;
                }
                let result = request.check().and_then(|()| task(state, &request));
                let _ = tx.send(result);
            }))
            .map_err(|_| crashed())?;
        tokio::time::timeout(DRIVER_DEADLINE, rx)
            .await
            .map_err(|_| timeout())?
            .map_err(|_| crashed())?
    }
    pub(super) fn enqueue(
        &self,
        task: impl FnOnce(&mut T, &Request) + Send + 'static,
    ) -> Result<()> {
        let request = Request::new();
        self.tx
            .try_send(Box::new(move |state| {
                if request.check().is_ok() {
                    task(state, &request);
                }
            }))
            .map_err(|_| crashed())
    }
}
fn crashed() -> DriverError {
    DriverError::Crashed {
        maybe_executed: false,
    }
}
fn timeout() -> DriverError {
    DriverError::Timeout {
        maybe_executed: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn worker_reuses_thread_and_dropped_queued_calls_never_execute() {
        let worker = Worker::start(|| Ok(0u32), |_| {}).await.unwrap();
        let first = worker
            .call(|_, r| {
                r.check()?;
                Ok(std::thread::current().id())
            })
            .await
            .unwrap();
        let second = worker
            .call(|_, _| Ok(std::thread::current().id()))
            .await
            .unwrap();
        assert_eq!(first, second);
        let (started, ready) = oneshot::channel();
        worker
            .enqueue(move |_, _| {
                let _ = started.send(());
                std::thread::sleep(Duration::from_millis(100));
            })
            .unwrap();
        ready.await.unwrap();
        let queued = worker.call(|count, _| {
            *count += 1;
            Ok(())
        });
        assert!(tokio::time::timeout(Duration::from_millis(10), queued)
            .await
            .is_err());
        assert_eq!(worker.call(|count, _| Ok(*count)).await.unwrap(), 0);
    }
    #[tokio::test]
    async fn wedged_native_job_does_not_block_future_drop_or_stop_channel() {
        let worker = Worker::start(|| Ok(()), |_| {}).await.unwrap();
        let (started, ready) = oneshot::channel();
        worker
            .enqueue(move |_, _| {
                let _ = started.send(());
                std::thread::sleep(Duration::from_millis(100));
            })
            .unwrap();
        ready.await.unwrap();
        let cancel = gcu_core::driver::Cancel::default();
        let stop = cancel.clone();
        {
            let call = worker.call(|_, _| Ok(()));
            tokio::pin!(call);
            assert!(tokio::time::timeout(Duration::from_millis(10), &mut call)
                .await
                .is_err());
        }
        stop.cancel();
        tokio::time::timeout(Duration::from_millis(50), cancel.cancelled())
            .await
            .unwrap();
        let start = Instant::now();
        drop(worker);
        assert!(start.elapsed() < Duration::from_millis(20));
    }
    #[tokio::test]
    async fn worker_blocked_native_call_returns_at_driver_deadline() {
        let worker = Worker::start(|| Ok(()), |_| {}).await.unwrap();
        let (release, wait) = mpsc::channel();
        let start = Instant::now();
        let result = worker
            .call(move |_, _| {
                let _ = wait.recv_timeout(DRIVER_DEADLINE + Duration::from_secs(2));
                Ok(())
            })
            .await;
        let elapsed = start.elapsed();
        let _ = release.send(());
        assert_eq!(result, Err(timeout()));
        assert!(elapsed >= DRIVER_DEADLINE);
        assert!(elapsed < gcu_core::driver::NATIVE_TIMEOUT);
        worker.call(|_, _| Ok(())).await.unwrap();
    }
}
