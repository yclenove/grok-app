//! Command execution injected into install plans.
//!
//! Antigravity's user scope is the only writer that spawns a client CLI.
//! Tests pass [`RecordingExecutor`] so no real `agy` runs. [`ProcessExecutor`]
//! is the production default: it blocks the caller until the child exits or
//! the timeout elapses, then kills the child. There is no cancellation token.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use super::error::InstallError;

/// One child process an installer wants to run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandSpec {
    /// Absolute or `PATH`-resolved program.
    pub program: PathBuf,
    /// Arguments, not including the program.
    pub args: Vec<String>,
    /// Working directory. `None` inherits the parent's.
    pub cwd: Option<PathBuf>,
    /// Environment keys overlaid on the inherited environment.
    pub env: Vec<(String, String)>,
}

/// Captured result of a finished command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandOutput {
    /// Exit status. `0` is success.
    pub status: i32,
    /// Stdout decoded as lossy UTF-8.
    pub stdout: String,
    /// Stderr decoded as lossy UTF-8.
    pub stderr: String,
}

/// Runs client CLIs. Implementations block; they do not touch client config themselves.
pub trait CommandExecutor: Send + Sync {
    /// Run `spec` to completion.
    ///
    /// Blocking. A process-backed implementation must bound the wait and kill
    /// the child when the bound is hit. `RecordingExecutor` returns immediately.
    fn run(&self, spec: &CommandSpec) -> Result<CommandOutput, InstallError>;
}

/// Spawns a real process and kills it when `timeout` elapses.
///
/// Blocking. Default timeout is 30 seconds. Stdin is null. Stdout and stderr
/// are read on background threads so a full pipe cannot deadlock the wait.
#[derive(Debug, Clone)]
pub struct ProcessExecutor {
    /// How long `run` waits before killing the child.
    pub timeout: Duration,
}

impl Default for ProcessExecutor {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(30),
        }
    }
}

impl CommandExecutor for ProcessExecutor {
    fn run(&self, spec: &CommandSpec) -> Result<CommandOutput, InstallError> {
        let program = spec.program.display().to_string();
        let mut cmd = Command::new(&spec.program);
        cmd.args(&spec.args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(cwd) = &spec.cwd {
            cmd.current_dir(cwd);
        }
        for (key, value) in &spec.env {
            cmd.env(key, value);
        }
        let mut child = cmd.spawn().map_err(|source| InstallError::CommandFailed {
            program: program.clone(),
            status: -1,
            stderr: source.to_string(),
        })?;
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let out_handle = std::thread::spawn(move || read_pipe(stdout));
        let err_handle = std::thread::spawn(move || read_pipe(stderr));
        let started = Instant::now();
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if started.elapsed() >= self.timeout => {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = out_handle.join();
                    let _ = err_handle.join();
                    return Err(InstallError::CommandTimeout {
                        program,
                        timeout_ms: timeout_ms(self.timeout),
                    });
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(20)),
                Err(source) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(InstallError::CommandFailed {
                        program,
                        status: -1,
                        stderr: source.to_string(),
                    });
                }
            }
        };
        let stdout = join_pipe(out_handle, &program)?;
        let stderr = join_pipe(err_handle, &program)?;
        let code = status.code().unwrap_or(-1);
        if code != 0 {
            return Err(InstallError::CommandFailed {
                program,
                status: code,
                stderr: truncate(&stderr),
            });
        }
        Ok(CommandOutput {
            status: code,
            stdout,
            stderr,
        })
    }
}

fn timeout_ms(timeout: Duration) -> u64 {
    u64::try_from(timeout.as_millis()).unwrap_or(u64::MAX)
}

