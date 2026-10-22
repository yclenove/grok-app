//! Fresh-process probes use only synthetic durable markers, never a provider.
use super::{
    queue_dispatch,
    queue_regression_tests::{item, Fixture},
    queue_store, TurnResult,
};
use std::{
    fs,
    io::Write,
    process::{Command, Stdio},
};
const CHILD: &str = "session_api::queue_process_tests::queue_child";
fn child(f: &Fixture, mode: &str, label: &str) -> Command {
    let mut c = Command::new(std::env::current_exe().unwrap());
    c.args(["--exact", CHILD, "--test-threads=1", "--nocapture"])
        .env("GROK_QUEUE_TEST_ROOT", &f.0)
        .env("GROK_QUEUE_TEST_MODE", mode)
        .env("GROK_QUEUE_TEST_LABEL", label)
        .env("GROK_APP_HOME", f.0.join("app-home"))
        .env("GROK_HOME", f.0.join("agent-home"))
        .env("GROK_APP_ACP", "mock")
        .env("GROK_DISABLE_DEFAULT_ROOTS", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    c
}
#[test]
fn queue_child() {
    let Ok(mode) = std::env::var("GROK_QUEUE_TEST_MODE") else {
        return;
    };
    assert!(matches!(
        mode.as_str(),
        "before-claim-crash" | "claim-crash" | "dispatch-crash" | "race"
    ));
    let root = std::path::PathBuf::from(std::env::var_os("GROK_QUEUE_TEST_ROOT").unwrap());
    assert!(root
        .file_name()
        .unwrap()
        .to_string_lossy()
        .starts_with("grok-durable-queue-"));
    let label = std::env::var("GROK_QUEUE_TEST_LABEL").unwrap();
    assert!(matches!(label.as_str(), "a" | "b"));
    let q = item("q");
    let path = root.join(super::QUEUE_FILE);
    if mode == "before-claim-crash" {
        std::process::exit(76);
    }
    if mode == "claim-crash" {
        assert!(queue_store::claim(&path, &q).unwrap().is_some());
        std::process::exit(74);
    }
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let result = queue_dispatch::run_at(&root, &q, || async {
            let mut marker = fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(root.join("dispatches.txt"))
                .unwrap();
            marker.write_all(b"dispatched\n").unwrap();
            marker.sync_all().unwrap();
            drop(marker);
            if mode == "dispatch-crash" {
                std::process::exit(75);
            }
            tokio::time::sleep(std::time::Duration::from_millis(30)).await;
            TurnResult::started("s")
        })
        .await;
        fs::write(
            root.join(format!("result-{label}.json")),
            serde_json::to_vec(&result).unwrap(),
        )
        .unwrap();
    });
}
#[test]
fn process_exit_after_claim_keeps_payload_and_restart_does_not_dispatch() {
    let f = Fixture::new();
    queue_store::insert(&f.path(), item("q")).unwrap();
    let crash = child(&f, "claim-crash", "a").output().unwrap();
    assert_eq!(crash.status.code(), Some(74));
    let before = fs::read(f.path()).unwrap();
    assert!(!f.0.join("dispatches.txt").exists());
    let retry = child(&f, "race", "b").output().unwrap();
    assert!(
        retry.status.success(),
        "{}",
        String::from_utf8_lossy(&retry.stderr)
    );
    assert_eq!(fs::read(f.path()).unwrap(), before);
    assert!(!f.0.join("dispatches.txt").exists());
    assert_eq!(fs::read(f.0.join("result-b.json")).unwrap(), b"null");
}
#[test]
fn process_exit_after_dispatch_keeps_payload_and_restart_does_not_duplicate() {
    let f = Fixture::new();
    queue_store::insert(&f.path(), item("q")).unwrap();
    let crash = child(&f, "dispatch-crash", "a").output().unwrap();
    assert_eq!(crash.status.code(), Some(75));
    let before = fs::read(f.path()).unwrap();
    let retry = child(&f, "race", "b").output().unwrap();
    assert!(
        retry.status.success(),
        "{}",
        String::from_utf8_lossy(&retry.stderr)
    );
    assert_eq!(fs::read(f.path()).unwrap(), before);
    assert_eq!(
        fs::read(f.0.join("dispatches.txt")).unwrap(),
        b"dispatched\n"
    );
    assert_eq!(fs::read(f.0.join("result-b.json")).unwrap(), b"null");
}
#[test]
fn competing_hosts_cannot_dispatch_one_queue_item_twice() {
    let f = Fixture::new();
    queue_store::insert(&f.path(), item("q")).unwrap();
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
    assert_eq!(
        fs::read(f.0.join("dispatches.txt")).unwrap(),
        b"dispatched\n"
    );
    assert!(queue_store::list(&f.path()).unwrap().is_empty());
}

#[test]
fn process_exit_before_claim_preserves_ready_item_for_one_later_dispatch() {
    let f = Fixture::new();
    queue_store::insert(&f.path(), item("q")).unwrap();
    let before = fs::read(f.path()).unwrap();
    let crash = child(&f, "before-claim-crash", "a").output().unwrap();
    assert_eq!(crash.status.code(), Some(76));
    assert_eq!(fs::read(f.path()).unwrap(), before);
    assert!(!f.0.join("dispatches.txt").exists());
    let retry = child(&f, "race", "b").output().unwrap();
    assert!(
        retry.status.success(),
        "{}",
        String::from_utf8_lossy(&retry.stderr)
    );
    assert_eq!(
        fs::read(f.0.join("dispatches.txt")).unwrap(),
        b"dispatched\n"
    );
    assert!(queue_store::list(&f.path()).unwrap().is_empty());
}
