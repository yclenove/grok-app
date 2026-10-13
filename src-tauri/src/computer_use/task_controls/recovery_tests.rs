//! Dropped command waiters do not detach the original recovery worker.
use super::*;
use crate::acp_client::computer_use::TaskViewFixture;

#[tokio::test]
async fn dropped_resume_waiter_retains_native_worker_and_shutdown_fences_before_join() {
    for shutdown_during_capture in [false, true] {
        let f = TaskViewFixture::new().await;
        let (fake, address) = f.native_paused_execution();
        let mut call = f.recovery(&address);
        let cancel = call.cancel_handle();
        let registry = Arc::new(Registry::default());
        let (entered, started) = tokio::sync::oneshot::channel();
        let (release, blocked) = std::sync::mpsc::channel();
        fake.arm_capture_hook(Box::new(move || {
            entered.send(()).unwrap();
            blocked.recv_timeout(Duration::from_secs(3)).unwrap();
        }));
        let workers = registry.clone();
        let waiter = tokio::spawn(run_with_fence(
            workers,
            move || {
                call.prepare_native(Duration::from_secs(1))?;
                call.commit().map(TaskControlOutput::Resumed)
            },
            move || cancel.fence(),
        ));
        tokio::time::timeout(Duration::from_secs(2), started)
            .await
            .unwrap()
            .unwrap();
        waiter.abort();
        assert!(waiter.await.unwrap_err().is_cancelled());
        assert_eq!(registry.workers.test_entry_count(), 1);
        assert!(f.broker().claim_continuation("view-app").is_none());
        if shutdown_during_capture {
            registry.stop();
            let workers = registry.clone();
            let joining = tokio::spawn(async move { workers.join().await });
            tokio::task::yield_now().await;
            assert!(
                !joining.is_finished(),
                "shutdown must retain blocked native handle"
            );
            release.send(()).unwrap();
            tokio::time::timeout(Duration::from_secs(2), joining)
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            assert!(f.broker().claim_continuation("view-app").is_none());
        } else {
            release.send(()).unwrap();
            tokio::time::timeout(Duration::from_secs(2), async {
                while registry.workers.test_entry_count() != 0 {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            })
            .await
            .unwrap();
            assert!(
                f.broker().claim_continuation("view-app").is_some(),
                "losing a UI waiter does not cancel the user's Continue"
            );
            registry.join().await.unwrap();
            assert!(
                f.broker().claim_continuation("view-app").is_some(),
                "late cancellation cannot revoke an already committed recovery"
            );
        }
        assert_eq!(fake.observe_calls(), 1);
        f.finish().await;
    }
}
