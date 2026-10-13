//! Bounded original task handles survive cancellation of their command waiter.
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

struct Entry<R> {
    fence: Box<dyn Fn() + Send + Sync>,
    abandoned: AtomicBool,
    task: tokio::sync::Mutex<Option<tauri::async_runtime::JoinHandle<R>>>,
}

struct Waiter<R> {
    entry: Arc<Entry<R>>,
    completed: bool,
}

impl<R> Drop for Waiter<R> {
    fn drop(&mut self) {
        if !self.completed {
            self.entry.abandoned.store(true, Ordering::SeqCst);
        }
    }
}

pub(super) struct OwnedTasks<R> {
    stopping: AtomicBool,
    join_failed: AtomicBool,
    entries: parking_lot::Mutex<HashMap<uuid::Uuid, Arc<Entry<R>>>>,
}

impl<R> Default for OwnedTasks<R> {
    fn default() -> Self {
        Self {
            stopping: AtomicBool::new(false),
            join_failed: AtomicBool::new(false),
            entries: parking_lot::Mutex::new(HashMap::new()),
        }
    }
}

impl<R: Send + 'static> OwnedTasks<R> {
    #[cfg(test)]
    pub(super) fn owner_count_for_test(&self) -> usize {
        self.entries.lock().len()
    }

    #[cfg(test)]
    pub(super) fn singleton_entry_for_test(&self) -> Result<uuid::Uuid, String> {
        let entries = self.entries.lock();
        if entries.len() != 1 {
            return Err("original owned task is not unique".into());
        }
        Ok(*entries.keys().next().unwrap())
    }

    #[cfg(test)]
    pub(super) async fn join_abandoned_for_test(&self, id: uuid::Uuid) -> Result<R, String> {
        let entry = self
            .entries
            .lock()
            .get(&id)
            .cloned()
            .ok_or("original abandoned task missing")?;
        if !entry.abandoned.load(Ordering::SeqCst) {
            return Err("original task still has a live waiter".into());
        }
        let mut slot = entry
            .task
            .try_lock()
            .map_err(|_| "original abandoned task handle still held")?;
        // Keep the exact original handle in the registry across this await,
        // including if the diagnostic join waiter itself is cancelled.
        let result = slot
            .as_mut()
            .ok_or("original abandoned task already consumed")?
            .await
            .map_err(|_| "original abandoned task did not join".to_string());
        if result.is_err() {
            self.join_failed.store(true, Ordering::SeqCst);
        }
        slot.take();
        drop(slot);
        self.entries.lock().remove(&id);
        result
    }

    pub(super) async fn reap(&self) {
        let entries = self
            .entries
            .lock()
            .iter()
            .map(|(id, entry)| (*id, entry.clone()))
            .collect::<Vec<_>>();
        for (id, entry) in entries {
            if !entry.abandoned.load(Ordering::SeqCst) {
                continue;
            }
            let Ok(mut slot) = entry.task.try_lock() else {
                continue;
            };
            if !slot.as_ref().is_some_and(|task| task.inner().is_finished()) {
                continue;
            }
            if let Some(task) = slot.as_mut() {
                if task.await.is_err() {
                    self.join_failed.store(true, Ordering::SeqCst);
                    tracing::warn!("Computer Use owned task failed to join");
                }
            }
            slot.take();
            drop(slot);
            self.entries.lock().remove(&id);
        }
    }

    pub(super) async fn run(
        &self,
        future: impl std::future::Future<Output = R> + Send + 'static,
        fence: impl Fn() + Send + Sync + 'static,
    ) -> Result<R, String> {
        self.reap().await;
        let id = uuid::Uuid::new_v4();
        let entry = {
            let mut entries = self.entries.lock();
            if self.stopping.load(Ordering::SeqCst)
                || self.join_failed.load(Ordering::SeqCst)
                || entries.len() >= 32
            {
                return Err("Computer Use owned task unavailable".into());
            }
            let entry = Arc::new(Entry {
                fence: Box::new(fence),
                abandoned: AtomicBool::new(false),
                task: tokio::sync::Mutex::new(Some(tauri::async_runtime::spawn(future))),
            });
            entries.insert(id, entry.clone());
            entry
        };
        let mut waiter = Waiter {
            entry: entry.clone(),
            completed: false,
        };
        let result = {
            let mut slot = entry.task.lock().await;
            let task = slot.as_mut().ok_or("Computer Use owned task retired")?;
            // A dropped waiter leaves the original handle in its slot. Only
            // abandoned finished tasks may be reaped by a different caller.
            let result = task
                .await
                .map_err(|_| "Computer Use owned task did not join".to_string());
            slot.take();
            result
        };
        waiter.completed = true;
        if result.is_err() {
            self.join_failed.store(true, Ordering::SeqCst);
        }
        self.entries.lock().remove(&id);
        result
    }

    pub(super) fn stop(&self) {
        self.stopping.store(true, Ordering::SeqCst);
        for entry in self.entries.lock().values() {
            (entry.fence)();
        }
    }

    pub(super) async fn join(&self) -> Result<(), String> {
        self.stop();
        // The registry retains every original entry until its real join. A
        // cancelled shutdown waiter must not drop still-running handles.
        let entries = self
            .entries
            .lock()
            .iter()
            .map(|(id, entry)| (*id, entry.clone()))
            .collect::<Vec<_>>();
        let mut failed = self.join_failed.load(Ordering::SeqCst);
        for (id, entry) in entries {
            let mut slot = entry.task.lock().await;
            if let Some(task) = slot.as_mut() {
                if task.await.is_err() {
                    failed = true;
                    self.join_failed.store(true, Ordering::SeqCst);
                }
                slot.take();
            }
            drop(slot);
            self.entries.lock().remove(&id);
        }
        if failed || self.join_failed.load(Ordering::SeqCst) {
            self.join_failed.store(true, Ordering::SeqCst);
            Err("Computer Use owned tasks did not join".into())
        } else {
            Ok(())
        }
    }

    #[cfg(test)]
    pub(super) fn test_entry_count(&self) -> usize {
        self.entries.lock().len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn targeted_abandoned_join_retains_handle_and_preserves_live_results_and_admission() {
        let workers = Arc::new(OwnedTasks::<u32>::default());
        let (entered, started) = tokio::sync::oneshot::channel();
        let (release, blocked) = tokio::sync::oneshot::channel();
        let registry = workers.clone();
        let original = tokio::spawn(async move {
            registry
                .run(
                    async move {
                        entered.send(()).unwrap();
                        blocked.await.unwrap();
                        42
                    },
                    || {},
                )
                .await
        });
        started.await.unwrap();
        let id = workers.singleton_entry_for_test().unwrap();
        assert_eq!(
            workers.join_abandoned_for_test(id).await.unwrap_err(),
            "original task still has a live waiter"
        );
        original.abort();
        assert!(original.await.unwrap_err().is_cancelled());
        let (other_entered, other_started) = tokio::sync::oneshot::channel();
        let (other_release, other_blocked) = tokio::sync::oneshot::channel();
        let registry = workers.clone();
        let live = tokio::spawn(async move {
            registry
                .run(
                    async move {
                        other_entered.send(()).unwrap();
                        other_blocked.await.unwrap();
                        7
                    },
                    || {},
                )
                .await
        });
        other_started.await.unwrap();
        let registry = workers.clone();
        let join = tokio::spawn(async move { registry.join_abandoned_for_test(id).await });
        tokio::task::yield_now().await;
        assert!(!join.is_finished());
        join.abort();
        assert!(join.await.unwrap_err().is_cancelled());
        assert_eq!(workers.owner_count_for_test(), 2);
        let registry = workers.clone();
        let join = tokio::spawn(async move { registry.join_abandoned_for_test(id).await });
        tokio::task::yield_now().await;
        assert!(!join.is_finished());
        release.send(()).unwrap();
        assert_eq!(join.await.unwrap().unwrap(), 42);
        assert_eq!(workers.owner_count_for_test(), 1);
        other_release.send(()).unwrap();
        assert_eq!(live.await.unwrap().unwrap(), 7);
        assert_eq!(workers.run(async { 9 }, || {}).await.unwrap(), 9);
        assert_eq!(workers.owner_count_for_test(), 0);
        workers.join().await.unwrap();
    }

    #[tokio::test]
    async fn targeted_abandoned_join_preserves_join_failure_and_existing_stop() {
        for panic_after_release in [false, true] {
            let workers = Arc::new(OwnedTasks::<()>::default());
            let (entered, started) = tokio::sync::oneshot::channel();
            let (release, blocked) = tokio::sync::oneshot::channel();
            let registry = workers.clone();
            let original = tokio::spawn(async move {
                registry
                    .run(
                        async move {
                            entered.send(()).unwrap();
                            blocked.await.unwrap();
                            assert!(!panic_after_release, "synthetic original producer failure");
                        },
                        || {},
                    )
                    .await
            });
            started.await.unwrap();
            let id = workers.singleton_entry_for_test().unwrap();
            original.abort();
            assert!(original.await.unwrap_err().is_cancelled());
            if !panic_after_release {
                workers.stop();
            }
            release.send(()).unwrap();
            assert_eq!(
                workers.join_abandoned_for_test(id).await.is_err(),
                panic_after_release
            );
            assert_eq!(workers.owner_count_for_test(), 0);
            assert_eq!(
                workers.join_failed.load(Ordering::SeqCst),
                panic_after_release
            );
            assert_eq!(
                workers.stopping.load(Ordering::SeqCst),
                !panic_after_release
            );
            assert!(workers.run(async {}, || {}).await.is_err());
            assert_eq!(workers.join().await.is_err(), panic_after_release);
        }
    }

    #[tokio::test]
    async fn cancelled_waiter_keeps_original_task_until_stop_and_join() {
        let workers = Arc::new(OwnedTasks::<()>::default());
        let (entered, started) = tokio::sync::oneshot::channel();
        let (release, blocked) = tokio::sync::oneshot::channel();
        let fenced = Arc::new(AtomicBool::new(false));
        let counter = fenced.clone();
        let registry = workers.clone();
        let waiter = tokio::spawn(async move {
            registry
                .run(
                    async move {
                        entered.send(()).unwrap();
                        blocked.await.unwrap();
                    },
                    move || {
                        counter.store(true, Ordering::SeqCst);
                    },
                )
                .await
        });
        started.await.unwrap();
        waiter.abort();
        assert!(waiter.await.err().unwrap().is_cancelled());
        workers.stop();
        assert!(fenced.load(Ordering::SeqCst));
        let registry = workers.clone();
        let join = tokio::spawn(async move { registry.join().await });
        tokio::task::yield_now().await;
        assert!(
            !join.is_finished(),
            "original task is still physically blocked"
        );
        release.send(()).unwrap();
        join.await.unwrap().unwrap();
        assert!(workers.entries.lock().is_empty());
        assert!(
            workers.run(async {}, || {}).await.is_err(),
            "shutdown cannot start another worker"
        );
    }

    #[tokio::test]
    async fn a_completed_live_waiter_cannot_have_its_result_stolen_by_reaping() {
        let workers = OwnedTasks::<u32>::default();
        let id = uuid::Uuid::new_v4();
        let entry = Arc::new(Entry {
            fence: Box::new(|| {}),
            abandoned: AtomicBool::new(false),
            task: tokio::sync::Mutex::new(Some(tauri::async_runtime::spawn(async { 42 }))),
        });
        workers.entries.lock().insert(id, entry.clone());
        while !entry
            .task
            .lock()
            .await
            .as_ref()
            .unwrap()
            .inner()
            .is_finished()
        {
            tokio::task::yield_now().await;
        }
        workers.reap().await;
        assert!(workers.entries.lock().contains_key(&id));
        let mut task = entry.task.lock().await;
        assert_eq!(task.as_mut().unwrap().await.unwrap(), 42);
        task.take();
        drop(task);
        workers.entries.lock().remove(&id);
        workers.join().await.unwrap();
    }

    #[tokio::test]
    async fn cancelled_shutdown_waiter_keeps_original_task_for_next_join() {
        let workers = Arc::new(OwnedTasks::<()>::default());
        let (entered, started) = tokio::sync::oneshot::channel();
        let (release, blocked) = tokio::sync::oneshot::channel();
        let registry = workers.clone();
        let waiter = tokio::spawn(async move {
            registry
                .run(
                    async move {
                        entered.send(()).unwrap();
                        blocked.await.unwrap();
                    },
                    || {},
                )
                .await
        });
        tokio::time::timeout(std::time::Duration::from_secs(2), started)
            .await
            .unwrap()
            .unwrap();
        waiter.abort();
        assert!(waiter.await.unwrap_err().is_cancelled());
        let registry = workers.clone();
        let first_join = tokio::spawn(async move { registry.join().await });
        tokio::task::yield_now().await;
        first_join.abort();
        assert!(first_join.await.unwrap_err().is_cancelled());
        assert_eq!(workers.test_entry_count(), 1);
        let registry = workers.clone();
        let next_join = tokio::spawn(async move { registry.join().await });
        tokio::task::yield_now().await;
        assert!(!next_join.is_finished());
        release.send(()).unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(2), next_join)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(workers.test_entry_count(), 0);
    }

    #[tokio::test]
    async fn auto_reaped_join_failure_is_retained_and_prevents_new_work() {
        let workers = Arc::new(OwnedTasks::<()>::default());
        let (entered, started) = tokio::sync::oneshot::channel();
        let (release, blocked) = tokio::sync::oneshot::channel();
        let registry = workers.clone();
        let waiter = tokio::spawn(async move {
            registry
                .run(
                    async move {
                        entered.send(()).unwrap();
                        blocked.await.unwrap();
                        panic!("synthetic owned task panic");
                    },
                    || {},
                )
                .await
        });
        tokio::time::timeout(std::time::Duration::from_secs(2), started)
            .await
            .unwrap()
            .unwrap();
        waiter.abort();
        assert!(waiter.await.unwrap_err().is_cancelled());
        release.send(()).unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            while workers.test_entry_count() != 0 {
                workers.reap().await;
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert!(workers.run(async {}, || {}).await.is_err());
        assert!(workers.join().await.is_err());
    }
}
