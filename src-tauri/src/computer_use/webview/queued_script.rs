//! A unique pre-dispatch owner. Dropping an uninvoked native dispatcher closure
//! proves non-execution; dropping a callback after handoff proves nothing.

use std::sync::mpsc::Sender;

use super::ScriptOperation;

pub(crate) struct QueuedScript {
    operation: Option<ScriptOperation>,
    reply: Sender<Result<String, String>>,
}

impl QueuedScript {
    pub(crate) fn new(operation: ScriptOperation, reply: Sender<Result<String, String>>) -> Self {
        Self {
            operation: Some(operation),
            reply,
        }
    }

    pub(crate) fn check(&self) -> Result<(), String> {
        self.operation
            .as_ref()
            .expect("queued script owns its undispatched operation")
            .check()
    }

    /// Transfer immediately before native dispatch. This guard must not survive
    /// into a native callback: callback destruction is not physical completion.
    pub(crate) fn handoff(mut self) -> ScriptOperation {
        self.operation
            .take()
            .expect("queued script is handed off exactly once")
    }

    pub(crate) fn reject(mut self, error: &str) {
        self.finish_undispatched(error);
    }

    fn finish_undispatched(&mut self, error: &str) {
        if let Some(operation) = self.operation.take() {
            operation.finished();
            let _ = self.reply.send(Err(error.into()));
        }
    }
}

impl Drop for QueuedScript {
    fn drop(&mut self) {
        self.finish_undispatched("WebView native dispatch discarded before execution");
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc::{self, Receiver, TryRecvError};

    use super::*;
    use crate::computer_use::webview::execution::ScriptTracker;

    fn queued(tracker: &ScriptTracker) -> (QueuedScript, Receiver<Result<String, String>>) {
        let operation = tracker
            .begin("old", "view", &Default::default(), &Default::default())
            .unwrap();
        let (tx, rx) = mpsc::channel();
        (QueuedScript::new(operation, tx), rx)
    }

    #[test]
    fn discarded_native_closure_releases_only_undispatched_work() {
        let tracker = ScriptTracker::default();
        let (queued, reply) = queued(&tracker);
        let native_callback = move || queued.handoff();
        assert!(!tracker.is_idle("old"));
        drop(native_callback); // Native window disappeared before dispatch.
        assert!(tracker.is_idle("old"));
        assert!(reply
            .recv()
            .unwrap()
            .unwrap_err()
            .contains("before execution"));
        assert!(matches!(reply.try_recv(), Err(TryRecvError::Disconnected)));
    }

    #[test]
    fn handoff_never_settles_even_when_native_callback_is_lost() {
        let tracker = ScriptTracker::default();
        let (queued, reply) = queued(&tracker);
        let operation = queued.handoff();
        assert!(!tracker.is_idle("old"));
        assert!(matches!(reply.try_recv(), Err(TryRecvError::Disconnected)));
        let witness = operation.clone();
        drop(operation);
        assert!(!tracker.is_idle("old"));
        witness.finished(); // Actual native completion or exact exit witness.
        assert!(tracker.is_idle("old"));
    }

    #[test]
    fn cancelling_or_losing_business_receiver_does_not_discard_native_queue() {
        let tracker = ScriptTracker::default();
        let (queued, reply) = queued(&tracker);
        drop(reply);
        tracker.cancel("old");
        assert!(queued.check().is_err());
        assert!(!tracker.is_idle("old"));
        drop(queued); // Only now is there proof of pre-dispatch abandonment.
        assert!(tracker.is_idle("old"));
    }

    #[test]
    fn rejected_queue_replies_once_and_cannot_settle_a_replacement() {
        let tracker = ScriptTracker::default();
        let (queued, reply) = queued(&tracker);
        let stale = queued.operation.as_ref().unwrap().clone();
        queued.reject("native document retired before execution");
        assert!(reply
            .recv()
            .unwrap()
            .unwrap_err()
            .contains("document retired"));
        assert!(matches!(reply.try_recv(), Err(TryRecvError::Disconnected)));
        let replacement = tracker
            .begin("new", "view", &Default::default(), &Default::default())
            .unwrap();
        stale.finished();
        assert!(!tracker.is_idle("new"));
        assert!(replacement.check().is_ok());
        replacement.finished();
    }

    #[test]
    fn dropping_a_host_command_receiver_rejects_all_unconsumed_tickets() {
        let tracker = ScriptTracker::default();
        let (queued, reply) = queued(&tracker);
        let (commands, receiver) = mpsc::channel();
        assert!(commands.send(queued).is_ok());
        assert!(!tracker.is_idle("old"));
        drop(receiver);
        assert!(tracker.is_idle("old"));
        assert!(reply
            .recv()
            .unwrap()
            .unwrap_err()
            .contains("before execution"));
    }
}
