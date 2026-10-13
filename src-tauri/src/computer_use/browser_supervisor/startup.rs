//! Bounded startup IPC and owned output drains. Never retain raw worker errors.

use std::io::{BufRead, BufReader, Read};
use std::process::{ChildStderr, ChildStdout};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

const PORT_LINE_BYTES: usize = 1024;
const DIAGNOSTIC_CODES: [&str; 7] = [
    "ERR_MODULE_NOT_FOUND",
    "MODULE_NOT_FOUND",
    "ERR_INVALID_ARG_TYPE",
    "ERR_DLOPEN_FAILED",
    "SyntaxError",
    "EACCES",
    "ENOENT",
];

#[derive(Default)]
struct Diagnostics {
    bytes: u64,
    code: Option<&'static str>,
    // Only a fixed-size matching window, never a retained error line or log.
    tail: Vec<u8>,
}

impl Diagnostics {
    fn observe(&mut self, chunk: &[u8]) {
        self.bytes = self.bytes.saturating_add(chunk.len() as u64);
        if self.code.is_none() {
            self.tail.extend_from_slice(chunk);
            self.code = DIAGNOSTIC_CODES.iter().copied().find(|code| {
                self.tail
                    .windows(code.len())
                    .any(|window| window == code.as_bytes())
            });
            let keep = DIAGNOSTIC_CODES
                .iter()
                .map(|code| code.len())
                .max()
                .unwrap()
                - 1;
            let discard = self.tail.len().saturating_sub(keep);
            self.tail.drain(..discard);
        }
    }

    fn summary(&self) -> String {
        format!(
            "stderr_bytes={}; stderr_code={}",
            self.bytes,
            self.code.unwrap_or("unclassified")
        )
    }
}

pub(super) struct StartupOutput {
    port: mpsc::Receiver<Result<u16, String>>,
    stdout: Option<JoinHandle<()>>,
    stderr: Option<JoinHandle<()>>,
    diagnostics: Arc<Mutex<Diagnostics>>,
}

impl StartupOutput {
    pub(super) fn start(stdout: ChildStdout, stderr: ChildStderr) -> Self {
        let diagnostics = Arc::new(Mutex::new(Diagnostics::default()));
        let seen = diagnostics.clone();
        // Start BEFORE waiting for stdout. Even pre-port diagnostics must not
        // block the worker on pipe capacity. Output is discarded after matching.
        let stderr = std::thread::spawn(move || {
            let mut reader = BufReader::new(stderr);
            let mut chunk = [0_u8; 4096];
            loop {
                match reader.read(&mut chunk) {
                    Ok(0) => break,
                    Ok(count) => seen
                        .lock()
                        .unwrap_or_else(|error| error.into_inner())
                        .observe(&chunk[..count]),
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(_) => break,
                }
            }
        });
        let (send, port) = mpsc::channel();
        let stdout = std::thread::spawn(move || {
            let mut stdout = stdout;
            let _ = send.send(port_message(&mut stdout));
            // Keep the original pipe open after publication. Later output is
            // discarded with a fixed-size buffer, never read as unbounded lines.
            let _ = std::io::copy(&mut stdout, &mut std::io::sink());
        });
        Self {
            port,
            stdout: Some(stdout),
            stderr: Some(stderr),
            diagnostics,
        }
    }

    pub(super) fn wait_port(&self, timeout: Duration) -> Result<u16, String> {
        self.port
            .recv_timeout(timeout)
            .map_err(|_| "worker did not publish port".to_string())?
    }

    pub(super) fn summary(&self) -> String {
        self.diagnostics
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .summary()
    }

