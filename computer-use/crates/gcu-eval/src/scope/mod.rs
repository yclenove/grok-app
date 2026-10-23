//! S3 fake-only private bootstrap and teardown. No real identity claims.
use crate::{
    runner::{Environment, RunConfig},
    Error,
};
use gcu_host_protocol::{
    codec,
    scope::{Ack, Control, Scope, LEASE, MAX_CONTROL_FRAME, VERSION},
    transport::BoxStream,
};
use std::{net::SocketAddr, path::Path, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, BufReader},
    process::{Child, Command},
    sync::{mpsc, oneshot, watch},
};

/// Authority held by a trusted harness, never by the model/relay. The owner
/// must call [`Self::close`] and await engine exit before deleting its run root.
/// Drop closes control and kills the child; it cannot claim cleanup completed.
pub struct Supervisor {
    data_address: SocketAddr,
    run_id: String,
    updates: mpsc::Sender<Update>,
    task: Option<tokio::task::JoinHandle<Result<(), Error>>>,
    close: Option<watch::Sender<bool>>,
}
struct Update {
    frame: Control,
    reply: oneshot::Sender<Result<Ack, Error>>,
}
impl Supervisor {
    /// Spawn only `gcu scoped-engine --driver fake`, install over inherited
    /// control, verify ACK, then read the data address. Every startup stage has
    /// a five-second cap. Cancellation kills the child and closes control.
    /// No fixture state file, env value or client input supplies authority.
    pub async fn start(
        binary: &Path,
        env: &Environment,
        scene: &str,
        profile: &str,
        run_id: String,
        scope: Scope,
    ) -> Result<Self, Error> {
        let mut command = Command::new(binary);
        command
            .args([
                "scoped-engine",
                "--driver",
                "fake",
                "--scene",
                scene,
                "--profile",
                profile,
            ])
            .env_clear()
            .env("HOME", &env.home)
            .env("USERPROFILE", &env.home)
            .env("XDG_STATE_HOME", &env.home)
            .env("XDG_CONFIG_HOME", &env.home)
            .env("XDG_RUNTIME_DIR", &env.work)
            .env("TMPDIR", &env.work)
            .env("TEMP", &env.work)
            .env("TMP", &env.work)
            .env("GCU_ALLOW_FAKE", "1")
            .current_dir(&env.work)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true);
        let mut control = gcu_host_protocol::scope::launch::prepare(command.as_std_mut())
            .await
            .map_err(protocol)?;
        let mut child = command
            .spawn()
            .map_err(|_| Error::msg("scope_engine_spawn_failed"))?;
        // `prepare` parked the child's end of the control socket in `command`.
        // Drop it now so this side sees EOF as soon as the engine exits.
        drop(command);
        let data_address = match bootstrap(&mut child, &mut control, &run_id, scope).await {
            Ok(address) => address,
            Err(error) => {
                drop(control);
                if wait_exit(&mut child).await.is_err() {
                    return Err(Error::msg("cleanup_failed; scope_startup_exit_failed"));
                }
                return Err(error);
            }
        };
        let generation = 1;
        let (updates, rx) = mpsc::channel(1);
        let (close, closing) = watch::channel(false);
        let task_run = run_id.clone();
        let task =
            tokio::spawn(
                async move { own(child, control, task_run, generation, rx, closing).await },
            );
        Ok(Self {
            data_address,
            run_id,
            updates,
            task: Some(task),
            close: Some(close),
        })
    }
    /// Public data address; it grants no ability to change scope. Memory only.
    pub fn data_address(&self) -> SocketAddr {
        self.data_address
    }
    /// Change the entire set from the trusted harness. Five-second total
    /// request cap. Errors are terminal on the engine; never retry an update.
    pub async fn rebind(&self, generation: u64, scope: Scope) -> Result<Ack, Error> {
        let (reply, receive) = oneshot::channel();
        let frame = Control::Rebind {
            run_id: self.run_id.clone(),
            generation,
            scope,
        };
        let result = tokio::time::timeout(Duration::from_secs(5), async {
            self.updates
                .send(Update { frame, reply })
                .await
                .map_err(|_| Error::msg("scope_control_closed"))?;
            receive
                .await
                .map_err(|_| Error::msg("scope_control_closed"))?
        })
        .await
        .map_err(|_| Error::msg("scope_rebind_timeout"))
        .and_then(|result| result);
        if result.is_err() {
            if let Some(close) = &self.close {
                close.send_replace(true);
            }
        }
        result
    }
    /// Revoke, close control, and wait for exit (five seconds, then kill/wait
    /// with another five-second bound). A teardown failure stays an error.
    pub async fn close(mut self) -> Result<(), Error> {
        if let Some(close) = self.close.take() {
            close.send_replace(true);
        }
        if let Some(mut task) = self.task.take() {
            let result = tokio::time::timeout(Duration::from_secs(12), &mut task).await;
            return match result {
                Ok(Ok(result)) => result,
                Ok(Err(_)) => Err(Error::msg("scope_supervisor_failed")),
                Err(_) => {
                    task.abort();
                    Err(Error::msg("scope_teardown_timeout"))
                }
            };
        }
        Ok(())
    }
}
impl Drop for Supervisor {
    fn drop(&mut self) {
        // Abort drops Child (kill_on_drop) and the only control end. No renewal
        // task is detached on cancellation. Explicit close is the wait barrier.
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}
async fn exchange(control: &mut BoxStream, frame: &Control) -> Result<Ack, Error> {
    tokio::time::timeout(Duration::from_secs(2), async {
        codec::write_frame(control, frame).await.map_err(protocol)?;
        codec::read_frame_limited(control, MAX_CONTROL_FRAME)
            .await
            .map_err(protocol)?
            .ok_or_else(|| Error::msg("scope_ack_eof"))
    })
    .await
    .map_err(|_| Error::msg("scope_ack_timeout"))?
}
fn protocol(_: gcu_host_protocol::Error) -> Error {
    Error::msg("scope_control_failed")
}

async fn own(
    mut child: Child,
    mut control: BoxStream,
    run: String,
    mut generation: u64,
    mut updates: mpsc::Receiver<Update>,
    mut close: watch::Receiver<bool>,
) -> Result<(), Error> {
    let mut seq = 0u64;
    let mut renew = tokio::time::interval(LEASE / 3);
    let result = loop {
        tokio::select! {biased;
            _=close.changed()=>break Ok(()),
            exited=child.wait()=>{let _=exited;break Err(Error::msg("scope_engine_exited"));},
            update=updates.recv()=>{
                let Some(update)=update else {break Ok(());};
                let expected=match &update.frame {Control::Rebind {run_id,generation,scope}=>scope.digest(run_id,*generation).ok().map(|scope_digest|Ack {run_id:run_id.clone(),generation:*generation,scope_digest}),_=>None};
                match exchange(&mut control,&update.frame).await {
                    Ok(ack) if Some(&ack)==expected.as_ref()=>{
                        generation=ack.generation;seq=0;
                        let _=update.reply.send(Ok(ack));
                    }
                    _=>{let _=update.reply.send(Err(Error::msg("scope_rebind_failed")));break Err(Error::msg("scope_rebind_failed"));}
                }
            }
            _=renew.tick()=>{
                let Some(next)=seq.checked_add(1) else {break Err(Error::msg("scope_sequence_exhausted"));};seq=next;
                if !matches!(tokio::time::timeout(Duration::from_secs(1),codec::write_frame(&mut control,&Control::Renew {run_id:run.clone(),generation,seq})).await,Ok(Ok(()))) {break Err(Error::msg("scope_renew_failed"));}
            }
        }
    };
    let _ = tokio::time::timeout(
        Duration::from_millis(200),
        codec::write_frame(&mut control, &Control::Revoke { run_id: run }),
    )
    .await;
    drop(control);
    wait_exit(&mut child).await?;
    result
}

/// S3 is deliberately unavailable with a real driver; S2/S4/S6 own that proof.
pub(crate) fn validate(cfg: &RunConfig) -> Result<(), Error> {
    if cfg.fake_scope.is_some() && cfg.real_driver {
        return Err(Error::msg("native_scope_not_run_s2_s4_s6"));
    }
    Ok(())
}

async fn bootstrap(
    child: &mut Child,
    control: &mut BoxStream,
    run_id: &str,
    scope: Scope,
) -> Result<SocketAddr, Error> {
    let generation = 1;
    let digest = scope.digest(run_id, generation).map_err(protocol)?;
    let frame = Control::Install {
        version: VERSION,
        run_id: run_id.to_owned(),
        generation,
        scope_digest: digest.clone(),
        scope,
    };
    let ack = exchange(control, &frame).await?;
    if ack
        != (Ack {
            run_id: run_id.to_owned(),
            generation,
            scope_digest: digest,
        })
    {
        return Err(Error::msg("scope_ack_mismatch"));
    }
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| Error::msg("scope_engine_stdout_missing"))?;
    let mut line = String::new();
    let mut reader = BufReader::new(stdout.take(1024));
    tokio::time::timeout(Duration::from_secs(5), reader.read_line(&mut line))
        .await
        .map_err(|_| Error::msg("scope_data_ready_timeout"))?
        .map_err(|_| Error::msg("scope_data_ready_failed"))?;
    let data_address: SocketAddr = serde_json::from_str::<serde_json::Value>(&line)
        .ok()
        .and_then(|v| {
            v.get("data_address")
                .and_then(|s| s.as_str())
                .and_then(|s| s.parse().ok())
        })
        .filter(|a: &SocketAddr| a.ip() == std::net::Ipv4Addr::LOCALHOST && a.port() != 0)
        .ok_or_else(|| Error::msg("scope_data_address_invalid"))?;
    Ok(data_address)
}

async fn wait_exit(child: &mut Child) -> Result<(), Error> {
    match tokio::time::timeout(Duration::from_secs(5), child.wait()).await {
        Ok(Ok(_)) => Ok(()),
        Ok(Err(_)) => Err(Error::msg("scope_engine_wait_failed")),
        Err(_) => {
            child
                .start_kill()
                .map_err(|_| Error::msg("scope_engine_kill_failed"))?;
            tokio::time::timeout(Duration::from_secs(5), child.wait())
                .await
                .map_err(|_| Error::msg("scope_engine_exit_timeout"))?
                .map_err(|_| Error::msg("scope_engine_wait_failed"))?;
            Ok(())
        }
    }
}
