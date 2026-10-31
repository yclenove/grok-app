//! Approval decisions, grant file, and the approval document contract.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use assert_cmd::Command;
use gcu_cli::approval::{
    allows, approval_page, autoclick_from_env, confirm_for_test, decide_with_autoclick,
    dialog_plan, grant, handoff_for_test, list_text, revoke, storage_key, window_problems,
    DialogCopy, HookRequest, StoreFault,
};
use gcu_host_api::{
    AppIdentity, AppInfo, ApprovalDecision, Cancel, GrantProof, HandoffReason, ImageRef, Platform,
    Risk, SignatureState,
};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;
use tempfile::TempDir;

static AUTOCLICK_ENV: Mutex<()> = Mutex::new(());

fn sample(name: &str, signature: SignatureState) -> AppInfo {
    AppInfo {
        name: name.into(),
        pid: Some(7),
        identity: AppIdentity {
            platform: Platform::Linux,
            key: "demo.app".into(),
            team_id: None,
            publisher: Some("Example Publisher".into()),
            sha256: Some([0xab; 32]),
            signature,
            image_path: "/usr/bin/demo".into(),
            proof: GrantProof {
                file_dev: Some(1),
                file_inode: Some(2),
                ..GrantProof::default()
            },
        },
        icon: Some(ImageRef::from_bytes("image/png", 1, 1, vec![1, 2, 3])),
    }
}

fn hook<'a>(
    app: &'a AppInfo,
    path: Option<&'a Path>,
    cancel: &'a Cancel,
    autoclick: Option<&'a str>,
    timeout: Duration,
    always_enabled: bool,
) -> HookRequest<'a> {
    HookRequest {
        file: path,
        app,
        reason: None,
        risk: Risk::Low,
        timeout,
        always_enabled,
        cancel,
        autoclick,
        mode: None,
        interactive: false,
        summary: "click Send",
        crop_attached: false,
    }
}

fn gcu(home: &TempDir) -> Command {
    let mut command = Command::from_std(std::process::Command::new(assert_cmd::cargo::cargo_bin!(
        "gcu"
    )));
    command
        .timeout(Duration::from_secs(30))
        .env("XDG_CONFIG_HOME", home.path())
        .env("XDG_RUNTIME_DIR", home.path())
        .env("XDG_STATE_HOME", home.path().join("state"))
        .env("LOCALAPPDATA", home.path().join("local"))
        .env("APPDATA", home.path())
        .env("HOME", home.path())
        .env("TMPDIR", home.path())
        .env("GCU_ALLOW_FAKE", "1")
        .env_remove("GCU_APPROVAL")
        .env_remove("GCU_APPROVAL_TEST_AUTOCLICK")
        .env_remove("GCU_EVAL_READ_ERROR")
        .env_remove("GCU_EVAL_ORACLE_FILE");
    command
}

fn state_file(home: &TempDir) -> PathBuf {
    #[cfg(windows)]
    {
        home.path()
            .join("local")
            .join("grok-computer-use")
            .join("state")
            .join("approvals.json")
    }
    #[cfg(target_os = "macos")]
    {
        home.path()
            .join("Library/Application Support/grok-computer-use/state/approvals.json")
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        home.path()
            .join("state")
            .join("grok-computer-use")
            .join("approvals.json")
    }
}

#[test]
fn catalog_locales_match_english_and_label_model_reason() {
    assert!(gcu_mcp::audit().is_empty(), "{:?}", gcu_mcp::audit());
    assert_eq!(gcu_mcp::text("zh", "approval.reason_label"), "模型提供");
    assert_eq!(gcu_mcp::text("zh-TW", "approval.reason_label"), "模型提供");
    assert_eq!(gcu_mcp::locale_id("zh_CN.UTF-8"), "zh");
    assert_eq!(gcu_mcp::locale_id("zh-Hant"), "zh-TW");
    assert_eq!(gcu_mcp::locale_id("pt_PT.UTF-8"), "pt-BR");
    assert_eq!(gcu_mcp::locale_id("de_DE.UTF-8"), "de");
    assert_eq!(gcu_mcp::locale_id("tl"), "fil");
    assert_eq!(gcu_mcp::locale_id("C"), "en");
}

