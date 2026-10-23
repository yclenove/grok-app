//! Scope DTO/parser/transport tests. No core or native driver dependency.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use gcu_host_protocol::scope::{Control, Scope};
use serde_json::json;
#[test]
fn every_control_field_is_required_and_unknown_fields_are_rejected() {
    let scope = Scope {
        engine_instance: "e".into(),
        desktop_session: "d".into(),
        control_lease: "c".into(),
        targets: vec![],
    };
    let frames = [
        json!({"type":"Install","version":1,"run_id":"r","generation":1,"scope_digest":"d","scope":scope}),
        json!({"type":"Rebind","run_id":"r","generation":2,"scope":scope}),
        json!({"type":"Renew","run_id":"r","generation":1,"seq":1}),
        json!({"type":"Revoke","run_id":"r"}),
    ];
    for frame in frames {
        assert!(serde_json::from_value::<Control>(frame.clone()).is_ok());
        for key in frame.as_object().unwrap().keys() {
            let mut missing = frame.clone();
            missing.as_object_mut().unwrap().remove(key);
            assert!(
                serde_json::from_value::<Control>(missing).is_err(),
                "missing {key}"
            );
        }
        let mut extra = frame.clone();
        extra["unrecognized"] = json!(true);
        assert!(serde_json::from_value::<Control>(extra).is_err());
    }
}
#[test]
fn digest_binds_run_generation_and_entire_allowed_set() {
    let scope = Scope {
        engine_instance: "e".into(),
        desktop_session: "d".into(),
        control_lease: "c".into(),
        targets: vec![],
    };
    let digest = scope.digest("run", 1).unwrap();
    assert_ne!(digest, scope.digest("other", 1).unwrap());
    assert_ne!(digest, scope.digest("run", 2).unwrap());
    let mut other = scope.clone();
    other.desktop_session = "other".into();
    assert_ne!(digest, other.digest("run", 1).unwrap());
}

#[cfg(target_os = "linux")]
mod process {
    use std::{
        os::unix::fs::{DirBuilderExt, FileTypeExt, MetadataExt},
        path::PathBuf,
        process::{Child, Command, ExitStatus, Stdio},
        time::Duration,
    };
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    const OVERALL_LIMIT: Duration = Duration::from_secs(20);
    const PHASE_LIMIT: Duration = Duration::from_secs(10);

    struct Diagnostics(PathBuf);
    impl Diagnostics {
        fn new() -> Self {
            let nonce = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path =
                std::env::temp_dir().join(format!("gcu-scope-fds-{}-{nonce}", std::process::id()));
            std::fs::DirBuilder::new()
                .mode(0o700)
                .create(&path)
                .unwrap();
            Self(path)
        }
        fn output(&self) -> String {
            std::fs::read_to_string(self.0.join("helpers.log"))
                .unwrap_or_else(|error| format!("could not read helper diagnostics: {error}"))
        }
    }
    impl Drop for Diagnostics {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    // std::process::Child otherwise leaves a running helper behind on timeout.
    struct FixtureChild(Child);
    impl FixtureChild {
        async fn wait(&mut self, role: &str) -> Result<ExitStatus, String> {
            tokio::time::timeout(PHASE_LIMIT, async {
                loop {
                    if let Some(status) = self.0.try_wait().map_err(|error| {
                        format!("{role}: failed to wait for subprocess: {error}")
                    })? {
                        return Ok(status);
                    }
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
            })
            .await
            .unwrap_or_else(|_| {
                Err(format!(
                    "{role}: subprocess timed out after {PHASE_LIMIT:?}"
                ))
            })
        }
    }
    impl Drop for FixtureChild {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let end = std::time::Instant::now() + Duration::from_secs(1);
            while std::time::Instant::now() < end {
                match self.0.try_wait() {
                    Ok(Some(_)) | Err(_) => break,
                    Ok(None) => std::thread::sleep(Duration::from_millis(5)),
                }
            }
        }
    }

