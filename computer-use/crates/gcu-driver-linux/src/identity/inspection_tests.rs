use super::*;

// Channels make the worker's progress deterministic; no sleeps or CPU-speed assumptions.
#[tokio::test(start_paused = true)]
async fn async_inspection_timeout_cancels_blocking_worker() {
    let (started, ready) = tokio::sync::oneshot::channel();
    let (release, wait) = std::sync::mpsc::channel();
    let (finished, done) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(run_async(move |control| {
        let _ = started.send(());
        wait.recv().unwrap();
        let result = control.check();
        let _ = finished.send(result.as_ref().unwrap_err().kind());
        result
    }));
    ready.await.unwrap();
    // Tokio timers round deadlines to millisecond ticks. Cross that boundary explicitly.
    tokio::time::advance(INSPECTION_TIMEOUT + Duration::from_millis(1)).await;
    assert_eq!(
        task.await.unwrap().unwrap_err().kind(),
        io::ErrorKind::TimedOut
    );
    release.send(()).unwrap();
    assert_eq!(done.await.unwrap(), io::ErrorKind::Interrupted);
}

#[tokio::test]
async fn dropped_inspection_cancels_detached_hash_worker() {
    let (started, ready) = tokio::sync::oneshot::channel();
    let (release, wait) = std::sync::mpsc::channel();
    let (finished, done) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(run_async(move |control| {
        let _ = started.send(());
        wait.recv().unwrap();
        let result = control.check();
        let _ = finished.send(result.as_ref().unwrap_err().kind());
        result
    }));
    ready.await.unwrap();
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    release.send(()).unwrap();
    assert_eq!(done.await.unwrap(), io::ErrorKind::Interrupted);
}
