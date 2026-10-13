//! One process-owned expiry/cleanup consumer, independent of UI/model polling.
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, OnceLock, Weak,
};
use std::time::Duration;

use parking_lot::Mutex;
use tokio::sync::Notify;

use super::ComputerUseBroker;

type ConsumerTask = Arc<tokio::sync::Mutex<Option<tauri::async_runtime::JoinHandle<()>>>>;

struct Watcher {
    stopping: AtomicBool,
    join_failed: AtomicBool,
    wake: Notify,
    task: Mutex<Option<ConsumerTask>>,
    #[cfg(test)]
    cleanup_errors: Arc<std::sync::atomic::AtomicUsize>,
    #[cfg(test)]
    join_entered: Notify,
}

fn watcher() -> &'static Watcher {
    static WATCHER: OnceLock<Watcher> = OnceLock::new();
    WATCHER.get_or_init(|| Watcher {
        stopping: AtomicBool::new(false),
        join_failed: AtomicBool::new(false),
        wake: Notify::new(),
        task: Mutex::new(None),
        #[cfg(test)]
        cleanup_errors: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        #[cfg(test)]
        join_entered: Notify::new(),
    })
}

pub(super) fn start(broker: &Arc<ComputerUseBroker>) {
    let state = watcher();
    let mut task = state.task.lock();
    if task.is_some() || state.stopping.load(Ordering::SeqCst) {
        return;
    }
    *task = Some(Arc::new(tokio::sync::Mutex::new(Some(
        tauri::async_runtime::spawn(run(Arc::downgrade(broker), Duration::from_secs(1), state)),
    ))));
}

async fn run(broker: Weak<ComputerUseBroker>, interval: Duration, state: &Watcher) {
    let mut tick = tokio::time::interval(interval);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            _ = state.wake.notified() => {},
            _ = tick.tick() => {},
        }
        if state.stopping.load(Ordering::SeqCst) {
            break;
        }
        let Some(broker) = broker.upgrade() else {
            break;
        };
        // At most one blocking consumer: wait for its original join before
        // scheduling another. Direct authority deadlines still guard admission
        // while physical cleanup is busy or fails and remains retryable.
        #[cfg(test)]
        let errors = state.cleanup_errors.clone();
        let joined = tauri::async_runtime::spawn_blocking(move || {
            broker.sweep_task_consent();
            for cleanup in broker.take_cleanup_tickets() {
                if broker.finish_stop_cleanup(&cleanup).is_err() {
                    #[cfg(test)]
                    errors.fetch_add(1, Ordering::SeqCst);
                    tracing::warn!("Computer Use owned task cleanup remains pending");
                }
            }
        })
        .await;
        if joined.is_err() {
            tracing::warn!("Computer Use expiry consumer failed");
        }
    }
}

/// Run the unchanged consumer against an isolated Broker, not the process
/// singleton. The test retains and joins this original future; no user data.
#[cfg(all(test, windows))]
pub(crate) async fn run_fixture_cleanup_watch(
    broker: Arc<ComputerUseBroker>,
    stop: tokio::sync::oneshot::Receiver<()>,
    errors: Arc<std::sync::atomic::AtomicUsize>,
) {
    let state = Watcher {
        stopping: AtomicBool::new(false),
        join_failed: AtomicBool::new(false),
        wake: Notify::new(),
        task: Mutex::new(None),
        cleanup_errors: errors,
        join_entered: Notify::new(),
    };
    let finish = async {
        let _ = stop.await;
        state.stopping.store(true, Ordering::SeqCst);
        state.wake.notify_one();
    };
    tokio::join!(
        run(Arc::downgrade(&broker), Duration::from_millis(25), &state),
        finish
    );
}

pub(super) fn stop() {
    let state = watcher();
    state.stopping.store(true, Ordering::SeqCst);
    state.wake.notify_one();
}

pub(super) async fn join() -> Result<(), String> {
    join_with(watcher()).await
}

async fn join_with(state: &Watcher) -> Result<(), String> {
    state.stopping.store(true, Ordering::SeqCst);
    state.wake.notify_one();
    // Clone its slot, never take its running handle. A cancelled shutdown
    // waiter drops only this borrow; the next caller can join the same owner.
    let original = state.task.lock().clone();
    if let Some(original) = original {
        let mut slot = original.lock().await;
        #[cfg(test)]
        state.join_entered.notify_one();
        if let Some(task) = slot.as_mut() {
            if task.await.is_err() {
                state.join_failed.store(true, Ordering::SeqCst);
            }
            slot.take();
        }
        drop(slot);
        let mut registered = state.task.lock();
        if registered
            .as_ref()
            .is_some_and(|current| Arc::ptr_eq(current, &original))
        {
            registered.take();
        }
    } else {
        #[cfg(test)]
        state.join_entered.notify_one();
    }
    if state.join_failed.load(Ordering::SeqCst) {
        Err("Computer Use expiry consumer did not join".into())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    mod ownership;

    #[tokio::test]
    async fn owned_watcher_stops_and_joins_without_creating_executors() {
        let broker = Arc::new(ComputerUseBroker::request_endpoint(
            super::super::BrokerOptions::default(),
        ));
        let state = Watcher {
            stopping: AtomicBool::new(false),
            join_failed: AtomicBool::new(false),
            wake: Notify::new(),
            task: Mutex::new(None),
            cleanup_errors: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            join_entered: Notify::new(),
        };
        let runner = run(Arc::downgrade(&broker), Duration::from_millis(5), &state);
        let stopper = async {
            tokio::time::sleep(Duration::from_millis(20)).await;
            state.stopping.store(true, Ordering::SeqCst);
            state.wake.notify_one();
        };
        tokio::time::timeout(Duration::from_secs(2), async {
            tokio::join!(runner, stopper);
        })
        .await
        .unwrap();
        assert_eq!(broker.capabilities().backend_id, "request-only");
        assert!(!broker.feature_enabled());
    }
}