    #[tokio::test]
    async fn inherited_control_cannot_reach_client_relay_or_script_grandchildren() {
        let diagnostics = Diagnostics::new();
        let log = std::fs::File::create(diagnostics.0.join("helpers.log")).unwrap();
        let result = tokio::time::timeout(OVERALL_LIMIT, async {
            let mut command = Command::new(std::env::current_exe().unwrap());
            command
                .args([
                    "--ignored",
                    "--exact",
                    "process::import_helper",
                    "--nocapture",
                ])
                .env("S3_PRIVATE_IMPORT_HELPER", "1")
                .env("S3_SCOPE_TEST_DIR", &diagnostics.0)
                .stdout(Stdio::from(log.try_clone().unwrap()))
                .stderr(Stdio::from(log));
            let mut control = gcu_host_protocol::scope::launch::prepare(&mut command)
                .await
                .map_err(|error| format!("prepare failed: {error}"))?;
            let mut child = FixtureChild(
                command
                    .spawn()
                    .map_err(|error| format!("import helper spawn failed: {error}"))?,
            );
            // spawn borrows Command: its configured Stdio still owns another
            // copy of the child socket. Keeping it alive prevents peer EOF.
            drop(command);
            let mut proof = [0; 1];
            tokio::time::timeout(PHASE_LIMIT, control.read_exact(&mut proof))
                .await
                .map_err(|_| "import helper proof timed out".to_string())?
                .map_err(|error| format!("import helper proof failed: {error}"))?;
            if proof != [1] {
                return Err(format!("invalid import helper proof: {proof:?}"));
            }
            let status = child.wait("import helper").await?;
            if !status.success() {
                return Err(format!("import helper failed: {status}"));
            }

            // A descendant retaining control would keep this end alive.
            let n = tokio::time::timeout(PHASE_LIMIT, control.read(&mut proof))
                .await
                .map_err(|_| {
                    "control EOF timed out after client/relay/script-worker probes; see role diagnostics"
                        .to_string()
                })?
                .map_err(|error| format!("control EOF read failed: {error}"))?;
            if n != 0 {
                return Err(format!("expected control EOF, received {proof:?}"));
            }
            Ok(())
        })
        .await
        .unwrap_or_else(|_| Err(format!("scope subprocess test exceeded {OVERALL_LIMIT:?}")));
        assert!(result.is_ok(), "{result:?}\n{}", diagnostics.output());
    }
    #[tokio::test]
    #[ignore = "subprocess fixture"]
    async fn import_helper() {
        if std::env::var_os("S3_PRIVATE_IMPORT_HELPER").is_none() {
            return;
        }
        tokio::time::timeout(OVERALL_LIMIT, async {
            eprintln!("import helper: checking bootstrap stdin");
            // prepare installs the child socket at fd 0. Record it before
            // inherited replaces stdin with /dev/null, not after importing.
            let metadata = std::fs::metadata("/proc/self/fd/0").unwrap();
            assert!(
                metadata.file_type().is_socket(),
                "bootstrap stdin is not a socket"
            );
            let identity = format!("{}:{}", metadata.dev(), metadata.ino());
            let results = PathBuf::from(std::env::var_os("S3_SCOPE_TEST_DIR").unwrap());
            let mut control = gcu_host_protocol::scope::launch::inherited().unwrap();
            for role in ["client", "relay", "script-worker"] {
                eprintln!("{role}: starting descriptor probe");
                let mut command = Command::new(std::env::current_exe().unwrap());
                command
                    .args([
                        "--ignored",
                        "--exact",
                        "process::probe_helper",
                        "--nocapture",
                    ])
                    .env("S3_CONTROL_IDENTITY", &identity)
                    .env("S3_CHILD_ROLE", role)
                    // Both go straight to the parent's log file: no unread
                    // pipe can block a helper while its parent waits for exit.
                    .stdout(Stdio::inherit())
                    .stderr(Stdio::inherit());
                // Client default stdin exercises the stdio re-inheritance trap;
                // relay and script worker exercise both explicit stdio forms.
                match role {
                    "relay" => {
                        command.stdin(Stdio::piped());
                    }
                    "script-worker" => {
                        command.stdin(Stdio::null());
                    }
                    _ => {}
                }
                let mut child = FixtureChild(command.spawn().unwrap());
                drop(command);
                let status = child.wait(role).await.unwrap();
                assert!(
                    status.success(),
                    "{role}: descriptor probe failed: {status}"
                );
                // libtest exits successfully when --exact selects zero tests.
                // Require evidence that this particular probe really ran.
                assert_eq!(
                    std::fs::read_to_string(results.join(role)).unwrap_or_default(),
                    "ok",
                    "{role}: probe helper did not complete its descriptor scan"
                );
                eprintln!("{role}: descriptor probe passed");
            }
            eprintln!("import helper: writing completion proof");
            control.write_all(&[1]).await.unwrap();
        })
        .await
        .expect("import helper exceeded its overall deadline; see last role above");
    }
    #[test]
    #[ignore = "subprocess fixture"]
    fn probe_helper() {
        let Some(expected) = std::env::var_os("S3_CONTROL_IDENTITY") else {
            return;
        };
        let expected = expected.to_str().unwrap();
        let role = std::env::var("S3_CHILD_ROLE").unwrap();
        for fd in std::fs::read_dir("/proc/self/fd")
            .unwrap()
            .filter_map(Result::ok)
        {
            if let Ok(metadata) = std::fs::metadata(fd.path()) {
                assert_ne!(
                    format!("{}:{}", metadata.dev(), metadata.ino()),
                    expected,
                    "{role}: inherited private control descriptor at {}",
                    fd.path().display()
                );
            }
        }
        let results = PathBuf::from(std::env::var_os("S3_SCOPE_TEST_DIR").unwrap());
        std::fs::write(results.join(role), "ok").unwrap();
    }
}

#[tokio::test]
async fn scope_frame_limit_rejects_prefix_without_waiting_for_payload() {
    use tokio::io::AsyncWriteExt;
    tokio::time::timeout(std::time::Duration::from_secs(1), async {
        let (mut writer, mut reader) = tokio::io::duplex(64);
        writer
            .write_all(&((gcu_host_protocol::scope::MAX_CONTROL_FRAME + 1) as u32).to_be_bytes())
            .await
            .unwrap();
        assert!(gcu_host_protocol::codec::read_frame_limited::<_, Control>(
            &mut reader,
            gcu_host_protocol::scope::MAX_CONTROL_FRAME
        )
        .await
        .is_err());
    })
    .await
    .unwrap();
}