#[test]
fn page_shows_os_identity_and_neutralizes_model_reason() {
    let app = sample("<img src=x>", SignatureState::Unsigned);
    let page = approval_page("en", &app, Some("line\n<b>\u{202E}end"), Risk::High, true);
    assert!(page.contains("Not signed"), "{page}");
    assert!(page.contains("Example Publisher"), "{page}");
    assert!(page.contains("/usr/bin/demo"), "{page}");
    assert!(page.contains("Model-provided"), "{page}");
    assert!(page.contains("line &lt;b&gt;"), "{page}");
    assert!(page.contains('\u{FFFD}'), "{page}");
    assert!(!page.contains('\u{202E}'), "{page}");
    assert!(!page.contains("<img src=x>"), "{page}");
    assert!(page.contains("&lt;img src=x&gt;"), "{page}");
    assert!(page.contains("data:image/png;base64,AQID"), "{page}");
    assert!(page.contains("class=\"aux\""), "{page}");
    assert!(page.contains("plugins or libraries"), "{page}");
    assert!(
        page.contains("id=\"deny\" data-decision=\"deny\" autofocus"),
        "{page}"
    );
    assert!(!page.contains("{{"), "{page}");
    assert!(
        window_problems(&page).is_empty(),
        "{:?}",
        window_problems(&page)
    );
    assert!(page.contains("Enter in chat"), "{page}");
    let script = page
        .split("<script>")
        .nth(1)
        .unwrap()
        .split("</script>")
        .next()
        .unwrap();
    assert!(!script.contains("Enter"), "{script}");
    assert!(script.contains("Escape"), "{script}");

    let zh = approval_page("zh", &app, Some("理由"), Risk::Low, false);
    assert!(zh.contains("模型提供"), "{zh}");
    assert!(zh.contains("允许 Grok Computer Use 使用「"), "{zh}");
    assert!(zh.contains("data-decision=\"always\" hidden"), "{zh}");

    let mut svg = sample("Demo", SignatureState::Valid);
    svg.icon = Some(ImageRef::from_bytes(
        "image/svg+xml",
        1,
        1,
        b"<svg>".to_vec(),
    ));
    let plain = approval_page("en", &svg, None, Risk::Low, true);
    assert!(!plain.contains("data:image/svg"), "{plain}");
    assert!(plain.contains("Valid"), "{plain}");
}

#[tokio::test]
async fn autoclick_covers_four_decisions_timeout_and_cancel() {
    let app = sample("Demo", SignatureState::Unsigned);
    for (hook_name, expected) in [
        ("turn", ApprovalDecision::Turn),
        ("session", ApprovalDecision::Session),
        ("always", ApprovalDecision::Always),
        ("deny", ApprovalDecision::Deny),
    ] {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("approvals.json");
        let cancel = Cancel::new();
        let decision = decide_with_autoclick(&hook(
            &app,
            Some(&path),
            &cancel,
            Some(hook_name),
            Duration::from_secs(2),
            true,
        ))
        .await;
        assert_eq!(decision, expected, "{hook_name}");
        if expected == ApprovalDecision::Always {
            assert!(allows(&path, &app.identity));
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
                assert_eq!(mode, 0o600);
            }
        } else {
            assert!(!path.exists(), "{hook_name}");
        }
    }

    let cancel = Cancel::new();
    let started = std::time::Instant::now();
    let decision = decide_with_autoclick(&hook(
        &app,
        None,
        &cancel,
        Some("timeout"),
        Duration::from_millis(150),
        true,
    ))
    .await;
    assert_eq!(decision, ApprovalDecision::Timeout);
    assert!(started.elapsed() >= Duration::from_millis(100));

    let cancel = Cancel::new();
    let watcher = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(40)).await;
        watcher.cancel();
    });
    let started = std::time::Instant::now();
    let decision = decide_with_autoclick(&hook(
        &app,
        None,
        &cancel,
        Some("cancel"),
        Duration::from_secs(5),
        true,
    ))
    .await;
    assert_eq!(decision, ApprovalDecision::Deny);
    assert!(started.elapsed() < Duration::from_secs(2));

    let allowed = confirm_for_test(&hook(
        &app,
        None,
        &Cancel::new(),
        Some("confirm-allow"),
        Duration::from_secs(2),
        true,
    ))
    .await;
    let denied = confirm_for_test(&hook(
        &app,
        None,
        &Cancel::new(),
        Some("confirm-deny"),
        Duration::from_secs(2),
        true,
    ))
    .await;
    assert!(allowed);
    assert!(!denied);
}

