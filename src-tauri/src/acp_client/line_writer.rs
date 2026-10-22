//! Serialized ACP frames; interrupted writes poison before unlocking the writer.
use super::*;

pub(crate) struct LineWriteFailure {
    pub(crate) message: String,
    pub(crate) may_have_written: bool,
}

struct FrameWrite<'a> {
    client: &'a AcpClient,
    stdin: tokio::sync::MutexGuard<'a, Option<Box<dyn AsyncWrite + Unpin + Send>>>,
    written: &'a mut usize,
    complete: bool,
    failure: Option<String>,
}

impl Drop for FrameWrite<'_> {
    fn drop(&mut self) {
        if !self.complete && *self.written > 0 {
            // This also runs when cancellation or timeout drops the future.
            // Keep the mutex until both the writer and client are unusable.
            *self.stdin = None;
            self.client.retire_failed_line(
                self.failure
                    .as_deref()
                    .unwrap_or("ACP frame interrupted before flush completed"),
            );
        }
    }
}

impl AcpClient {
    pub(super) async fn write_line_tracked(&self, value: &Value) -> Result<(), LineWriteFailure> {
        self.write_line_tracked_checked(value, || Ok(())).await
    }

    pub(super) async fn write_line_tracked_checked<F>(
        &self,
        value: &Value,
        check: F,
    ) -> Result<(), LineWriteFailure>
    where
        F: FnOnce() -> Result<(), String> + Send,
    {
        let mut line = serde_json::to_string(value).map_err(|error| LineWriteFailure {
            message: error.to_string(),
            may_have_written: false,
        })?;
        line.push('\n');
        let mut written = 0;
        // Keep the original deadline around lock acquisition, all short writes
        // and flush. Count accepted bytes before interpreting any later error.
        let write = async {
            let stdin = self.stdin.lock().await;
            // A reverse reply may have waited behind another frame. Validate
            // its captured gate after that wait, before accepting any byte.
            check()?;
            // A queued frame may have waited through retirement or shutdown.
            // `stopped` is turn cancellation; reader_alive owns transport life.
            if !self.reader_alive.load(Ordering::SeqCst) {
                return Err("stdin closed".to_string());
            }
            let mut frame = FrameWrite {
                client: self,
                stdin,
                written: &mut written,
                complete: false,
                failure: None,
            };
            let result = match frame.stdin.as_mut() {
                Some(writer) => write_frame(writer.as_mut(), line.as_bytes(), frame.written).await,
                None => Err("stdin closed".to_string()),
            };
            match &result {
                Ok(()) => frame.complete = true,
                Err(message) => frame.failure = Some(message.clone()),
            }
            result
        };
        match tokio::time::timeout(Duration::from_secs(STDIN_WRITE_TIMEOUT_SECS), write).await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(message)) => Err(LineWriteFailure {
                message,
                may_have_written: written > 0,
            }),
            Err(_) => {
                let message = format!(
                    "stdin write timeout after {STDIN_WRITE_TIMEOUT_SECS}s (agent may be wedged)"
                );
                // Preserve the original deadline's retirement even if it
                // expired while waiting for the lock, before accepting bytes.
                self.retire_failed_line(&message);
                Err(LineWriteFailure {
                    message,
                    may_have_written: written > 0,
                })
            }
        }
    }

    fn retire_failed_line(&self, reason: &str) {
        // EOF/intentional kill and another failed frame can own retirement.
        // Only its first transition fails waiters and emits a process exit.
        if !self.reader_alive.swap(false, Ordering::SeqCst) {
            return;
        }
        self.stopped.store(true, Ordering::SeqCst);
        self.reader_stop.notify_one();
        if let Some(binding) = &self.main_migration {
            binding.revoke();
        }
        if let Some(configuration) = &self.process_config {
            configuration.cancel_frozen_routes();
        }
        self.set_import_history_grant(None);
        // Drop must not format/slice arbitrary stderr text or await cleanup.
        error!(reason, "ACP transport retired after interrupted frame");
        self.fail_all_pending(reason);
        let _ = self
            .event_tx
            .send((None, AcpEvent::ProcessExited { code: None }));
        if let Some(client) = self.self_weak.upgrade() {
            // Own the original transport, independently of the cancelled RPC
            // future. The weak self reference adds no permanent Arc cycle.
            tokio::spawn(async move {
                if tokio::time::timeout(
                    Duration::from_secs(STDIN_WRITE_TIMEOUT_SECS),
                    client.kill(),
                )
                .await
                .is_err()
                {
                    warn!(
                        secs = STDIN_WRITE_TIMEOUT_SECS,
                        "acp kill after failed frame also timed out"
                    );
                }
            });
        }
    }
}

async fn write_frame(
    writer: &mut (dyn AsyncWrite + Unpin + Send),
    bytes: &[u8],
    written: &mut usize,
) -> Result<(), String> {
    while *written < bytes.len() {
        match writer.write(&bytes[*written..]).await {
            Ok(0) => return Err(std::io::Error::from(std::io::ErrorKind::WriteZero).to_string()),
            Ok(count) => *written += count,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error.to_string()),
        }
    }
    writer.flush().await.map_err(|error| error.to_string())
}
