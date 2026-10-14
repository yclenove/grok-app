//! Disposable process faults occur only after a real fake down and parent ack.
use gcu_core::driver::*;
use gcu_driver_windows::{
    input::api::{Event, InputTracker, Target},
    worker::{codec, serve_with_input},
};
use std::{
    io::Write,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};
struct FaultTracker {
    inner: Arc<dyn InputTracker>,
    down: AtomicBool,
    dir: PathBuf,
    fault: String,
}
#[async_trait::async_trait]
impl InputTracker for FaultTracker {
    async fn prepare(&self, target: Target, events: Vec<Event>, delivery: Delivery) -> Result<()> {
        self.down.store(
            events.len() == 1 && matches!(events[0], Event::Button(_, true, ..)),
            Ordering::SeqCst,
        );
        self.inner.prepare(target, events, delivery).await
    }
    async fn complete(&self, delivered: bool) -> Result<()> {
        self.inner.complete(delivered).await?;
        if delivered && self.down.swap(false, Ordering::SeqCst) && self.fault != "input_normal" {
            std::fs::write(self.dir.join("held"), b"down").unwrap();
            if self.fault == "input_crash" {
                std::process::abort();
            }
            std::thread::sleep(std::time::Duration::from_secs(60));
        }
        Ok(())
    }
}
pub async fn run(dir: PathBuf, fault: String) {
    let (input, mut forwarding) = tokio::io::duplex(64 * 1024);
    let log = dir.clone();
    tokio::spawn(async move {
        let mut source = tokio::io::stdin();
        while let Some(frame) = codec::read_frame::<_, serde_json::Value>(&mut source)
            .await
            .unwrap()
        {
            if frame["request"]["op"] == "perform" {
                let mut calls = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(log.join("calls"))
                    .unwrap();
                writeln!(calls, "mutation").unwrap();
            }
            if codec::write_frame(&mut forwarding, &frame).await.is_err() {
                return;
            }
        }
    });
    serve_with_input(
        input,
        tokio::io::stdout(),
        |namespace, tracker| async move {
            let tracker = Arc::new(FaultTracker {
                inner: tracker,
                down: AtomicBool::new(false),
                dir,
                fault,
            });
            Ok(super::fixture::driver_with_input(&namespace, Some(tracker)).await)
        },
    )
    .await
    .unwrap();
}
