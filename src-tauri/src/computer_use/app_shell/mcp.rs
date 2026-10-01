//! Owned line-oriented MCP client used only by the debug App harness.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use serde_json::{json, Value};

pub(super) struct McpClient {
    child: Option<Child>,
    stdin: Option<std::process::ChildStdin>,
    rx: mpsc::Receiver<String>,
    stdout_reader: Option<std::thread::JoinHandle<()>>,
    stderr_reader: Option<std::thread::JoinHandle<()>>,
    next_id: i64,
}

impl McpClient {
    pub(super) fn spawn(entry: &Value) -> Result<Self, String> {
        let command = entry
            .get("command")
            .and_then(Value::as_str)
            .ok_or("MCP command missing")?;
        let args = entry
            .get("args")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let mut cmd = Command::new(command);
        for arg in &args {
            if let Some(s) = arg.as_str() {
                cmd.arg(s);
            }
        }
        if let Some(rows) = entry.get("env").and_then(Value::as_array) {
            for row in rows {
                if let (Some(name), Some(value)) = (
                    row.get("name").and_then(Value::as_str),
                    row.get("value").and_then(Value::as_str),
                ) {
                    cmd.env(name, value);
                }
            }
        }
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000);
        }
        let mut child = cmd.spawn().map_err(|e| format!("spawn MCP: {e}"))?;
        let stdin = child.stdin.take().ok_or("MCP stdin missing")?;
        let stdout = child.stdout.take().ok_or("MCP stdout missing")?;
        let stderr_reader = child.stderr.take().map(|stderr| {
            std::thread::spawn(move || {
                let _ = std::io::copy(&mut BufReader::new(stderr), &mut std::io::sink());
            })
        });
        let (tx, rx) = mpsc::channel();
        let stdout_reader = std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                let mut line = String::new();
                match reader.read_line(&mut line) {
                    Ok(0) => break,
                    Ok(_) => {
                        if tx.send(line).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        });
        Ok(Self {
            child: Some(child),
            stdin: Some(stdin),
            rx,
            stdout_reader: Some(stdout_reader),
            stderr_reader,
            next_id: 1,
        })
    }

    pub(super) fn initialize(&mut self) -> Result<(), String> {
        let _ = self.call(
            "initialize",
            json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {"name": "cu-app-shell", "version": "1"}
            }),
        )?;
        self.notify("notifications/initialized", json!({}))
    }

    fn notify(&mut self, method: &str, params: Value) -> Result<(), String> {
        let msg = json!({"jsonrpc":"2.0","method":method,"params":params});
        self.write(&msg)
    }

    pub(super) fn call(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let id = self.next_id;
        self.next_id += 1;
        let msg = json!({"jsonrpc":"2.0","id":id,"method":method,"params":params});
        self.write(&msg)?;
        let line = self
            .rx
            .recv_timeout(Duration::from_secs(20))
            .map_err(|_| format!("MCP {method} timed out"))?;
        let value: Value =
            serde_json::from_str(line.trim()).map_err(|e| format!("MCP {method} json: {e}"))?;
        if let Some(err) = value.get("error") {
            return Err(format!("MCP {method} error: {err}"));
        }
        value
            .get("result")
            .cloned()
            .ok_or_else(|| format!("MCP {method} missing result"))
    }

    pub(super) fn tool(&mut self, name: &str, args: Value) -> Result<Value, String> {
        let raw = self.call("tools/call", json!({"name": name, "arguments": args}))?;
        if raw.get("isError") == Some(&json!(true)) {
            let text = raw
                .get("content")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .find_map(|part| part.get("text").and_then(Value::as_str))
                .unwrap_or("");
            return Err(format!("{name} failed: {text}"));
        }
        if let Some(text) = raw
            .get("content")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .find_map(|part| part.get("text").and_then(Value::as_str))
        {
            if let Ok(value) = serde_json::from_str::<Value>(text) {
                return Ok(value);
            }
        }
        Ok(raw)
    }

    fn write(&mut self, msg: &Value) -> Result<(), String> {
        let line = format!("{msg}\n");
        self.stdin
            .as_mut()
            .ok_or_else(|| "MCP stdin closed".to_string())?
            .write_all(line.as_bytes())
            .map_err(|e| e.to_string())?;
        self.stdin
            .as_mut()
            .ok_or_else(|| "MCP stdin closed".to_string())?
            .flush()
            .map_err(|e| e.to_string())
    }

    pub(super) fn kill(&mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        self.stdin.take();
        if let Some(mut child) = self.child.take() {
            if !matches!(child.try_wait(), Ok(Some(_))) {
                let _ = child.kill();
            }
            let _ = child.wait();
        }
        if let Some(reader) = self.stdout_reader.take() {
            let _ = reader.join();
        }
        if let Some(reader) = self.stderr_reader.take() {
            let _ = reader.join();
        }
    }
}

impl Drop for McpClient {
    fn drop(&mut self) {
        self.shutdown();
    }
}
