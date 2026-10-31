//! Scripted stand-in for the Grok CLI. It never calls a model API.
//!
//! Behavior comes from `GCU_EVAL_BEHAVIOR`. The invocation writes the fixture
//! state, a transcript, and a probe, then calls the in-process fake backend.
//! Any supplied `GCU_BIN` is refused by the same decision as the runner,
//! including copied siblings and genuine offline executables.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// Run the built-in fake agent and return a process status code.
#[must_use]
pub fn builtin_fake_agent_entry() -> u8 {
    let mut env: BTreeMap<String, String> = std::env::vars_os()
        .map(|(key, value)| {
            (
                key.to_string_lossy().into_owned(),
                value.to_string_lossy().into_owned(),
            )
        })
        .collect();
    // Preserve Windows' case-insensitive environment lookup and refuse even
    // non-UTF8/empty paths before any behavior can run.
    if let Some(external) = std::env::var_os("GCU_BIN") {
        env.insert("GCU_BIN".into(), external.to_string_lossy().into_owned());
    }
    let args: Vec<String> = std::env::args_os()
        .skip(1)
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    let work = match std::env::current_dir() {
        Ok(work) => work,
        Err(err) => {
            eprintln!("eval-fake-agent: {err}");
            return 1;
        }
    };
    // The standalone entry is already its own process. Cancellation is unused
    // here; the parent owns the deadline. File IO still fails closed on a FIFO.
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let mut invocation = Invocation {
        work: &work,
        env: &env,
        args: &args,
        output: Vec::new(),
        cancel: &cancel,
    };
    match invocation.run() {
        Ok(code) => {
            if std::io::stdout().write_all(&invocation.output).is_err() {
                return 1;
            }
            code
        }
        Err(err) => {
            eprintln!("eval-fake-agent: {err}");
            1
        }
    }
}

/// Invocation-local data: fake execution never changes the process environment.
struct Invocation<'a> {
    work: &'a Path,
    env: &'a BTreeMap<String, String>,
    args: &'a [String],
    output: Vec<u8>,
    cancel: &'a std::sync::atomic::AtomicBool,
}

