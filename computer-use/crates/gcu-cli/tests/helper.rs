//! Helper admission, handshake, and bundle facts.
//! Socket tests use a private temporary directory. No production socket is touched.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use gcu_cli::helper::auth::{
    admit, app_name, bundle_id, designated_requirement, directory_is_private, idle_expired,
    launch_command, socket_is_private, valid_identifier, versions_match, Admit, AuthInput,
    CodeSignature, EndpointStamp, IdleClock, RejectReason, DEV_APP_NAME, DEV_BUNDLE_ID, IDLE_LIMIT,
    RELEASE_APP_NAME, RELEASE_BUNDLE_ID,
};
#[cfg(unix)]
use gcu_cli::helper::HelperError;
use std::time::Duration;

fn signature(identifier: &str, team: Option<&str>, fill: u8) -> CodeSignature {
    let cdhash = format!("{fill:02x}").repeat(20);
    CodeSignature::new(identifier, team, &cdhash).unwrap()
}

fn socket_stamp(uid: u32, ino: u64) -> EndpointStamp {
    EndpointStamp {
        dev: 7,
        ino,
        uid,
        mode: 0o600,
        socket: true,
        directory: false,
        symlink: false,
    }
}

fn decide(
    local: &CodeSignature,
    peer_uid: u32,
    peer: Option<&CodeSignature>,
    bound: EndpointStamp,
    observed: EndpointStamp,
) -> Admit {
    admit(AuthInput {
        local_uid: 1000,
        local,
        peer_uid,
        peer,
        bound,
        observed,
        directory: None,
    })
}

#[test]
fn same_uid_wrong_identity_rejected() {
    let local = signature("ai.grok.computer-use.helper.dev", None, 0xab);
    let peer = signature("ai.attacker.helper", None, 0xab);
    let stamp = socket_stamp(1000, 4);
    let decision = decide(&local, 1000, Some(&peer), stamp, stamp);
    assert_eq!(
        decision,
        Admit::Reject(RejectReason::SigningIdentityMismatch)
    );
    assert!(!decision.serves_mcp());
    assert!(!matches!(decision, Admit::Replace));
}

#[test]
fn same_uid_unsigned_peer_rejected() {
    let local = signature("ai.grok.computer-use.helper.dev", None, 0x11);
    let stamp = socket_stamp(1000, 4);
    assert_eq!(
        decide(&local, 1000, None, stamp, stamp),
        Admit::Reject(RejectReason::UnsignedPeer)
    );
}

#[test]
fn cdhash_mismatch_same_identity_replaces_without_serving() {
    let local = signature("ai.grok.computer-use.helper.dev", None, 0x11);
    let peer = signature("ai.grok.computer-use.helper.dev", None, 0x22);
    let stamp = socket_stamp(1000, 4);
    let decision = decide(&local, 1000, Some(&peer), stamp, stamp);
    assert_eq!(decision, Admit::Replace);
    assert!(!decision.serves_mcp());
}

#[test]
fn team_id_mismatch_rejected() {
    let local = signature("ai.grok.computer-use.helper", None, 0x11);
    let peer = signature("ai.grok.computer-use.helper", Some("TEAM123456"), 0x11);
    let stamp = socket_stamp(1000, 4);
    assert_eq!(
        decide(&local, 1000, Some(&peer), stamp, stamp),
        Admit::Reject(RejectReason::SigningIdentityMismatch)
    );
}

#[test]
fn uid_mismatch_rejected_before_identity() {
    let local = signature("ai.grok.computer-use.helper.dev", None, 0x11);
    let stamp = socket_stamp(1000, 4);
    assert_eq!(
        decide(&local, 1001, Some(&local), stamp, stamp),
        Admit::Reject(RejectReason::UidMismatch)
    );
}

