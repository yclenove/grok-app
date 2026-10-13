//! Original native control handles remain owned after their UI waiter drops.
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

#[derive(Debug)]
pub(crate) enum TaskControlOutput {
    Paused,
    Preview(Box<super::protocol::Observation>),
    Resumed(grok_computer_use_core::broker::HostTaskResumeReceipt),
    Uploaded(Box<grok_computer_use_core::broker::WebViewUploadCompletion>),
}

type Workers = super::owned_tasks::OwnedTasks<Result<TaskControlOutput, String>>;

#[derive(Default)]
struct Registry {
    workers: Arc<Workers>,
    stopping: Arc<AtomicBool>,
    wake: Arc<tokio::sync::Notify>,
    reaper: tokio::sync::Mutex<Option<tauri::async_runtime::JoinHandle<()>>>,
}

fn registry() -> Arc<Registry> {
    static REGISTRY: OnceLock<Arc<Registry>> = OnceLock::new();
    REGISTRY
        .get_or_init(|| Arc::new(Registry::default()))
        .clone()
}

impl Registry {
    async fn start_reaper(&self) -> Result<(), String> {
        let mut slot = self.reaper.lock().await;
        if self.stopping.load(Ordering::SeqCst) {
            return Err("Computer Use controls are stopping".into());
        }
        if slot.is_none() {
            let workers = self.workers.clone();
            let stopping = self.stopping.clone();
            let wake = self.wake.clone();
            *slot = Some(tauri::async_runtime::spawn(async move {
                let mut tick = tokio::time::interval(Duration::from_millis(250));
                tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                loop {
                    tokio::select! {
                        _ = tick.tick() => {},
                        _ = wake.notified() => {},
                    }
                    if stopping.load(Ordering::SeqCst) {
                        return;
                    }
                    workers.reap().await;
                }
            }));
        }
        Ok(())
    }

    fn stop(&self) {
        self.stopping.store(true, Ordering::SeqCst);
        self.workers.stop();
        self.wake.notify_one();
    }

    async fn join(&self) -> Result<(), String> {
        self.stop();
        let mut slot = self.reaper.lock().await;
        // Keep the original slot while awaiting, even if the join waiter drops.
        let reaper = if let Some(task) = slot.as_mut() {
            let result = task
                .await
                .map_err(|_| "Computer Use control reaper did not join".to_string());
            slot.take();
            result
        } else {
            Ok(())
        };
        drop(slot);
        let workers = self.workers.join().await;
        reaper.and(workers)
    }
}

pub(super) fn stop() {
    registry().stop();
}
pub(super) async fn join() -> Result<(), String> {
    registry().join().await
}

/// Read-only identity witness; never takes a live waiter's result or resets flags.
#[cfg(all(test, windows))]
pub(crate) fn original_worker_for_test() -> Result<Option<uuid::Uuid>, String> {
    let workers = registry().workers.clone();
    if workers.owner_count_for_test() == 0 {
        Ok(None)
    } else {
        workers.singleton_entry_for_test().map(Some)
    }
}

pub(crate) async fn run(
    work: impl FnOnce() -> Result<TaskControlOutput, String> + Send + 'static,
) -> Result<TaskControlOutput, String> {
    run_with(registry(), work).await
}

pub(crate) async fn run_fenced(
    work: impl FnOnce() -> Result<TaskControlOutput, String> + Send + 'static,
    fence: impl Fn() + Send + Sync + 'static,
) -> Result<TaskControlOutput, String> {
    run_with_fence(registry(), work, fence).await
}

async fn run_with(
    registry: Arc<Registry>,
    work: impl FnOnce() -> Result<TaskControlOutput, String> + Send + 'static,
) -> Result<TaskControlOutput, String> {
    // Cancelling an observer/command waiter never revokes the user's task.
    run_with_fence(registry, work, || {}).await
}

async fn run_with_fence(
    registry: Arc<Registry>,
    work: impl FnOnce() -> Result<TaskControlOutput, String> + Send + 'static,
    fence: impl Fn() + Send + Sync + 'static,
) -> Result<TaskControlOutput, String> {
    registry.start_reaper().await?;
    registry
        .workers
        .run(
            async move {
                tauri::async_runtime::spawn_blocking(work)
                    .await
                    .map_err(|_| "Computer Use control worker did not join".to_string())?
            },
            fence,
        )
        .await?
}

#[cfg(test)]
mod recovery_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn cancelled_control_waiter_keeps_native_handle_until_real_join() {
        let registry = Arc::new(Registry::default());
        let (entered, started) = tokio::sync::oneshot::channel();
        let (release, blocked) = std::sync::mpsc::channel();
        let workers = registry.clone();
        let waiter = tokio::spawn(run_with(workers, move || {
            entered.send(()).unwrap();
            blocked.recv_timeout(Duration::from_secs(3)).unwrap();
            Ok(TaskControlOutput::Paused)
        }));
        tokio::time::timeout(Duration::from_secs(2), started)
            .await
            .unwrap()
            .unwrap();
        waiter.abort();
        assert!(waiter.await.unwrap_err().is_cancelled());
        let workers = registry.clone();
        let join = tokio::spawn(async move { workers.join().await });
        tokio::task::yield_now().await;
        assert!(!join.is_finished());
        release.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(2), join)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(run_with(registry, || Ok(TaskControlOutput::Paused))
            .await
            .is_err());
    }

    #[tokio::test]
    async fn control_native_panic_is_an_error_and_join_still_closes_original_handle() {
        let registry = Arc::new(Registry::default());
        let result = run_with(registry.clone(), || {
            panic!("synthetic native control panic")
        })
        .await;
        assert!(
            matches!(result, Err(error) if error == "Computer Use control worker did not join")
        );
        tokio::time::timeout(Duration::from_secs(2), registry.join())
            .await
            .unwrap()
            .unwrap();
    }

    #[tokio::test]
    async fn dropped_control_is_automatically_joined_without_another_command() {
        let registry = Arc::new(Registry::default());
        let (entered, started) = tokio::sync::oneshot::channel();
        let (release, blocked) = std::sync::mpsc::channel();
        let workers = registry.clone();
        let waiter = tokio::spawn(run_with(workers, move || {
            entered.send(()).unwrap();
            blocked.recv_timeout(Duration::from_secs(3)).unwrap();
            Ok(TaskControlOutput::Paused)
        }));
        tokio::time::timeout(Duration::from_secs(2), started)
            .await
            .unwrap()
            .unwrap();
        waiter.abort();
        assert!(waiter.await.unwrap_err().is_cancelled());
        assert_eq!(registry.workers.test_entry_count(), 1);
        release.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(2), async {
            while registry.workers.test_entry_count() != 0 {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        registry.join().await.unwrap();
    }
}
