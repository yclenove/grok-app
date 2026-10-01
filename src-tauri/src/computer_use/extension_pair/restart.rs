//! The disposable browser and the crashable product Host have separate process owners.
//! Control remains on inherited private pipes; no fault-injection HTTP endpoint exists.
use super::{control_reply, dispatch::DispatchFixture, resources::ProbeResources};
use grok_computer_use_core::broker::ComputerUseBroker;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Stdio};
use std::sync::{mpsc, Arc};
use std::time::Duration;

pub(super) const CHECKS: &[&str] = &[
    "execution-clock-real-transactions",
    "production-sw-negotiates",
    "app-restart-recovers-held-claim",
    "app-restart-recovers-before-claim",
    "app-restart-recovers-before-injection",
    "app-restart-recovers-claimed-wait",
    "app-restart-recovers-applied-click",
    "app-then-worker-restart-recovers-held-claim",
    "app-then-worker-restart-recovers-before-claim",
    "app-then-worker-restart-recovers-before-injection",
    "app-then-worker-restart-recovers-claimed-wait",
    "app-then-worker-restart-recovers-applied-click",
    "worker-then-app-restart-recovers-held-claim",
    "worker-then-app-restart-recovers-before-claim",
    "worker-then-app-restart-recovers-before-injection",
    "worker-then-app-restart-recovers-claimed-wait",
    "worker-then-app-restart-recovers-applied-click",
];

pub(super) const RETIREMENT_CHECKS: &[&str] = &[
    "execution-clock-real-transactions",
    "production-sw-negotiates",
    "live-predecessor-keeps-original-owner",
    "lost-host-retirement-reply-keeps-owner",
    "failed-local-deletion-retries-cleanup-only",
];

pub(super) fn owned_home() -> Result<PathBuf, String> {
    let home = std::env::var_os("GROK_APP_HOME")
        .map(PathBuf::from)
        .ok_or("not_run: pairing probe requires an isolated GROK_APP_HOME")?
        .canonicalize()
        .map_err(|_| "isolated probe home missing")?;
    let marker = home
        .parent()
        .ok_or("isolated probe owner missing")?
        .join("owner.json");
    let owner: Value =
        serde_json::from_slice(&std::fs::read(marker).map_err(|_| "isolated probe owner missing")?)
            .map_err(|_| "invalid isolated probe owner")?;
    let declared = PathBuf::from(owner["ownedHome"].as_str().unwrap_or_default())
        .canonicalize()
        .map_err(|_| "invalid isolated probe home")?;
    if home != declared {
        return Err("pairing probe home does not match its owner marker".into());
    }
    Ok(home)
}

pub(super) enum ControlHost {
    Local {
        broker: Arc<ComputerUseBroker>,
        endpoint: String,
        fixture: DispatchFixture,
    },
    Process(Box<HostProcesses>),
}

impl ControlHost {
    pub(super) fn new(restarting: bool, home: &Path) -> Result<Self, String> {
        if restarting {
            return Ok(Self::Process(Box::new(HostProcesses {
                active: HostProcess::spawn(home)?,
                previous: None,
            })));
        }
        let broker = super::super::ensure_host_runtime();
        if !broker.feature_enabled() {
            return Err("enable Computer Use in the isolated probe home before pairing".into());
        }
        let endpoint = super::super::ipc::ensure(broker.clone())?.url.clone();
        let fixture = DispatchFixture::new(broker.clone());
        Ok(Self::Local {
            broker,
            endpoint,
            fixture,
        })
    }

    pub(super) fn control(&mut self, value: &Value) -> Result<Value, String> {
        match self {
            Self::Local {
                broker,
                endpoint,
                fixture,
            } => {
                if value["command"]
                    .as_str()
                    .is_some_and(|name| name.starts_with("dispatch-"))
                {
                    fixture.control(value)
                } else {
                    control_reply(broker, endpoint, value)
                }
            }
            Self::Process(host) => host.call(value),
        }
    }
}

pub(super) struct HostProcesses {
    active: HostProcess,
    previous: Option<HostProcess>,
}

impl HostProcesses {
    fn call(&mut self, value: &Value) -> Result<Value, String> {
        match value["command"].as_str() {
            Some(command @ ("host-restart" | "host-successor")) => {
                if self.previous.is_some() {
                    return Err("original Host must be explicitly retired first".into());
                }
                let previous_pid = self.active.child.id();
                if command == "host-restart" {
                    self.active.stop()?;
                }
                let replacement = HostProcess::spawn(&self.active.home)?;
                let pid = replacement.child.id();
                let previous = std::mem::replace(&mut self.active, replacement);
                if command == "host-successor" {
                    self.previous = Some(previous);
                }
                Ok(json!({"ok":true,"previousPid":previous_pid,"pid":pid}))
            }
            Some("host-predecessor-state") => {
                let previous = self.previous.as_mut().ok_or("original Host missing")?;
                let alive = previous
                    .child
                    .try_wait()
                    .map_err(|_| "original Host query failed")?
                    .is_none();
                let state = previous.call(&json!({"command":"dispatch-state"}))?;
                Ok(json!({"alive":alive,"pid":previous.child.id(),"idle":state["idle"]}))
            }
            Some("host-predecessor-stop") => {
                let previous = self.previous.as_mut().ok_or("original Host missing")?;
                let pid = previous.child.id();
                previous.stop()?;
                self.previous = None;
                Ok(json!({"ok":true,"previousPid":pid}))
            }
            _ => self.active.call(value),
        }
    }
}

