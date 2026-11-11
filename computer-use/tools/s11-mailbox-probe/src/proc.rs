//! Direct-child runner. After the stored deadline the parent never blocks.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::os::unix::io::{FromRawFd, IntoRawFd};
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use crate::budget::{GRACE_MS, OUT_CAP, REAP_MS};
use crate::osutil::{self, SIGKILL, SIGTERM};
use crate::runner::{check_cmd, Cmd, CmdOut, Runner};

struct Live {
    id: u32,
    child: Child,
    end: Instant,
    stdout: Option<File>,
    stderr: Option<File>,
    out: Vec<u8>,
    err: Vec<u8>,
}

pub struct ProcessRunner {
    live: Option<Live>,
    next: u32,
}

impl ProcessRunner {
    pub fn new() -> Self {
        Self {
            live: None,
            next: 1,
        }
    }
}

impl Default for ProcessRunner {
    fn default() -> Self {
        Self::new()
    }
}

fn pipe_file(piped: impl IntoRawFd) -> io::Result<File> {
    let raw = piped.into_raw_fd();
    osutil::set_nonblock(raw)?;
    Ok(unsafe { File::from_raw_fd(raw) })
}

fn stdin_file(bytes: &[u8]) -> io::Result<File> {
    static N: AtomicU64 = AtomicU64::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    let path: PathBuf = std::env::temp_dir().join(format!("s11-in-{}-{n}", std::process::id()));
    let mut opts = OpenOptions::new();
    opts.read(true).write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut file = opts.open(&path)?;
    let _ = fs::remove_file(&path);
    file.write_all(bytes)?;
    file.seek(SeekFrom::Start(0))?;
    Ok(file)
}

fn spawn_child(cmd: &Cmd, end: Instant) -> Result<Live, &'static str> {
    check_cmd(cmd).map_err(|_| "rejected")?;
    let mut command = Command::new(&cmd.argv[0]);
    command.args(&cmd.argv[1..]);
    command.env_clear();
    for (k, v) in &cmd.env {
        command.env(k, v);
    }
    if cmd.stdin.is_empty() {
        command.stdin(Stdio::null());
    } else {
        let file = stdin_file(&cmd.stdin).map_err(|_| "stdin")?;
        command.stdin(Stdio::from(file));
    }
    if cmd.null_stdio {
        command.stdout(Stdio::null());
        command.stderr(Stdio::null());
    } else {
        command.stdout(Stdio::piped());
        command.stderr(Stdio::piped());
    }
    unsafe {
        command.pre_exec(osutil::own_pgroup);
    }
    let mut child = command.spawn().map_err(|_| "spawn")?;
    let pid = child.id() as i32;
    if pid <= 1 {
        let _ = child.kill();
        let _ = child.try_wait();
        return Err("bad-pid");
    }
    let _ = osutil::set_own_group(pid);
    let stdout = if cmd.null_stdio {
        None
    } else {
        Some(pipe_file(child.stdout.take().ok_or("stdio")?).map_err(|_| "stdio")?)
    };
    let stderr = if cmd.null_stdio {
        None
    } else {
        Some(pipe_file(child.stderr.take().ok_or("stdio")?).map_err(|_| "stdio")?)
    };
    Ok(Live {
        id: 0,
        child,
        end,
        stdout,
        stderr,
        out: Vec::new(),
        err: Vec::new(),
    })
}

enum Drain {
    Pending,
    Eof,
    Overflow,
    Err,
}

fn drain(file: &mut File, buf: &mut Vec<u8>) -> Drain {
    let mut tmp = [0u8; 8192];
    loop {
        if buf.len() > OUT_CAP {
            return Drain::Overflow;
        }
        match file.read(&mut tmp) {
            Ok(0) => return Drain::Eof,
            Ok(n) => {
                buf.extend_from_slice(&tmp[..n]);
                if buf.len() > OUT_CAP {
                    return Drain::Overflow;
                }
            }
            Err(err) if err.kind() == io::ErrorKind::WouldBlock => return Drain::Pending,
            Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => return Drain::Err,
        }
    }
}

