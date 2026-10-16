//! Bounded native work queues. Cache reads never enter these queues.

use crate::discovery::State;
use gcu_core::driver::{Cancel, DriverError, Result, NATIVE_TIMEOUT};
use std::{sync::mpsc, time::Duration};
use tokio::sync::oneshot;

pub(super) type Task = Box<dyn FnOnce(&mut State) + Send>;

pub(super) fn start(name: &str, mut state: State) -> Result<mpsc::SyncSender<Task>> {
    let (tx, rx) = mpsc::sync_channel::<Task>(32);
    std::thread::Builder::new()
        .name(name.into())
        .spawn(move || {
            while let Ok(task) = rx.recv() {
                task(&mut state);
            }
        })
        .map_err(|_| DriverError::Crashed {
            maybe_executed: false,
        })?;
    Ok(tx)
}

pub(super) async fn call<T: Send + 'static>(
    worker: &mpsc::SyncSender<Task>,
    timeout: Duration,
    cancel: Option<&Cancel>,
    mutation: bool,
    task: impl FnOnce(&mut State) -> Result<T> + Send + 'static,
) -> Result<T> {
    if cancel.is_some_and(Cancel::is_cancelled) {
        return Err(DriverError::Cancelled {
            maybe_executed: false,
        });
    }
    let (tx, rx) = oneshot::channel();
    worker
        .try_send(Box::new(move |state| {
            if !tx.is_closed() {
                let _ = tx.send(task(state));
            }
        }))
        .map_err(|_| DriverError::Crashed {
            maybe_executed: false,
        })?;
    let wait = tokio::time::timeout(timeout.min(NATIVE_TIMEOUT), rx);
    let result = if let Some(cancel) = cancel {
        tokio::select! {
            biased;
            _ = cancel.cancelled() => {
                return Err(DriverError::Cancelled { maybe_executed: mutation });
            }
            result = wait => result,
        }
    } else {
        wait.await
    };
    result
        .map_err(|_| DriverError::Timeout {
            maybe_executed: mutation,
        })?
        .map_err(|_| DriverError::Crashed {
            maybe_executed: mutation,
        })?
}