#[tokio::test]
async fn eval_modes_only_auto_grant_apps_and_never_confirm_or_persist() {
    let home = TempDir::new().unwrap();
    let path = home.path().join("approvals.json");
    let app = sample("Fixture A", SignatureState::Unsigned);
    let eval = cfg!(all(feature = "eval-autoapprove", debug_assertions));
    for (mode, expected) in [
        ("auto-allow-turn", ApprovalDecision::Turn),
        ("auto-allow-session", ApprovalDecision::Session),
        ("auto-allow-always", ApprovalDecision::Session),
        ("auto-deny", ApprovalDecision::Deny),
    ] {
        let cancel = Cancel::new();
        let mut req = hook(
            &app,
            Some(&path),
            &cancel,
            Some("deny"),
            Duration::from_secs(1),
            true,
        );
        req.mode = Some(mode);
        assert_eq!(
            decide_with_autoclick(&req).await,
            if eval {
                expected
            } else {
                ApprovalDecision::Deny
            }
        );
        // Even the test hook cannot turn an active eval app grant into a
        // sensitive/foreground confirmation. Outside eval builds the mode is inert.
        req.autoclick = Some("confirm-allow");
        assert_eq!(confirm_for_test(&req).await, !eval, "{mode}");
        cancel.cancel();
        assert_eq!(decide_with_autoclick(&req).await, ApprovalDecision::Deny);
        assert!(!confirm_for_test(&req).await);
        assert!(!path.exists(), "{mode} persisted Always");
    }
}

#[tokio::test]
async fn pre_cancelled_or_disabled_always_does_not_write() {
    let app = sample("Demo", SignatureState::Unsigned);
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("approvals.json");
    let cancel = Cancel::new();
    cancel.cancel();
    let decision = decide_with_autoclick(&hook(
        &app,
        Some(&path),
        &cancel,
        Some("always"),
        Duration::from_secs(2),
        true,
    ))
    .await;
    assert_eq!(decision, ApprovalDecision::Deny);
    assert!(!path.exists());

    let decision = decide_with_autoclick(&hook(
        &app,
        Some(&path),
        &Cancel::new(),
        Some("always"),
        Duration::from_secs(2),
        false,
    ))
    .await;
    assert_eq!(decision, ApprovalDecision::Deny);
    assert!(!path.exists());
    assert!(!allows(&path, &app.identity));
}

#[test]
fn grant_file_round_trip_revoke_and_corrupt_fail_closed() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("approvals.json");
    let app = sample("Demo", SignatureState::Unsigned);
    grant(&path, &app.identity).unwrap();
    assert!(allows(&path, &app.identity));
    let listed = list_text(&path, "en").unwrap();
    assert!(listed.contains(&storage_key(&app.identity)), "{listed}");
    let raw: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let record = &raw[&storage_key(&app.identity)];
    assert_eq!(record["decision"], "always");
    assert_eq!(record["identity"]["path_hash"].as_str().unwrap().len(), 64);
    assert!(record["granted_at"].as_str().unwrap().ends_with('Z'));
    assert!(revoke(&path, &storage_key(&app.identity)).unwrap());
    assert!(!path.exists());
    assert_eq!(
        list_text(&path, "en").unwrap(),
        gcu_mcp::text("en", "approvals.empty")
    );
    assert!(!revoke(&path, "linux:missing").unwrap());

    // A valid signature used to match the publisher alone and ignore path/hash.
    // The record below also omits the certificate, so it does not authorize.
    let windows = AppIdentity {
        platform: Platform::Windows,
        key: r"C:\App.exe".into(),
        team_id: None,
        publisher: Some("CN=Publisher".into()),
        sha256: None,
        signature: SignatureState::Valid,
        image_path: r"D:\moved.exe".into(),
        proof: GrantProof::default(),
    };
    let other = AppIdentity {
        image_path: r"E:\other.exe".into(),
        sha256: Some([1; 32]),
        ..windows.clone()
    };
    let win_path = dir.path().join("windows.json");
    grant(&win_path, &windows).unwrap();
    assert!(!allows(&win_path, &windows));
    assert!(!allows(&win_path, &other));
    assert!(storage_key(&windows).contains(r"C:\App.exe"));

    grant(&path, &app.identity).unwrap();
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(&path)
        .unwrap();
    use std::io::Write;
    file.write_all(b"{").unwrap();
    drop(file);
    let before = std::fs::read(&path).unwrap();
    assert_eq!(
        grant(&path, &app.identity).unwrap_err(),
        StoreFault::Corrupt
    );
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert!(!allows(&path, &app.identity));
}

