//! One connection's turn boundary, duplicate notifications and idle fallback.
use gcu_core::session::Session;
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{sync::Notify, time::Instant};

/// Architecture §8 fallback when a client has no turn hook.
pub const DEFAULT_TURN_IDLE: Duration = Duration::from_secs(45);

struct State {
    active: usize,
    open: bool,
    last: Instant,
    timeout: Duration,
    ended: VecDeque<String>,
}
/// Session-local lifecycle handle. Never clears the Host's Stop latch.
#[derive(Clone)]
pub struct Lifecycle(Arc<Inner>);
struct Inner {
    session: Arc<Session>,
    state: Mutex<State>,
    transition: tokio::sync::Mutex<()>,
    changed: Notify,
    closed: gcu_host_api::Cancel,
}
impl Lifecycle {
    pub(crate) fn new(session: Arc<Session>) -> Self {
        Self(Arc::new(Inner {
            session,
            state: Mutex::new(State {
                active: 0,
                open: false,
                last: Instant::now(),
                timeout: DEFAULT_TURN_IDLE,
                ended: VecDeque::new(),
            }),
            transition: tokio::sync::Mutex::new(()),
            changed: Notify::new(),
            closed: gcu_host_api::Cancel::new(),
        }))
    }
    /// Configure the idle fallback. Zero is clamped to one millisecond.
    /// Memory only; briefly locks state, with no timeout or cancellation.
    pub fn set_idle_timeout(&self, timeout: Duration) {
        self.0
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .timeout = timeout.max(Duration::from_millis(1));
        self.0.changed.notify_one();
    }
    pub(crate) fn active(&self) -> Active {
        self.0
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .active += 1;
        self.0.changed.notify_one();
        Active(self.clone())
    }
    pub(crate) async fn begin_turn(&self) -> Result<(), gcu_core::session::SessionClosed> {
        let _transition = self.0.transition.lock().await;
        self.0.session.begin_turn()?;
        self.0.state.lock().unwrap_or_else(|e| e.into_inner()).open = true;
        self.0.changed.notify_one();
        Ok(())
    }
    /// End this connection's turn, retiring Turn grants and releasing input.
    /// Repeated event IDs (bounded to the latest 128) are ignored, including
    /// after a new turn starts. Without an ID, duplicates before new work are
    /// ignored. Waits on the transition lock and native release (at most five
    /// seconds); cancellation drops the wait, never restores grants or Stop.
    pub async fn turn_ended(&self, event_id: Option<&str>) {
        self.end(event_id, false).await;
    }
    async fn end(&self, event_id: Option<&str>, idle: bool) {
        let _transition = self.0.transition.lock().await;
        {
            let mut state = self.0.state.lock().unwrap_or_else(|e| e.into_inner());
            if self.0.session.is_closed() || self.0.closed.is_cancelled() {
                return;
            }
            if idle && (state.active != 0 || state.last.elapsed() < state.timeout) {
                return;
            }
            if let Some(id) = event_id {
                if state.ended.iter().any(|old| old == id) {
                    return;
                }
                // Only a small opaque identifier is retained, never hook payloads.
                if id.len() > 128 {
                    return;
                }
                state.ended.push_back(id.to_owned());
                if state.ended.len() > 128 {
                    state.ended.pop_front();
                }
            }
            if !state.open {
                return;
            }
            state.open = false;
        }
        self.0.session.turn_ended().await;
        self.0.changed.notify_one();
    }
    pub(crate) fn close(&self) {
        self.0.closed.cancel();
    }
    pub(crate) async fn idle(self) {
        loop {
            let deadline = {
                let state = self.0.state.lock().unwrap_or_else(|e| e.into_inner());
                (state.open && state.active == 0).then(|| state.last + state.timeout)
            };
            tokio::select! {
                _ = self.0.closed.cancelled() => return,
                _ = self.0.changed.notified() => (),
                _ = async {
                    if let Some(deadline) = deadline { tokio::time::sleep_until(deadline).await; }
                    else { std::future::pending::<()>().await; }
                } => self.end(None, true).await,
            }
        }
    }
}
pub(crate) struct Active(Lifecycle);
impl Drop for Active {
    fn drop(&mut self) {
        let mut state = self.0 .0.state.lock().unwrap_or_else(|e| e.into_inner());
        state.last = Instant::now();
        state.active -= 1;
        drop(state);
        self.0 .0.changed.notify_one();
    }
}

#[cfg(test)]
#[path = "lifecycle_tests.rs"]
mod tests;
