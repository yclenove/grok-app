//! Injectable command runner. `finish` uses the deadline stored at `spawn`.

use std::time::Duration;

use crate::argv::closed_program;
use crate::budget::{ATTEMPT_MS, UPLOAD_CAP};

#[derive(Clone, Debug)]
pub struct Cmd {
    pub argv: Vec<String>,
    pub timeout: Duration,
    pub stdin: Vec<u8>,
    pub env: Vec<(String, String)>,
    pub null_stdio: bool,
}

#[derive(Clone, Debug)]
pub struct CmdOut {
    pub status: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub timed_out: bool,
    pub overflow: bool,
    pub reap_unknown: bool,
    pub io_error: bool,
}

impl CmdOut {
    pub fn ok_text(text: &str) -> Self {
        Self {
            status: Some(0),
            stdout: text.as_bytes().to_vec(),
            stderr: Vec::new(),
            timed_out: false,
            overflow: false,
            reap_unknown: false,
            io_error: false,
        }
    }

    pub fn status_text(status: i32, text: &str) -> Self {
        Self {
            status: Some(status),
            stdout: text.as_bytes().to_vec(),
            stderr: Vec::new(),
            timed_out: false,
            overflow: false,
            reap_unknown: false,
            io_error: false,
        }
    }

    pub fn failed(reason: &str) -> Self {
        Self {
            status: None,
            stdout: Vec::new(),
            stderr: reason.as_bytes().to_vec(),
            timed_out: false,
            overflow: false,
            reap_unknown: false,
            io_error: true,
        }
    }

    pub fn timed_out() -> Self {
        Self {
            status: None,
            stdout: Vec::new(),
            stderr: Vec::new(),
            timed_out: true,
            overflow: false,
            reap_unknown: false,
            io_error: false,
        }
    }

    pub fn clean_exit(&self) -> bool {
        self.status == Some(0)
            && !self.timed_out
            && !self.overflow
            && !self.reap_unknown
            && !self.io_error
    }
}

pub fn trusted_text(out: &CmdOut) -> Result<&str, &'static str> {
    if out.timed_out || out.overflow || out.reap_unknown || out.io_error || out.status != Some(0) {
        return Err("untrusted");
    }
    std::str::from_utf8(&out.stdout).map_err(|_| "untrusted")
}

pub fn check_cmd(cmd: &Cmd) -> Result<(), &'static str> {
    let Some(prog) = cmd.argv.first() else {
        return Err("argv");
    };
    if !prog.starts_with('/') || prog.contains("..") || prog.contains('\0') {
        return Err("argv");
    }
    let ms = cmd.timeout.as_millis();
    if ms == 0 || ms > u128::from(ATTEMPT_MS) {
        return Err("timeout");
    }
    if cmd.stdin.len() > UPLOAD_CAP {
        return Err("stdin");
    }
    for (k, v) in &cmd.env {
        if k != "TART_HOME" || !v.starts_with('/') || v.contains("..") || v.contains('\0') {
            return Err("env");
        }
    }
    Ok(())
}

/// Host path: closed program set, then the generic command check.
pub fn host_cmd(
    argv: Vec<String>,
    timeout: Duration,
    stdin: Vec<u8>,
    tart_home: Option<&std::path::Path>,
    null_stdio: bool,
) -> Result<Cmd, &'static str> {
    closed_program(&argv)?;
    let mut env = Vec::new();
    if let Some(home) = tart_home {
        env.push(("TART_HOME".into(), home.display().to_string()));
    }
    let cmd = Cmd {
        argv,
        timeout,
        stdin,
        env,
        null_stdio,
    };
    check_cmd(&cmd)?;
    Ok(cmd)
}

pub trait Runner {
    fn run(&mut self, cmd: &Cmd) -> CmdOut;
    fn spawn(&mut self, cmd: &Cmd) -> Result<u32, &'static str>;
    fn finish(&mut self, id: u32) -> CmdOut;
}

#[cfg(test)]
#[derive(Clone, Debug)]
struct Script {
    needle: String,
    outs: Vec<CmdOut>,
    used: usize,
}

#[cfg(test)]
pub struct Fake {
    pub calls: Vec<Vec<String>>,
    script: Vec<Script>,
    spawned: Vec<(u32, CmdOut)>,
    next: u32,
}

#[cfg(test)]
impl Fake {
    pub fn new(script: Vec<(&str, CmdOut)>) -> Self {
        Self::sequence(script.into_iter().map(|(n, o)| (n, vec![o])).collect())
    }

    /// Each needle yields the next scripted output, then repeats its last one.
    pub fn sequence(script: Vec<(&str, Vec<CmdOut>)>) -> Self {
        Self {
            calls: Vec::new(),
            script: script
                .into_iter()
                .map(|(needle, outs)| Script {
                    needle: needle.into(),
                    outs,
                    used: 0,
                })
                .collect(),
            spawned: Vec::new(),
            next: 1,
        }
    }

    fn reply(&mut self, argv: &[String]) -> CmdOut {
        note_keygen(argv);
        let joined = argv.join(" ");
        let Some(slot) = self.script.iter_mut().find(|s| joined.contains(&s.needle)) else {
            return CmdOut::failed("unscripted");
        };
        if slot.outs.is_empty() {
            return CmdOut::failed("unscripted");
        }
        let i = slot.used.min(slot.outs.len() - 1);
        slot.used += 1;
        slot.outs[i].clone()
    }
}

#[cfg(test)]
fn note_keygen(argv: &[String]) {
    if argv.first().map(String::as_str) != Some("/usr/bin/ssh-keygen") {
        return;
    }
    let Some(pos) = argv.iter().position(|a| a == "-f") else {
        return;
    };
    let Some(path) = argv.get(pos + 1) else {
        return;
    };
    let body = "A".repeat(64);
    let _ = std::fs::write(path, b"private\n");
    let _ = std::fs::write(format!("{path}.pub"), format!("ssh-ed25519 {body}\n"));
}

#[cfg(test)]
impl Runner for Fake {
    fn run(&mut self, cmd: &Cmd) -> CmdOut {
        self.calls.push(cmd.argv.clone());
        if check_cmd(cmd).is_err() {
            return CmdOut::failed("rejected");
        }
        self.reply(&cmd.argv)
    }

    fn spawn(&mut self, cmd: &Cmd) -> Result<u32, &'static str> {
        self.calls.push(cmd.argv.clone());
        check_cmd(cmd)?;
        let id = self.next;
        self.next += 1;
        let out = self.reply(&cmd.argv);
        self.spawned.push((id, out));
        Ok(id)
    }

    fn finish(&mut self, id: u32) -> CmdOut {
        self.spawned
            .iter()
            .find(|(i, _)| *i == id)
            .map(|(_, o)| o.clone())
            .unwrap_or_else(|| CmdOut::failed("missing-spawn"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_zero_timeout_and_bad_env() {
        let mut cmd = Cmd {
            argv: vec!["/bin/echo".into()],
            timeout: Duration::from_millis(0),
            stdin: Vec::new(),
            env: Vec::new(),
            null_stdio: false,
        };
        assert_eq!(check_cmd(&cmd), Err("timeout"));
        cmd.timeout = Duration::from_millis(500);
        cmd.env.push(("PATH".into(), "/bin".into()));
        assert_eq!(check_cmd(&cmd), Err("env"));
        assert!(host_cmd(
            vec!["/bin/sh".into()],
            Duration::from_millis(500),
            Vec::new(),
            None,
            false
        )
        .is_err());
    }
}