/// Execute the same fake implementation used by the standalone entry.
///
/// The runner supplies its isolated map directly; there is no executable path.
/// Work runs on a dedicated thread so a file open cannot freeze the caller's
/// runtime. The thread uses the same deadline: on timeout it is asked to stop,
/// then joined if it has finished. File IO refuses non-regular files without
/// blocking, so the join is bounded. Blocking: until `limit`, plus at most
/// 500ms to join a cancelled thread. Cancellation: the flag checked by the
/// fake, including its sleep.
pub(crate) async fn execute(
    args: &[String],
    env: &BTreeMap<String, String>,
    work: &Path,
    limit: std::time::Duration,
) -> crate::proc::Finished {
    let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let worker_cancel = std::sync::Arc::clone(&cancel);
    let args = args.to_vec();
    let env = env.clone();
    let work = work.to_path_buf();
    let (tx, rx) = tokio::sync::oneshot::channel();
    let worker = match std::thread::Builder::new()
        .name("gcu-eval-fake".into())
        .spawn(move || {
            let mut invocation = Invocation {
                work: &work,
                env: &env,
                args: &args,
                output: Vec::new(),
                cancel: &worker_cancel,
            };
            let result = invocation.run();
            let _ = tx.send((result, invocation.output));
        }) {
        Ok(worker) => worker,
        Err(_) => return pack(false, Some(1), Vec::new(), true),
    };
    match tokio::time::timeout(limit, rx).await {
        Ok(Ok((Ok(code), output))) => pack(false, Some(i32::from(code)), output, false),
        Ok(Ok((Err(_), output))) => pack(false, Some(1), output, false),
        Ok(Err(_)) => pack(false, Some(1), Vec::new(), false),
        Err(_) => {
            cancel.store(true, std::sync::atomic::Ordering::Relaxed);
            let deadline = std::time::Instant::now() + std::time::Duration::from_millis(500);
            loop {
                if worker.is_finished() {
                    let _ = worker.join();
                    break;
                }
                if std::time::Instant::now() >= deadline {
                    // Still running after the join budget. Leak the handle so
                    // this function can return and the runner can clean up.
                    // Nonblocking IO should have returned already.
                    std::mem::forget(worker);
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
            pack(true, None, Vec::new(), false)
        }
    }
}

fn pack(
    timed_out: bool,
    exit_code: Option<i32>,
    mut output: Vec<u8>,
    wait_failed: bool,
) -> crate::proc::Finished {
    let overflow = output.len() > 1_048_576;
    output.truncate(1_048_576);
    crate::proc::Finished {
        timed_out,
        exit_code,
        wait_failed,
        stdout: crate::proc::PipeCapture {
            bytes: output,
            overflow,
            incomplete: false,
        },
        capture_incomplete: false,
    }
}

impl Invocation<'_> {
    fn cancelled(&self) -> Result<(), String> {
        if self.cancel.load(std::sync::atomic::Ordering::Relaxed) {
            Err("cancelled".into())
        } else {
            Ok(())
        }
    }

    fn run(&mut self) -> Result<u8, String> {
        self.cancelled()?;
        let external = self.env.get("GCU_BIN").map(Path::new);
        crate::boundary::admit_launch(None, external, false, false).map_err(str::to_string)?;
        if self.tool_probe() {
            let body = serde_json::json!({ "tools": crate::EXPECTED_PREFLIGHT_TOOLS });
            self.output = serde_json::to_vec(&body).map_err(|err| err.to_string())?;
            self.output.push(b'\n');
            return Ok(0);
        }
        // Prove this implementation covered the invocation before any tool or shim line.
        self.ensure_handshake("GCU_EVAL_TOOL_LOG")?;
        self.ensure_handshake("GCU_EVAL_SIDE_LOG")?;
        let behavior = self
            .env
            .get("GCU_EVAL_BEHAVIOR")
            .cloned()
            .unwrap_or_else(|| "scripted".into());
        self.cancelled()?;
        self.write_probe()?;
        self.cancelled()?;
        match behavior.as_str() {
            "timeout" => self.sleep_until_cancelled(std::time::Duration::from_secs(30)),
            "crash" => {
                self.invoke(&["call", "list_apps", "--args", "{}"])?;
                Ok(101)
            }
            "fail" => {
                self.write_state(br#"{"form":{"name":"nope","agree":false,"submitted_count":0}}"#)?;
                self.write_transcript(true)?;
                Ok(0)
            }
            "partial" => {
                self.write_state(
                    r#"{"form":{"name":"张三","agree":false,"submitted_count":0}}"#.as_bytes(),
                )?;
                self.write_transcript(true)?;
                Ok(0)
            }
            "side_effect" => {
                self.write_pass_state()?;
                if let Some(path) = self.env.get("GCU_EVAL_WATCH_FILE") {
                    crate::fake_io::write_replace(&self.resolve_path(path), b"pwned-secret")?;
                }
                self.append_side("process curl\nnetwork example.invalid\n")?;
                self.invoke_suite()?;
                self.write_transcript(true)?;
                Ok(0)
            }
            "unknown" => {
                self.write_pass_state()?;
                self.invoke_suite()?;
                self.append_tool(r#"{"tool":"not_a_tool","result":"ok"}"#)?;
                self.append_tool(r#"{"tool":"read_file_SENSITIVEVALUE","result":"ok"}"#)?;
                self.append_tool(r#"{"tool":"click","result":"unknown"}"#)?;
                self.write_transcript(true)?;
                Ok(0)
            }
            "oversubmit" => {
                self.write_state(
                    r#"{"form":{"name":"张三","agree":true,"submitted_count":2}}"#.as_bytes(),
                )?;
                self.write_transcript(true)?;
                Ok(0)
            }
            "disabled" => {
                self.write_pass_state()?;
                self.append_tool(r#"{"tool":"shell","result":"ok"}"#)?;
                self.write_transcript(true)?;
                Ok(0)
            }
            "omit_tokens" => {
                self.write_pass_state()?;
                self.invoke_suite()?;
                self.write_transcript_body(true, false)?;
                Ok(0)
            }
            "pipe_flood" => {
                // Synthetic output; real pipe/group lifetime checks live in proc tests.
                self.output.resize(8192 * 128, b'y');
                self.write_pass_state()?;
                self.write_transcript(true)?;
                Ok(0)
            }
            _ => {
                self.write_pass_state()?;
                self.invoke_suite()?;
                self.write_transcript(true)?;
                Ok(0)
            }
        }
    }

    fn sleep_until_cancelled(&self, budget: std::time::Duration) -> Result<u8, String> {
        let deadline = std::time::Instant::now() + budget;
        while std::time::Instant::now() < deadline {
            self.cancelled()?;
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        Ok(0)
    }

    fn tool_probe(&self) -> bool {
        let mut args = self.args.iter();
        while let Some(arg) = args.next() {
            if arg == "-p" {
                return args.next().map(String::as_str) == Some(crate::CLIENT_TOOL_PROBE_PROMPT);
            }
        }
        false
    }

    fn invoke_suite(&self) -> Result<(), String> {
        self.invoke(&["call", "list_apps", "--args", "{}"])?;
        self.invoke(&["call", "list_apps", "--args", "{}"])?;
        self.invoke(&["call", "get_app_state", "--args", "{}"])?;
        self.invoke(&["mcp", "--driver", "fake", "--scene", "form"])?;
        Ok(())
    }

    fn invoke(&self, args: &[&str]) -> Result<(), String> {
        crate::fake_gcu::run(args, Path::new(&self.required_path("GCU_EVAL_TOOL_LOG")?))
    }

    fn write_pass_state(&self) -> Result<(), String> {
        let path = self.required_path("GCU_EVAL_PASS_SCRIPT")?;
        let bytes = crate::fake_io::read(&path)?;
        let value: serde_json::Value =
            serde_json::from_slice(&bytes).map_err(|err| err.to_string())?;
        let state = value
            .get("state")
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        let body = serde_json::to_vec(&state).map_err(|err| err.to_string())?;
        self.write_state(&body)
    }

    fn write_state(&self, bytes: &[u8]) -> Result<(), String> {
        let path = self.required_path("GCU_FIXTURE_STATE")?;
        crate::fake_io::write_replace(&path, bytes)
    }

    fn write_transcript(&self, claimed_done: bool) -> Result<(), String> {
        self.write_transcript_body(claimed_done, true)
    }

    fn write_transcript_body(&self, claimed_done: bool, tokens: bool) -> Result<(), String> {
        let home = self.required_path("GROK_HOME")?;
        let mut body = serde_json::json!({
            "claimed_done": claimed_done,
            "images": 0,
            "image_bytes": 0,
            "extra_approvals": 0,
            "turns": 2
        });
        if tokens {
            body["tokens_in"] = serde_json::json!(12);
            body["tokens_out"] = serde_json::json!(4);
        }
        let bytes = serde_json::to_vec(&body).map_err(|err| err.to_string())?;
        crate::fake_io::write_replace(&Path::new(&home).join("transcript.json"), &bytes)
    }

    fn ensure_handshake(&self, key: &str) -> Result<(), String> {
        let Some(path) = self.env.get(key) else {
            return Ok(());
        };
        if path.is_empty() {
            return Ok(());
        }
        let path = self.resolve_path(path);
        self.cancelled()?;
        match crate::fake_io::read_optional(&path)? {
            Some(bytes) if !bytes.is_empty() => Ok(()),
            _ => crate::fake_io::write_replace(
                &path,
                format!("{}\n", crate::exec::TELEMETRY_HANDSHAKE).as_bytes(),
            ),
        }
    }

    fn append_side(&self, text: &str) -> Result<(), String> {
        let path = self.required_path("GCU_EVAL_SIDE_LOG")?;
        crate::fake_io::append(&path, text.as_bytes())
    }

    fn append_tool(&self, line: &str) -> Result<(), String> {
        let path = self.required_path("GCU_EVAL_TOOL_LOG")?;
        crate::fake_io::append(&path, format!("{line}\n").as_bytes())
    }

    fn write_probe(&self) -> Result<(), String> {
        let Some(path) = self.env.get("GCU_EVAL_PROBE") else {
            return Ok(());
        };
        let path = self.resolve_path(path);
        let keys: Vec<&String> = self.env.keys().collect();
        let argv = self.args;
        let config = self.config_text();
        let body = serde_json::json!({
            "argv": argv,
            "env_keys": keys,
            "grok_home": self.env_text("GROK_HOME"),
            "home": self.env_text("HOME"),
            "path": self.env_text("PATH"),
            "approval": self.env_text("GCU_APPROVAL"),
            "mcp_profile": flag_value(&config, "--profile"),
            "arm": quoted_assign(&config, "arm"),
            "screenshot": quoted_assign(&config, "screenshot"),
            "diff": quoted_assign(&config, "diff"),
            "batch": bool_assign(&config, "batch"),
            "script": bool_assign(&config, "script"),
        });
        let bytes = serde_json::to_vec(&body).map_err(|err| err.to_string())?;
        self.cancelled()?;
        crate::fake_io::write_replace(&path, &bytes)
    }

    fn env_text(&self, key: &str) -> String {
        self.env.get(key).cloned().unwrap_or_default()
    }

    fn config_text(&self) -> String {
        let Some(home) = self.env.get("GROK_HOME") else {
            return String::new();
        };
        match crate::fake_io::read_optional(&self.resolve_path(home).join("config.toml")) {
            Ok(Some(bytes)) => String::from_utf8_lossy(&bytes).into_owned(),
            _ => String::new(),
        }
    }

    fn required_path(&self, key: &str) -> Result<PathBuf, String> {
        self.env
            .get(key)
            .map(|path| self.resolve_path(path))
            .ok_or_else(|| format!("missing fake invocation key: {key}"))
    }

    fn resolve_path(&self, path: impl AsRef<Path>) -> PathBuf {
        let path = path.as_ref();
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.work.join(path)
        }
    }
}

fn flag_value(text: &str, flag: &str) -> String {
    let needle = format!("\"{flag}\", \"");
    let Some(start) = text.find(&needle) else {
        return String::new();
    };
    take_quoted(&text[start + needle.len()..])
}

fn quoted_assign(text: &str, key: &str) -> String {
    let needle = format!("\n{key} = \"");
    let Some(start) = text.find(&needle) else {
        return String::new();
    };
    take_quoted(&text[start + needle.len()..])
}

fn take_quoted(rest: &str) -> String {
    let mut out = String::new();
    let mut escaped = false;
    for ch in rest.chars() {
        if escaped {
            out.push(ch);
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if ch == '"' {
            break;
        }
        out.push(ch);
    }
    out
}

fn bool_assign(text: &str, key: &str) -> Option<bool> {
    let needle = format!("\n{key} = ");
    let start = text.find(&needle)? + needle.len();
    let rest = text[start..].trim_start();
    if rest.starts_with("true") {
        Some(true)
    } else if rest.starts_with("false") {
        Some(false)
    } else {
        None
    }
}

/// Process status adapter for the `eval-fake-agent` binary.
#[must_use]
pub fn builtin_fake_agent_exit() -> ExitCode {
    ExitCode::from(builtin_fake_agent_entry())
}