#[test]
fn endpoint_inode_change_rejected() {
    let local = signature("ai.grok.computer-use.helper.dev", None, 0x11);
    let decision = decide(
        &local,
        1000,
        Some(&local),
        socket_stamp(1000, 4),
        socket_stamp(1000, 9),
    );
    assert_eq!(decision, Admit::Reject(RejectReason::EndpointReplaced));
}

#[test]
fn foreign_socket_mode_rejected() {
    let local = signature("ai.grok.computer-use.helper.dev", None, 0x11);
    let mut observed = socket_stamp(1000, 4);
    observed.mode = 0o666;
    assert_eq!(
        decide(&local, 1000, Some(&local), socket_stamp(1000, 4), observed),
        Admit::Reject(RejectReason::EndpointForeign)
    );
    observed.mode = 0o600;
    observed.symlink = true;
    assert!(!socket_is_private(observed, 1000));
    let directory = EndpointStamp {
        dev: 1,
        ino: 2,
        uid: 1000,
        mode: 0o777,
        socket: false,
        directory: true,
        symlink: false,
    };
    assert!(!directory_is_private(directory, 1000));
    let decision = admit(AuthInput {
        local_uid: 1000,
        local: &local,
        peer_uid: 1000,
        peer: Some(&local),
        bound: socket_stamp(1000, 4),
        observed: socket_stamp(1000, 4),
        directory: Some(directory),
    });
    assert_eq!(decision, Admit::Reject(RejectReason::EndpointForeign));
}

#[test]
fn versions_must_match_exactly() {
    assert!(versions_match("0.0.0", "0.0.0"));
    assert!(!versions_match("0.0.0", "0.0.1"));
    assert!(!versions_match("", ""));
    assert!(!versions_match("0.0.0", ""));
    assert!(!versions_match("bad id", "bad id"));
}

#[test]
fn idle_due_at_ten_minutes() {
    assert_eq!(IDLE_LIMIT, Duration::from_secs(600));
    assert!(!idle_expired(Duration::from_secs(599), 0));
    assert!(idle_expired(Duration::from_secs(600), 0));
    assert!(!idle_expired(Duration::from_secs(10_000), 1));
    let mut clock = IdleClock::new(IDLE_LIMIT);
    assert!(!clock.tick(Duration::from_secs(599)));
    assert!(clock.tick(Duration::from_secs(1)));
    let mut clock = IdleClock::new(IDLE_LIMIT);
    assert!(!clock.tick(Duration::from_secs(599)));
    clock.connect();
    assert!(!clock.tick(Duration::from_secs(10_000)));
    clock.disconnect();
    assert!(!clock.tick(Duration::from_secs(599)));
    assert!(clock.tick(Duration::from_secs(1)));
}

#[test]
fn launch_command_uses_open_not_exec() {
    let argv = launch_command(DEV_BUNDLE_ID).unwrap();
    assert_eq!(
        argv,
        vec!["open", "-g", "-j", "-b", DEV_BUNDLE_ID, "--args", "agent"]
    );
    assert!(!argv.iter().any(|arg| arg.contains('/')));
    assert!(launch_command("not an id").is_none());
    assert!(launch_command("quote\"id").is_none());
}

#[test]
fn designated_requirement_is_identifier_only() {
    let requirement = designated_requirement(DEV_BUNDLE_ID).unwrap();
    assert_eq!(
        requirement,
        format!("designated => identifier \"{DEV_BUNDLE_ID}\"")
    );
    assert!(!requirement.contains("cdhash"));
    assert!(designated_requirement("a\\b").is_none());
    assert!(designated_requirement("a\"b").is_none());
    assert!(!valid_identifier(""));
}

#[test]
fn bundle_id_debug_uses_dev_suffix() {
    assert_eq!(bundle_id(true), DEV_BUNDLE_ID);
    assert_eq!(bundle_id(false), RELEASE_BUNDLE_ID);
    assert_eq!(app_name(true), DEV_APP_NAME);
    assert_eq!(app_name(false), RELEASE_APP_NAME);
    assert_ne!(DEV_BUNDLE_ID, RELEASE_BUNDLE_ID);
}