fn read_pipe(pipe: Option<impl Read>) -> std::io::Result<String> {
    let mut buf = Vec::new();
    if let Some(mut pipe) = pipe {
        pipe.read_to_end(&mut buf)?;
    }
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

fn join_pipe(
    handle: std::thread::JoinHandle<std::io::Result<String>>,
    program: &str,
) -> Result<String, InstallError> {
    match handle.join() {
        Ok(Ok(text)) => Ok(text),
        Ok(Err(source)) => Err(InstallError::Read {
            path: PathBuf::from(program),
            source,
        }),
        Err(_) => Err(InstallError::CommandFailed {
            program: program.to_owned(),
            status: -1,
            stderr: "output reader panicked".into(),
        }),
    }
}

fn truncate(text: &str) -> String {
    const MAX: usize = 2_048;
    if text.len() <= MAX {
        text.to_owned()
    } else {
        format!("{}…", &text[..MAX])
    }
}

/// Records `run` arguments and returns success without spawning.
///
/// Blocking only on its mutex. Poisoned locks return [`InstallError::LockPoisoned`].
#[derive(Debug, Default)]
pub struct RecordingExecutor {
    calls: Mutex<Vec<CommandSpec>>,
}

impl RecordingExecutor {
    /// Empty recording.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Calls recorded so far, in order.
    pub fn calls(&self) -> Result<Vec<CommandSpec>, InstallError> {
        self.calls
            .lock()
            .map(|guard| guard.clone())
            .map_err(|_| InstallError::LockPoisoned)
    }
}

impl CommandExecutor for RecordingExecutor {
    fn run(&self, spec: &CommandSpec) -> Result<CommandOutput, InstallError> {
        self.calls
            .lock()
            .map_err(|_| InstallError::LockPoisoned)?
            .push(spec.clone());
        Ok(CommandOutput {
            status: 0,
            stdout: String::new(),
            stderr: String::new(),
        })
    }
}

/// Answers a `~/.grok` confirmation prompt.
pub trait Confirmer: Send + Sync {
    /// Return `Ok(true)` to allow the change. Blocking if the implementation waits on a person.
    fn confirm(&self, prompt: &str) -> Result<bool, InstallError>;
}

/// Confirmer with a fixed answer. Used by tests and by callers that already decided.
#[derive(Debug, Clone, Copy)]
pub struct StaticConfirmer {
    /// Answer returned from [`Confirmer::confirm`].
    pub accept: bool,
}

impl Confirmer for StaticConfirmer {
    fn confirm(&self, _prompt: &str) -> Result<bool, InstallError> {
        Ok(self.accept)
    }
}

/// Absolute path of a client binary an installer may spawn.
#[derive(Debug, Clone, Copy, Default)]
pub struct ClientBins<'a> {
    /// `agy`, required for Antigravity user scope when a command must run.
    pub agy: Option<&'a Path>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn process_executor_echo_returns_stdout() {
        let executor = ProcessExecutor {
            timeout: Duration::from_secs(5),
        };
        let output = executor
            .run(&CommandSpec {
                program: PathBuf::from("/bin/echo"),
                args: vec!["ok".into()],
                cwd: None,
                env: Vec::new(),
            })
            .unwrap();
        assert_eq!(output.stdout.trim(), "ok");
        assert_eq!(output.status, 0);
    }

    #[test]
    fn process_executor_timeout_kills_sleep() {
        let executor = ProcessExecutor {
            timeout: Duration::from_millis(200),
        };
        let error = executor
            .run(&CommandSpec {
                program: PathBuf::from("/bin/sleep"),
                args: vec!["5".into()],
                cwd: None,
                env: Vec::new(),
            })
            .unwrap_err();
        assert!(matches!(error, InstallError::CommandTimeout { .. }));
    }

    #[test]
    fn recording_executor_stores_argv_and_does_not_require_a_binary() {
        let executor = RecordingExecutor::new();
        executor
            .run(&CommandSpec {
                program: PathBuf::from("/tmp/missing-agy"),
                args: vec!["mcp".into(), "add".into()],
                cwd: Some(PathBuf::from("/tmp")),
                env: vec![("HOME".into(), "/tmp/home".into())],
            })
            .unwrap();
        let calls = executor.calls().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].args, ["mcp", "add"]);
    }
}
