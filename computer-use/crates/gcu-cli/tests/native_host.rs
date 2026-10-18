//! Native messaging boundary tests. Every filesystem write uses an injected temporary home.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use gcu_cli::native_host::{
    codec::{read, write, MAX_MESSAGE_BYTES},
    install::{Browser, InstallPlan, Platform, Registry},
    Connection, ExtensionIdentity, RouterGate, Scope,
};
use serde_json::{json, Value};
use std::{collections::BTreeMap, io, path::Path};

#[path = "native_host_support/store.rs"]
mod store;

fn identity() -> ExtensionIdentity {
    // Public test bytes, never a production identity or a Chrome signing key.
    ExtensionIdentity::from_manifest(r#"{"key":"AQIDBA=="}"#).unwrap()
}

#[tokio::test]
async fn transport_real_duplex_handshake_share_revoke_and_eof_are_scoped() {
    use tokio::io::AsyncWriteExt;
    let (host, browser) = tokio::io::duplex(4096);
    let (mut input, mut output) = tokio::io::split(host);
    let (mut browser_in, mut browser_out) = tokio::io::split(browser);
    let identity = identity();
    let origin = identity.origin();
    let host = tokio::spawn(async move {
        gcu_cli::native_host::serve(&mut input, &mut output, &identity, &origin).await
    });
    let hello = read(&mut browser_in).await.unwrap().unwrap();
    assert_eq!(hello["kind"], "hello");
    let id = hello["connectionId"].clone();
    write(
        &mut browser_out,
        &json!({"kind":"ready", "protocol":1, "connectionId":id}),
    )
    .await
    .unwrap();
    write(
        &mut browser_out,
        &json!({"kind":"shared", "connectionId":id,
        "scope":{"tabId":7,"frameId":0,"documentId":"document-1","shareId":"share-1"}}),
    )
    .await
    .unwrap();
    let ack = read(&mut browser_in).await.unwrap().unwrap();
    assert_eq!(
        ack,
        json!({"kind":"sharedAck","connectionId":id,"shareId":"share-1"})
    );
    write(
        &mut browser_out,
        &json!({"kind":"revoked","connectionId":id,"shareId":"share-1"}),
    )
    .await
    .unwrap();
    browser_out.shutdown().await.unwrap();
    host.await.unwrap().unwrap();
    assert_eq!(read(&mut browser_in).await.unwrap(), None);
}

#[tokio::test(start_paused = true)]
async fn transport_handshake_partial_frame_and_blocked_output_have_deadlines() {
    use tokio::io::AsyncWriteExt;
    let (mut input, mut browser) = tokio::io::duplex(64);
    browser.write_all(&[1]).await.unwrap();
    assert!(matches!(
        read(&mut input).await,
        Err(gcu_cli::native_host::Error::Deadline)
    ));
    let (mut blocked, _unread) = tokio::io::duplex(1);
    assert!(matches!(
        write(&mut blocked, &json!({"test":true})).await,
        Err(gcu_cli::native_host::Error::Deadline)
    ));
    let (mut input, _connected_but_silent) = tokio::io::duplex(64);
    let mut output = Vec::new();
    let identity = identity();
    let origin = identity.origin();
    assert!(matches!(
        gcu_cli::native_host::serve(&mut input, &mut output, &identity, &origin).await,
        Err(gcu_cli::native_host::Error::Deadline)
    ));
}

#[test]
fn routing_response_session_is_server_owned_and_duplicate_replies_are_rejected() {
    let other = connection();
    let mut connection = connection();
    shared(&mut connection);
    connection.bind("share-1", "session-a").unwrap();
    let request = connection
        .request(
            "session-a",
            "share-1",
            json!({"type":"click","snapshotId":"s","nodeId":"n"}),
            &Gate(true),
        )
        .unwrap();
    let reply = json!({"kind":"response","connectionId":connection.id(),"requestId":request["request"]["requestId"], "result":{"kind":"action","outcome":"done"}});
    let gcu_cli::native_host::Event::Response(result) = connection.receive(reply.clone()).unwrap()
    else {
        panic!("expected response")
    };
    assert_eq!(result.session_id, "session-a");
    assert!(connection.receive(reply).is_err());
    assert_ne!(connection.id(), other.id());
}
fn ready(connection: &mut Connection) {
    connection
        .receive(json!({"kind":"ready", "protocol":1,
        "connectionId":connection.id()}))
        .unwrap();
}
fn shared(connection: &mut Connection) {
    connection
        .receive(json!({"kind":"shared", "connectionId":connection.id(),
        "scope":{"tabId":7,"frameId":0,"documentId":"document-1","shareId":"share-1"}}))
        .unwrap();
}
struct Gate(bool);
impl RouterGate for Gate {
    fn authorize(&self, _session: &str, _scope: &Scope, _operation: &Value) -> bool {
        self.0
    }
}
fn connection() -> Connection {
    let identity = identity();
    let mut connection = Connection::new(&identity, &identity.origin()).unwrap();
    ready(&mut connection);
    connection
}

#[tokio::test]
async fn codec_little_endian_frames_round_trip_without_stdout_text() {
    let mut bytes = Vec::new();
    write(&mut bytes, &json!({"kind":"ready"})).await.unwrap();
    let length = u32::from_le_bytes(bytes[..4].try_into().unwrap()) as usize;
    assert_eq!(length, bytes.len() - 4);
    let mut input = bytes.as_slice();
    assert_eq!(
        read(&mut input).await.unwrap(),
        Some(json!({"kind":"ready"}))
    );
    assert_eq!(read(&mut input).await.unwrap(), None);
}

#[tokio::test]
async fn codec_bad_truncated_oversized_and_mutated_frames_fail_closed() {
    let mut valid = Vec::new();
    write(&mut valid, &json!({"kind":"ready"})).await.unwrap();
    for length in 1..valid.len() {
        assert!(
            read(&mut &valid[..length]).await.is_err(),
            "truncated at {length}"
        );
    }
    for length in [0, MAX_MESSAGE_BYTES as u32 + 1, u32::MAX] {
        assert!(read(&mut &length.to_le_bytes()[..]).await.is_err());
    }
    for payload in [b"{bad".as_slice(), b"null", b"[]", b"\xff", b"{} {}"] {
        let mut bytes = (payload.len() as u32).to_le_bytes().to_vec();
        bytes.extend_from_slice(payload);
        assert!(read(&mut bytes.as_slice()).await.is_err());
    }
    // Table-driven fuzz of the length prefix, exercising every byte/bit.
    for byte in 0..4 {
        for bit in 0..8 {
            let mut bytes = valid.clone();
            bytes[byte] ^= 1 << bit;
            assert!(read(&mut bytes.as_slice()).await.is_err());
        }
    }
    let huge = json!({"text":"x".repeat(MAX_MESSAGE_BYTES)});
    let mut output = Vec::new();
    assert!(write(&mut output, &huge).await.is_err());
    assert!(output.is_empty());
}

#[test]
fn identity_missing_key_and_wrong_extension_origins_are_rejected() {
    assert!(ExtensionIdentity::from_manifest("{}").is_err());
    assert!(ExtensionIdentity::from_manifest(r#"{"key":"__MAINTAINER_PUBLIC_KEY__"}"#).is_err());
    let identity = identity();
    assert_eq!(identity.id().len(), 32);
    assert!(identity.id().bytes().all(|b| (b'a'..=b'p').contains(&b)));
    for origin in [
        "",
        "chrome-extension://__MAINTAINER_EXTENSION_ID__/",
        "https://example.test/",
        &identity
            .origin()
            .replace("chrome-extension:", "moz-extension:"),
        &format!("{}extra", identity.origin()),
        &identity.origin().to_uppercase(),
    ] {
        assert!(Connection::new(&identity, origin).is_err());
    }
}

#[test]
fn routing_unshared_cross_session_and_unapproved_requests_are_rejected() {
    let mut connection = connection();
    assert!(connection.bind("share-1", "session-a").is_err());
    shared(&mut connection);
    connection.bind("share-1", "session-a").unwrap();
    assert!(connection.bind("share-1", "session-b").is_err());
    assert!(connection
        .request(
            "session-b",
            "share-1",
            json!({"type":"observe"}),
            &Gate(true)
        )
        .is_err());
    assert!(connection
        .request(
            "session-a",
            "share-1",
            json!({"type":"observe"}),
            &Gate(false)
        )
        .is_err());
    let request = connection
        .request(
            "session-a",
            "share-1",
            json!({"type":"observe"}),
            &Gate(true),
        )
        .unwrap();
    assert_eq!(request["request"]["sessionId"], "session-a");
    assert!(connection
        .request(
            "session-a",
            "share-1",
            json!({"type":"observe"}),
            &Gate(true)
        )
        .is_err());
    assert!(connection
        .receive(json!({"kind":"response","connectionId":connection.id(),
        "requestId":request["request"]["requestId"], "result":{"kind":"snapshot","snapshot":{}},
        "sessionId":"session-b"}))
        .is_err());
}

#[test]
fn routing_handshake_scope_and_reply_mutations_are_rejected() {
    let mut connection = connection();
    for scope in [
        json!({"tabId":7,"frameId":1,"documentId":"document-1","shareId":"share-1"}),
        json!({"tabId":7,"frameId":0,"documentId":"document-1","shareId":"share-1","windowId":4}),
        json!({"tabId":7,"frameId":0,"documentId":"document-1","shareId":"share-1","sessionId":"page"}),
        json!({"tabId":-1,"frameId":0,"documentId":"document-1","shareId":"share-1"}),
    ] {
        assert!(connection
            .receive(json!({"kind":"shared","connectionId":connection.id(),"scope":scope}))
            .is_err());
    }
    assert!(connection
        .receive(json!({"kind":"shared","connectionId":"old","scope":{}}))
        .is_err());
    assert!(connection.receive(json!({"kind":"response","connectionId":connection.id(),"requestId":"unsolicited","result":{"kind":"action","outcome":"done"}})).is_err());
    let identity = identity();
    let mut unopened = Connection::new(&identity, &identity.origin()).unwrap();
    assert!(unopened
        .receive(json!({"kind":"ready","protocol":1,"connectionId":"other"}))
        .is_err());
    assert!(unopened.bind("share-1", "session-a").is_err());
}

#[test]
fn routing_disconnect_revoke_and_session_end_retire_pending_without_replay() {
    for boundary in ["disconnect", "revoke", "session_end"] {
        let mut connection = connection();
        shared(&mut connection);
        connection.bind("share-1", "session-a").unwrap();
        let request = connection
            .request(
                "session-a",
                "share-1",
                json!({"type":"click","snapshotId":"snapshot-1","nodeId":"node-1"}),
                &Gate(true),
            )
            .unwrap();
        let cancelled = match boundary {
            "disconnect" => connection.disconnect(),
            "session_end" => connection.close_session("session-a"),
            _ => connection.revoke("share-1"),
        };
        assert_eq!(cancelled.len(), 1);
        assert_eq!(cancelled[0].session_id, "session-a");
        assert_eq!(
            cancelled[0].result,
            json!({"kind":"action","outcome":"unknown"})
        );
        assert!(connection
            .request(
                "session-a",
                "share-1",
                json!({"type":"observe"}),
                &Gate(true)
            )
            .is_err());
        assert!(connection.receive(json!({"kind":"response","connectionId":connection.id(),
            "requestId":request["request"]["requestId"],"result":{"kind":"action","outcome":"done"}})).is_err());
    }
}

#[derive(Default)]
struct FakeRegistry {
    values: BTreeMap<Browser, String>,
    writes: usize,
}
impl Registry for FakeRegistry {
    fn get(&self, browser: Browser) -> io::Result<Option<String>> {
        Ok(self.values.get(&browser).cloned())
    }
    fn set(&mut self, browser: Browser, path: Option<&str>) -> io::Result<()> {
        self.writes += 1;
        if let Some(path) = path {
            self.values.insert(browser, path.to_owned());
        } else {
            self.values.remove(&browser);
        }
        Ok(())
    }
}

#[test]
fn install_uninstall_all_platforms_are_idempotent_and_only_allow_fixed_origin() {
    for platform in [Platform::Linux, Platform::Macos, Platform::Windows] {
        let home = tempfile::tempdir().unwrap();
        let gcu = home.path().join("gcu");
        let mut registry = FakeRegistry::default();
        let plan = InstallPlan::new(
            platform,
            home.path(),
            &gcu,
            Some(&identity()),
            false,
            &registry,
        )
        .unwrap();
        assert!(!plan.is_noop());
        assert_eq!(
            std::fs::read_dir(home.path()).unwrap().count(),
            0,
            "planning must be read-only"
        );
        let manifests = plan.manifests();
        assert_eq!(manifests.len(), 2);
        let roots = match platform {
            Platform::Linux => [
                ".config/google-chrome/NativeMessagingHosts",
                ".config/microsoft-edge/NativeMessagingHosts",
            ],
            Platform::Macos => [
                "Library/Application Support/Google/Chrome/NativeMessagingHosts",
                "Library/Application Support/Microsoft Edge/NativeMessagingHosts",
            ],
            Platform::Windows => [
                "AppData/Local/grok-computer-use/NativeMessagingHosts/chrome",
                "AppData/Local/grok-computer-use/NativeMessagingHosts/edge",
            ],
        };
        for (path, root) in manifests.iter().zip(roots) {
            assert_eq!(
                *path,
                home.path().join(root).join("ai.grok.computer_use.json")
            );
        }
        plan.apply(&mut registry).unwrap();
        if platform == Platform::Windows {
            assert_eq!(registry.writes, 2);
            assert_eq!(
                registry.values[&Browser::Chrome],
                manifests[0].to_str().unwrap()
            );
            assert_eq!(
                registry.values[&Browser::Edge],
                manifests[1].to_str().unwrap()
            );
            assert_eq!(
                Browser::Chrome.registry_key(),
                "Software\\Google\\Chrome\\NativeMessagingHosts\\ai.grok.computer_use"
            );
            assert_eq!(
                Browser::Edge.registry_key(),
                "Software\\Microsoft\\Edge\\NativeMessagingHosts\\ai.grok.computer_use"
            );
        } else {
            assert_eq!(registry.writes, 0);
        }
        for path in &manifests {
            let manifest: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
            assert_eq!(manifest["allowed_origins"], json!([identity().origin()]));
            assert_eq!(manifest["path"], gcu.to_str().unwrap());
            assert!(manifest.get("args").is_none());
        }
        let writes = registry.writes;
        let second = InstallPlan::new(
            platform,
            home.path(),
            &gcu,
            Some(&identity()),
            false,
            &registry,
        )
        .unwrap();
        assert!(second.is_noop());
        second.apply(&mut registry).unwrap();
        assert_eq!(registry.writes, writes);
        // Removal remains available when a newer build has no configured key.
        let backups = InstallPlan::new(platform, home.path(), &gcu, None, true, &registry)
            .unwrap()
            .apply(&mut registry)
            .unwrap();
        assert_eq!(backups.len(), 2);
        for backup in backups {
            assert!(backup.is_file());
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                assert_eq!(
                    std::fs::metadata(backup).unwrap().permissions().mode() & 0o777,
                    0o600
                );
            }
        }
        for path in manifests {
            assert!(!path.exists());
        }
        let remove = InstallPlan::new(platform, home.path(), &gcu, None, true, &registry).unwrap();
        assert!(remove.is_noop());
        remove.apply(&mut registry).unwrap();
        assert!(registry.values.is_empty());
    }
}

#[test]
fn install_foreign_registration_and_changed_plan_are_preserved() {
    let home = tempfile::tempdir().unwrap();
    let gcu = home.path().join("gcu");
    let mut registry = FakeRegistry::default();
    registry
        .values
        .insert(Browser::Chrome, "foreign.json".into());
    assert!(InstallPlan::new(
        Platform::Windows,
        home.path(),
        &gcu,
        Some(&identity()),
        false,
        &registry
    )
    .is_err());
    registry.values.clear();
    let plan = InstallPlan::new(
        Platform::Linux,
        home.path(),
        &gcu,
        Some(&identity()),
        false,
        &registry,
    )
    .unwrap();
    let path = plan.manifests()[0].clone();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "foreign").unwrap();
    assert!(plan.apply(&mut registry).is_err());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "foreign");
    assert!(InstallPlan::new(Platform::Linux, home.path(), &gcu, None, true, &registry).is_err());
    assert!(InstallPlan::new(Platform::Linux, home.path(), &gcu, None, false, &registry).is_err());
    assert!(InstallPlan::new(
        Platform::Linux,
        Path::new("relative"),
        &gcu,
        Some(&identity()),
        false,
        &registry
    )
    .is_err());
}