#[test]
fn plist_names_helper_not_terminal() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../apps/gcu-helper-macos/Info.plist");
    let plist = std::fs::read_to_string(path).unwrap();
    assert!(plist.contains(DEV_BUNDLE_ID));
    assert!(plist.contains("<key>LSUIElement</key>"));
    assert!(plist.contains("<true/>"));
    assert!(plist.contains("not to the terminal"));
    let icon = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../apps/gcu-helper-macos/assets/icon.png");
    let bytes = std::fs::read(icon).unwrap();
    assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
}

#[test]
fn same_identity_and_cdhash_serves() {
    let local = signature("ai.grok.computer-use.helper.dev", None, 0x11);
    let stamp = socket_stamp(1000, 4);
    let decision = decide(&local, 1000, Some(&local), stamp, stamp);
    assert_eq!(decision, Admit::Serve);
    assert!(decision.serves_mcp());
}

#[cfg(not(target_os = "macos"))]
#[test]
fn agent_off_macos_exits_two() {
    let home = tempfile::TempDir::new().unwrap();
    let mut command = assert_cmd::Command::new(assert_cmd::cargo::cargo_bin!("gcu"));
    command
        .args(["agent"])
        .env("HOME", home.path())
        .env("TMPDIR", home.path())
        .env("XDG_CONFIG_HOME", home.path())
        .env("XDG_RUNTIME_DIR", home.path())
        .env("XDG_STATE_HOME", home.path().join("state"))
        .env_remove("GCU_APPROVAL")
        .env_remove("GCU_ALLOW_FAKE");
    command.assert().code(2);
}

#[cfg(unix)]
mod sockets {
    use super::*;
    use gcu_cli::helper::client::{client_attach, ClientAttach, ClientConfig, RecordingTerminate};
    use gcu_cli::helper::frame::{
        read_frame, write_frame, Frame, Role, WireProfile, CONTROL_MAX, MCP_MAX,
    };
    use gcu_cli::helper::proxy::{
        endpoint_stamp, start_listener, FixedAudit, ListenConfig, ListenStop, PeerObservation,
        SessionRegistry,
    };
    use gcu_core::driver::Driver;
    use gcu_core::policy::GrantScope;
    use gcu_host_api::{
        AppIdentity, AppInfo, ApprovalDecision, Platform, PolicyConfig, SignatureState,
    };
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    fn uid() -> u32 {
        // SAFETY: geteuid reads the process credential and takes no pointer.
        unsafe { libc::geteuid() }
    }

    fn host() -> gcu_mcp::Host {
        gcu_mcp::Host {
            approvals: Arc::new(gcu_host_api::testing::ScriptedApprovals::new()),
            policy: Arc::new(gcu_host_api::testing::MemoryPolicyStore::new(
                PolicyConfig::default(),
            )),
            events: Arc::new(gcu_host_api::testing::RecordingEventSink::new()),
            activity: Arc::new(gcu_host_api::testing::FakeUserActivity::new()),
        }
    }

    fn driver() -> Arc<dyn Driver> {
        Arc::new(gcu_driver_fake::FakeDriver::from_scene("form").unwrap())
    }

    fn set_mode(path: &Path, mode: u32) {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::symlink_metadata(path).unwrap().permissions();
        permissions.set_mode(mode);
        std::fs::set_permissions(path, permissions).unwrap();
    }

    struct Started {
        _dir: tempfile::TempDir,
        path: PathBuf,
        registry: Arc<SessionRegistry>,
        running: gcu_cli::helper::proxy::Running,
    }

