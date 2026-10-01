//! Native script occupancy survives a business timeout, unbind and replacement.
//! Only actual completion, proven native exit or pre-dispatch rejection settles a ticket.
//! Dropping a caller/callback, cancellation and navigation are not completion.

use std::collections::HashMap;
use std::sync::Arc;

use grok_computer_use_core::execution::ActionCancellation;
use parking_lot::Mutex;
use uuid::Uuid;

const MAX_PENDING: usize = 64;

struct Pending {
    run: String,
    native: String,
    cancellation: ActionCancellation,
}

#[derive(Clone, Default)]
pub(super) struct ScriptTracker(Arc<Mutex<HashMap<Uuid, Pending>>>);

/// Native backend-owned completion ticket. It deliberately has no Drop cleanup.
#[derive(Clone)]
pub struct ScriptOperation {
    id: Uuid,
    tracker: ScriptTracker,
    cancellation: ActionCancellation,
    caller: ActionCancellation,
    binding: ActionCancellation,
}

impl ScriptTracker {
    pub fn begin(
        &self,
        run: &str,
        native: &str,
        caller: &ActionCancellation,
        binding: &ActionCancellation,
    ) -> Result<ScriptOperation, String> {
        caller.check()?;
        binding.check()?;
        let mut pending = self.0.lock();
        if pending.values().any(|entry| entry.native == native) {
            return Err("WebView native execution is still pending".into());
        }
        if pending.len() >= MAX_PENDING {
            return Err("WebView native execution capacity exhausted".into());
        }
        let id = Uuid::new_v4();
        let cancellation = ActionCancellation::default();
        pending.insert(
            id,
            Pending {
                run: run.into(),
                native: native.into(),
                cancellation: cancellation.clone(),
            },
        );
        Ok(ScriptOperation {
            id,
            tracker: self.clone(),
            cancellation,
            caller: caller.clone(),
            binding: binding.clone(),
        })
    }

    pub fn cancel(&self, run: &str) {
        for entry in self.0.lock().values().filter(|entry| entry.run == run) {
            entry.cancellation.cancel();
        }
    }

    pub fn is_idle(&self, run: &str) -> bool {
        !self.0.lock().values().any(|entry| entry.run == run)
    }

    pub fn native_is_idle(&self, native: &str) -> bool {
        !self.0.lock().values().any(|entry| entry.native == native)
    }
}

impl ScriptOperation {
    /// Check again on the native dispatch thread, not only before queueing it.
    pub(crate) fn check(&self) -> Result<(), String> {
        self.cancellation.check()?;
        self.caller.check()?;
        self.binding.check()?;
        if !self.tracker.0.lock().contains_key(&self.id) {
            return Err("WebView native execution already settled".into());
        }
        Ok(())
    }

    /// Call only at native completion, proven native exit or before dispatch.
    /// Never call merely because the response receiver timed out/disconnected.
    pub(crate) fn finished(&self) {
        self.tracker.0.lock().remove(&self.id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timeout_drop_and_cancel_do_not_release_native_occupancy() {
        let tracker = ScriptTracker::default();
        let operation = tracker
            .begin("old", "view", &Default::default(), &Default::default())
            .unwrap();
        let callback = operation.clone();
        drop(operation);
        tracker.cancel("old");
        assert!(!tracker.is_idle("old"));
        assert!(callback.check().is_err());
        assert!(tracker
            .begin("new", "view", &Default::default(), &Default::default())
            .is_err());
        callback.finished();
        assert!(tracker.is_idle("old"));
        let replacement = tracker
            .begin("new", "view", &Default::default(), &Default::default())
            .unwrap();
        callback.finished(); // A duplicate old completion cannot settle the replacement.
        assert!(!tracker.is_idle("new"));
        replacement.finished();
        assert!(tracker.is_idle("new"));
    }

    #[test]
    fn cancellation_is_exact_run_and_native_instance_scoped() {
        let tracker = ScriptTracker::default();
        let a = tracker
            .begin("a", "native-a", &Default::default(), &Default::default())
            .unwrap();
        let b = tracker
            .begin("b", "native-b", &Default::default(), &Default::default())
            .unwrap();
        tracker.cancel("a");
        assert!(a.check().is_err());
        assert!(b.check().is_ok());
        assert!(!tracker.is_idle("a"));
        assert!(!tracker.is_idle("b"));
        a.finished();
        assert!(b.check().is_ok());
        b.finished();
    }

    #[test]
    fn caller_cancellation_reaches_already_queued_native_dispatch() {
        let tracker = ScriptTracker::default();
        let caller = ActionCancellation::default();
        let queued = tracker
            .begin("run", "view", &caller, &Default::default())
            .unwrap();
        caller.cancel();
        assert!(queued.check().is_err());
        assert!(!tracker.is_idle("run"));
        queued.finished(); // Native dispatch thread has rejected it before executing.
        assert!(tracker.is_idle("run"));
        assert!(tracker
            .begin("run", "view", &caller, &Default::default())
            .is_err());
    }

    #[test]
    fn lost_callbacks_are_bounded_but_never_expired_into_false_idle() {
        let tracker = ScriptTracker::default();
        for n in 0..MAX_PENDING {
            drop(
                tracker
                    .begin(
                        "run",
                        &format!("view-{n}"),
                        &Default::default(),
                        &Default::default(),
                    )
                    .unwrap(),
            );
        }
        assert!(!tracker.is_idle("run"));
        assert!(tracker
            .begin("run", "overflow", &Default::default(), &Default::default())
            .is_err());
        assert_eq!(tracker.0.lock().len(), MAX_PENDING);
    }
}