#[cfg(unix)]
#[test]
fn symlink_grant_file_is_not_followed() {
    let dir = TempDir::new().unwrap();
    let target = dir.path().join("secret");
    std::fs::write(&target, b"secret-contents").unwrap();
    let link = dir.path().join("approvals.json");
    std::os::unix::fs::symlink(&target, &link).unwrap();
    let app = sample("Demo", SignatureState::Unsigned);
    assert_eq!(grant(&link, &app.identity).unwrap_err(), StoreFault::Unsafe);
    assert_eq!(std::fs::read(&target).unwrap(), b"secret-contents");
    assert!(std::fs::symlink_metadata(&link)
        .unwrap()
        .file_type()
        .is_symlink());
    assert!(!allows(&link, &app.identity));
}

#[tokio::test]
async fn handoff_returns_without_opening_a_desktop() {
    let app = sample("Demo", SignatureState::Unknown);
    let started = std::time::Instant::now();
    handoff_for_test(
        &app,
        &HandoffReason::Login,
        &Cancel::new(),
        false,
        Duration::from_secs(30),
    )
    .await;
    assert!(started.elapsed() < Duration::from_secs(1));
}

#[test]
fn dialog_plan_uses_argv_and_defaults_to_deny() {
    let copy = DialogCopy {
        title: "UNIQUE-TITLE".into(),
        text: "UNIQUE-BODY".into(),
        rows: vec![
            ("deny".into(), "Deny".into()),
            ("turn".into(), "This turn".into()),
            ("always".into(), "Always".into()),
        ],
        timeout_secs: 3,
    };
    assert!(dialog_plan(&[], &copy).is_none());
    let zenity = dialog_plan(&["zenity"], &copy).unwrap();
    assert_eq!(zenity.program, "zenity");
    assert_ne!(zenity.program, "sh");
    let true_at = zenity.args.iter().position(|arg| arg == "TRUE").unwrap();
    assert_eq!(zenity.args[true_at + 1], "deny");
    assert!(zenity
        .args
        .windows(2)
        .any(|pair| pair == ["--timeout", "3"]));
    assert!(zenity.args.iter().any(|arg| arg == "UNIQUE-BODY"));
    let kdialog = dialog_plan(&["kdialog"], &copy).unwrap();
    assert_eq!(kdialog.program, "kdialog");
    assert!(kdialog
        .args
        .windows(2)
        .any(|pair| pair == ["--default", "deny"]));
    let powershell = dialog_plan(&["powershell"], &copy).unwrap();
    let script = powershell.args.join("\n");
    assert!(script.contains("AcceptButton"));
    assert!(!script.contains("UNIQUE-BODY"));
    assert!(powershell
        .env
        .iter()
        .any(|(_, value)| value == "UNIQUE-BODY"));
    let osascript = dialog_plan(&["osascript"], &copy).unwrap();
    assert_eq!(osascript.args[0], "-e");
    assert!(!osascript.args[1].contains("UNIQUE-TITLE"));
    assert!(osascript.args.iter().any(|arg| arg == "UNIQUE-TITLE"));
    assert!(dialog_plan(&["kdialog", "zenity"], &copy).unwrap().program == "zenity");
}

#[test]
fn autoclick_environment_parses_under_lock() {
    let _guard = AUTOCLICK_ENV
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let previous = std::env::var("GCU_APPROVAL_TEST_AUTOCLICK").ok();
    std::env::set_var("GCU_APPROVAL_TEST_AUTOCLICK", "turn");
    assert_eq!(autoclick_from_env().as_deref(), Some("turn"));
    std::env::remove_var("GCU_APPROVAL_TEST_AUTOCLICK");
    assert!(autoclick_from_env().is_none());
    if let Some(value) = previous {
        std::env::set_var("GCU_APPROVAL_TEST_AUTOCLICK", value);
    }
}

