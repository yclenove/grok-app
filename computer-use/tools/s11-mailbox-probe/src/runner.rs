//! External commands go through `Runner`. Tests script replies. The macOS
//! process runner is the only implementation that spawns, and tests never call it.

use crate::argv::validate_host_argv;
use crate::budget::{OUT_CAP, UPLOAD_CAP};

#[derive(Clone, Debug)]
pub struct Cmd {
    pub argv: Vec<String>,
    pub timeout_ms: u64,
    pub stdin: Vec<u8>,
    /// Extra environment. Only `TART_HOME` is accepted, and only as an absolute path.
    pub env: Vec<(String, String)>,
}

impl Cmd {
    pub fn new(argv: Vec<String>, timeout_ms: u64) -> Self {
        Self {
            argv,
            timeout_ms,
            stdin: Vec::new(),
            env: Vec::new(),
        }
    }

    pub fn with_stdin(mut self, stdin: Vec<u8>) -> Self {
        self.stdin = stdin;
        self
    }
}

#[derive(Clone, Debug)]
pub struct CmdOut {
    pub status: i32,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub timed_out: bool,
    pub overflow: bool,
    pub reap_unknown: bool,
}

impl CmdOut {
    pub fn ok(stdout: impl AsRef<[u8]>) -> Self {
        Self {
            status: 0,
            stdout: stdout.as_ref().to_vec(),
            stderr: Vec::new(),
            timed_out: false,
            overflow: false,
            reap_unknown: false,
        }
    }

    pub fn code(status: i32, stdout: &str, stderr: &str) -> Self {
        Self {
            status,
            stdout: stdout.as_bytes().to_vec(),
            stderr: stderr.as_bytes().to_vec(),
            timed_out: false,
            overflow: false,
            reap_unknown: false,
        }
    }

    pub fn timeout() -> Self {
        Self {
            status: -1,
            stdout: Vec::new(),
            stderr: Vec::new(),
            timed_out: true,
            overflow: false,
            reap_unknown: false,
        }
    }

    pub fn overflow() -> Self {
        Self {
            status: -1,
            stdout: Vec::new(),
            stderr: Vec::new(),
            timed_out: false,
            overflow: true,
            reap_unknown: false,
        }
    }

    pub fn trusted_text(&self) -> Result<String, &'static str> {
        if self.timed_out {
            return Err("timeout");
        }
        if self.overflow {
            return Err("overflow");
        }
        if self.reap_unknown {
            return Err("reap-unknown");
        }
        String::from_utf8(self.stdout.clone()).map_err(|_| "utf8")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Poll {
    Running,
    Exited(i32),
}

#[allow(dead_code)]
pub struct Spawn {
    pub argv: Vec<String>,
    pub poll: Poll,
}

pub trait Runner {
    fn run(&mut self, cmd: &Cmd) -> CmdOut;
    fn spawn(&mut self, cmd: &Cmd) -> Result<u64, &'static str>;
    fn poll(&mut self, id: u64) -> Poll;
    fn finish(&mut self, id: u64, timeout_ms: u64) -> CmdOut;
}

pub fn check_cmd(cmd: &Cmd) -> Result<(), &'static str> {
    validate_host_argv(&cmd.argv)?;
    if cmd.timeout_ms == 0 || cmd.timeout_ms > 120_000 {
        return Err("timeout-range");
    }
    if cmd.stdin.len() > UPLOAD_CAP {
        return Err("stdin-cap");
    }
    for (key, value) in &cmd.env {
        if key != "TART_HOME"
            || !value.starts_with('/')
            || value.contains('\0')
            || value.contains('\n')
        {
            return Err("env");
        }
    }
    Ok(())
}

/// Records argv and returns scripted replies in call order.
#[allow(dead_code)]
pub struct Scripted {
    pub calls: Vec<Vec<String>>,
    pub spawns: Vec<Vec<String>>,
    pub replies: Vec<CmdOut>,
    pub early_exit: Option<i32>,
    next_id: u64,
    pub ledger_snap: Vec<String>,
    pub ledger_path: Option<std::path::PathBuf>,
}

#[allow(dead_code)]
impl Scripted {
    pub fn new(replies: Vec<CmdOut>) -> Self {
        Self {
            calls: Vec::new(),
            spawns: Vec::new(),
            replies,
            early_exit: None,
            next_id: 1,
            ledger_snap: Vec::new(),
            ledger_path: None,
        }
    }
}

#[allow(dead_code)]
impl Runner for Scripted {
    fn run(&mut self, cmd: &Cmd) -> CmdOut {
        if let Err(code) = check_cmd(cmd) {
            return CmdOut::code(127, "", code);
        }
        self.calls.push(cmd.argv.clone());
        if let Some(path) = &self.ledger_path {
            let text = std::fs::read_to_string(path).unwrap_or_default();
            self.ledger_snap.push(text);
        }
        if self.replies.is_empty() {
            CmdOut::code(127, "", "no-scripted-reply")
        } else {
            self.replies.remove(0)
        }
    }

    fn spawn(&mut self, cmd: &Cmd) -> Result<u64, &'static str> {
        check_cmd(cmd)?;
        self.spawns.push(cmd.argv.clone());
        let id = self.next_id;
        self.next_id += 1;
        Ok(id)
    }

    fn poll(&mut self, _id: u64) -> Poll {
        match self.early_exit {
            Some(code) => Poll::Exited(code),
            None => Poll::Running,
        }
    }

    fn finish(&mut self, _id: u64, _timeout_ms: u64) -> CmdOut {
        CmdOut::ok("")
    }
}

pub fn needs_elevation(stderr: &str) -> bool {
    let s = stderr.to_ascii_lowercase();
    s.contains("operation not permitted")
        || s.contains("permission denied")
        || s.contains("must be root")
        || s.contains("administrator")
        || s.contains("full disk access")
        || s.contains("not privileged")
}

pub fn cap_output(buf: &mut Vec<u8>) -> bool {
    if buf.len() > OUT_CAP {
        buf.truncate(OUT_CAP);
        true
    } else {
        false
    }
}