    async fn start(peer: PeerObservation, version: &str, timeout: Duration) -> Started {
        let dir = tempfile::TempDir::new().unwrap();
        set_mode(dir.path(), 0o700);
        let path = dir.path().join("helper.sock");
        let local = signature("ai.grok.computer-use.helper.dev", None, 0x11);
        let registry = Arc::new(SessionRegistry::new());
        let running = start_listener(ListenConfig {
            path: path.clone(),
            audit: Arc::new(FixedAudit::new(local.clone(), peer)),
            version: version.to_string(),
            uid: uid(),
            host: host(),
            driver: driver(),
            registry: Arc::clone(&registry),
            handshake_timeout: timeout,
            idle_limit: Duration::from_secs(600),
        })
        .await
        .unwrap();
        Started {
            _dir: dir,
            path,
            registry,
            running,
        }
    }

    async fn stop(started: Started) -> ListenStop {
        started.running.stop.notify_one();
        tokio::time::timeout(Duration::from_secs(2), started.running.done)
            .await
            .unwrap()
            .unwrap()
    }

    fn client_audit(local: CodeSignature, server: CodeSignature) -> Arc<FixedAudit> {
        Arc::new(FixedAudit::new(
            local.clone(),
            PeerObservation {
                uid: uid(),
                pid: 4242,
                signature: Some(server),
            },
        ))
    }

    fn client_config(
        path: PathBuf,
        audit: Arc<FixedAudit>,
        version: &str,
        control: Arc<RecordingTerminate>,
    ) -> ClientConfig {
        ClientConfig {
            path,
            audit,
            uid: uid(),
            version: version.to_string(),
            profile: WireProfile::Default,
            turn_idle_secs: 45,
            control,
            handshake_timeout: Duration::from_secs(2),
        }
    }

    fn sample_app() -> AppInfo {
        AppInfo {
            name: "Notes".to_string(),
            pid: Some(42),
            identity: AppIdentity {
                platform: Platform::Macos,
                key: "com.apple.Notes".to_string(),
                team_id: Some("APPLETEAM1".to_string()),
                publisher: None,
                sha256: None,
                signature: SignatureState::Valid,
                image_path: "/Applications/Notes.app".to_string(),
                proof: gcu_host_api::GrantProof::default(),
            },
            icon: None,
        }
    }

