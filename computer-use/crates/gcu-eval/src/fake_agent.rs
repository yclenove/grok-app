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
    let mut invocation = Invocation {
        work: &work,
        env: &env,
        args: &args,
        output: Vec::new(),
    };
    match crate::block_on(invocation.run()) {
        Ok(Ok(code)) => {
            if std::io::stdout().write_all(&invocation.output).is_err() {
                return 1;
            }
            code
        }
        result => {
            eprintln!("eval-fake-agent: {result:?}");
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
}

/// Execute the same fake implementation used by both standalone entries.
/// The runner supplies its isolated map directly; there is no executable path.
pub(crate) async fn execute(
    args: &[String],
    env: &BTreeMap<String, String>,
    work: &Path,
    limit: std::time::Duration,
) -> crate::proc::Finished {
    let mut invocation = Invocation {
        work,
        env,
        args,
        output: Vec::new(),
    };
    let result = tokio::time::timeout(limit, invocation.run()).await;
    let timed_out = result.is_err();
    let exit_code = match result {
        Ok(Ok(code)) => Some(i32::from(code)),
        Ok(Err(_)) => Some(1),
        Err(_) => None,
    };
    let overflow = invocation.output.len() > 1_048_576;
    invocation.output.truncate(1_048_576);
    crate::proc::Finished {
        timed_out,
        exit_code,
        wait_failed: false,
        stdout: crate::proc::PipeCapture {
            bytes: invocation.output,
            overflow,
            incomplete: false,
        },
        capture_incomplete: false,
    }
}

impl Invocation<'_> {
    async fn run(&mut self) -> Result<u8, String> {
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
        self.write_probe()?;
        match behavior.as_str() {
            "timeout" => {
                tokio::time::sleep(std::time::Duration::from_secs(30)).await;
                Ok(0)
            }
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
                    std::fs::write(self.resolve_path(path), b"pwned-secret")
                        .map_err(|err| err.to_string())?;
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
        let bytes = std::fs::read(&path).map_err(|err| err.to_string())?;
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
        std::fs::write(path, bytes).map_err(|err| err.to_string())
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
        std::fs::write(Path::new(&home).join("transcript.json"), bytes)
            .map_err(|err| err.to_string())
    }

    fn ensure_handshake(&self, key: &str) -> Result<(), String> {
        let Some(path) = self.env.get(key) else {
            return Ok(());
        };
        if path.is_empty() {
            return Ok(());
        }
        let path = self.resolve_path(path);
        match std::fs::read_to_string(&path) {
            Ok(text) if !text.is_empty() => Ok(()),
            _ => std::fs::write(path, format!("{}\n", crate::exec::TELEMETRY_HANDSHAKE))
                .map_err(|err| err.to_string()),
        }
    }

    fn append_side(&self, text: &str) -> Result<(), String> {
        let path = self.required_path("GCU_EVAL_SIDE_LOG")?;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .map_err(|err| err.to_string())?;
        file.write_all(text.as_bytes())
            .map_err(|err| err.to_string())
    }

    fn append_tool(&self, line: &str) -> Result<(), String> {
        let path = self.required_path("GCU_EVAL_TOOL_LOG")?;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .map_err(|err| err.to_string())?;
        writeln!(file, "{line}").map_err(|err| err.to_string())
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
        std::fs::write(path, bytes).map_err(|err| err.to_string())
    }

    fn env_text(&self, key: &str) -> String {
        self.env.get(key).cloned().unwrap_or_default()
    }

    fn config_text(&self) -> String {
        let Some(home) = self.env.get("GROK_HOME") else {
            return String::new();
        };
        std::fs::read_to_string(self.resolve_path(home).join("config.toml")).unwrap_or_default()
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
