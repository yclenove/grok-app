//! One owned cleanup task per in-flight attempt. IPC waiter cancellation never
//! cancels the cleanup or makes a second caller observe fabricated success.
use std::future::Future;
use std::sync::Mutex;
use tokio::sync::watch;

type Report = Result<(), String>;
type Receiver = watch::Receiver<Option<Report>>;

pub(super) struct ShutdownGate {
    current: Mutex<Option<Receiver>>,
}

impl ShutdownGate {
    pub(super) const fn new() -> Self {
        Self {
            current: Mutex::new(None),
        }
    }

    pub(super) async fn run<F>(&self, cleanup: F) -> Report
    where
        F: Future<Output = Report> + Send + 'static,
    {
        let mut receiver = {
            let mut current = self
                .current
                .lock()
                .map_err(|_| "App update cleanup gate unavailable")?;
            match current.as_ref() {
                Some(receiver) if receiver.borrow().is_none() && receiver.has_changed().is_ok() => {
                    receiver.clone()
                }
                _ => {
                    let (sender, receiver) = watch::channel(None);
                    *current = Some(receiver.clone());
                    // Owned task outlives a dropped command future. Panic drops
                    // sender; receivers report failure, never completion.
                    tokio::spawn(async move {
                        sender.send_replace(Some(cleanup.await));
                    });
                    receiver
                }
            }
        };
        loop {
            if let Some(report) = receiver.borrow().clone() {
                return report;
            }
            receiver
                .changed()
                .await
                .map_err(|_| "App update cleanup task ended without a completion report")?;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use tokio::sync::oneshot;

    #[tokio::test]
    async fn concurrent_callers_share_the_real_failure_not_early_success() {
        let gate = ShutdownGate::new();
        let calls = AtomicUsize::new(0);
        let result = tokio::join!(
            gate.run(async {
                tokio::task::yield_now().await;
                Err("native cleanup pending".into())
            }),
            async {
                let unexpected = Arc::new(AtomicUsize::new(0));
                let used = unexpected.clone();
                let result = gate
                    .run(async move {
                        used.fetch_add(1, Ordering::SeqCst);
                        Ok(())
                    })
                    .await;
                calls.store(unexpected.load(Ordering::SeqCst), Ordering::SeqCst);
                result
            }
        );
        assert_eq!(result.0, Err("native cleanup pending".into()));
        assert_eq!(result.1, result.0);
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn failed_attempt_is_retryable_and_success_is_not_a_permanent_bypass() {
        let gate = ShutdownGate::new();
        assert_eq!(
            gate.run(async { Err("pending".into()) }).await,
            Err("pending".into())
        );
        assert_eq!(gate.run(async { Ok(()) }).await, Ok(()));
        // A later command must recheck: the App can still be alive after a
        // relaunch error, with newly active services or authority.
        assert_eq!(
            gate.run(async { Err("new work".into()) }).await,
            Err("new work".into())
        );
    }

    #[tokio::test]
    async fn canceling_a_waiter_does_not_release_the_original_cleanup() {
        let gate = Arc::new(ShutdownGate::new());
        let (started, observed) = oneshot::channel();
        let (release, released) = oneshot::channel();
        let first_gate = gate.clone();
        let first = tokio::spawn(async move {
            first_gate
                .run(async move {
                    started.send(()).unwrap();
                    released.await.unwrap();
                    Err("original result".into())
                })
                .await
        });
        observed.await.unwrap();
        first.abort();
        assert!(first.await.unwrap_err().is_cancelled());
        let second = gate.run(async { panic!("must not start a replacement") });
        tokio::pin!(second);
        assert!(matches!(
            futures_util::poll!(&mut second),
            std::task::Poll::Pending
        ));
        release.send(()).unwrap();
        assert_eq!(second.await, Err("original result".into()));
    }

    #[tokio::test]
    async fn panic_is_a_failed_report_and_allows_an_explicit_new_attempt() {
        let gate = ShutdownGate::new();
        assert!(gate
            .run(async { panic!("owned cleanup failure") })
            .await
            .is_err());
        assert_eq!(gate.run(async { Ok(()) }).await, Ok(()));
    }
}
