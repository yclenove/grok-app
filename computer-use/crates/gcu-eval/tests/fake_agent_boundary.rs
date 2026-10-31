//! Production executable admission, including relocation of the genuine caller.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "gcu-fake-admission-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct OwnedChild(Child);

impl Drop for OwnedChild {
    fn drop(&mut self) {
        if matches!(self.0.try_wait(), Ok(Some(_))) {
            return;
        }
        let _ = self.0.kill();
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            match self.0.try_wait() {
                Ok(Some(_)) | Err(_) => return,
                Ok(None) => std::thread::sleep(Duration::from_millis(5)),
            }
        }
        eprintln!("could not confirm fake-agent child exit within cleanup budget");
    }
}

/// A freshly written executable can fail one spawn while another thread still
/// holds its write handle: `ERROR_SHARING_VIOLATION` (32) on Windows,
/// `ETXTBSY` "Text file busy" (26) on Unix. Only that one code is retried.
fn is_transient_spawn_error(err: &std::io::Error) -> bool {
    #[cfg(windows)]
    {
        err.raw_os_error() == Some(32)
    }
    #[cfg(unix)]
    {
        err.raw_os_error() == Some(26)
    }
    #[cfg(not(any(windows, unix)))]
    {
        let _ = err;
        false
    }
}

fn bounded_output(command: &mut Command) -> Output {
    // Files avoid blocked pipe writers; the guard kills/reaps even on assertion failure.
    let root = Scratch::new();
    let stdout = root.0.join("stdout");
    let stderr = root.0.join("stderr");
    let mut spawned = None;
    let mut last_error = None;
    for attempt in 0..5 {
        // `spawn` may consume the stdio handles, so each attempt opens new files.
        let out = std::fs::File::create(&stdout).unwrap();
        let err = std::fs::File::create(&stderr).unwrap();
        match command.stdin(Stdio::null()).stdout(out).stderr(err).spawn() {
            Ok(child) => {
                spawned = Some(child);
                break;
            }
            Err(error) if is_transient_spawn_error(&error) && attempt < 4 => {
                last_error = Some(error);
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(error) => panic!("spawn failed: {error}"),
        }
    }
    let child = spawned.unwrap_or_else(|| {
        panic!("transient spawn error persisted: {last_error:?}");
    });
    let mut child = OwnedChild(child);
    // Release the parent's file handles so Windows can remove the scratch root.
    command.stdout(Stdio::null()).stderr(Stdio::null());
    let deadline = Instant::now() + Duration::from_secs(5);
    let status = loop {
        if let Some(status) = child.0.try_wait().unwrap() {
            break status;
        }
        assert!(Instant::now() < deadline, "fake-agent entry did not exit");
        std::thread::sleep(Duration::from_millis(5));
    };
    Output {
        status,
        stdout: std::fs::read(stdout).unwrap(),
        stderr: std::fs::read(stderr).unwrap(),
    }
}

fn caller(hidden: bool) -> &'static Path {
    Path::new(if hidden {
        env!("CARGO_BIN_EXE_gcu-eval")
    } else {
        env!("CARGO_BIN_EXE_eval-fake-agent")
    })
}

fn executable_name(stem: &str) -> String {
    format!("{stem}{}", std::env::consts::EXE_SUFFIX)
}

fn write_start_sentinel(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::write(
            path,
            b"#!/bin/sh\nprintf '%s\\n' list_apps >> \"$GCU_EVAL_TOOL_LOG\"\nexit 0\n",
        )
        .unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    // Windows needs a real executable; its harmless tool log is the sentinel.
    #[cfg(not(unix))]
    std::fs::copy(env!("CARGO_BIN_EXE_eval-fake-gcu"), path).unwrap();
}

fn entry(path: &Path, hidden: bool, flags: u8) -> Command {
    let mut command = Command::new(path);
    if hidden {
        command.arg(gcu_eval::INTERNAL_FAKE_AGENT);
    }
    command.env_clear().env("GCU_EVAL_BEHAVIOR", "crash");
    // flags=0 reproduces the review schedule with all three markers absent.
    for (bit, key) in [
        (1, "GCU_EVAL_ATTESTED"),
        (2, "GCU_EVAL_BUILTIN_GCU"),
        (4, "GCU_EVAL_GCU_REEXEC"),
    ] {
        if flags & bit != 0 {
            command.env(key, "1");
        }
    }
    command
}

fn assert_refused(command: &mut Command, sentinel: &Path) {
    let output = bounded_output(command);
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains(gcu_eval::EXTERNAL_GCU_REFUSED));
    // Admission precedes even handshake/probe writing. A started sentinel
    // executable would append list_apps to this file.
    assert!(!sentinel.exists());
}

#[test]
fn in_process_backend_works_for_original_and_relocated_genuine_entries() {
    let root = Scratch::new();
    for hidden in [false, true] {
        let relocated = root.0.join(executable_name("copied-caller"));
        std::fs::copy(caller(hidden), &relocated).unwrap();
        for path in [caller(hidden), relocated.as_path()] {
            for flags in 0..8 {
                let log = root.0.join("tools.jsonl");
                let output =
                    bounded_output(entry(path, hidden, flags).env("GCU_EVAL_TOOL_LOG", &log));
                // A successful in-process backend is followed by scripted crash.
                assert_eq!(
                    output.status.code(),
                    Some(101),
                    "{}",
                    String::from_utf8_lossy(&output.stderr)
                );
                assert!(std::fs::read_to_string(&log).unwrap().contains("list_apps"));
                std::fs::remove_file(log).unwrap();
            }
        }
    }
}

#[test]
fn relocated_genuine_callers_never_bless_forged_sibling_backends() {
    let root = Scratch::new();
    for hidden in [false, true] {
        let relocated = root.0.join(executable_name("copied-caller"));
        std::fs::copy(caller(hidden), &relocated).unwrap();
        for stem in ["eval-fake-gcu", "gcu-eval"] {
            let external = root.0.join(executable_name(stem));
            // Neither backend form has code identity just because it is adjacent.
            write_start_sentinel(&external);
            let control = root.0.join("sentinel-control.jsonl");
            let output = bounded_output(
                Command::new(&external)
                    .env_clear()
                    .env("GCU_EVAL_TOOL_LOG", &control)
                    .args(["call", "list_apps", "--args", "{}"]),
            );
            assert!(output.status.success());
            assert!(std::fs::read_to_string(&control)
                .unwrap()
                .contains("list_apps"));
            std::fs::remove_file(control).unwrap();
            for flags in 0..8 {
                let log = root.0.join("backend-started.jsonl");
                assert_refused(
                    entry(&relocated, hidden, flags)
                        .env("GCU_BIN", &external)
                        .env("GCU_EVAL_TOOL_LOG", &log),
                    &log,
                );
            }
        }
    }
}

#[test]
fn genuine_external_artifacts_and_empty_backend_paths_are_also_refused() {
    let root = Scratch::new();
    for hidden in [false, true] {
        for backend in [
            env!("CARGO_BIN_EXE_eval-fake-gcu"),
            env!("CARGO_BIN_EXE_gcu-eval"),
            "",
        ] {
            let log = root.0.join("backend-started.jsonl");
            assert_refused(
                entry(caller(hidden), hidden, 7)
                    .env("GCU_BIN", backend)
                    .env("GCU_EVAL_TOOL_LOG", &log),
                &log,
            );
        }
    }
}
