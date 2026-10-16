//! Persistent native-thread executor. Dropped futures abandon queued/in-flight work.
use super::patterns::{ActionContext, PatternResult};
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
pub(crate) struct Request {
    abandoned: AtomicBool,
    deadline: Instant,
    context: Option<ActionContext>,
}
impl Request {
    fn new() -> Self {
        Self {
            abandoned: AtomicBool::new(false),
            deadline: Instant::now() + DRIVER_DEADLINE,
            context: None,
        }
    }
    pub(crate) fn check(&self) -> Result<()> {
        if let Some(context) = &self.context {
            context.check()?;
        }
        if self.abandoned.load(Ordering::Acquire) || Instant::now() >= self.deadline {
            Err(DriverError::Timeout {
                maybe_executed: false,
            })
        } else {
            Ok(())
        }
    }
}
impl Request {
    pub(crate) fn dispatch(
        &self,
        mutation: impl FnOnce() -> Result<PatternResult>,
    ) -> Result<PatternResult> {
        self.check()?;
        let context = self.context.as_ref().ok_or(DriverError::InvalidRequest)?;
        let previous = context.begin()?;
        let result = mutation();
        if matches!(
            result,
            Ok(PatternResult::Unsupported | PatternResult::Unavailable)
        ) || result.as_ref().is_err_and(|e| !e.maybe_executed())
        {
            context.restore(previous);
        }
        result.map_err(|e| context.error(e))
    }
}
struct Abandon {
    request: Arc<Request>,
    completed: bool,
}
impl Drop for Abandon {
    fn drop(&mut self) {
        self.request.abandoned.store(true, Ordering::Release);
        if !self.completed {
            if let Some(context) = &self.request.context {
                context.close();
            }
        }
    }
}
pub(crate) struct Worker<T> {
    tx: mpsc::SyncSender<Task<T>>,
}
impl<T: 'static> Worker<T> {
    pub(crate) async fn start(
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
    pub(crate) async fn call<R: Send + 'static>(
        &self,
        task: impl FnOnce(&mut T, &Request) -> Result<R> + Send + 'static,
    ) -> Result<R> {
        self.call_inner(None, task).await
    }
    pub(crate) async fn call_action<R: Send + 'static>(
        &self,
        context: ActionContext,
        task: impl FnOnce(&mut T, &Request) -> Result<R> + Send + 'static,
    ) -> Result<R> {
        self.call_inner(Some(context), task).await
    }
    async fn call_inner<R: Send + 'static>(
        &self,
        context: Option<ActionContext>,
        task: impl FnOnce(&mut T, &Request) -> Result<R> + Send + 'static,
    ) -> Result<R> {
        let _timing = crate::timing::Timing::provider("uia_queue_wait_and_call");
        let (tx, rx) = oneshot::channel();
        let mut request = Request::new();
        request.context = context.clone();
        let request = Arc::new(request);
        let mut abandon = Abandon {
            request: request.clone(),
            completed: false,
        };
        self.tx
            .try_send(Box::new(move |state| {
                let _timing = crate::timing::Timing::provider("uia_dequeued_call");
                if tx.is_closed() {
                    return;
                }
                let result = request.check().and_then(|()| task(state, &request));
                let _ = tx.send(result);
            }))
            .map_err(|_| crashed())?;
        let response = tokio::time::timeout(DRIVER_DEADLINE, rx).await;
        if response.is_err() {
            if let Some(context) = &context {
                context.close();
            }
        }
        let result = response
            .map_err(|_| timeout())
            .and_then(|r| r.map_err(|_| crashed()))
            .and_then(|r| r);
        abandon.completed = true;
        result.map_err(|error| context.as_ref().map_or(error.clone(), |c| c.error(error)))
    }
    pub(crate) fn enqueue(
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
    async fn action_preparation_cancelled_before_dispatch_never_writes_later() {
        let worker = Worker::start(|| Ok(0u32), |_| {}).await.unwrap();
        let cancel = gcu_core::driver::Cancel::default();
        let context = ActionContext::new(&cancel);
        let marker = context.clone();
        let (started, ready) = oneshot::channel();
        let (release, waiting) = mpsc::channel();
        let operation = worker.call_action(context, move |count, r| {
            let _ = started.send(());
            let _ = waiting.recv_timeout(Duration::from_millis(500));
            r.dispatch(|| {
                *count += 1;
                Ok(PatternResult::Performed)
            })
        });
        tokio::pin!(operation);
        tokio::select! {
            result = &mut operation => panic!("unexpected completion: {result:?}"),
            _ = ready => {}
        }
        cancel.cancel();
        release.send(()).unwrap();
        assert_eq!(
            operation.await,
            Err(DriverError::Cancelled {
                maybe_executed: false
            })
        );
        assert!(!marker.maybe_executed());
        assert_eq!(worker.call(|count, _| Ok(*count)).await.unwrap(), 0);
    }
    #[tokio::test]
    async fn action_inflight_cancelled_returns_unknown_without_replay() {
        let worker = Worker::start(|| Ok(0u32), |_| {}).await.unwrap();
        let cancel = gcu_core::driver::Cancel::default();
        let context = ActionContext::new(&cancel);
        let marker = context.clone();
        let (started, ready) = oneshot::channel();
        let (release, waiting) = mpsc::channel();
        {
            let operation = worker.call_action(context, move |count, r| {
                r.dispatch(|| {
                    *count += 1;
                    let _ = started.send(());
                    let _ = waiting.recv_timeout(Duration::from_millis(500));
                    Ok(PatternResult::Performed)
                })
            });
            tokio::pin!(operation);
            tokio::select! {
                result = &mut operation => panic!("unexpected completion: {result:?}"),
                _ = ready => {}
            }
            cancel.cancel();
            assert!(marker.maybe_executed());
            // Dropping the future must not join the blocked native call.
        }
        release.send(()).unwrap();
        assert_eq!(worker.call(|count, _| Ok(*count)).await.unwrap(), 1);
    }
    #[tokio::test]
    async fn action_queued_future_dropped_never_dispatches_after_worker_unblocks() {
        let worker = Worker::start(|| Ok(0u32), |_| {}).await.unwrap();
        let (started, ready) = oneshot::channel();
        let (release, waiting) = mpsc::channel();
        worker
            .enqueue(move |_, _| {
                let _ = started.send(());
                let _ = waiting.recv_timeout(Duration::from_millis(500));
            })
            .unwrap();
        ready.await.unwrap();
        let context = ActionContext::new(&gcu_core::driver::Cancel::default());
        assert!(tokio::time::timeout(
            Duration::from_millis(10),
            worker.call_action(context.clone(), |count, r| r.dispatch(|| {
                *count += 1;
                Ok(PatternResult::Performed)
            }))
        )
        .await
        .is_err());
        assert!(!context.maybe_executed());
        release.send(()).unwrap();
        assert_eq!(worker.call(|count, _| Ok(*count)).await.unwrap(), 0);
    }
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