    #[tokio::test]
    async fn same_binary_second_session_does_not_inherit_grants() {
        let local = signature("ai.grok.computer-use.helper.dev", None, 0x11);
        let started = start(
            PeerObservation {
                uid: uid(),
                pid: 7,
                signature: Some(local.clone()),
            },
            "0.0.0",
            Duration::from_secs(2),
        )
        .await;
        let control = Arc::new(RecordingTerminate::new());
        let first = client_attach(client_config(
            started.path.clone(),
            client_audit(local.clone(), local.clone()),
            "0.0.0",
            Arc::clone(&control),
        ))
        .await
        .unwrap();
        let ClientAttach::Ready(first_stream) = first else {
            panic!("expected a served connection");
        };
        assert_eq!(started.registry.len(), 1);
        let session = started.registry.sessions()[0].clone();
        let app = sample_app();
        session.begin_turn().unwrap();
        session.record_decision(ApprovalDecision::Turn, &app);
        session.record_decision(ApprovalDecision::Session, &app);
        assert_eq!(
            session.grants().covers(&PolicyConfig::default(), &app),
            Some(GrantScope::Turn)
        );
        let (mut read, mut write) = tokio::net::UnixStream::into_split(first_stream);
        write_frame(
            &mut write,
            &Frame::Mcp {
                body: serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": 1,
                    "method": "initialize",
                    "params": {
                        "protocolVersion": "2025-11-25",
                        "capabilities": {},
                        "clientInfo": {"name": "helper-test", "version": "0"}
                    }
                }),
            },
            MCP_MAX,
        )
        .await
        .unwrap();
        let response = tokio::time::timeout(Duration::from_secs(5), read_frame(&mut read, MCP_MAX))
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let Frame::Mcp { body } = response else {
            panic!("expected an MCP frame");
        };
        assert!(body.get("error").is_none(), "{body}");
        assert!(body.get("result").is_some(), "{body}");

        let second = client_attach(client_config(
            started.path.clone(),
            client_audit(local.clone(), local),
            "0.0.0",
            control,
        ))
        .await
        .unwrap();
        let ClientAttach::Ready(second_stream) = second else {
            panic!("expected a second served connection");
        };
        assert_eq!(started.registry.len(), 2);
        let sessions = started.registry.sessions();
        assert!(sessions[1]
            .grants()
            .covers(&PolicyConfig::default(), &app)
            .is_none());
        assert_eq!(
            sessions[0].grants().covers(&PolicyConfig::default(), &app),
            Some(GrantScope::Turn)
        );
        drop(second_stream);
        drop(read);
        drop(write);
        assert_eq!(stop(started).await, ListenStop::Stopped);
    }

    #[tokio::test]
    async fn unsigned_same_uid_connection_opens_no_session() {
        let started = start(
            PeerObservation {
                uid: uid(),
                pid: 7,
                signature: None,
            },
            "0.0.0",
            Duration::from_secs(2),
        )
        .await;
        let token = started.path.with_file_name("token");
        std::fs::write(&token, b"not-a-credential").unwrap();
        let mut stream = tokio::net::UnixStream::connect(&started.path)
            .await
            .unwrap();
        write_frame(
            &mut stream,
            &Frame::Hello {
                version: "0.0.0".to_string(),
                role: Role::Cli,
                profile: WireProfile::Default,
                turn_idle_secs: 45,
            },
            CONTROL_MAX,
        )
        .await
        .ok();
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(started.registry.is_empty());
        assert_eq!(std::fs::read(&token).unwrap(), b"not-a-credential");
        assert_eq!(stop(started).await, ListenStop::Stopped);
    }

    #[tokio::test]
    async fn fake_helper_client_writes_nothing() {
        let dir = tempfile::TempDir::new().unwrap();
        set_mode(dir.path(), 0o700);
        let path = dir.path().join("helper.sock");
        let listener = tokio::net::UnixListener::bind(&path).unwrap();
        set_mode(&path, 0o600);
        let local = signature("ai.grok.computer-use.helper.dev", None, 0x11);
        let audit = Arc::new(FixedAudit::new(
            local,
            PeerObservation {
                uid: uid(),
                pid: 9,
                signature: Some(signature("ai.attacker.helper", None, 0x22)),
            },
        ));
        let control = Arc::new(RecordingTerminate::new());
        let attach = tokio::spawn(client_attach(client_config(
            path.clone(),
            audit,
            "0.0.0",
            Arc::clone(&control),
        )));
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut buffer = [0u8; 32];
        let read = stream.read(&mut buffer).await.unwrap();
        assert_eq!(read, 0);
        match attach.await.unwrap() {
            Err(HelperError::Refused(RejectReason::SigningIdentityMismatch)) => {}
            other => panic!("unexpected {other:?}"),
        }
        assert!(control.pids.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn cdhash_mismatch_shuts_down_without_killing_stranger() {
        let server_sig = signature("ai.grok.computer-use.helper.dev", None, 0x11);
        let client_sig = signature("ai.grok.computer-use.helper.dev", None, 0x22);
        let started = start(
            PeerObservation {
                uid: uid(),
                pid: 77,
                signature: Some(client_sig.clone()),
            },
            "0.0.0",
            Duration::from_secs(2),
        )
        .await;
        let control = Arc::new(RecordingTerminate::new());
        let attached = client_attach(client_config(
            started.path.clone(),
            client_audit(client_sig, server_sig),
            "0.0.0",
            Arc::clone(&control),
        ))
        .await
        .unwrap();
        assert!(matches!(attached, ClientAttach::Replaced { .. }));
        assert!(started.registry.is_empty());
        assert!(control.pids.lock().unwrap().is_empty());
        let stop = tokio::time::timeout(Duration::from_secs(2), started.running.done)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stop, ListenStop::Shutdown);
    }

    #[tokio::test]
    async fn version_mismatch_shuts_helper_down() {
        let local = signature("ai.grok.computer-use.helper.dev", None, 0x11);
        let started = start(
            PeerObservation {
                uid: uid(),
                pid: 7,
                signature: Some(local.clone()),
            },
            "0.0.1",
            Duration::from_secs(2),
        )
        .await;
        let control = Arc::new(RecordingTerminate::new());
        let attached = client_attach(client_config(
            started.path.clone(),
            client_audit(local.clone(), local),
            "0.0.0",
            control,
        ))
        .await
        .unwrap();
        assert!(matches!(attached, ClientAttach::Replaced { .. }));
        assert!(started.registry.is_empty());
        let stop = tokio::time::timeout(Duration::from_secs(2), started.running.done)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stop, ListenStop::Shutdown);
    }

    #[tokio::test]
    async fn extra_wire_field_does_not_stop_helper() {
        let local = signature("ai.grok.computer-use.helper.dev", None, 0x11);
        let started = start(
            PeerObservation {
                uid: uid(),
                pid: 7,
                signature: Some(local),
            },
            "0.0.0",
            Duration::from_secs(2),
        )
        .await;
        let mut stream = tokio::net::UnixStream::connect(&started.path)
            .await
            .unwrap();
        let payload = br#"{"kind":"hello","version":"0.0.0","role":"cli","profile":"default","turn_idle_secs":45,"extra":true}"#;
        stream
            .write_all(&(payload.len() as u32).to_be_bytes())
            .await
            .unwrap();
        stream.write_all(payload).await.unwrap();
        let mut ignored = [0u8; 8];
        let _ = tokio::time::timeout(Duration::from_millis(500), stream.read(&mut ignored)).await;
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(started.registry.is_empty());
        assert_eq!(stop(started).await, ListenStop::Stopped);
    }

    #[tokio::test]
    async fn probe_connection_opens_no_session() {
        let local = signature("ai.grok.computer-use.helper.dev", None, 0x11);
        let started = start(
            PeerObservation {
                uid: uid(),
                pid: 7,
                signature: Some(local),
            },
            "0.0.0",
            Duration::from_millis(200),
        )
        .await;
        drop(
            tokio::net::UnixStream::connect(&started.path)
                .await
                .unwrap(),
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
        let mut hung = tokio::net::UnixStream::connect(&started.path)
            .await
            .unwrap();
        let _ = hung.write_all(&[0, 0, 0, 1, b'{']).await;
        tokio::time::sleep(Duration::from_millis(400)).await;
        assert!(started.registry.is_empty());
        assert_eq!(stop(started).await, ListenStop::Stopped);
    }

    #[tokio::test]
    async fn symlink_endpoint_refused_and_left_in_place() {
        let dir = tempfile::TempDir::new().unwrap();
        set_mode(dir.path(), 0o700);
        let real = dir.path().join("real.sock");
        let _listener = std::os::unix::net::UnixListener::bind(&real).unwrap();
        let link = dir.path().join("helper.sock");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let error = start_listener(ListenConfig {
            path: link.clone(),
            audit: Arc::new(FixedAudit::new(
                signature("ai.grok.computer-use.helper.dev", None, 0x11),
                PeerObservation {
                    uid: uid(),
                    pid: 1,
                    signature: None,
                },
            )),
            version: "0.0.0".to_string(),
            uid: uid(),
            host: host(),
            driver: driver(),
            registry: Arc::new(SessionRegistry::new()),
            handshake_timeout: Duration::from_secs(1),
            idle_limit: Duration::from_secs(600),
        })
        .await
        .unwrap_err();
        assert!(matches!(error, HelperError::Foreign));
        assert!(std::fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink());
    }

    #[tokio::test]
    async fn inode_change_stops_listener_without_unlinking_replacement() {
        let local = signature("ai.grok.computer-use.helper.dev", None, 0x11);
        let started = start(
            PeerObservation {
                uid: uid(),
                pid: 7,
                signature: Some(local),
            },
            "0.0.0",
            Duration::from_secs(2),
        )
        .await;
        let old = endpoint_stamp(&started.path).unwrap();
        std::fs::remove_file(&started.path).unwrap();
        let _replacement = std::os::unix::net::UnixListener::bind(&started.path).unwrap();
        let stop = tokio::time::timeout(Duration::from_secs(2), started.running.done)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stop, ListenStop::Replaced);
        let now = endpoint_stamp(&started.path).unwrap();
        assert_ne!((old.dev, old.ino), (now.dev, now.ino));
        assert!(started.path.exists());
    }

    #[tokio::test]
    async fn world_writable_directory_refused() {
        let dir = tempfile::TempDir::new().unwrap();
        let socket_dir = dir.path().join("open");
        std::fs::create_dir(&socket_dir).unwrap();
        set_mode(&socket_dir, 0o777);
        let error = start_listener(ListenConfig {
            path: socket_dir.join("helper.sock"),
            audit: Arc::new(FixedAudit::new(
                signature("ai.grok.computer-use.helper.dev", None, 0x11),
                PeerObservation {
                    uid: uid(),
                    pid: 1,
                    signature: None,
                },
            )),
            version: "0.0.0".into(),
            uid: uid(),
            host: host(),
            driver: driver(),
            registry: Arc::new(SessionRegistry::new()),
            handshake_timeout: Duration::from_secs(1),
            idle_limit: Duration::from_secs(600),
        })
        .await
        .unwrap_err();
        assert!(matches!(error, HelperError::Foreign));
        let mode = {
            use std::os::unix::fs::PermissionsExt;
            std::fs::symlink_metadata(&socket_dir)
                .unwrap()
                .permissions()
                .mode()
                & 0o777
        };
        assert_eq!(mode, 0o777);
    }

    #[tokio::test]
    async fn bound_socket_is_owner_only() {
        let local = signature("ai.grok.computer-use.helper.dev", None, 0x11);
        let started = start(
            PeerObservation {
                uid: uid(),
                pid: 1,
                signature: Some(local),
            },
            "0.0.0",
            Duration::from_secs(2),
        )
        .await;
        let socket = endpoint_stamp(&started.path).unwrap();
        let directory = endpoint_stamp(started.path.parent().unwrap()).unwrap();
        assert!(socket_is_private(socket, uid()));
        assert!(directory_is_private(directory, uid()));
        assert_eq!(stop(started).await, ListenStop::Stopped);
    }

    #[tokio::test]
    async fn stale_socket_reclaimed() {
        let dir = tempfile::TempDir::new().unwrap();
        set_mode(dir.path(), 0o700);
        let path = dir.path().join("helper.sock");
        let stale = std::os::unix::net::UnixListener::bind(&path).unwrap();
        set_mode(&path, 0o600);
        drop(stale);
        assert!(
            std::os::unix::net::UnixStream::connect(&path).is_err(),
            "a dropped listener must refuse before it is reclaimed"
        );
        let local = signature("ai.grok.computer-use.helper.dev", None, 0x11);
        let running = start_listener(ListenConfig {
            path: path.clone(),
            audit: Arc::new(FixedAudit::new(
                local.clone(),
                PeerObservation {
                    uid: uid(),
                    pid: 1,
                    signature: Some(local),
                },
            )),
            version: "0.0.0".into(),
            uid: uid(),
            host: host(),
            driver: driver(),
            registry: Arc::new(SessionRegistry::new()),
            handshake_timeout: Duration::from_secs(1),
            idle_limit: Duration::from_secs(600),
        })
        .await
        .unwrap();
        let live = tokio::time::timeout(
            Duration::from_secs(1),
            tokio::net::UnixStream::connect(&path),
        )
        .await
        .unwrap();
        assert!(
            live.is_ok(),
            "the reclaimed socket must accept a connection"
        );
        drop(live);
        assert!(socket_is_private(endpoint_stamp(&path).unwrap(), uid()));
        running.stop.notify_one();
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(2), running.done)
                .await
                .unwrap()
                .unwrap(),
            ListenStop::Stopped
        );
    }

    #[tokio::test]
    async fn live_socket_not_stolen() {
        let local = signature("ai.grok.computer-use.helper.dev", None, 0x11);
        let started = start(
            PeerObservation {
                uid: uid(),
                pid: 1,
                signature: Some(local.clone()),
            },
            "0.0.0",
            Duration::from_secs(2),
        )
        .await;
        let ino = endpoint_stamp(&started.path).unwrap().ino;
        let error = start_listener(ListenConfig {
            path: started.path.clone(),
            audit: Arc::new(FixedAudit::new(
                local.clone(),
                PeerObservation {
                    uid: uid(),
                    pid: 1,
                    signature: Some(local),
                },
            )),
            version: "0.0.0".into(),
            uid: uid(),
            host: host(),
            driver: driver(),
            registry: Arc::new(SessionRegistry::new()),
            handshake_timeout: Duration::from_secs(1),
            idle_limit: Duration::from_secs(600),
        })
        .await
        .unwrap_err();
        assert!(matches!(error, HelperError::Busy));
        assert_eq!(endpoint_stamp(&started.path).unwrap().ino, ino);
        assert!(started.registry.is_empty());
        assert_eq!(stop(started).await, ListenStop::Stopped);
    }

    #[tokio::test]
    async fn foreign_mode_socket_not_unlinked() {
        let dir = tempfile::TempDir::new().unwrap();
        set_mode(dir.path(), 0o700);
        let path = dir.path().join("helper.sock");
        let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
        set_mode(&path, 0o666);
        let local = signature("ai.grok.computer-use.helper.dev", None, 0x11);
        let error = start_listener(ListenConfig {
            path: path.clone(),
            audit: Arc::new(FixedAudit::new(
                local.clone(),
                PeerObservation {
                    uid: uid(),
                    pid: 1,
                    signature: Some(local),
                },
            )),
            version: "0.0.0".into(),
            uid: uid(),
            host: host(),
            driver: driver(),
            registry: Arc::new(SessionRegistry::new()),
            handshake_timeout: Duration::from_secs(1),
            idle_limit: Duration::from_secs(600),
        })
        .await
        .unwrap_err();
        assert!(matches!(error, HelperError::Foreign));
        assert!(path.exists());
        drop(listener);
    }

    #[tokio::test]
    async fn idle_listener_with_zero_connections_exits() {
        let dir = tempfile::TempDir::new().unwrap();
        set_mode(dir.path(), 0o700);
        let path = dir.path().join("helper.sock");
        let local = signature("ai.grok.computer-use.helper.dev", None, 0x11);
        let running = start_listener(ListenConfig {
            path,
            audit: Arc::new(FixedAudit::new(
                local.clone(),
                PeerObservation {
                    uid: uid(),
                    pid: 1,
                    signature: Some(local),
                },
            )),
            version: "0.0.0".into(),
            uid: uid(),
            host: host(),
            driver: driver(),
            registry: Arc::new(SessionRegistry::new()),
            handshake_timeout: Duration::from_secs(1),
            idle_limit: Duration::from_millis(400),
        })
        .await
        .unwrap();
        let stop = tokio::time::timeout(Duration::from_secs(3), running.done).await;
        assert_eq!(stop.unwrap().unwrap(), ListenStop::Idle);
    }
}
