//! Offline hook/control/installer tests. Every user directory is temporary.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use serde_json::{json, Value};
use std::{
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tempfile::TempDir;
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, ChildStdout, Command},
};

fn command(home: &Path) -> Command {
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("gcu"));
    for name in [
        "HOME",
        "APPDATA",
        "LOCALAPPDATA",
        "XDG_CONFIG_HOME",
        "XDG_STATE_HOME",
        "XDG_RUNTIME_DIR",
        "TMPDIR",
        "TMP",
        "TEMP",
    ] {
        cmd.env(name, home);
    }
    cmd.env_remove("GCU_APPROVAL")
        .env("GCU_ALLOW_FAKE", "1")
        .kill_on_drop(true);
    cmd
}
fn home() -> TempDir {
    #[cfg(unix)]
    let root = Path::new("/tmp").to_path_buf();
    #[cfg(windows)]
    let root = std::env::temp_dir();
    tempfile::Builder::new()
        .prefix("te")
        .tempdir_in(root)
        .unwrap()
}
async fn hook(home: &Path, args: &[&std::ffi::OsStr], stdin: &[u8]) -> std::process::Output {
    let mut child = command(home)
        .arg("turn-ended")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // A hook that rejects its arguments exits without reading stdin, so the write can hit a
    // closed pipe; that is a legitimate outcome, not a test failure.
    let mut input = child.stdin.take().unwrap();
    let _ = input.write_all(stdin).await;
    drop(input);
    tokio::time::timeout(Duration::from_secs(15), child.wait_with_output())
        .await
        .unwrap()
        .unwrap()
}
fn clean_success(out: &std::process::Output) {
    assert!(out.status.success(), "{out:?}");
    assert!(out.stdout.is_empty(), "{out:?}");
    assert!(out.stderr.is_empty(), "{out:?}");
}
fn diagnosed(out: &std::process::Output) {
    assert!(out.status.success(), "hooks must be nonfatal: {out:?}");
    assert!(out.stdout.is_empty(), "{out:?}");
    assert_eq!(
        String::from_utf8_lossy(&out.stderr).lines().count(),
        1,
        "{out:?}"
    );
    assert!(String::from_utf8_lossy(&out.stderr).contains("turn-ended"));
}

