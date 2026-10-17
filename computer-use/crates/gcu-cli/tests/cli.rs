//! CLI process contracts; every desktop and user directory is a private fixture.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use assert_cmd::Command;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Stdio};
use tempfile::TempDir;
#[cfg(target_os = "linux")]
#[path = "support/resume.rs"]
mod resume;
#[cfg(target_os = "linux")]
#[path = "support/unsafe_pty.rs"]
mod unsafe_pty;

fn command(home: &TempDir) -> Command {
    let mut c = Command::from_std(native_command(home));
    c.timeout(std::time::Duration::from_secs(20));
    c
}
fn native_command(home: &TempDir) -> std::process::Command {
    let mut c = std::process::Command::new(assert_cmd::cargo::cargo_bin!("gcu"));
    c.env("XDG_CONFIG_HOME", home.path())
        .env("XDG_RUNTIME_DIR", home.path())
        .env("XDG_STATE_HOME", home.path().join("state"))
        .env("LOCALAPPDATA", home.path().join("local"))
        .env("APPDATA", home.path())
        .env("HOME", home.path())
        .env("TMPDIR", home.path())
        .env("GCU_ALLOW_FAKE", "1")
        .env_remove("GCU_APPROVAL")
        .env_remove("GCU_EVAL_READ_ERROR")
        .env_remove("GCU_EVAL_ORACLE_FILE");
    c
}

#[test]
fn arguments_unsupported_modes_exit_two() {
    let h = TempDir::new().unwrap();
    for args in [
        vec!["mcp"],
        vec!["call", "list_apps"],
        vec!["doctor"],
        vec!["mcp", "--driver", "real"],
        vec!["mcp", "--transport", "http"],
        vec![
            "mcp",
            "--connect",
            "http://127.0.0.1:9999",
            "--token-fd",
            "4",
        ],
        vec!["call", "list_apps", "--driver", "fake", "--args", "oops"],
        vec!["call", "list_apps", "--driver", "fake", "--args", "[]"],
        vec![
            "call",
            "click",
            "--driver",
            "fake",
            "--args",
            r#"{"app":"fake.form","element_index":1,"click_count":0}"#,
        ],
        vec![
            "call",
            "click",
            "--driver",
            "fake",
            "--args",
            r#"{"app":"fake.form","element_index":1,"x":20,"y":20}"#,
        ],
        vec![
            "call",
            "scroll",
            "--driver",
            "fake",
            "--args",
            r#"{"app":"fake.form","element_index":1,"direction":"down","pages":0}"#,
        ],
        vec!["doctor", "--driver", "fake", "--scene", "missing"],
    ] {
        command(&h).args(args).assert().code(2);
    }
}

