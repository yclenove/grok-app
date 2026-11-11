//! macOS process runner. Unit tests must never call `run` or `spawn`: those
//! start a real process. `new` does not. There is no Linux runner that can
//! reach tart, hdiutil, or ssh.

use std::io::{Read, Write};
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use crate::budget::OUT_CAP;
use crate::osutil::{self, SIGKILL, SIGTERM};
use crate::runner::{check_cmd, Cmd, CmdOut, Poll, Runner};

struct Kid {
    child: Child,
    status: Option<i32>,
}

pub struct ProcessRunner {
    kids: Vec<(u64, Kid)>,
    next: u64,
}

impl ProcessRunner {
    pub fn new() -> Self {
        Self {
            kids: Vec::new(),
            next: 1,
        }
    }
}

impl Default for ProcessRunner {
    fn default() -> Self {
        Self::new()
    }
}

impl Runner for ProcessRunner {
    fn run(&mut self, cmd: &Cmd) -> CmdOut {
        if let Err(code) = check_cmd(cmd) {
            return CmdOut::code(127, "", code);
        }
        let mut child = match spawn_child(cmd, true) {
            Ok(child) => child,
            Err(out) => return out,
        };
        wait_piped(&mut child, cmd)
    }

    fn spawn(&mut self, cmd: &Cmd) -> Result<u64, &'static str> {
        check_cmd(cmd)?;
        let child = match spawn_child(cmd, false) {
            Ok(child) => child,
            Err(_) => return Err("spawn"),
        };
        let id = self.next;
        self.next = self.next.saturating_add(1);
        self.kids.push((
            id,
            Kid {
                child,
                status: None,
            },
        ));
        Ok(id)
    }

    fn poll(&mut self, id: u64) -> Poll {
        let Some(kid) = self.kids.iter_mut().find(|(key, _)| *key == id) else {
            return Poll::Exited(-1);
        };
        if let Some(code) = kid.1.status {
            return Poll::Exited(code);
        }
        match kid.1.child.try_wait() {
            Ok(Some(status)) => {
                let code = status.code().unwrap_or(-1);
                kid.1.status = Some(code);
                Poll::Exited(code)
            }
            _ => Poll::Running,
        }
    }

    fn finish(&mut self, id: u64, timeout_ms: u64) -> CmdOut {
        let Some(pos) = self.kids.iter().position(|(key, _)| *key == id) else {
            return lost(false);
        };
        let (_, mut kid) = self.kids.remove(pos);
        if let Some(code) = kid.status {
            return CmdOut::code(code, "", "");
        }
        let started = Instant::now();
        let limit = Duration::from_millis(timeout_ms);
        loop {
            match kid.child.try_wait() {
                Ok(Some(status)) => return CmdOut::code(status.code().unwrap_or(-1), "", ""),
                Ok(None) if started.elapsed() >= limit => {
                    return match terminate(&mut kid.child) {
                        Some(status) => CmdOut {
                            status: status.code().unwrap_or(-1),
                            stdout: Vec::new(),
                            stderr: Vec::new(),
                            timed_out: true,
                            overflow: false,
                            reap_unknown: false,
                        },
                        None => lost(true),
                    };
                }
                Ok(None) => thread::sleep(Duration::from_millis(20)),
                Err(_) => return lost(false),
            }
        }
    }
}

fn lost(timed_out: bool) -> CmdOut {
    CmdOut {
        status: -1,
        stdout: Vec::new(),
        stderr: Vec::new(),
        timed_out,
        overflow: false,
        reap_unknown: true,
    }
}

fn terminate(child: &mut Child) -> Option<ExitStatus> {
    let pid = child.id();
    if pid > 1 && pid <= i32::MAX as u32 {
        let _ = osutil::signal_group(pid as i32, SIGTERM);
    }
    thread::sleep(Duration::from_millis(200));
    if let Ok(Some(status)) = child.try_wait() {
        return Some(status);
    }
    if pid > 1 && pid <= i32::MAX as u32 {
        let _ = osutil::signal_group(pid as i32, SIGKILL);
    }
    let _ = child.kill();
    child.wait().ok()
}