#[test]
fn approvals_help_has_no_allow_all_command() {
    let home = TempDir::new().unwrap();
    let output = gcu(&home)
        .args(["approvals", "--help"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8_lossy(&output).to_ascii_lowercase();
    assert!(!text.contains("allow-all"), "{text}");
    assert!(!text.contains("allow all"), "{text}");
    assert!(text.contains("revoke"), "{text}");
}

#[test]
fn binary_ignores_autoclick_environment() {
    let home = TempDir::new().unwrap();
    let output = gcu(&home)
        .env("GCU_APPROVAL_TEST_AUTOCLICK", "turn")
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
    let value: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(
        value["structuredContent"]["error"]["code"],
        "approval_denied"
    );
}

#[test]
fn approval_environment_gate_matches_the_build() {
    let home = TempDir::new().unwrap();
    let eval = cfg!(all(feature = "eval-autoapprove", debug_assertions));
    for value in ["auto-allow-turn", "auto-allow-always", "auto-yes"] {
        let status = gcu(&home)
            .env("GCU_APPROVAL", value)
            .arg("version")
            .assert();
        if eval && value.starts_with("auto-allow-") {
            status.success();
        } else {
            status.code(2);
        }
    }
    if !eval {
        return;
    }
    gcu(&home)
        .env("GCU_APPROVAL", "auto-allow-always")
        .args([
            "call",
            "get_app_state",
            "--driver",
            "fake",
            "--args",
            r#"{"app":"fake.form","include_screenshot":false}"#,
            "--json",
        ])
        .assert()
        .success();
    assert!(!state_file(&home).exists());
}

fn wire(id: &AppIdentity) -> gcu_host_protocol::messages::Identity {
    use gcu_host_protocol::messages::{GrantProof, Identity, Platform as WirePlatform, Signature};
    Identity {
        platform: match id.platform {
            Platform::Macos => WirePlatform::Macos,
            Platform::Windows => WirePlatform::Windows,
            Platform::Linux => WirePlatform::Linux,
        },
        key: id.key.clone(),
        team_id: id.team_id.clone(),
        publisher: id.publisher.clone(),
        sha256: id.sha256,
        signature: match id.signature {
            SignatureState::Valid => Signature::Valid,
            SignatureState::Invalid => Signature::Invalid,
            SignatureState::Unsigned => Signature::Unsigned,
            SignatureState::Unknown => Signature::Unknown,
        },
        image_path: id.image_path.clone(),
        proof: GrantProof {
            cdhash: id.proof.cdhash.clone(),
            certificate_sha256: id.proof.certificate_sha256.clone(),
            product: id.proof.product.clone(),
            build: id.proof.build.clone(),
            file_dev: id.proof.file_dev,
            file_inode: id.proof.file_inode,
        },
    }
}

fn wire_allows(saved: &AppIdentity, current: &AppIdentity) -> bool {
    let matched = gcu_host_protocol::persistent_grant_matches(&wire(saved), &wire(current));
    assert_eq!(saved.grant_eq(current), matched);
    matched
}

fn complete_mac() -> AppIdentity {
    AppIdentity {
        platform: Platform::Macos,
        key: "com.example.notes".into(),
        team_id: Some("TEAM123456".into()),
        publisher: Some("Example".into()),
        sha256: Some([0x11; 32]),
        signature: SignatureState::Valid,
        image_path: "/Applications/Notes.app/Contents/MacOS/Notes".into(),
        proof: GrantProof {
            cdhash: Some("0123456789abcdef0123456789abcdef01234567".into()),
            certificate_sha256: Some("22".repeat(32)),
            product: Some("Notes".into()),
            build: Some("100".into()),
            ..GrantProof::default()
        },
    }
}

fn replace_owner_bytes(path: &Path, body: &[u8], seed: &AppIdentity) {
    grant(path, seed).unwrap();
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(path)
        .unwrap();
    use std::io::Write;
    file.write_all(body).unwrap();
}

fn complete_windows() -> AppIdentity {
    AppIdentity {
        platform: Platform::Windows,
        key: r"C:\Program Files\Notes\Notes.exe".into(),
        team_id: None,
        publisher: Some("CN=Publisher".into()),
        sha256: Some([0x22; 32]),
        signature: SignatureState::Valid,
        image_path: r"C:\Program Files\Notes\Notes.exe".into(),
        proof: GrantProof {
            certificate_sha256: Some("33".repeat(32)),
            product: Some("Notes".into()),
            ..GrantProof::default()
        },
    }
}

#[test]
fn persistent_grant_reauth_agrees_on_the_file_and_the_host_wire() {
    let dir = TempDir::new().unwrap();
    let mac = complete_mac();
    let mac_path = dir.path().join("mac.json");
    grant(&mac_path, &mac).unwrap();
    assert!(allows(&mac_path, &mac));
    assert!(wire_allows(&mac, &mac));

    let mut rebuilt = mac.clone();
    rebuilt.proof.cdhash = Some("ff".repeat(20));
    assert!(!allows(&mac_path, &rebuilt));
    assert!(!wire_allows(&mac, &rebuilt));

    let mut other_cert = mac.clone();
    other_cert.proof.certificate_sha256 = Some("99".repeat(32));
    assert_eq!(mac.publisher, other_cert.publisher);
    assert!(!allows(&mac_path, &other_cert));
    assert!(!wire_allows(&mac, &other_cert));

    let mut missing_hash = mac.clone();
    missing_hash.sha256 = None;
    assert!(!allows(&mac_path, &missing_hash));
    assert!(!wire_allows(&mac, &missing_hash));

    let windows = complete_windows();
    let win_path = dir.path().join("win.json");
    grant(&win_path, &windows).unwrap();
    assert!(allows(&win_path, &windows));
    let mut moved = windows.clone();
    moved.image_path = r"D:\Notes\Notes.exe".into();
    moved.key = moved.image_path.clone();
    moved.sha256 = Some([0x66; 32]);
    assert!(!allows(&win_path, &moved));
    assert!(!wire_allows(&windows, &moved));

    let linux = sample("Demo", SignatureState::Unsigned).identity;
    let linux_path = dir.path().join("linux.json");
    grant(&linux_path, &linux).unwrap();
    assert!(allows(&linux_path, &linux));
    assert!(wire_allows(&linux, &linux));
    let mut rehashed = linux.clone();
    rehashed.sha256 = Some([0x99; 32]);
    assert!(!allows(&linux_path, &rehashed));
    assert!(!wire_allows(&linux, &rehashed));
}

#[test]
fn legacy_narrow_record_loads_but_does_not_authorize_until_a_new_grant() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("approvals.json");
    let hash = "11".repeat(32);
    let body = format!(
        r#"{{
              "macos:com.example.notes": {{
                "decision": "always",
                "granted_at": "2020-01-01T00:00:00Z",
                "identity": {{
                  "team_id": "TEAM123456",
                  "publisher": "Example",
                  "path_hash": "{hash}",
                  "platform": "macos",
                  "key": "com.example.notes",
                  "signature": "valid",
                  "image_path": "/Applications/Notes.app/Contents/MacOS/Notes"
                }}
              }}
            }}"#
    );
    // `grant` creates the owner-only file. Replacing the bytes keeps that mode.
    replace_owner_bytes(&path, body.as_bytes(), &complete_mac());
    let listed = list_text(&path, "en").unwrap();
    assert!(listed.contains("macos:com.example.notes"), "{listed}");
    let mut legacy = complete_mac();
    legacy.proof = GrantProof::default();
    assert!(!allows(&path, &legacy));
    assert!(!allows(&path, &complete_mac()));
    assert!(!wire_allows(&legacy, &legacy));
    grant(&path, &complete_mac()).unwrap();
    assert!(allows(&path, &complete_mac()));
    assert!(!allows(&path, &legacy));
    assert!(revoke(&path, "macos:com.example.notes").unwrap());
}

#[test]
fn malformed_proof_field_is_corrupt_and_left_unchanged() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("approvals.json");
    let app = sample("Demo", SignatureState::Unsigned);
    let body = br#"{
      "linux:demo.app": {
        "decision": "always",
        "granted_at": "2020-01-01T00:00:00Z",
        "identity": {
          "platform": "linux",
          "key": "demo.app",
          "signature": "unsigned",
          "image_path": "/usr/bin/demo",
          "file_dev": "nope"
        }
      }
    }"#;
    replace_owner_bytes(&path, body, &app.identity);
    assert!(!allows(&path, &app.identity));
    assert_eq!(
        grant(&path, &app.identity).unwrap_err(),
        StoreFault::Corrupt
    );
    assert_eq!(std::fs::read(&path).unwrap(), body);
}
