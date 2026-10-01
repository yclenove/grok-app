use super::macos_native_protocol::{self as protocol, State};
use std::io::{BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

pub struct Fixture {
    child: Child,
    input: Option<ChildStdin>,
    replies: Receiver<Result<Vec<u8>, String>>,
    reader: Option<std::thread::JoinHandle<()>>,
    pub nonce: String,
    pub state: Option<State>,
    sequence: u64,
}

impl Fixture {
    pub fn start(path: &Path) -> Result<Self, String> {
        Self::launch(Command::new(path))
    }

    #[cfg(test)]
    pub fn contract_child(mode: &str) -> Result<Self, String> {
        let script = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tools/computer-use-fixtures/macos/pipe-contract-child.mjs");
        let mut command = Command::new("node");
        command.arg(script).env("GROK_CU_PIPE_CONTRACT_FAULT", mode);
        Self::launch(command)
    }

    fn launch(mut command: Command) -> Result<Self, String> {
        let nonce = uuid::Uuid::new_v4().to_string();
        let mut child = command
            .args(["--owned-fixture", &nonce])
            .env("GROK_CU_MACOS_FIXTURE", "owned-cocoa")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| format!("start owned Cocoa fixture: {e}"))?;
        let input = child.stdin.take();
        let output = child.stdout.take().expect("piped stdout");
        let (sender, replies) = mpsc::sync_channel(2);
        let reader = std::thread::spawn(move || {
            let mut output = BufReader::new(output);
            loop {
                let reply = protocol::read_line(&mut output);
                let failed = reply.is_err();
                if sender.send(reply).is_err() || failed {
                    break;
                }
            }
        });
        let mut fixture = Self {
            child,
            input,
            replies,
            reader: Some(reader),
            nonce,
            state: None,
            sequence: 0,
        };
        fixture.receive()?;
        Ok(fixture)
    }

    fn receive(&mut self) -> Result<State, String> {
        let bytes = self
            .replies
            .recv_timeout(Duration::from_secs(4))
            .map_err(|_| "owned fixture reply timeout/disconnect")??;
        let state = protocol::parse(
            &bytes,
            &self.nonce,
            self.child.id(),
            self.sequence,
            self.state.as_ref().map(|s| s.window_id),
        )?;
        self.state = Some(state.clone());
        Ok(state)
    }

    pub fn request(&mut self, command: &str) -> Result<State, String> {
        if !matches!(command, "state" | "focus" | "move" | "quit") {
            return Err("fixture has no input injection command".into());
        }
        self.sequence += 1;
        let input = self.input.as_mut().ok_or("fixture pipe closed")?;
        serde_json::to_writer(
            &mut *input,
            &serde_json::json!({"version":protocol::VERSION,
            "nonce":self.nonce, "id":self.sequence, "command":command}),
        )
        .map_err(|e| e.to_string())?;
        input
            .write_all(b"\n")
            .and_then(|()| input.flush())
            .map_err(|e| e.to_string())?;
        self.receive()
    }

    pub fn wait(&mut self, check: impl Fn(&State) -> bool) -> Result<State, String> {
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            let state = self.request("state")?;
            if check(&state) {
                return Ok(state);
            }
            if Instant::now() >= deadline {
                return Err("owned control postcondition timeout".into());
            }
            std::thread::sleep(Duration::from_millis(15));
        }
    }

    pub fn quiet(&mut self, before: &State) -> Result<State, String> {
        // Bounded native-queue observation, not an atomic dispatch guarantee.
        let mut state = before.clone();
        for _ in 0..10 {
            std::thread::sleep(Duration::from_millis(20));
            state = self.request("state")?;
            protocol::unchanged(before, &state)?;
        }
        Ok(state)
    }

    pub fn shutdown(&mut self) -> Result<(), String> {
        self.input.take();
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if let Some(status) = self.child.try_wait().map_err(|e| e.to_string())? {
                if !status.success() {
                    return Err(format!("owned fixture exit: {status}"));
                }
                break;
            }
            if Instant::now() >= deadline {
                return Err("owned fixture exit unconfirmed".into());
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        // Drain so even a full bounded sender can finish before joining.
        let deadline = Instant::now() + Duration::from_secs(1);
        while self
            .reader
            .as_ref()
            .is_some_and(|reader| !reader.is_finished())
        {
            while self.replies.try_recv().is_ok() {}
            if Instant::now() >= deadline {
                return Err("fixture reader exit unconfirmed".into());
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        if let Some(reader) = self.reader.take() {
            reader.join().map_err(|_| "fixture reader panicked")?;
        }
        Ok(())
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.input.take();
        if !matches!(self.child.try_wait(), Ok(Some(_))) {
            // Only our retained child handle: no kill-by-name, user App, or permission changes.
            let _ = self.child.kill();
            let deadline = Instant::now() + Duration::from_secs(2);
            while matches!(self.child.try_wait(), Ok(None)) && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }
}