fn pump(live: &mut Live) -> (bool, bool) {
    let mut overflow = false;
    let mut io_error = false;
    if let Some(out) = live.stdout.as_mut() {
        match drain(out, &mut live.out) {
            Drain::Overflow => overflow = true,
            Drain::Err => io_error = true,
            _ => {}
        }
    }
    if let Some(err) = live.stderr.as_mut() {
        match drain(err, &mut live.err) {
            Drain::Overflow => overflow = true,
            Drain::Err => io_error = true,
            _ => {}
        }
    }
    (overflow, io_error)
}

fn drop_pipes(live: &mut Live) {
    live.stdout.take();
    live.stderr.take();
}

fn finish_live(mut live: Live) -> CmdOut {
    let pid = live.child.id() as i32;
    let end = live.end;
    let mut overflow = false;
    let mut io_error = false;
    let mut timed_out = false;

    if Instant::now() >= end {
        return reap_past(&mut live, pid);
    }

    let grace = Duration::from_millis(GRACE_MS);
    let reap = Duration::from_millis(REAP_MS);
    let now = Instant::now();
    let left = end.saturating_duration_since(now);
    let (grace, reap) = if grace + reap >= left {
        let half = left / 2;
        (half, left.saturating_sub(half))
    } else {
        (grace, reap)
    };
    let term_at = end - grace - reap;

    let status = loop {
        let (over, bad) = pump(&mut live);
        overflow |= over;
        io_error |= bad;
        if overflow {
            let _ = osutil::signal_group(pid, SIGKILL);
            timed_out = true;
            break None;
        }
        match live.child.try_wait() {
            Ok(Some(st)) => break st.code().or(Some(1)),
            Ok(None) if Instant::now() >= term_at => break None,
            Ok(None) => thread::sleep(Duration::from_millis(10)),
            Err(_) => {
                io_error = true;
                break None;
            }
        }
    };

    if status.is_none() && !overflow {
        let _ = osutil::signal_group(pid, SIGTERM);
        timed_out = true;
        let grace_end = Instant::now() + grace;
        let grace_end = if grace_end < end { grace_end } else { end };
        loop {
            let (over, bad) = pump(&mut live);
            overflow |= over;
            io_error |= bad;
            match live.child.try_wait() {
                Ok(Some(st)) => {
                    let _ = (over, bad);
                    drop_pipes(&mut live);
                    let _ = osutil::signal_group(pid, SIGKILL);
                    return pack(
                        &live,
                        st.code().or(Some(1)),
                        timed_out,
                        overflow,
                        false,
                        io_error,
                    );
                }
                Ok(None) if Instant::now() >= grace_end => break,
                Ok(None) => thread::sleep(Duration::from_millis(10)),
                Err(_) => {
                    io_error = true;
                    break;
                }
            }
        }
        let _ = osutil::signal_group(pid, SIGKILL);
    }

    loop {
        let (over, bad) = pump(&mut live);
        overflow |= over;
        io_error |= bad;
        match live.child.try_wait() {
            Ok(Some(st)) => {
                drop_pipes(&mut live);
                let _ = osutil::signal_group(pid, SIGKILL);
                return pack(
                    &live,
                    st.code().or(Some(1)),
                    timed_out,
                    overflow,
                    false,
                    io_error,
                );
            }
            Ok(None) if Instant::now() >= end => {
                drop_pipes(&mut live);
                let _ = osutil::signal_group(pid, SIGKILL);
                let _ = live.child.try_wait();
                return pack(&live, None, true, overflow, true, io_error);
            }
            Ok(None) => thread::sleep(Duration::from_millis(10)),
            Err(_) => {
                drop_pipes(&mut live);
                return pack(&live, None, timed_out, overflow, true, true);
            }
        }
    }
}

