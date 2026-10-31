//! Test the process utility directly; in-process fake clients cannot exercise pipes/groups.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::*;

#[test]
fn real_pipes_are_drained_during_a_flood() {
    crate::block_on(async {
        let mut command = tokio::process::Command::new("/bin/sh");
        command.args(["-c", "i=0; while [ $i -lt 10000 ]; do printf 'stdout stdout stdout\\n'; printf 'stderr stderr stderr\\n' >&2; i=$((i+1)); done"]);
        let result = run_command(&mut command, Duration::from_secs(5), 1024, 1024).await.unwrap();
        assert_eq!(result.exit_code, Some(0));
        assert!(!result.timed_out && !result.capture_incomplete);
        assert!(result.stdout.overflow);
        assert_eq!(result.stdout.bytes.len(), 1024);
    }).unwrap();
}

#[test]
fn timeout_kills_the_client_process_group() {
    crate::block_on(async {
        let mut command = tokio::process::Command::new("/bin/sh");
        command.args(["-c", "/bin/sleep 300 & printf '%s\\n' $!; wait"]);
        let result = run_command(&mut command, Duration::from_millis(800), 1024, 1024)
            .await
            .unwrap();
        assert!(result.timed_out);
        let pid = std::str::from_utf8(&result.stdout.bytes).unwrap().trim();
        assert!(!pid.is_empty());
        for _ in 0..20 {
            let alive = std::process::Command::new("/bin/kill")
                .args(["-0", pid])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .is_ok_and(|status| status.success());
            if !alive {
                return;
            }
            // A container init may delay reaping an orphan. A zombie has
            // already exited; this utility does not claim to own its reap.
            #[cfg(target_os = "linux")]
            if std::fs::read_to_string(format!("/proc/{pid}/status")).is_ok_and(|status| {
                status
                    .lines()
                    .any(|line| line.starts_with("State:") && line.contains('Z'))
            }) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        panic!("grandchild {pid} survived the client timeout");
    })
    .unwrap();
}