#[test]
fn version_and_doctor_report_honest_fake_metadata() {
    let h = TempDir::new().unwrap();
    command(&h).arg("version").assert().success();
    let o = command(&h)
        .args(["doctor", "--driver", "fake", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v: Value = serde_json::from_slice(&o).unwrap();
    assert_eq!(v["driver"], "fake");
    assert!(v["permissions"]["note"].as_str().unwrap().contains("fake"));
    assert!(v["capabilities"]["observe"]["tree"].as_bool().unwrap());
    assert!(v["recent_calls"].as_array().unwrap().is_empty());
    assert_eq!(v["logging"]["destination"], "stderr");
}

#[test]
fn call_json_preserves_mcp_result_and_business_exit() {
    let h = TempDir::new().unwrap();
    for (tool, args, code, error) in [
        ("list_apps", "{}", 0, None),
        (
            "get_app_state",
            r#"{"app":"fake.form"}"#,
            4,
            Some("approval_denied"),
        ),
        (
            "get_app_state",
            r#"{"app":"missing"}"#,
            4,
            Some("app_not_found"),
        ),
        (
            "press_key",
            r#"{"app":"fake.form","key":"bogus_key"}"#,
            2,
            Some("invalid_key"),
        ),
    ] {
        let o = command(&h)
            .args(["call", tool, "--driver", "fake", "--args", args, "--json"])
            .assert()
            .code(code)
            .get_output()
            .stdout
            .clone();
        let v: Value = serde_json::from_slice(&o).unwrap();
        assert!(v["content"].is_array(), "{v}");
        assert_eq!(v["isError"], error.is_some());
        if let Some(error) = error {
            assert_eq!(v["structuredContent"]["error"]["code"], error);
        }
    }
}

#[test]
fn fake_and_approval_environment_build_gates_fail_closed() {
    let h = TempDir::new().unwrap();
    let code = if cfg!(debug_assertions) { 0 } else { 2 };
    command(&h)
        .env_remove("GCU_ALLOW_FAKE")
        .args(["doctor", "--driver", "fake"])
        .assert()
        .code(code);
    let code = if cfg!(all(feature = "eval-autoapprove", debug_assertions)) {
        0
    } else {
        2
    };
    command(&h)
        .env("GCU_APPROVAL", "auto-allow-session")
        .args(["doctor", "--driver", "fake"])
        .assert()
        .code(code);
    command(&h)
        .env("GCU_APPROVAL", "invalid")
        .args(["doctor", "--driver", "fake"])
        .assert()
        .code(2);
}

#[test]
fn resume_non_tty_refuses_before_reading_credentials() {
    let h = TempDir::new().unwrap();
    let output = command(&h)
        .arg("resume")
        .assert()
        .code(2)
        .get_output()
        .stderr
        .clone();
    assert!(String::from_utf8_lossy(&output).contains("TTY"));
    let output = command(&h)
        .args(["resume", "--help"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert!(String::from_utf8_lossy(&output).contains("same UID"));
}

#[test]
fn config_malformed_or_invalid_values_exit_two() {
    let h = TempDir::new().unwrap();
    // The config root differs per platform: macOS keeps it under Library/Application Support,
    // while Windows (APPDATA) and Linux (XDG_CONFIG_HOME) use the temp root itself.
    let root = if cfg!(target_os = "macos") {
        h.path().join("Library").join("Application Support")
    } else {
        h.path().to_path_buf()
    };
    let dir = root.join("grok-computer-use");
    std::fs::create_dir_all(&dir).unwrap();
    for config in [
        "[observe",
        "[guard]\nuser_idle_ms = -1",
        "[observe]\nmax_image_edge = 0",
    ] {
        std::fs::write(dir.join("config.toml"), config).unwrap();
        command(&h)
            .args(["doctor", "--driver", "fake"])
            .assert()
            .code(2);
    }
}

struct Mcp {
    child: Child,
    input: Option<ChildStdin>,
    output: BufReader<ChildStdout>,
    id: u64,
}
impl Mcp {
    fn start(h: &TempDir, profile: &str) -> Self {
        Self::start_with_approval(
            h,
            profile,
            cfg!(all(feature = "eval-autoapprove", debug_assertions)),
        )
    }
    fn start_with_approval(h: &TempDir, profile: &str, automatic: bool) -> Self {
        let mut c = std::process::Command::new(assert_cmd::cargo::cargo_bin!("gcu"));
        c.args([
            "mcp",
            "--driver",
            "fake",
            "--scene",
            "form",
            "--profile",
            profile,
        ])
        .env("XDG_CONFIG_HOME", h.path())
        .env("XDG_RUNTIME_DIR", h.path())
        .env("XDG_STATE_HOME", h.path().join("state"))
        .env("LOCALAPPDATA", h.path().join("local"))
        .env("APPDATA", h.path())
        .env("HOME", h.path())
        .env("TMPDIR", h.path())
        .env("GCU_ALLOW_FAKE", "1")
        .env_remove("GCU_APPROVAL")
        .env_remove("GCU_EVAL_READ_ERROR")
        .env_remove("GCU_EVAL_ORACLE_FILE")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
        if automatic {
            c.env("GCU_APPROVAL", "auto-allow-session")
                .env("GCU_EVAL_ORACLE_FILE", h.path().join("oracle.json"));
        }
        let mut child = c.spawn().unwrap();
        let input = child.stdin.take();
        let output = BufReader::new(child.stdout.take().unwrap());
        let mut m = Self {
            child,
            input,
            output,
            id: 0,
        };
        m.request("initialize", json!({"protocolVersion":"2025-11-25","capabilities":{"elicitation":{"form":{}}},"clientInfo":{"name":"cli-test","version":"0"}}));
        m.send(json!({"jsonrpc":"2.0","method":"notifications/initialized"}));
        m
    }
    fn send(&mut self, value: Value) {
        writeln!(self.input.as_mut().unwrap(), "{value}").unwrap();
        self.input.as_mut().unwrap().flush().unwrap();
    }
    fn request(&mut self, method: &str, params: Value) -> Value {
        self.id += 1;
        self.send(json!({"jsonrpc":"2.0","id":self.id,"method":method,"params":params}));
        let mut line = String::new();
        assert!(self.output.read_line(&mut line).unwrap() > 0);
        let v: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(v["id"], self.id, "{v}");
        assert!(v.get("error").is_none(), "{v}");
        v["result"].clone()
    }
    fn call(&mut self, tool: &str, args: Value) -> Value {
        self.request("tools/call", json!({"name":tool,"arguments":args}))
    }
    fn close(mut self) {
        self.input.take();
        assert!(self.child.wait().unwrap().success());
    }
}

#[cfg(all(feature = "eval-autoapprove", debug_assertions))]
#[test]
fn call_form_click_is_verified_by_oracle_and_image_survives_exit() {
    let h = TempDir::new().unwrap();
    for (tool, args) in [
        ("get_app_state", r#"{"app":"fake.form"}"#),
        ("click", r#"{"app":"fake.form","element_index":3}"#),
    ] {
        let oracle = h.path().join("oracle.json");
        let o = command(&h)
            .env("GCU_APPROVAL", "auto-allow-session")
            .env("GCU_EVAL_ORACLE_FILE", &oracle)
            .args(["call", tool, "--driver", "fake", "--args", args])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        let o = String::from_utf8(o).unwrap();
        if tool == "get_app_state" {
            let path = o
                .lines()
                .find_map(|l| l.strip_prefix("Screenshot file: "))
                .unwrap();
            assert!(std::fs::read(path).unwrap().starts_with(b"\x89PNG"));
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                assert_eq!(
                    std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
                    0o600
                );
                assert_eq!(
                    std::fs::metadata(std::path::Path::new(path).parent().unwrap())
                        .unwrap()
                        .permissions()
                        .mode()
                        & 0o777,
                    0o700
                );
            }
        } else {
            let v: Value = serde_json::from_slice(&std::fs::read(oracle).unwrap()).unwrap();
            assert_eq!(v["presses"]["submit"], 1);
            let report = command(&h)
                .args(["doctor", "--driver", "fake", "--json"])
                .assert()
                .success()
                .get_output()
                .stdout
                .clone();
            let report: Value = serde_json::from_slice(&report).unwrap();
            assert_eq!(report["recent_calls"][0]["tool"], "click");
            assert!(report["recent_calls"][0].get("target").is_none());
            assert!(report["recent_calls"][0].get("thumbnail").is_none());
        }
    }
}

#[cfg(all(feature = "eval-autoapprove", debug_assertions))]
#[test]
fn call_config_denied_app_and_observation_limits_are_enforced() {
    let h = TempDir::new().unwrap();
    let dir = h.path().join("grok-computer-use");
    std::fs::create_dir(&dir).unwrap();
    std::fs::write(
        dir.join("config.toml"),
        "[observe]\nmax_tree_nodes=2\nmax_image_edge=320\nimage_format='jpeg'",
    )
    .unwrap();
    let out = command(&h)
        .env("GCU_APPROVAL", "auto-allow-session")
        .args([
            "call",
            "get_app_state",
            "--driver",
            "fake",
            "--args",
            r#"{"app":"fake.form"}"#,
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v: Value = serde_json::from_slice(&out).unwrap();
    let screenshot = &v["structuredContent"]["state"]["screenshot"];
    assert!(screenshot["width"].as_u64().unwrap() <= 320);
    assert_eq!(screenshot["mime"], "image/jpeg");
    assert!(v["structuredContent"]["state"]["truncated"].is_object());
    let output = command(&h)
        .env("GCU_APPROVAL", "auto-allow-session")
        .args([
            "call",
            "get_app_state",
            "--driver",
            "fake",
            "--args",
            r#"{"app":"fake.form"}"#,
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let output = String::from_utf8(output).unwrap();
    let path = output
        .lines()
        .find_map(|line| line.strip_prefix("Screenshot file: "))
        .unwrap();
    assert!(std::fs::read(path).unwrap().starts_with(b"\xff\xd8"));
    std::fs::write(
        dir.join("config.toml"),
        "[policy]\nextra_denied_apps=['fake.form']",
    )
    .unwrap();
    let out = command(&h)
        .env("GCU_APPROVAL", "auto-allow-session")
        .args([
            "call",
            "get_app_state",
            "--driver",
            "fake",
            "--args",
            r#"{"app":"fake.form"}"#,
            "--json",
        ])
        .assert()
        .code(4)
        .get_output()
        .stdout
        .clone();
    let v: Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["structuredContent"]["error"]["code"], "app_denied");
}
impl Drop for Mcp {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn mcp_profiles_never_expose_model_resume() {
    let h = TempDir::new().unwrap();
    for (profile, count) in [("minimal", 9), ("default", 14), ("full", 14)] {
        let mut m = Mcp::start(&h, profile);
        let list = m.request("tools/list", json!({}));
        let tools = list["tools"].as_array().unwrap();
        assert_eq!(tools.len(), count);
        assert_eq!(
            tools.iter().any(|tool| tool["name"] == "wait_for"),
            profile != "minimal"
        );
        assert!(tools
            .iter()
            .all(|t| !t["name"].as_str().unwrap().contains("resume")));
        assert_eq!(m.call("resume", json!({}))["isError"], true);
        m.close();
    }
}

#[test]
fn mcp_client_elicitation_cannot_bypass_default_denial() {
    let h = TempDir::new().unwrap();
    let mut m = Mcp::start_with_approval(&h, "default", false);
    let denied = m.call("get_app_state", json!({"app":"fake.form"}));
    assert_eq!(
        denied["structuredContent"]["error"]["code"],
        "approval_denied"
    );
    m.close();
}

#[cfg(unix)]
#[test]
fn mcp_control_stop_persists_across_process_restart() {
    use std::io::Read;
    let h = TempDir::new().unwrap();
    let mut m = Mcp::start(&h, "default");
    let directory = std::fs::read_dir(h.path())
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| {
            p.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("gcu-control-")
        })
        .unwrap();
    let mut control =
        std::os::unix::net::UnixStream::connect(directory.join("control.sock")).unwrap();
    control
        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .unwrap();
    control.write_all(&4u16.to_be_bytes()).unwrap();
    control.write_all(b"stop").unwrap();
    let mut ack = [0; 3];
    control.read_exact(&mut ack).unwrap();
    assert_eq!(&ack, b"OK\n");
    for name in ["token", "challenge"] {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(directory.join(name))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
    assert_eq!(
        m.call("get_app_state", json!({"app":"fake.form"}))["structuredContent"]["error"]["code"],
        "stopped_by_user"
    );
    m.close();
    assert!(!directory.exists());
    let mut next = Mcp::start(&h, "default");
    assert_eq!(
        next.call("get_app_state", json!({"app":"fake.form"}))["structuredContent"]["error"]
            ["code"],
        "stopped_by_user"
    );
    next.close();
}

#[cfg(all(feature = "eval-autoapprove", debug_assertions))]
#[test]
fn mcp_form_submit_process_is_verified_by_fake_oracle() {
    let h = TempDir::new().unwrap();
    let mut m = Mcp::start(&h, "default");
    m.request("tools/list", json!({}));
    for (tool, args) in [
        ("get_app_state", json!({"app":"fake.form"})),
        ("click", json!({"app":"fake.form","element_index":1})),
        ("type_text", json!({"app":"fake.form","text":"Ada"})),
        ("click", json!({"app":"fake.form","element_index":3})),
    ] {
        assert_eq!(m.call(tool, args)["isError"], false);
    }
    m.close();
    let oracle: Value =
        serde_json::from_slice(&std::fs::read(h.path().join("oracle.json")).unwrap()).unwrap();
    assert_eq!(oracle["text"]["text"], "Ada");
    assert_eq!(oracle["presses"]["submit"], 1);
    assert_eq!(oracle["held_input"], 0);
}

#[test]
fn call_private_file_io_failure_exits_internal_ten() {
    let h = TempDir::new().unwrap();
    command(&h)
        .env("TMPDIR", h.path().join("missing"))
        .env("TMP", h.path().join("missing"))
        .env("TEMP", h.path().join("missing"))
        .args(["call", "list_apps", "--driver", "fake", "--json"])
        .assert()
        .code(10);
}

#[cfg(all(feature = "eval-autoapprove", debug_assertions))]
#[test]
fn call_permission_ambiguous_and_out_of_bounds_match_exit_groups() {
    let h = TempDir::new().unwrap();
    for (error, code) in [("permission_missing", 3), ("ambiguous_app", 2)] {
        let out = command(&h)
            .env("GCU_EVAL_READ_ERROR", error)
            .args([
                "call",
                "get_app_state",
                "--driver",
                "fake",
                "--args",
                r#"{"app":"fake.form"}"#,
                "--json",
            ])
            .assert()
            .code(code)
            .get_output()
            .stdout
            .clone();
        let v: Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(v["structuredContent"]["error"]["code"], error);
    }
    let out = command(&h)
        .env("GCU_APPROVAL", "auto-allow-session")
        .args([
            "call",
            "click",
            "--driver",
            "fake",
            "--args",
            r#"{"app":"fake.form","x":99999,"y":99999}"#,
            "--json",
        ])
        .assert()
        .code(2)
        .get_output()
        .stdout
        .clone();
    let v: Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["structuredContent"]["error"]["code"], "out_of_bounds");
}
