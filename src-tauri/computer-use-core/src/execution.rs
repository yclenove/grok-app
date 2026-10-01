//! Cancellation travels with an admitted action, including after a caller times out.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[derive(Clone, Debug, Default)]
pub struct ActionCancellation(Arc<CancellationState>);

#[derive(Debug, Default)]
struct CancellationState {
    cancelled: AtomicBool,
    changed: tokio::sync::Notify,
}

impl ActionCancellation {
    pub fn cancel(&self) {
        self.0.cancelled.store(true, Ordering::SeqCst);
        self.0.changed.notify_waiters();
    }

    pub fn check(&self) -> Result<(), String> {
        if self.0.cancelled.load(Ordering::SeqCst) {
            Err("action cancelled; dispatch revoked".into())
        } else {
            Ok(())
        }
    }

    /// Sticky notification shared by every clone. Register before reading the
    /// flag so cancellation racing the check cannot leave a waiter asleep.
    pub async fn cancelled(&self) {
        loop {
            let changed = self.0.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if self.0.cancelled.load(Ordering::SeqCst) {
                return;
            }
            changed.await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn cancellation_wakes_all_clones_and_late_waiters() {
        let token = ActionCancellation::default();
        let mut waiters = Vec::new();
        for _ in 0..32 {
            let clone = token.clone();
            waiters.push(tokio::spawn(async move { clone.cancelled().await }));
        }
        tokio::task::yield_now().await;
        token.cancel();
        token.cancel();
        for waiter in waiters {
            tokio::time::timeout(Duration::from_secs(1), waiter)
                .await
                .expect("all listeners wake")
                .unwrap();
        }
        tokio::time::timeout(Duration::from_secs(1), token.cancelled())
            .await
            .expect("late listener sees sticky cancellation");
        assert!(token.check().is_err());
        assert!(ActionCancellation::default().check().is_ok());
    }
}