fn reap_past(live: &mut Live, pid: i32) -> CmdOut {
    match live.child.try_wait() {
        Ok(Some(st)) => {
            let _ = pump(live);
            drop_pipes(live);
            pack(live, st.code().or(Some(1)), false, false, false, false)
        }
        _ => {
            let _ = osutil::signal_group(pid, SIGKILL);
            match live.child.try_wait() {
                Ok(Some(st)) => {
                    drop_pipes(live);
                    pack(live, st.code().or(Some(1)), true, false, false, false)
                }
                _ => {
                    drop_pipes(live);
                    pack(live, None, true, false, true, false)
                }
            }
        }
    }
}

fn pack(
    live: &Live,
    status: Option<i32>,
    timed_out: bool,
    overflow: bool,
    reap_unknown: bool,
    io_error: bool,
) -> CmdOut {
    CmdOut {
        status,
        stdout: live.out.clone(),
        stderr: live.err.clone(),
        timed_out,
        overflow,
        reap_unknown,
        io_error,
    }
}

impl Runner for ProcessRunner {
    fn run(&mut self, cmd: &Cmd) -> CmdOut {
        let end = Instant::now() + cmd.timeout;
        let live = match spawn_child(cmd, end) {
            Ok(live) => live,
            Err(reason) => return CmdOut::failed(reason),
        };
        finish_live(live)
    }

    fn spawn(&mut self, cmd: &Cmd) -> Result<u32, &'static str> {
        if self.live.is_some() {
            return Err("busy");
        }
        let end = Instant::now() + cmd.timeout;
        let mut live = spawn_child(cmd, end)?;
        let id = self.next;
        self.next += 1;
        live.id = id;
        self.live = Some(live);
        Ok(id)
    }

    fn finish(&mut self, id: u32) -> CmdOut {
        let Some(live) = self.live.take() else {
            return CmdOut::failed("missing-spawn");
        };
        if live.id != id {
            self.live = Some(live);
            return CmdOut::failed("missing-spawn");
        }
        finish_live(live)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn echo() -> Cmd {
        Cmd {
            argv: vec!["/bin/echo".into(), "hello".into()],
            timeout: Duration::from_millis(2_000),
            stdin: Vec::new(),
            env: Vec::new(),
            null_stdio: false,
        }
    }

    #[test]
    fn echo_sleep_overflow_and_orphan_are_bounded() {
        let mut r = ProcessRunner::new();
        let out = r.run(&echo());
        assert_eq!(out.status, Some(0), "{:?}", out.stderr);
        assert_eq!(out.stdout, b"hello\n");
        assert!(!out.timed_out);

        let started = Instant::now();
        let out = r.run(&Cmd {
            argv: vec!["/bin/sleep".into(), "30".into()],
            timeout: Duration::from_millis(500),
            stdin: Vec::new(),
            env: Vec::new(),
            null_stdio: false,
        });
        assert!(started.elapsed() < Duration::from_secs(3), "sleep hung");
        assert!(out.timed_out);
        assert!(!out.reap_unknown);

        let started = Instant::now();
        let out = r.run(&Cmd {
            argv: vec!["/bin/cat".into(), "/dev/zero".into()],
            timeout: Duration::from_secs(2),
            stdin: Vec::new(),
            env: Vec::new(),
            null_stdio: false,
        });
        assert!(started.elapsed() < Duration::from_secs(3), "cat hung");
        assert!(out.overflow);

        let started = Instant::now();
        let out = r.run(&Cmd {
            argv: vec!["/bin/sh".into(), "-c".into(), "sleep 30 & exit 0".into()],
            timeout: Duration::from_secs(2),
            stdin: Vec::new(),
            env: Vec::new(),
            null_stdio: false,
        });
        assert!(started.elapsed() < Duration::from_secs(3), "orphan hung");
        assert_eq!(out.status, Some(0));

        let started = Instant::now();
        let out = r.run(&Cmd {
            argv: vec![
                "/bin/sh".into(),
                "-c".into(),
                "trap '' TERM; sleep 30".into(),
            ],
            timeout: Duration::from_millis(700),
            stdin: Vec::new(),
            env: Vec::new(),
            null_stdio: false,
        });
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "term-ignore hung"
        );
        assert!(out.timed_out || out.status.is_some());
        assert!(!out.reap_unknown);
    }
}
