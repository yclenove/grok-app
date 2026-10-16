//! Approval decisions, grant file, and the approval document contract.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use assert_cmd::Command;
use gcu_cli::approval::{
    allows, approval_page, autoclick_from_env, confirm_for_test, decide_with_autoclick,
    dialog_plan, grant, handoff_for_test, list_text, revoke, storage_key, window_problems,
    DialogCopy, HookRequest, StoreFault,
};
use gcu_host_api::{
    AppIdentity, AppInfo, ApprovalDecision, Cancel, HandoffReason, ImageRef, Platform, Risk,
    SignatureState,
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
        return home
            .path()
            .join("local")
            .join("grok-computer-use")
            .join("state")
            .join("approvals.json");
    }
    #[cfg(target_os = "macos")]
    {
        return home
            .path()
            .join("Library/Application Support/grok-computer-use/state/approvals.json");
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

    let windows = AppIdentity {
        platform: Platform::Windows,
        key: r"C:\App.exe".into(),
        team_id: None,
        publisher: Some("CN=Publisher".into()),
        sha256: None,
        signature: SignatureState::Valid,
        image_path: r"D:\moved.exe".into(),
    };
    let other = AppIdentity {
        image_path: r"E:\other.exe".into(),
        sha256: Some([1; 32]),
        ..windows.clone()
    };
    let win_path = dir.path().join("windows.json");
    grant(&win_path, &windows).unwrap();
    assert!(allows(&win_path, &other));
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
