//! Child process lifecycle; supervisor owns cancellation and recovery.
use super::*;
pub(super) struct Process {
    child: Child,
    input: ChildStdin,
    output: ChildStdout,
    epoch: u64,
    namespace: String,
    #[cfg(windows)]
    _job: crate::worker::unsafe_job::Job,
}
impl Process {
    pub(super) async fn ready(config: &WorkerConfig, epoch: u64) -> Result<(Self, Reply)> {
        let _startup = crate::timing::Timing::start("worker_startup");
        let mut process = Self::spawn(config, epoch).await?;
        let _handshake = crate::timing::Timing::start("worker_handshake");
        let probe = process
            .exchange(Request::Doctor, 0, config.timeout, None, None)
            .await?;
        if !matches!(probe, Reply::Doctor { ready: true, .. }) {
            return Err(crashed(false));
        }
        Ok((process, probe))
    }
    async fn spawn(config: &WorkerConfig, epoch: u64) -> Result<Self> {
        let _timing = crate::timing::Timing::start("worker_spawn");
        let mut command = Command::new(&config.executable);
        command
            .args(&config.args)
            .env_clear()
            .kill_on_drop(true)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            // Only stderr carries payload-free tracing; stdout remains private
            // framed RPC. Native stage timings must survive startup/read failures.
            .stderr(Stdio::inherit());
        // No Host/Stop secret handles or credential environment reaches the child.
        for key in [
            "SystemRoot",
            "WINDIR",
            "PATH",
            "TEMP",
            "TMP",
            "APPDATA",
            "LOCALAPPDATA",
            "USERPROFILE",
        ] {
            if let Some(value) = std::env::var_os(key) {
                command.env(key, value);
            }
        }
        command.envs(config.environment.iter().cloned());
        // Parent pid for the worker's pointer gate. The worker reads it once and refuses
        // to start if it is missing or invalid. The host ignores this variable. Not a secret.
        command.env("GCU_HOST_PID", std::process::id().to_string());
        // The disposable native harness opts in to full provider timing. Write
        // child stderr directly to a per-worker file: a slow PowerShell console
        // must not stall COM calls and manufacture two-second tree timeouts.
        #[cfg(debug_assertions)]
        if let Some((_, directory)) = config
            .environment
            .iter()
            .find(|(key, _)| key.to_str() == Some("GCU_NATIVE_TRACE_DIR"))
        {
            let path = PathBuf::from(directory.as_os_str())
                .join(format!("worker-{}-{epoch}.log", config.namespace));
            let file = tokio::task::spawn_blocking(move || std::fs::File::create(path))
                .await
                .map_err(|_| crashed(false))?
                .map_err(|e| DriverError::Native {
                    message: format!("worker diagnostic file: {e}"),
                    maybe_executed: false,
                })?;
            command.stderr(Stdio::from(file));
        }
        let mut child = command.spawn().map_err(|_| crashed(false))?;
        // Windows std Command uses an explicit inherited standard-handle list.
        // The child waits for our first frame, so no FFI runs before assignment.
        #[cfg(windows)]
        let job = crate::worker::unsafe_job::Job::attach(&child)?;
        let input = child.stdin.take().ok_or_else(|| crashed(false))?;
        let output = child.stdout.take().ok_or_else(|| crashed(false))?;
        Ok(Self {
            child,
            input,
            output,
            epoch,
            namespace: config.namespace.clone(),
            #[cfg(windows)]
            _job: job,
        })
    }
    pub(super) async fn exchange(
        &mut self,
        request: Request,
        id: u64,
        timeout: Duration,
        dispatched: Option<&AtomicBool>,
        recovery: Option<&crate::worker::input::Recovery>,
    ) -> Result<Reply> {
        let mutation = !request.is_read_only();
        if !request.valid_epoch(self.epoch) {
            return Err(DriverError::StaleElement);
        }
        // Encoding occurs before any pipe write: invalid/oversized requests are certain.
        let call = Call {
            version: VERSION,
            id,
            epoch: self.epoch,
            namespace: self.namespace.clone(),
            request,
        };
        let expected = call.request.clone();
        let operation = async {
            let bytes = tokio::task::spawn_blocking(move || codec::encode(&call))
                .await
                .map_err(|_| crashed(false))?
                .map_err(|_| DriverError::InvalidRequest)?;
            if let Some(dispatched) = dispatched {
                dispatched.store(true, Ordering::SeqCst);
            }
            codec::write_payload(&mut self.input, &bytes)
                .await
                .map_err(|_| crashed(mutation))?;
            let result = loop {
                let reply: Response = codec::read_frame(&mut self.output)
                    .await
                    .map_err(|_| crashed(mutation))?
                    .ok_or_else(|| crashed(mutation))?;
                if reply.version != VERSION || reply.id != id || reply.epoch != self.epoch {
                    return Err(crashed(mutation));
                }
                let result = reply.result.map_err(|Failure(e)| e)?;
                if let Some(recovery) = recovery {
                    if let Some(result) = recovery
                        .receive(result, &expected, &mut self.input, id, self.epoch)
                        .await?
                    {
                        break result;
                    }
                } else {
                    break result;
                }
            };
            // A correlated native response knows whether mutation dispatch began.
            // Only transport loss must conservatively assume possible execution.
            if !matches_reply(&expected, &result, self.epoch) {
                return Err(crashed(mutation));
            }
            Ok(result)
        };
        tokio::time::timeout(timeout, operation)
            .await
            .map_err(|_| DriverError::Timeout {
                maybe_executed: mutation
                    && dispatched.is_some_and(|flag| flag.load(Ordering::SeqCst)),
            })?
    }
    pub(super) async fn kill(&mut self) {
        let _timing = crate::timing::Timing::start("worker_kill");
        tracing::info!(target: "gcu_windows_timing", stage = "worker_kill", phase = "requested", pid = ?self.child.id(), epoch = self.epoch);
        if let Err(error) = self.child.kill().await {
            if error.kind() != std::io::ErrorKind::InvalidInput {
                tracing::warn!("worker termination failed");
            }
        }
        if self.child.wait().await.is_err() {
            tracing::warn!("worker reap failed");
        }
    }
}