#[cfg(unix)]
#[test]
fn install_symlinked_home_and_browser_directory_are_rejected_without_writes() {
    use std::os::unix::fs::symlink;
    let fixture = tempfile::tempdir().unwrap();
    let real = fixture.path().join("real");
    std::fs::create_dir(&real).unwrap();
    let link = fixture.path().join("home");
    symlink(&real, &link).unwrap();
    let registry = FakeRegistry::default();
    let gcu = fixture.path().join("gcu");
    assert!(InstallPlan::new(
        Platform::Linux,
        &link,
        &gcu,
        Some(&identity()),
        false,
        &registry
    )
    .is_err());
    symlink(&real, fixture.path().join(".config")).unwrap();
    assert!(InstallPlan::new(
        Platform::Linux,
        fixture.path(),
        &gcu,
        Some(&identity()),
        false,
        &registry
    )
    .is_err());
    assert_eq!(std::fs::read_dir(&real).unwrap().count(), 0);
}

#[test]
fn cli_native_browser_launch_and_extension_client_are_registered() {
    let home = tempfile::tempdir().unwrap();
    for argv in [
        vec![
            "native-host",
            "chrome-extension://__MAINTAINER_EXTENSION_ID__/",
        ],
        vec![
            "chrome-extension://__MAINTAINER_EXTENSION_ID__/",
            "--parent-window=0",
        ],
    ] {
        let result = std::process::Command::new(env!("CARGO_BIN_EXE_gcu"))
            .args(argv)
            .env_remove("GCU_APPROVAL")
            .env("HOME", home.path())
            .env("USERPROFILE", home.path())
            .output()
            .unwrap();
        assert_eq!(result.status.code(), Some(2));
        assert!(
            result.stdout.is_empty(),
            "origin rejection must not contaminate native stdout"
        );
    }
    // Avoid a real Windows registry backend in a portable process test.
    #[cfg(not(windows))]
    {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_gcu"))
            .args(["uninstall", "chrome-extension", "--json", "--home"])
            .arg(home.path())
            .env_remove("GCU_APPROVAL")
            .output()
            .unwrap();
        assert!(output.status.success());
        let report: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(report["client"], "chrome-extension");
        assert_eq!(report["noop"], true);
    }
}
