//! Interactive terminal recovery against a running MCP process.
use super::{native_command, unsafe_pty, Mcp};
use std::io::Write;
use std::process::{Child, Stdio};
use tempfile::TempDir;

#[test]
fn resume_authenticated_pty_requires_manual_confirmation_and_clears_stop() {
    use std::io::Read;
    let h = TempDir::new().unwrap();
    let mut m = Mcp::start_with_approval(&h, "default", false);
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
    let marker = h.path().join("state/grok-computer-use/stopped");
    let wait_for_stop = || {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !marker.exists() {
            assert!(
                std::time::Instant::now() < deadline,
                "Signal Stop timed out"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    };
    unsafe_pty::stop_process(m.child.id()).unwrap();
    wait_for_stop();
    struct ChildGuard(Child);
    impl Drop for ChildGuard {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    for confirmed in [false, true] {
        let (mut master, slave) = unsafe_pty::open().unwrap();
        let mut c = native_command(&h);
        c.args(["resume", "--session"])
            .arg(&directory)
            .stdin(Stdio::from(slave.try_clone().unwrap()))
            .stdout(Stdio::from(slave))
            .stderr(Stdio::null());
        let mut child = ChildGuard(c.spawn().unwrap());
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let mut output = String::new();
        while !output.contains("to confirm:") {
            assert!(
                std::time::Instant::now() < deadline,
                "PTY prompt timed out: {output}"
            );
            let mut bytes = [0; 1024];
            match master.read(&mut bytes) {
                Ok(n) => output.push_str(&String::from_utf8_lossy(&bytes[..n])),
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(std::time::Duration::from_millis(10))
                }
                other => panic!("PTY read failed: {other:?}"),
            }
        }
        let challenge = std::fs::read_to_string(directory.join("challenge")).unwrap();
        assert!(output.contains(&challenge));
        let token = std::fs::read_to_string(directory.join("token")).unwrap();
        assert!(!output.contains(&token));
        writeln!(
            master,
            "{}",
            if confirmed {
                format!("yes {challenge}")
            } else {
                "no".into()
            }
        )
        .unwrap();
        let status = loop {
            assert!(std::time::Instant::now() < deadline, "PTY resume timed out");
            if let Some(status) = child.0.try_wait().unwrap() {
                break status;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        };
        assert_eq!(status.code(), Some(if confirmed { 0 } else { 2 }));
        let result = m.call("get_app_state", serde_json::json!({"app":"fake.form"}));
        assert_eq!(
            result["structuredContent"]["error"]["code"],
            if confirmed {
                "approval_denied"
            } else {
                "stopped_by_user"
            }
        );
    }
    assert!(!marker.exists());
    unsafe_pty::stop_process(m.child.id()).unwrap();
    wait_for_stop();
    assert_eq!(
        m.call("get_app_state", serde_json::json!({"app":"fake.form"}))["structuredContent"]
            ["error"]["code"],
        "stopped_by_user"
    );
    m.close();
}