struct Mcp {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
    directory: PathBuf,
    id: u64,
}
impl Mcp {
    async fn start(home: &Path) -> Self {
        let existing: Vec<_> = std::fs::read_dir(home)
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        let mut child = command(home)
            .args(["mcp", "--driver", "fake"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let input = child.stdin.take().unwrap();
        let output = BufReader::new(child.stdout.take().unwrap());
        let mut mcp = Self {
            child,
            input,
            output,
            directory: PathBuf::new(),
            id: 0,
        };
        mcp.request("initialize", json!({"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"turn-test","version":"0"}})).await;
        mcp.send(json!({"jsonrpc":"2.0","method":"notifications/initialized"}))
            .await;
        mcp.directory = std::fs::read_dir(home)
            .unwrap()
            .map(|e| e.unwrap().path())
            .find(|p| {
                !existing.contains(p)
                    && p.file_name()
                        .unwrap()
                        .to_string_lossy()
                        .starts_with("gcu-control-")
            })
            .unwrap();
        mcp
    }
    async fn send(&mut self, value: Value) {
        self.input
            .write_all(format!("{value}\n").as_bytes())
            .await
            .unwrap();
    }
    async fn request(&mut self, method: &str, params: Value) -> Value {
        self.id += 1;
        self.send(json!({"jsonrpc":"2.0","id":self.id,"method":method,"params":params}))
            .await;
        let mut line = String::new();
        tokio::time::timeout(Duration::from_secs(10), self.output.read_line(&mut line))
            .await
            .unwrap()
            .unwrap();
        let value: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(value["id"], self.id);
        value["result"].clone()
    }
    async fn close(mut self) {
        self.input.shutdown().await.unwrap();
        drop(self.input);
        assert!(
            tokio::time::timeout(Duration::from_secs(10), self.child.wait())
                .await
                .unwrap()
                .unwrap()
                .success()
        );
        assert!(!self.directory.exists());
    }
}
async fn stop(directory: &Path) {
    #[cfg(unix)]
    let mut stream = tokio::net::UnixStream::connect(directory.join("control.sock"))
        .await
        .unwrap();
    #[cfg(windows)]
    let mut stream = tokio::net::windows::named_pipe::ClientOptions::new()
        .open(std::fs::read_to_string(directory.join("endpoint")).unwrap())
        .unwrap();
    stream.write_u16(4).await.unwrap();
    stream.write_all(b"stop").await.unwrap();
    let mut ack = [0; 3];
    stream.read_exact(&mut ack).await.unwrap();
    assert_eq!(&ack, b"OK\n");
}

#[tokio::test]
async fn grok_end_turn_duplicate_and_codex_notify_use_existing_control() {
    let home = home();
    let mcp = Mcp::start(home.path()).await;
    let token = std::fs::read(mcp.directory.join("token")).unwrap();
    for _ in 0..2 {
        clean_success(&hook(home.path(), &[], br#"{"hookEventName":"stop","reason":"end_turn","lastAssistantMessage":"ok","future":true}"#).await);
    }
    let payload = r#"{"type":"agent-turn-complete","thread-id":"t","turn-id":"1","future":{}}"#;
    clean_success(&hook(home.path(), &[payload.as_ref()], b"").await);
    assert_eq!(
        std::fs::read(mcp.directory.join("token")).unwrap(),
        token,
        "turn-ended must not consume resume credentials"
    );
    mcp.close().await;
}

#[tokio::test]
async fn turn_ended_when_stopped_keeps_latch_across_calls() {
    let home = home();
    let mut mcp = Mcp::start(home.path()).await;
    stop(&mcp.directory).await;
    clean_success(
        &hook(
            home.path(),
            &[],
            br#"{"hookEventName":"stop","reason":"end_turn"}"#,
        )
        .await,
    );
    let out = mcp
        .request(
            "tools/call",
            json!({"name":"get_app_state","arguments":{"app":"fake.form"}}),
        )
        .await;
    assert_eq!(out["structuredContent"]["error"]["code"], "stopped_by_user");
    mcp.close().await;
}

#[tokio::test]
async fn grok_shutdown_cancelled_and_other_client_events_are_ignored() {
    let home = home();
    for payload in [
        r#"{"hookEventName":"stop","reason":"shutdown"}"#,
        r#"{"hookEventName":"stop_cancelled","reason":"max_turns"}"#,
        r#"{"type":"other-event"}"#,
    ] {
        clean_success(&hook(home.path(), &[], payload.as_bytes()).await);
    }
}

#[tokio::test]
async fn malformed_stdin_and_arguments_are_visible_nonfatal_and_redacted() {
    let home = home();
    for payload in [
        b"{".as_slice(),
        b"[]",
        b"",
        br#"{"reason":5}"#,
        br#"{"reason":"end_turn"}"#,
    ] {
        diagnosed(&hook(home.path(), &[], payload).await);
    }
    diagnosed(&hook(home.path(), &["--session".as_ref()], b"{}").await);
    let secret = b"malformed-private-message";
    let out = hook(home.path(), &[], secret).await;
    diagnosed(&out);
    assert!(!String::from_utf8_lossy(&out.stderr).contains("private-message"));
    diagnosed(&hook(home.path(), &[], &vec![b' '; 1024 * 1024 + 1]).await);
}

#[tokio::test]
async fn missing_previous_notify_and_session_emit_one_nonfatal_diagnostic() {
    let home = home();
    let missing = home.path().join("missing-notify");
    let payload = r#"{"type":"agent-turn-complete"}"#;
    let out = hook(
        home.path(),
        &[
            "--previous-notify".as_ref(),
            missing.as_os_str(),
            payload.as_ref(),
        ],
        b"",
    )
    .await;
    diagnosed(&out);
    assert!(String::from_utf8_lossy(&out.stderr).contains("previous notify"));
}

#[tokio::test]
async fn missing_or_ambiguous_session_is_refused_without_affecting_other_sessions() {
    let home = home();
    let payload = r#"{"type":"agent-turn-complete","thread-id":"t","turn-id":"1"}"#;
    diagnosed(&hook(home.path(), &[payload.as_ref()], b"").await);
    let first = Mcp::start(home.path()).await;
    let mut second = Mcp::start(home.path()).await;
    let selected = home.path().join("missing");
    diagnosed(
        &hook(
            home.path(),
            &["--session".as_ref(), selected.as_os_str(), payload.as_ref()],
            b"",
        )
        .await,
    );
    assert!(!second.request("tools/list", json!({})).await["tools"]
        .as_array()
        .unwrap()
        .is_empty());
    // Real simultaneous sessions require explicit selection on every platform.
    diagnosed(&hook(home.path(), &[payload.as_ref()], b"").await);
    clean_success(
        &hook(
            home.path(),
            &[
                "--session".as_ref(),
                first.directory.as_os_str(),
                payload.as_ref(),
            ],
            b"",
        )
        .await,
    );
    first.close().await;
    second.close().await;
}

#[cfg(unix)]
#[tokio::test]
async fn replaced_socket_or_unsafe_session_is_refused_before_payload_is_sent() {
    use std::os::unix::fs::PermissionsExt;
    let home = home();
    let mcp = Mcp::start(home.path()).await;
    let path = mcp.directory.join("control.sock");
    std::fs::remove_file(&path).unwrap();
    let replacement = tokio::net::UnixListener::bind(&path).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    let payload = r#"{"type":"agent-turn-complete"}"#;
    diagnosed(
        &hook(
            home.path(),
            &[
                "--session".as_ref(),
                mcp.directory.as_os_str(),
                payload.as_ref(),
            ],
            b"",
        )
        .await,
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(50), replacement.accept())
            .await
            .is_err()
    );
    std::fs::set_permissions(&mcp.directory, std::fs::Permissions::from_mode(0o755)).unwrap();
    diagnosed(
        &hook(
            home.path(),
            &[
                "--session".as_ref(),
                mcp.directory.as_os_str(),
                payload.as_ref(),
            ],
            b"",
        )
        .await,
    );
    std::fs::set_permissions(&mcp.directory, std::fs::Permissions::from_mode(0o700)).unwrap();
    mcp.close().await;
}

#[cfg(unix)]
#[path = "turn_ended_support/notify.rs"]
mod notify;

#[tokio::test]
async fn installed_codex_empty_previous_notify_is_restored_and_hook_still_works() {
    use gcu_cli::install::{
        installer, ClientBins, ClientId, InstallCtx, RecordingExecutor, Scope, StaticConfirmer,
    };
    let home = home();
    let mcp = Mcp::start(home.path()).await;
    let path = home.path().join(".codex/config.toml");
    std::fs::create_dir(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "notify = []\n").unwrap();
    let ctx = InstallCtx {
        home: home.path(),
        grok_home: None,
        project_dir: home.path(),
        gcu_exe: Path::new(assert_cmd::cargo::cargo_bin!("gcu")),
        scope: Scope::User,
        interactive: false,
        yes: false,
        backup_stamp: Some("test"),
        confirmer: &StaticConfirmer { accept: false },
        executor: &RecordingExecutor::new(),
        bins: ClientBins::default(),
    };
    let inst = installer(ClientId::Codex);
    inst.apply(&ctx).unwrap();
    let doc: toml::Value = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let mut args: Vec<_> = doc["notify"]
        .as_array()
        .unwrap()
        .iter()
        .skip(2)
        .map(|v| v.as_str().unwrap().to_owned())
        .collect();
    args.push(r#"{"type":"agent-turn-complete"}"#.into());
    let refs: Vec<_> = args.iter().map(std::ffi::OsStr::new).collect();
    clean_success(&hook(home.path(), &refs, b"").await);
    inst.uninstall(&ctx).unwrap();
    let doc: toml::Value = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert!(doc["notify"].as_array().unwrap().is_empty());
    mcp.close().await;
}

#[cfg(windows)]
#[tokio::test]
async fn replaced_pipe_server_process_is_refused_before_credentials_are_sent() {
    let home = home();
    let mcp = Mcp::start(home.path()).await;
    let name = format!(r"\\.\pipe\gcu-test-{}", std::process::id());
    let mut pipe = tokio::net::windows::named_pipe::ServerOptions::new()
        .first_pipe_instance(true)
        .reject_remote_clients(true)
        .create(&name)
        .unwrap();
    std::fs::write(mcp.directory.join("endpoint"), &name).unwrap();
    let path = mcp.directory.join("endpoint-id");
    let mut id: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    id["name"] = json!(name);
    // The endpoint still claims the original server PID; the OS reveals this
    // different process before the hook can write a token to the replacement.
    std::fs::write(&path, id.to_string()).unwrap();
    let server = tokio::spawn(async move {
        pipe.connect().await.unwrap();
        let mut byte = [0];
        match pipe.read(&mut byte).await {
            Ok(size) => assert_eq!(size, 0),
            Err(error) => assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe),
        }
    });
    let payload = r#"{"type":"agent-turn-complete"}"#;
    diagnosed(
        &hook(
            home.path(),
            &[
                "--session".as_ref(),
                mcp.directory.as_os_str(),
                payload.as_ref(),
            ],
            b"",
        )
        .await,
    );
    tokio::time::timeout(Duration::from_secs(5), server)
        .await
        .unwrap()
        .unwrap();
    mcp.close().await;
}