    /// An observation deadline does not discard the original thread handles.
    /// The supervisor retains this owner until both readers physically finish.
    pub(super) fn join(&mut self, timeout: Duration) -> Result<(), String> {
        let deadline = Instant::now() + timeout;
        while self
            .stdout
            .as_ref()
            .is_some_and(|reader| !reader.is_finished())
            || self
                .stderr
                .as_ref()
                .is_some_and(|reader| !reader.is_finished())
        {
            if Instant::now() >= deadline {
                return Err("owned worker output cleanup is pending".into());
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let mut failed = false;
        for reader in [&mut self.stdout, &mut self.stderr] {
            if let Some(reader) = reader.take() {
                failed |= reader.join().is_err();
            }
        }
        if failed {
            Err("owned worker output reader failed".into())
        } else {
            Ok(())
        }
    }
}

fn port_message(stdout: impl Read) -> Result<u16, String> {
    let mut reader = BufReader::new(stdout.take((PORT_LINE_BYTES + 1) as u64));
    let mut line = Vec::new();
    reader
        .read_until(b'\n', &mut line)
        .map_err(|_| "worker port read failed".to_string())?;
    if line.is_empty() {
        return Err("worker stdout closed before port".into());
    }
    if line.len() > PORT_LINE_BYTES {
        return Err("worker port message exceeds limit".into());
    }
    let body: serde_json::Value = serde_json::from_slice(&line)
        .map_err(|_| "worker port message is invalid JSON".to_string())?;
    body.get("port")
        .and_then(|value| value.as_u64())
        .and_then(|port| u16::try_from(port).ok())
        .filter(|port| *port != 0)
        .ok_or_else(|| "worker port is missing or invalid".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malformed_port_messages_never_echo_worker_output() {
        for message in [
            "private-sentinel",
            "{\"token\":\"private-sentinel\"}",
            "{\"port\":0}",
            "{\"port\":65536}",
            "{\"port\":\"42\"}",
        ] {
            let error = port_message(message.as_bytes()).unwrap_err();
            assert!(!error.contains("private-sentinel"));
        }
        assert_eq!(
            port_message(b"{\"port\":65535}\n".as_slice()).unwrap(),
            65535
        );
    }

    #[test]
    fn oversized_port_input_is_not_consumed_or_retained() {
        struct Flood(usize);
        impl Read for Flood {
            fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
                self.0 += bytes.len();
                bytes.fill(b'x');
                Ok(bytes.len())
            }
        }
        let mut source = Flood(0);
        assert!(port_message(&mut source)
            .unwrap_err()
            .contains("exceeds limit"));
        assert_eq!(source.0, PORT_LINE_BYTES + 1);
    }

    #[test]
    fn diagnostics_are_bounded_and_match_codes_across_chunks_without_raw_text() {
        let mut seen = Diagnostics::default();
        for _ in 0..1024 {
            seen.observe(&[b'x'; 4096]);
        }
        seen.observe(b"private-sentinel ERR_MODULE_");
        seen.observe(b"NOT_FOUND private-path-and-token");
        let summary = seen.summary();
        assert!(summary.contains("stderr_code=ERR_MODULE_NOT_FOUND"));
        assert!(!summary.contains("private-"));
        assert!(seen.tail.len() < 32);
        assert!(seen.bytes > 4 * 1024 * 1024);
    }

    #[test]
    fn output_deadline_retains_both_original_readers_until_their_actual_join() {
        let (_, port) = mpsc::channel();
        let (release_stdout, waiting_stdout) = mpsc::channel();
        let (release_stderr, waiting_stderr) = mpsc::channel();
        let stdout = std::thread::spawn(move || {
            waiting_stdout.recv().unwrap();
        });
        let stderr = std::thread::spawn(move || {
            waiting_stderr.recv().unwrap();
        });
        let mut output = StartupOutput {
            port,
            stdout: Some(stdout),
            stderr: Some(stderr),
            diagnostics: Arc::new(Mutex::new(Diagnostics::default())),
        };
        assert!(output.join(Duration::from_millis(20)).is_err());
        assert!(output.stdout.is_some() && output.stderr.is_some());
        release_stdout.send(()).unwrap();
        release_stderr.send(()).unwrap();
        output.join(Duration::from_secs(1)).unwrap();
        assert!(output.stdout.is_none() && output.stderr.is_none());
    }
}