pub(super) struct HostProcess {
    child: Child,
    input: ChildStdin,
    replies: mpsc::Receiver<Value>,
    reader: Option<std::thread::JoinHandle<()>>,
    resources: ProbeResources,
    home: PathBuf,
    stopped: bool,
}

impl Drop for ControlHost {
    fn drop(&mut self) {
        if let Self::Local { broker, .. } = self {
            broker.tabs().revoke_pairing();
        }
    }
}

impl HostProcess {
    fn spawn(home: &Path) -> Result<Self, String> {
        let mut resources = ProbeResources::create()?;
        let mut command = crate::process_util::command(
            std::env::current_exe().map_err(|_| "probe executable unavailable")?,
        );
        command
            .arg("existing-tab-host-fixture")
            .env_clear()
            .env("GROK_APP_HOME", home);
        for name in ["SystemRoot", "TEMP", "TMP", "TMPDIR"] {
            if let Some(value) = std::env::var_os(name) {
                command.env(name, value);
            }
        }
        #[cfg(windows)]
        command.env(
            "PATH",
            PathBuf::from(std::env::var_os("SystemRoot").ok_or("Windows system root missing")?)
                .join("System32"),
        );
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|_| "restart Host spawn failed")?;
        if let Err(error) = resources.attach(&child) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
        let input = child.stdin.take().ok_or("restart Host input missing")?;
        let output = child.stdout.take().ok_or("restart Host output missing")?;
        let (tx, replies) = mpsc::sync_channel(1);
        let reader = std::thread::spawn(move || {
            let mut output = BufReader::new(output);
            while let Ok(Some(value)) = read_packet(&mut output) {
                if tx.send(value).is_err() {
                    break;
                }
            }
        });
        let mut host = Self {
            child,
            input,
            replies,
            reader: Some(reader),
            resources,
            home: home.to_path_buf(),
            stopped: false,
        };
        // The child waits for this packet before starting runtime children: Job ownership comes first.
        if host.call(&json!({"command":"ready"}))?["ok"] != true {
            return Err("restart Host handshake failed".into());
        }
        Ok(host)
    }

    fn call(&mut self, value: &Value) -> Result<Value, String> {
        writeln!(self.input, "{value}").map_err(|_| "restart Host input closed")?;
        self.input
            .flush()
            .map_err(|_| "restart Host input closed")?;
        self.replies
            .recv_timeout(Duration::from_secs(15))
            .map_err(|_| "restart Host reply unavailable".into())
    }

    fn stop(&mut self) -> Result<(), String> {
        if self.stopped {
            return Ok(());
        }
        if self
            .child
            .try_wait()
            .map_err(|_| "restart Host status unavailable")?
            .is_none()
        {
            self.child
                .kill()
                .map_err(|_| "restart Host termination failed")?;
        }
        self.child
            .wait()
            .map_err(|_| "restart Host termination unconfirmed")?;
        self.resources.cleanup()?;
        if let Some(reader) = self.reader.take() {
            reader.join().map_err(|_| "restart Host reader failed")?;
        }
        self.stopped = true;
        Ok(())
    }
}

impl Drop for HostProcess {
    fn drop(&mut self) {
        if self.stop().is_err() {
            eprintln!("restart Host cleanup incomplete");
        }
    }
}

fn read_packet(input: &mut impl BufRead) -> Result<Option<Value>, String> {
    let mut line = String::new();
    let count = input
        .take(65537)
        .read_line(&mut line)
        .map_err(|_| "private pipe unavailable")?;
    if count == 0 {
        return Ok(None);
    }
    if count > 65536 || !line.ends_with('\n') {
        return Err("private packet limit".into());
    }
    serde_json::from_str(&line)
        .map(Some)
        .map_err(|_| "invalid private packet".into())
}

pub(super) fn run_host() -> Result<(), String> {
    let home = owned_home()?;
    let mut input = BufReader::new(std::io::stdin().lock());
    if read_packet(&mut input)?
        .as_ref()
        .and_then(|v| v["command"].as_str())
        != Some("ready")
    {
        return Err("private restart Host handshake missing".into());
    }
    let _cleanup = super::ProbeHostCleanup;
    let mut host = ControlHost::new(false, &home)?;
    let mut output = std::io::stdout().lock();
    writeln!(output, "{}", json!({"ok":true})).map_err(|_| "private pipe closed")?;
    output.flush().map_err(|_| "private pipe closed")?;
    while let Some(value) = read_packet(&mut input)? {
        let reply = host.control(&value)?;
        writeln!(output, "{reply}").map_err(|_| "private pipe closed")?;
        output.flush().map_err(|_| "private pipe closed")?;
    }
    Ok(())
}