fn spawn_child(cmd: &Cmd, piped: bool) -> Result<Child, CmdOut> {
    let mut command = Command::new(&cmd.argv[0]);
    command.args(&cmd.argv[1..]);
    for (key, value) in &cmd.env {
        command.env(key, value);
    }
    if piped {
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
    } else {
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
    }
    unsafe {
        command.pre_exec(osutil::own_pgroup);
    }
    match command.spawn() {
        Ok(child) => {
            let pid = child.id();
            if pid > 1 && pid <= i32::MAX as u32 {
                let _ = osutil::set_own_group(pid as i32);
            }
            Ok(child)
        }
        Err(_) => Err(CmdOut::code(127, "", "spawn")),
    }
}

fn wait_piped(child: &mut Child, cmd: &Cmd) -> CmdOut {
    let stdin = child.stdin.take();
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let input = cmd.stdin.clone();
    let in_thread = thread::spawn(move || {
        if let Some(mut pipe) = stdin {
            if !input.is_empty() {
                let _ = pipe.write_all(&input);
            }
        }
    });
    let overflow = Arc::new(AtomicBool::new(false));
    let out_flag = Arc::clone(&overflow);
    let err_flag = Arc::clone(&overflow);
    let out_thread = thread::spawn(move || drain(stdout, out_flag));
    let err_thread = thread::spawn(move || drain(stderr, err_flag));
    let started = Instant::now();
    let limit = Duration::from_millis(cmd.timeout_ms);
    let waited = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Waited::Exit(status),
            Ok(None) if started.elapsed() >= limit => {
                break match terminate(child) {
                    Some(status) => Waited::TimedOut(status),
                    None => Waited::Lost,
                };
            }
            Ok(None) => thread::sleep(Duration::from_millis(10)),
            Err(_) => break Waited::Lost,
        }
    };
    let _ = in_thread.join();
    let stdout = out_thread.join().unwrap_or_default();
    let stderr = err_thread.join().unwrap_or_default();
    let overflowed = overflow.load(Ordering::Relaxed);
    let timed_out = matches!(waited, Waited::TimedOut(_));
    match waited {
        Waited::Lost => CmdOut {
            status: -1,
            stdout,
            stderr,
            timed_out: true,
            overflow: overflowed,
            reap_unknown: true,
        },
        Waited::Exit(_) | Waited::TimedOut(_) if overflowed => CmdOut {
            status: -1,
            stdout,
            stderr,
            timed_out,
            overflow: true,
            reap_unknown: false,
        },
        Waited::Exit(status) | Waited::TimedOut(status) => CmdOut {
            status: code_of(&status),
            stdout,
            stderr,
            timed_out,
            overflow: false,
            reap_unknown: false,
        },
    }
}

enum Waited {
    Exit(ExitStatus),
    TimedOut(ExitStatus),
    Lost,
}

fn code_of(status: &ExitStatus) -> i32 {
    status.code().unwrap_or(-1)
}

fn drain(pipe: Option<impl Read>, overflow: Arc<AtomicBool>) -> Vec<u8> {
    let Some(mut pipe) = pipe else {
        return Vec::new();
    };
    let mut buf = Vec::new();
    let mut tmp = [0u8; 8192];
    loop {
        match pipe.read(&mut tmp) {
            Ok(0) => break,
            Ok(n) => {
                if buf.len() >= OUT_CAP {
                    overflow.store(true, Ordering::Relaxed);
                    continue;
                }
                let room = OUT_CAP - buf.len();
                if n > room {
                    buf.extend_from_slice(&tmp[..room]);
                    overflow.store(true, Ordering::Relaxed);
                } else {
                    buf.extend_from_slice(&tmp[..n]);
                }
            }
            Err(err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        }
    }
    if crate::runner::cap_output(&mut buf) {
        overflow.store(true, Ordering::Relaxed);
    }
    buf
}
