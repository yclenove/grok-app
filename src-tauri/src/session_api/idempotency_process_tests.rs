//! Fresh-process crash and competing-host probes: synthetic callbacks only.
use super::{idempotency::run_at, idempotency_tests::Fixture, TurnResult, TurnStatus};
use std::{
    fs,
    io::Write,
    process::{Command, Stdio},
    time::Duration,
};

const CHILD: &str = "session_api::idempotency_process_tests::receipt_child";
fn child(f: &Fixture, mode: &str, label: &str) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", CHILD, "--test-threads=1", "--nocapture"])
        .env("GROK_RECEIPT_TEST_ROOT", &f.0)
        .env("GROK_RECEIPT_TEST_MODE", mode)
        .env("GROK_RECEIPT_TEST_LABEL", label)
        .env("GROK_APP_HOME", f.0.join("app-home"))
        .env("GROK_HOME", f.0.join("agent-home"))
        .env("GROK_APP_ACP", "mock")
        .env("GROK_DISABLE_DEFAULT_ROOTS", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}

#[test]
fn receipt_child() {
    let Ok(mode) = std::env::var("GROK_RECEIPT_TEST_MODE") else {
        return;
    };
    let root = std::path::PathBuf::from(std::env::var_os("GROK_RECEIPT_TEST_ROOT").unwrap());
    assert!(root
        .file_name()
        .unwrap()
        .to_string_lossy()
        .starts_with("grok-session-receipts-"));
    let label = std::env::var("GROK_RECEIPT_TEST_LABEL").unwrap();
    assert!(matches!(label.as_str(), "crash" | "a" | "b"));
    assert!(matches!(mode.as_str(), "crash" | "race"));
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let (result, _) = run_at(
            &root.join("receipts.json"),
            "s",
            "p",
            Some("key"),
            || async {
                let mut marker = fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(root.join("dispatches.txt"))
                    .unwrap();
                marker.write_all(b"dispatched\n").unwrap();
                marker.sync_all().unwrap();
                drop(marker);
                if mode == "crash" {
                    std::process::exit(73);
                }
                tokio::time::sleep(Duration::from_millis(30)).await;
                TurnResult::started("s")
            },
        )
        .await;
        fs::write(
            root.join(format!("result-{label}.json")),
            serde_json::to_vec(&result).unwrap(),
        )
        .unwrap();
    });
}

#[test]
fn process_exit_after_dispatch_leaves_claim_and_restart_does_not_dispatch() {
    let f = Fixture::new();
    let output = child(&f, "crash", "crash").output().unwrap();
    assert_eq!(
        output.status.code(),
        Some(73),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read_to_string(f.0.join("dispatches.txt")).unwrap(),
        "dispatched\n"
    );
    let before = fs::read(f.path()).unwrap();
    let output = child(&f, "race", "a").output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result: TurnResult =
        serde_json::from_slice(&fs::read(f.0.join("result-a.json")).unwrap()).unwrap();
    assert_eq!(result.status, TurnStatus::Unknown);
    assert!(!result.ok);
    assert_eq!(fs::read(f.path()).unwrap(), before);
    assert_eq!(
        fs::read_to_string(f.0.join("dispatches.txt")).unwrap(),
        "dispatched\n"
    );
}

#[test]
fn concurrent_processes_with_same_identity_dispatch_once() {
    let f = Fixture::new();
    let a = child(&f, "race", "a").spawn().unwrap();
    let b = child(&f, "race", "b").spawn().unwrap();
    for p in [a, b] {
        let output = p.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let a: TurnResult =
        serde_json::from_slice(&fs::read(f.0.join("result-a.json")).unwrap()).unwrap();
    let b: TurnResult =
        serde_json::from_slice(&fs::read(f.0.join("result-b.json")).unwrap()).unwrap();
    assert!(a.ok || b.ok);
    assert_eq!(a.session_id, "s");
    assert_eq!(b.session_id, "s");
    assert_eq!(
        fs::read_to_string(f.0.join("dispatches.txt")).unwrap(),
        "dispatched\n"
    );
}
