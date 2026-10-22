use super::{queue_lease, queue_regression_tests::Fixture};
use std::{
    fs,
    process::{Command, Stdio},
    time::{Duration, Instant},
};
#[test]
fn lease_child() {
    let Ok(root) = std::env::var("GROK_QUEUE_LEASE_CHILD") else {
        return;
    };
    let root = std::path::PathBuf::from(root);
    let _lease = queue_lease::try_acquire(&root).unwrap().unwrap();
    fs::write(root.join("ready"), b"ready").unwrap();
    let start = Instant::now();
    while !root.join("release").exists() {
        assert!(start.elapsed() < Duration::from_secs(10));
        std::thread::sleep(Duration::from_millis(10));
    }
}
#[test]
fn another_process_holds_lease_until_actual_exit_not_a_status_file() {
    let f = Fixture::new();
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "session_api::queue_lease_tests::lease_child",
            "--test-threads=1",
        ])
        .env("GROK_QUEUE_LEASE_CHILD", &f.0)
        .env("GROK_APP_HOME", f.0.join("home"))
        .env("GROK_HOME", f.0.join("agent"))
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let start = Instant::now();
    while !f.0.join("ready").exists() {
        assert!(
            child.try_wait().unwrap().is_none(),
            "child ended before ready"
        );
        assert!(start.elapsed() < Duration::from_secs(5));
        std::thread::sleep(Duration::from_millis(10));
    }
    let observed = queue_lease::try_acquire(&f.0);
    fs::write(f.0.join("release"), b"release").unwrap();
    assert!(child.wait().unwrap().success());
    assert!(
        observed.unwrap().is_none(),
        "cross-process lease did not exclude"
    );
    assert!(queue_lease::try_acquire(&f.0).unwrap().is_some());
}
