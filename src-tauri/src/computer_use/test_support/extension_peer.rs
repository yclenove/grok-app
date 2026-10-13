//! Private child pipe and exact Job/profile ownership for native extension tests.
use super::extension_resources::ProbeResources;
use serde_json::Value;
use std::{
    io::{BufRead, BufReader, Write},
    path::PathBuf,
    process::{Child, ChildStdin, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
};

pub(crate) struct ExtensionPeer {
    child: Child,
    input: Option<ChildStdin>,
    replies: mpsc::Receiver<Value>,
    reader: Option<std::thread::JoinHandle<()>>,
    resources: ProbeResources,
    closed: bool,
    started: Instant,
    consumed_commands: u32,
    last_command_category: &'static str,
}

impl ExtensionPeer {
    pub(crate) fn spawn(scenario: &'static str) -> Result<Self, String> {
        let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        // Explicit test binary only; never start a daily profile or mutate the locked seed.
        let browser_variable = if matches!(
            scenario,
            "active-late-capture-pause"
                | "active-late-capture-chat-stop"
                | "active-late-capture-early-continue"
                | "active-late-capture-early-stop"
                | "active-capture-success"
        ) {
            "GROK_CU_JOINT_CAPTURE_CHROME"
        } else {
            "GROK_CU_JOINT_CHROME"
        };
        let browser = std::env::var_os(browser_variable)
            .map(PathBuf::from)
            .filter(|path| path.is_absolute() && path.is_file())
            .ok_or("manual native joint gate requires explicit Chrome-for-Testing 140+")?;
        let mut resources = ProbeResources::create()?;
        let mut command =
            crate::process_util::command(manifest.join("resources/computer-use/seed/bin/node.exe"));
        command
            .arg(manifest.join("../tools/computer-use-probe/existing-task-joint-live.mjs"))
            .arg("--browser")
            .arg(browser)
            .arg("--owned-profile")
            .arg(&resources.profile)
            .arg("--profile-owner")
            .arg(&resources.owner)
            .arg("--scenario")
            .arg(scenario)
            .env_clear();
        for name in ["SystemRoot", "TEMP", "TMP"] {
            if let Some(value) = std::env::var_os(name) {
                command.env(name, value);
            }
        }
        let system = std::env::var_os("SystemRoot").ok_or("Windows system root missing")?;
        command.env("PATH", PathBuf::from(system).join("System32"));
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|_| "native extension fixture could not start")?;
        // The child cannot launch Chrome before the private ready reply.
        if let Err(error) = resources.attach(&child) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
        let input = child.stdin.take().ok_or("native fixture input missing")?;
        let output = child.stdout.take().ok_or("native fixture output missing")?;
        let (tx, replies) = mpsc::channel();
        let reader = std::thread::spawn(move || {
            for line in BufReader::new(output).lines() {
                let Ok(line) = line else { break };
                if line.len() > 16 * 1024 {
                    break;
                }
                let Ok(value) = serde_json::from_str(&line) else {
                    break;
                };
                if tx.send(value).is_err() {
                    break;
                }
            }
        });
        Ok(Self {
            child,
            input: Some(input),
            replies,
            reader: Some(reader),
            resources,
            closed: false,
            started: Instant::now(),
            consumed_commands: 0,
            last_command_category: "none",
        })
    }

    pub(crate) fn next(&mut self) -> Result<Value, String> {
        let value = self
            .replies
            .recv_timeout(Duration::from_secs(20))
            .map_err(|_| String::from("native fixture control did not settle"))?;
        self.consumed_commands = self.consumed_commands.saturating_add(1);
        self.last_command_category = match value["command"].as_str() {
            Some(
                "ready"
                | "before-request"
                | "state"
                | "shared-count"
                | "lease-witness"
                | "browser-owner-state",
            ) => "witness",
            Some(
                "begin" | "preparation" | "load" | "start-discovery" | "discovery-ready"
                | "finish-discovery",
            ) => "picker",
            Some(
                "approve" | "start-reservation" | "finish-reservation" | "start-publication"
                | "finish-publication",
            ) => "approval",
            Some("stop" | "stopped" | "preparation-after-stop") => "task-control",
            Some("replace-publication-owner" | "replace-picker-owner" | "repair-challenge") => {
                "owner"
            }
            None if matches!(value["event"].as_str(), Some("passed" | "failed")) => "terminal",
            _ => "other",
        };
        Ok(value)
    }

    pub(crate) fn control_witness(&self) -> Value {
        // Exact original owner path and fixed counters only; never command payloads or bindings.
        serde_json::json!({"profile":self.resources.profile,
            "consumedCommands":self.consumed_commands,
            "lastCommandCategory":self.last_command_category,
            "elapsedMs":self.started.elapsed().as_millis()})
    }

    pub(crate) fn reply(&mut self, value: &Value) -> Result<(), String> {
        let input = self.input.as_mut().ok_or("native fixture input closed")?;
        writeln!(input, "{value}").map_err(|_| "native fixture reply failed")?;
        input
            .flush()
            .map_err(|_| "native fixture reply failed".into())
    }

    pub(crate) fn active_processes(&self) -> Result<u32, String> {
        self.resources.active_processes_for_test()
    }

    pub(crate) fn process_members(&self) -> Result<Value, String> {
        self.resources.process_members_for_test()
    }

    pub(crate) fn finish(&mut self, passed: bool) -> Result<(), String> {
        if self.closed {
            return Ok(());
        }
        self.input.take();
        if !passed {
            self.resources.retain_failure_profile();
        }
        let deadline = Instant::now() + Duration::from_secs(20);
        let status = loop {
            match self
                .child
                .try_wait()
                .map_err(|_| "native fixture exit unavailable")?
            {
                Some(status) => break Some(status),
                None if Instant::now() >= deadline => break None,
                None => std::thread::sleep(Duration::from_millis(20)),
            }
        };
        if status.is_none() || status.is_some_and(|value| !value.success()) {
            self.resources.retain_failure_profile();
        }
        // The exact original Job must reach zero before files are considered disposable.
        self.resources.cleanup()?;
        if status.is_none() {
            let _ = self.child.wait();
        }
        if let Some(reader) = self.reader.take() {
            reader
                .join()
                .map_err(|_| "native fixture reader did not join")?;
        }
        self.closed = true;
        if status.is_some_and(|value| value.success()) {
            Ok(())
        } else {
            Err("native fixture exited unsuccessfully; original profile retained".into())
        }
    }
}

impl Drop for ExtensionPeer {
    fn drop(&mut self) {
        if !self.closed && self.finish(false).is_err() {
            eprintln!("native extension fixture cleanup failed; owner-marked profile retained");
        }
    }
}
