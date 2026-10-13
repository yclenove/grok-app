//! Bounded metadata observers. Releasing one never fences task authority.
use grok_computer_use_core::broker::{HostTaskChanges, HostTaskView};
use serde::Serialize;
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, OnceLock,
    },
    time::Duration,
};
use tokio::sync::{Mutex, Notify};

const MAX_SUBSCRIPTIONS: usize = 32;
const HEALTH_TICK: Duration = Duration::from_secs(1);
const MIN_UPDATE_INTERVAL: Duration = Duration::from_millis(100);
const DELIVERY_BUDGET: Duration = Duration::from_secs(4);

pub(crate) struct TaskWatchSource {
    pub(crate) changes: HostTaskChanges,
    pub(crate) host_instance_id: String,
    pub(crate) read: Arc<dyn Fn() -> Result<HostTaskView, String> + Send + Sync>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TaskSubscriptionStarted {
    pub(crate) subscription_id: String,
    pub(crate) session_id: String,
    pub(crate) connection_id: String,
    pub(crate) host_instance_id: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum TaskEventPayload {
    View { view: HostTaskView },
    Heartbeat { view: HostTaskView },
    Retired,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TaskEvent {
    #[serde(flatten)]
    pub(crate) owner: TaskSubscriptionStarted,
    /// Order only, never a grant. Strings preserve the full u64 in JS.
    pub(crate) sequence: String,
    #[serde(flatten)]
    pub(crate) payload: TaskEventPayload,
}

type EventSink = Arc<dyn Fn(TaskEvent) -> Result<(), String> + Send + Sync>;

#[derive(Default)]
struct Cancel {
    set: AtomicBool,
    wake: Notify,
}

impl Cancel {
    fn stop(&self) {
        self.set.store(true, Ordering::SeqCst);
        self.wake.notify_waiters();
    }

    async fn cancelled(&self) {
        loop {
            let wake = self.wake.notified();
            tokio::pin!(wake);
            wake.as_mut().enable();
            if self.set.load(Ordering::SeqCst) {
                return;
            }
            wake.await;
        }
    }
}

struct Entry {
    owner: TaskSubscriptionStarted,
    cancel: Arc<Cancel>,
    ack: Arc<Acknowledgement>,
    task: Mutex<Option<tauri::async_runtime::JoinHandle<()>>>,
}

/// One outstanding view per Channel. A frozen/closed renderer cannot build
/// an unbounded IPC backlog while the Broker keeps coalescing changes.
#[derive(Default)]
struct Acknowledgement {
    sent: AtomicU64,
    received: AtomicU64,
    wake: Notify,
}

impl Acknowledgement {
    fn accept(&self, sequence: u64) -> Result<(), String> {
        if sequence == 0 || self.sent.load(Ordering::SeqCst) != sequence {
            return Err("Computer Use observer sequence differs".into());
        }
        self.received.fetch_max(sequence, Ordering::SeqCst);
        self.wake.notify_waiters();
        Ok(())
    }

    async fn delivered(&self, sequence: u64) {
        loop {
            let wake = self.wake.notified();
            tokio::pin!(wake);
            wake.as_mut().enable();
            if self.received.load(Ordering::SeqCst) >= sequence {
                return;
            }
            wake.await;
        }
    }
}

#[derive(Default)]
struct Subscriptions {
    stopping: AtomicBool,
    entries: Arc<parking_lot::Mutex<HashMap<String, Arc<Entry>>>>,
    reaper: Mutex<Option<tauri::async_runtime::JoinHandle<()>>>,
    reaper_cancel: Arc<Cancel>,
    reaper_failed: Arc<AtomicBool>,
}

fn subscriptions() -> &'static Subscriptions {
    static REGISTRY: OnceLock<Subscriptions> = OnceLock::new();
    REGISTRY.get_or_init(Subscriptions::default)
}

pub(crate) async fn subscribe(
    session: &str,
    source: TaskWatchSource,
    sink: EventSink,
) -> Result<TaskSubscriptionStarted, String> {
    subscriptions().start(session, source, sink).await
}

pub(crate) async fn unsubscribe(session: &str, subscription: &str) -> Result<bool, String> {
    subscriptions().remove(session, subscription).await
}

pub(crate) fn acknowledge(session: &str, subscription: &str, sequence: &str) -> Result<(), String> {
    subscriptions().acknowledge(session, subscription, sequence)
}

pub(super) fn stop() {
    subscriptions().stop();
}

pub(super) async fn join() -> Result<(), String> {
    subscriptions().join().await
}

impl Subscriptions {
    async fn ensure_reaper(&self) -> Result<(), String> {
        let mut slot = self.reaper.lock().await;
        if self.stopping.load(Ordering::SeqCst) || self.reaper_failed.load(Ordering::SeqCst) {
            return Err("Computer Use observers are stopping or failed to join".into());
        }
        if slot.is_none() {
            let entries = self.entries.clone();
            let cancel = self.reaper_cancel.clone();
            let failed = self.reaper_failed.clone();
            // One owned reaper, never one detached cleanup task per event.
            *slot = Some(tauri::async_runtime::spawn(async move {
                let mut tick = tokio::time::interval(Duration::from_millis(250));
                tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                loop {
                    tokio::select! {
                        biased;
                        _ = cancel.cancelled() => return,
                        _ = tick.tick() => {},
                    }
                    let snapshot = entries.lock().values().cloned().collect::<Vec<_>>();
                    for entry in snapshot {
                        let Ok(mut task) = entry.task.try_lock() else {
                            continue;
                        };
                        if !task.as_ref().is_some_and(|task| task.inner().is_finished()) {
                            continue;
                        }
                        if task.as_mut().expect("finished observer").await.is_err() {
                            failed.store(true, Ordering::SeqCst);
                        }
                        task.take();
                        entries.lock().remove(&entry.owner.subscription_id);
                    }
                }
            }));
        }
        Ok(())
    }

    async fn reap(&self) -> Result<(), String> {
        let entries = self.entries.lock().values().cloned().collect::<Vec<_>>();
        for entry in entries {
            let Ok(mut slot) = entry.task.try_lock() else {
                continue;
            };
            if !slot.as_ref().is_some_and(|task| task.inner().is_finished()) {
                continue;
            }
            // Borrow the retained handle while awaiting. Cancelling this
            // caller cannot discard the original worker before its join.
            let result = slot.as_mut().expect("finished task").await;
            slot.take();
            self.entries.lock().remove(&entry.owner.subscription_id);
            result.map_err(|_| "Computer Use observer failed to join".to_string())?;
        }
        Ok(())
    }

    async fn start(
        &self,
        session: &str,
        source: TaskWatchSource,
        sink: EventSink,
    ) -> Result<TaskSubscriptionStarted, String> {
        self.reap().await?;
        self.ensure_reaper().await?;
        // Save version BEFORE the initial read. A change in the read/start
        // gap remains visible to changed_after; no runtime is bootstrapped.
        let version = source.changes.version();
        let initial = (source.read)()?;
        if initial.session_id != session {
            return Err("Computer Use observer owner differs".into());
        }
        let owner = TaskSubscriptionStarted {
            subscription_id: uuid::Uuid::new_v4().to_string(),
            session_id: session.into(),
            connection_id: initial.connection_id.clone(),
            host_instance_id: source.host_instance_id.clone(),
        };
        let cancel = Arc::new(Cancel::default());
        let ack = Arc::new(Acknowledgement::default());
        let mut entries = self.entries.lock();
        if self.stopping.load(Ordering::SeqCst) || entries.len() >= MAX_SUBSCRIPTIONS {
            return Err("Computer Use observer capacity unavailable".into());
        }
        let task = tauri::async_runtime::spawn(stream(
            owner.clone(),
            source,
            initial,
            version,
            sink,
            cancel.clone(),
            ack.clone(),
        ));
        entries.insert(
            owner.subscription_id.clone(),
            Arc::new(Entry {
                owner: owner.clone(),
                cancel,
                ack,
                task: Mutex::new(Some(task)),
            }),
        );
        // No await after insertion: a cancelled start waiter cannot leave a
        // worker between registry insertion and its returned identity.
        Ok(owner)
    }

    async fn join_entry(&self, entry: &Arc<Entry>) -> Result<(), String> {
        entry.cancel.stop();
        let mut slot = entry.task.lock().await;
        let result = match slot.as_mut() {
            Some(task) => task
                .await
                .map_err(|_| "Computer Use observer failed to join".to_string()),
            None => Ok(()),
        };
        slot.take();
        self.entries.lock().remove(&entry.owner.subscription_id);
        result
    }

    fn acknowledge(&self, session: &str, subscription: &str, sequence: &str) -> Result<(), String> {
        let entries = self.entries.lock();
        let entry = entries
            .get(subscription)
            .filter(|entry| entry.owner.session_id == session)
            .ok_or("Computer Use observer owner unavailable")?;
        let value = sequence
            .parse::<u64>()
            .map_err(|_| "Computer Use observer sequence invalid")?;
        if value.to_string() != sequence {
            return Err("Computer Use observer sequence invalid".into());
        }
        entry.ack.accept(value)
    }

    async fn remove(&self, session: &str, subscription: &str) -> Result<bool, String> {
        let entry = self.entries.lock().get(subscription).cloned();
        let Some(entry) = entry else {
            return Ok(false);
        };
        if entry.owner.session_id != session {
            return Err("Computer Use observer owner differs".into());
        }
        self.join_entry(&entry).await?;
        Ok(true)
    }

    fn stop(&self) {
        self.stopping.store(true, Ordering::SeqCst);
        self.reaper_cancel.stop();
        for entry in self.entries.lock().values() {
            entry.cancel.stop();
        }
    }

    async fn join(&self) -> Result<(), String> {
        self.stop();
        let mut reaper = self.reaper.lock().await;
        if let Some(task) = reaper.as_mut() {
            if task.await.is_err() {
                self.reaper_failed.store(true, Ordering::SeqCst);
            }
            reaper.take();
        }
        drop(reaper);
        let entries = self.entries.lock().values().cloned().collect::<Vec<_>>();
        let mut failed = self.reaper_failed.load(Ordering::SeqCst);
        for entry in entries {
            failed |= self.join_entry(&entry).await.is_err();
        }
        if failed {
            Err("Computer Use observers failed to join".into())
        } else {
            Ok(())
        }
    }
}

async fn stream(
    owner: TaskSubscriptionStarted,
    source: TaskWatchSource,
    initial: HostTaskView,
    mut observed: u64,
    sink: EventSink,
    cancel: Arc<Cancel>,
    ack: Arc<Acknowledgement>,
) {
    let mut sequence = 0_u64;
    let mut send = |payload| {
        let next = sequence.checked_add(1)?;
        sequence = next;
        ack.sent.store(next, Ordering::SeqCst);
        sink(TaskEvent {
            owner: owner.clone(),
            sequence: sequence.to_string(),
            payload,
        })
        .ok()
        .map(|()| next)
    };
    if cancel.set.load(Ordering::SeqCst) {
        return;
    }
    let Some(mut outstanding) = send(TaskEventPayload::View { view: initial }) else {
        return;
    };
    let mut last = tokio::time::Instant::now();
    let mut health = tokio::time::interval_at(last + HEALTH_TICK, HEALTH_TICK);
    health.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            biased;
            _ = cancel.cancelled() => return,
            result = tokio::time::timeout(DELIVERY_BUDGET, ack.delivered(outstanding)) => {
                if result.is_err() {
                    // At most one final retired event beyond the outstanding
                    // view. Never queue more views while waiting for a UI ack.
                    send(TaskEventPayload::Retired);
                    return;
                }
            },
        }
        let changed = tokio::select! {
            biased;
            _ = cancel.cancelled() => return,
            _ = source.changes.changed_after(observed) => true,
            _ = health.tick() => false,
        };
        tokio::select! {
            biased;
            _ = cancel.cancelled() => return,
            _ = tokio::time::sleep_until(last + MIN_UPDATE_INTERVAL) => {},
        }
        // Read version first again. A mutation during metadata read is never
        // absorbed into the version of an older view.
        observed = source.changes.version();
        let payload = match (source.read)() {
            Ok(view)
                if view.session_id == owner.session_id
                    && view.connection_id == owner.connection_id =>
            {
                if changed {
                    TaskEventPayload::View { view }
                } else {
                    TaskEventPayload::Heartbeat { view }
                }
            }
            _ => {
                send(TaskEventPayload::Retired);
                return;
            }
        };
        if cancel.set.load(Ordering::SeqCst) {
            return;
        }
        let Some(next) = send(payload) else {
            return;
        };
        outstanding = next;
        last = tokio::time::Instant::now();
    }
}

#[cfg(test)]
mod tests;
