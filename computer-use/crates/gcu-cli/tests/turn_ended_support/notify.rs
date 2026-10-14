use super::*;

#[cfg(unix)]
#[tokio::test]
async fn installed_codex_chain_preserves_argv_stdin_exit_status_then_notifies_and_uninstalls() {
    use gcu_cli::install::{
        installer, ClientBins, ClientId, InstallCtx, RecordingExecutor, Scope, StaticConfirmer,
    };
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let home = home();
    // A controlled endpoint verifies that the previous program has finished
    // before it receives anything, rather than inferring order from stdout.
    let directory = home.path().join("gcu-control-chain");
    std::fs::create_dir(&directory).unwrap();
    std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700)).unwrap();
    let socket = directory.join("control.sock");
    let listener = tokio::net::UnixListener::bind(&socket).unwrap();
    std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600)).unwrap();
    let metadata = std::fs::metadata(&socket).unwrap();
    for (name, data) in [
        ("token", "a".repeat(64)),
        (
            "endpoint-id",
            json!({"pid":std::process::id(),"device":metadata.dev(),"inode":metadata.ino()})
                .to_string(),
        ),
    ] {
        let path = directory.join(name);
        std::fs::write(&path, data).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    let seen = home.path().join("seen");
    let notify = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        assert_eq!(
            std::fs::read(seen).unwrap(),
            b"before",
            "previous notify must finish first"
        );
        let size = stream.read_u16().await.unwrap();
        let mut request = vec![0; size as usize];
        stream.read_exact(&mut request).await.unwrap();
        assert_eq!(
            String::from_utf8(request).unwrap(),
            format!("turn-ended {} [\"t\",\"chain\"]", "a".repeat(64))
        );
        stream.write_all(b"OK\n").await.unwrap();
    });
    let previous = home.path().join("old");
    let script = "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$HOME/args\"\ncat > \"$HOME/input\"\nprintf 'original-output'\n# A notification must not have happened before this command.\ncp \"$HOME/stamp\" \"$HOME/seen\"\nexit 7\n";
    std::fs::write(&previous, script).unwrap();
    std::fs::set_permissions(&previous, std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::write(home.path().join("stamp"), b"before").unwrap();
    let path = home.path().join(".codex/config.toml");
    std::fs::create_dir(path.parent().unwrap()).unwrap();
    let original = [
        previous.to_str().unwrap(),
        "--flag",
        "space ; $(literal)",
        "--previous-notify",
    ];
    let array = toml::Value::Array(
        original
            .iter()
            .map(|s| toml::Value::String((*s).into()))
            .collect(),
    );
    std::fs::write(&path, format!("notify = {array}\n")).unwrap();
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
    assert!(inst.plan(&ctx).unwrap().is_noop());
    let doc: toml::Value = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let mut args: Vec<_> = doc["notify"]
        .as_array()
        .unwrap()
        .iter()
        .skip(2)
        .map(|v| v.as_str().unwrap().to_owned())
        .collect();
    let payload = r#"{"type":"agent-turn-complete","thread-id":"t","turn-id":"chain"}"#;
    args.push(payload.into());
    let refs: Vec<_> = args.iter().map(std::ffi::OsStr::new).collect();
    let out = hook(home.path(), &refs, b"stdin\0exact\n\n").await;
    assert_eq!(out.status.code(), Some(7));
    assert_eq!(out.stdout, b"original-output");
    assert!(out.stderr.is_empty(), "{out:?}");
    assert_eq!(
        std::fs::read(home.path().join("input")).unwrap(),
        b"stdin\0exact\n\n"
    );
    assert_eq!(
        std::fs::read_to_string(home.path().join("args")).unwrap(),
        format!("{}\n{payload}\n", original[1..].join("\n"))
    );
    assert_eq!(std::fs::read(home.path().join("seen")).unwrap(), b"before");
    notify.await.unwrap();
    let mut bad_args = args.clone();
    *bad_args.last_mut().unwrap() = "{broken".into();
    let refs: Vec<_> = bad_args.iter().map(std::ffi::OsStr::new).collect();
    let out = hook(home.path(), &refs, b"same stdin").await;
    assert_eq!(
        out.status.code(),
        Some(7),
        "hook failure must preserve previous exit code"
    );
    assert_eq!(out.stdout, b"original-output");
    assert_eq!(String::from_utf8_lossy(&out.stderr).lines().count(), 1);
    inst.uninstall(&ctx).unwrap();
    let doc: toml::Value = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(doc["notify"], array);
}

#[cfg(unix)]
#[tokio::test]
async fn previous_notify_signal_is_preserved_even_when_hook_session_is_missing() {
    use std::os::unix::{fs::PermissionsExt, process::ExitStatusExt};
    let home = home();
    let previous = home.path().join("signal");
    std::fs::write(&previous, "#!/bin/sh\nkill -TERM \"$$\"\n").unwrap();
    std::fs::set_permissions(&previous, std::fs::Permissions::from_mode(0o700)).unwrap();
    let payload = r#"{"type":"agent-turn-complete"}"#;
    let out = hook(
        home.path(),
        &[
            "--previous-notify".as_ref(),
            previous.as_os_str(),
            payload.as_ref(),
        ],
        b"",
    )
    .await;
    assert_eq!(out.status.signal(), Some(15));
    assert_eq!(String::from_utf8_lossy(&out.stderr).lines().count(), 1);
}

#[cfg(unix)]
#[tokio::test]
async fn installed_grok_hook_malformed_stdin_is_diagnosed_without_turn_failure() {
    use gcu_cli::install::{
        installer, ClientBins, ClientId, InstallCtx, RecordingExecutor, Scope, StaticConfirmer,
    };
    let home = home();
    let ctx = InstallCtx {
        home: home.path(),
        grok_home: None,
        project_dir: home.path(),
        gcu_exe: Path::new(assert_cmd::cargo::cargo_bin!("gcu")),
        scope: Scope::User,
        interactive: false,
        yes: true,
        backup_stamp: Some("test"),
        confirmer: &StaticConfirmer { accept: true },
        executor: &RecordingExecutor::new(),
        bins: ClientBins::default(),
    };
    installer(ClientId::Grok).apply(&ctx).unwrap();
    let mut child = Command::new("sh")
        .arg(home.path().join(".grok/hooks/computer-use-turn-ended.sh"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"{broken")
        .await
        .unwrap();
    diagnosed(&child.wait_with_output().await.unwrap());
}
