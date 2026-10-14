//! The private worker command is separate from fake/MCP tool dispatch.
#![allow(clippy::unwrap_used, clippy::expect_used)]
#[cfg(not(windows))]
#[test]
fn worker_unsupported_platform_fails_without_fake_fallback() {
    let output = assert_cmd::Command::new(assert_cmd::cargo::cargo_bin!("gcu"))
        .arg("worker")
        .timeout(std::time::Duration::from_secs(5))
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("requires Windows"));
}
#[test]
fn worker_command_has_no_fault_injection_flags() {
    assert_cmd::Command::new(assert_cmd::cargo::cargo_bin!("gcu"))
        .args(["worker", "--inject-crash"])
        .assert()
        .failure();
}
