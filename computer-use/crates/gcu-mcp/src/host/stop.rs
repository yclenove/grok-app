//! Stop is independent of approvals/actions; only a trusted user can clear it.
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};
/// Injectable engine-side physical input source. Platform guard cards provide
/// listen-only taps/hooks/XInput2. Host protocol carries no activity updates.
pub trait PhysicalActivity: Send + Sync {
    /// Nonblocking age of last physical input. None means unavailable; callers
    /// may degrade the activity guard but must preserve foreground policy.
    fn ms_since_physical_input(&self) -> Option<u64>;
}
/// Default until a platform driver supplies its physical input guard.
pub struct UnavailableActivity;
impl PhysicalActivity for UnavailableActivity {
    fn ms_since_physical_input(&self) -> Option<u64> {
        None
    }
}
/// Engine view of a Host [`gcu_host_api::UserActivity`].
///
/// Only the physical-input age is forwarded. Stop bits stay on the Host latch.
pub struct UserActivityAdapter {
    inner: Arc<dyn gcu_host_api::UserActivity>,
}
impl UserActivityAdapter {
    /// Wrap one host activity source.
    #[must_use]
    pub fn new(inner: Arc<dyn gcu_host_api::UserActivity>) -> Self {
        Self { inner }
    }
}
impl PhysicalActivity for UserActivityAdapter {
    fn ms_since_physical_input(&self) -> Option<u64> {
        self.inner.ms_since_physical_input()
    }
}
/// Engine mirror of App's persistent Stop; shared across every remote session.
#[derive(Clone)]
pub struct StopMirror(Arc<State>);
struct State {
    all: AtomicBool,
    sessions: Mutex<HashMap<String, bool>>,
    sequence: Mutex<(u64, [u8; 32])>,
}
impl StopMirror {
    /// Initialize from the authenticated Host handshake; never assume resumed
    /// after engine restart. Nonblocking, no IO.
    pub fn new(stopped: bool) -> Self {
        Self(Arc::new(State {
            all: AtomicBool::new(stopped),
            sessions: Mutex::new(HashMap::new()),
            sequence: Mutex::new((0, [0; 32])),
        }))
    }
    /// Latch synchronously before awaiting input release. A global Stop includes
    /// sessions opened later. No action/approval queue is acquired.
    pub fn latch(&self, session: Option<&str>) {
        if let Some(id) = session {
            self.0
                .sessions
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(id.into(), true);
        } else {
            self.0.all.store(true, Ordering::SeqCst);
            self.0
                .sessions
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clear();
        }
    }
    /// Global latch only. A single-session Stop does not count.
    ///
    /// Nonblocking. Does not clear the latch.
    pub fn globally_stopped(&self) -> bool {
        self.0.all.load(Ordering::SeqCst)
    }
    /// Check the Host mirror; reads never consume it. Nonblocking.
    pub fn stopped(&self, session: &str) -> bool {
        self.0
            .sessions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(session)
            .copied()
            .unwrap_or_else(|| self.0.all.load(Ordering::SeqCst))
    }
    /// Consume a fresh trusted UI event from the authenticated stop connection.
    /// Never call from ordinary RPC or MCP. Returns false for stale sequences,
    /// zero/repeated events. No waits; replay cannot clear another Stop.
    pub fn resume_by_user(&self, session: Option<&str>, sequence: u64, event: [u8; 32]) -> bool {
        let mut previous = self.0.sequence.lock().unwrap_or_else(|e| e.into_inner());
        if sequence <= previous.0 || event == [0; 32] || event == previous.1 {
            return false;
        }
        *previous = (sequence, event);
        self.clear(session);
        true
    }
    pub(super) fn clear(&self, session: Option<&str>) {
        let mut sessions = self.0.sessions.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(id) = session {
            sessions.insert(id.into(), false);
        } else {
            self.0.all.store(false, Ordering::SeqCst);
            sessions.clear();
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stop_scope_and_fresh_user_events_preserve_host_latch() {
        let stop = StopMirror::new(true);
        assert!(stop.globally_stopped());
        assert!(stop.stopped("new"));
        assert!(!stop.resume_by_user(None, 0, [1; 32]));
        assert!(!stop.resume_by_user(None, 1, [0; 32]));
        assert!(stop.resume_by_user(Some("a"), 1, [1; 32]));
        assert!(!stop.stopped("a"));
        assert!(stop.stopped("b"));
        assert!(stop.globally_stopped());
        stop.latch(Some("a"));
        assert!(stop.stopped("a"));
        assert!(!stop.resume_by_user(None, 1, [2; 32]));
        assert!(!stop.resume_by_user(None, 2, [1; 32]));
        assert!(stop.resume_by_user(None, 2, [2; 32]));
        assert!(!stop.stopped("future"));
        stop.latch(None);
        assert!(stop.stopped("a"));
        assert!(stop.stopped("future"));
        assert!(UnavailableActivity.ms_since_physical_input().is_none());
        let adapter = UserActivityAdapter::new(Arc::new(Clock(Some(4))));
        assert_eq!(adapter.ms_since_physical_input(), Some(4));
    }
    struct Clock(Option<u64>);
    impl gcu_host_api::UserActivity for Clock {
        fn ms_since_physical_input(&self) -> Option<u64> {
            self.0
        }
        fn stop_requested(&self) -> bool {
            false
        }
        fn latched_stop(&self) -> bool {
            false
        }
        fn clear_stop_by_user(&self) {}
    }
}
