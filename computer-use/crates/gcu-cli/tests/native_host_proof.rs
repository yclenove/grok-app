//! A4a browser identity and window-mapping evidence.
//! Real Chrome/Edge launch chains are not executed here.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use gcu_cli::native_host::{
    build_id, check_launch,
    install::{Browser, InstallPlan, Platform, Registry},
    mapping_verdict, record_current, select_engine, select_session, BrokerGate, CancelKind,
    Connection, EngineEndpoint, ExpectedBuild, ExtensionIdentity, L2Decision, L2Refusal,
    LaunchInput, LaunchSnapshot, MapClaim, MappingVerdict, OsWindow, ProofBrowser, ProofOutcome,
    ProofPlatform, RecordFault, RouterGate, Scope, SessionCandidate, SharedDocument,
    REAL_MACHINE_EVIDENCE,
};
use serde_json::{json, Value};
use std::path::Path;

fn embedded() -> ExtensionIdentity {
    ExtensionIdentity::embedded().expect("manifest key")
}

fn origin() -> String {
    embedded().origin()
}

fn snapshot(parent: &str, argv: Vec<String>, exe: &str) -> LaunchSnapshot {
    LaunchSnapshot {
        host_pid: 40,
        host_instance: 8,
        host_exe: exe.into(),
        host_build_id: build_id(),
        parent_pid: 4,
        parent_instance: 3,
        parent_exe_name: parent.into(),
        argv,
    }
}

fn expect_exe(exe: &str) -> ExpectedBuild {
    ExpectedBuild {
        exe: exe.into(),
        build_id: build_id(),
    }
}

fn one_engine() -> Vec<EngineEndpoint> {
    vec![EngineEndpoint {
        engine_id: "engine-a".into(),
        instance: 5,
    }]
}

fn closed(
    platform: ProofPlatform,
    browser: ProofBrowser,
    launch: &LaunchSnapshot,
    engines: &[EngineEndpoint],
) -> L2Refusal {
    open_with(platform, browser, launch, engines).expect_err("l2 stays closed")
}

fn open_with(
    platform: ProofPlatform,
    browser: ProofBrowser,
    launch: &LaunchSnapshot,
    engines: &[EngineEndpoint],
) -> Result<BrokerGate, L2Refusal> {
    let identity = embedded();
    let origin = identity.origin();
    let expected = expect_exe(&launch.host_exe);
    BrokerGate::open(LaunchInput {
        identity: &identity,
        origin: &origin,
        platform,
        browser,
        launch,
        expected: &expected,
        engines,
    })
}

fn chrome_linux() -> BrokerGate {
    let argv = vec!["native-host".into(), origin()];
    let launch = snapshot("google-chrome-stable", argv, "/usr/bin/gcu");
    open_with(
        ProofPlatform::LinuxX11,
        ProofBrowser::Chrome,
        &launch,
        &one_engine(),
    )
    .expect("chrome launch shape")
}

fn document() -> SharedDocument {
    SharedDocument {
        share_id: "share-1".into(),
        tab_id: 7,
        frame_id: 0,
        document_id: "document-1".into(),
        browser_window_id: 3,
    }
}

fn claim() -> MapClaim {
    MapClaim {
        share_id: "share-1".into(),
        tab_id: 7,
        document_id: "document-1".into(),
        browser_window_id: 3,
        epoch: 1,
        core_window_id: "window-1".into(),
        os_window: None,
        title_similarity: 100,
        geometry_similarity: 100,
    }
}

fn session(engine: &str, window: &str, authorized: bool) -> SessionCandidate {
    SessionCandidate {
        session_id: format!("session-{engine}-{window}"),
        engine_id: engine.into(),
        authorized,
        epoch: 1,
        window_id: window.into(),
    }
}

fn reason(outcome: &ProofOutcome) -> L2Refusal {
    match outcome.decision {
        L2Decision::Unavailable { reason } => reason,
    }
}

fn template(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tools/browser-extension/manifests")
        .join(name);
    std::fs::read_to_string(path).unwrap()
}

#[test]
fn embedded_key_template_placeholder_and_install_origin_fail_closed() {
    let identity = embedded();
    assert_eq!(identity.id(), "jgpkhdjlddaoopklipcnpcjcpiancmff");
    assert_eq!(
        identity.origin(),
        "chrome-extension://jgpkhdjlddaoopklipcnpcjcpiancmff/"
    );
    assert!(ExtensionIdentity::from_manifest("{}").is_err());
    assert!(ExtensionIdentity::from_manifest(r#"{"key":"__MAINTAINER_PUBLIC_KEY__"}"#).is_err());
    assert!(ExtensionIdentity::from_manifest(r#"{"key":""}"#).is_err());
    let other = ExtensionIdentity::from_manifest(r#"{"key":"AQIDBA=="}"#).unwrap();
    assert_ne!(other.id(), identity.id());
    assert!(Connection::new(&identity, &other.origin()).is_err());
    assert!(Connection::new(&identity, "chrome-extension://__MAINTAINER_EXTENSION_ID__/").is_err());
    for name in ["linux.json", "macos.json", "windows.json"] {
        let body = template(name);
        assert!(body.contains("chrome-extension://__MAINTAINER_EXTENSION_ID_FROM_MANIFEST_KEY__/"));
        assert!(!body.contains(&identity.origin()));
    }
    let home = tempfile::tempdir().unwrap();
    let gcu = home.path().join("gcu");
    let plan = InstallPlan::new(
        Platform::Linux,
        home.path(),
        &gcu,
        Some(&identity),
        false,
        &EmptyRegistry,
    )
    .unwrap();
    plan.apply(&mut EmptyRegistry).unwrap();
    for path in plan.manifests() {
        let manifest: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert_eq!(manifest["allowed_origins"], json!([identity.origin()]));
        assert!(manifest.get("args").is_none());
    }
}

struct EmptyRegistry;
impl Registry for EmptyRegistry {
    fn get(&self, _: Browser) -> std::io::Result<Option<String>> {
        Ok(None)
    }
    fn set(&mut self, _: Browser, _: Option<&str>) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn wrong_origin_fake_parent_and_wrong_build_fail_closed() {
    let identity = embedded();
    let good = snapshot(
        "google-chrome-stable",
        vec!["native-host".into(), origin()],
        "/usr/bin/gcu",
    );
    let expected = expect_exe("/usr/bin/gcu");
    assert_eq!(
        check_launch(
            &identity,
            "https://example.test/",
            ProofPlatform::LinuxX11,
            ProofBrowser::Chrome,
            &good,
            &expected
        ),
        Err(L2Refusal::Identity)
    );
    let mut relay = good.clone();
    relay.parent_exe_name = "python3".into();
    assert_eq!(
        check_launch(
            &identity,
            &origin(),
            ProofPlatform::LinuxX11,
            ProofBrowser::Chrome,
            &relay,
            &expected
        ),
        Err(L2Refusal::FakeRelay)
    );
    relay.parent_exe_name = "gcu".into();
    assert_eq!(
        closed(
            ProofPlatform::LinuxX11,
            ProofBrowser::Chrome,
            &relay,
            &one_engine()
        ),
        L2Refusal::FakeRelay
    );
    let mut windows_name = good.clone();
    windows_name.parent_exe_name = "chrome.exe".into();
    assert_eq!(
        closed(
            ProofPlatform::LinuxX11,
            ProofBrowser::Chrome,
            &windows_name,
            &one_engine()
        ),
        L2Refusal::FakeRelay
    );
    let mut edge_parent = good.clone();
    edge_parent.parent_exe_name = "microsoft-edge-stable".into();
    assert_eq!(
        closed(
            ProofPlatform::LinuxX11,
            ProofBrowser::Chrome,
            &edge_parent,
            &one_engine()
        ),
        L2Refusal::FakeRelay
    );
    let mut other_build = good.clone();
    other_build.host_build_id = "gcu/other".into();
    assert_eq!(
        closed(
            ProofPlatform::LinuxX11,
            ProofBrowser::Chrome,
            &other_build,
            &one_engine()
        ),
        L2Refusal::BuildMismatch
    );
    let mut other_exe = good.clone();
    other_exe.host_exe = "/tmp/other-gcu".into();
    assert_eq!(
        check_launch(
            &embedded(),
            &origin(),
            ProofPlatform::LinuxX11,
            ProofBrowser::Chrome,
            &other_exe,
            &expect_exe("/usr/bin/gcu")
        ),
        Err(L2Refusal::BuildMismatch)
    );
    assert_eq!(
        closed(ProofPlatform::LinuxX11, ProofBrowser::Chrome, &good, &[]),
        L2Refusal::NoEngine
    );
    let many = vec![
        EngineEndpoint {
            engine_id: "engine-a".into(),
            instance: 5,
        },
        EngineEndpoint {
            engine_id: "engine-b".into(),
            instance: 6,
        },
    ];
    assert_eq!(
        closed(ProofPlatform::LinuxX11, ProofBrowser::Chrome, &good, &many),
        L2Refusal::AmbiguousEngine
    );
    assert!(select_engine(&[]).is_err());
}

#[test]
fn launch_shape_is_chrome_or_edge_and_parent_window_is_not_the_document() {
    let identity = embedded();
    let cases = [
        (ProofPlatform::LinuxX11, ProofBrowser::Chrome, "chrome"),
        (
            ProofPlatform::LinuxWayland,
            ProofBrowser::Edge,
            "microsoft-edge",
        ),
        (ProofPlatform::Macos, ProofBrowser::Chrome, "Google Chrome"),
        (ProofPlatform::Macos, ProofBrowser::Edge, "Microsoft Edge"),
        (ProofPlatform::Windows, ProofBrowser::Chrome, "chrome.exe"),
        (ProofPlatform::Windows, ProofBrowser::Edge, "msedge.exe"),
    ];
    for (platform, browser, parent) in cases {
        let exe = if platform == ProofPlatform::Windows {
            r"C:\gcu\gcu.exe"
        } else {
            "/usr/bin/gcu"
        };
        let launch = snapshot(parent, vec![origin()], exe);
        let expected = expect_exe(exe);
        let checked = check_launch(&identity, &origin(), platform, browser, &launch, &expected)
            .unwrap_or_else(|_| panic!("parent {parent}"));
        assert_eq!(checked.foreground_hwnd, None);
        assert!(!mapping_verdict(platform, browser).admits_l2());
    }
    let flagged = snapshot(
        "chrome.exe",
        vec![origin(), "--parent-window=65536".into()],
        r"C:\gcu\gcu.exe",
    );
    let win_expected = expect_exe(r"C:\gcu\gcu.exe");
    let checked = check_launch(
        &identity,
        &origin(),
        ProofPlatform::Windows,
        ProofBrowser::Chrome,
        &flagged,
        &win_expected,
    )
    .unwrap();
    assert_eq!(checked.foreground_hwnd, Some(65536));
    let mut gate = open_with(
        ProofPlatform::Windows,
        ProofBrowser::Chrome,
        &flagged,
        &one_engine(),
    )
    .expect("windows launch shape");
    gate.share(document()).unwrap();
    let mut bound = claim();
    bound.os_window = Some(OsWindow::Win32 { hwnd: 65536 });
    let outcome = gate.evaluate(&bound, &[session("engine-a", "window-1", true)]);
    assert_eq!(reason(&outcome), L2Refusal::WindowUnproven);
    assert_eq!(
        outcome.refused_session.as_deref(),
        Some("session-engine-a-window-1")
    );
    let mut wrong = bound.clone();
    wrong.os_window = Some(OsWindow::Win32 { hwnd: 99 });
    assert_eq!(
        reason(&gate.evaluate(&wrong, &[session("engine-a", "window-1", true)])),
        L2Refusal::WrongWindow
    );
    let zero = snapshot(
        "chrome.exe",
        vec![origin(), "--parent-window=0".into()],
        r"C:\gcu\gcu.exe",
    );
    assert_eq!(
        check_launch(
            &identity,
            &origin(),
            ProofPlatform::Windows,
            ProofBrowser::Chrome,
            &zero,
            &win_expected
        ),
        Err(L2Refusal::FakeRelay)
    );
    let linux_flag = snapshot(
        "chrome",
        vec![origin(), "--parent-window=65536".into()],
        "/usr/bin/gcu",
    );
    assert_eq!(
        check_launch(
            &identity,
            &origin(),
            ProofPlatform::LinuxX11,
            ProofBrowser::Chrome,
            &linux_flag,
            &expect_exe("/usr/bin/gcu")
        ),
        Err(L2Refusal::FakeRelay)
    );
}

#[test]
fn mapping_evidence_is_unproven_and_real_browsers_are_not_run() {
    for platform in [
        ProofPlatform::LinuxX11,
        ProofPlatform::LinuxWayland,
        ProofPlatform::Windows,
        ProofPlatform::Macos,
    ] {
        for browser in [ProofBrowser::Chrome, ProofBrowser::Edge] {
            assert!(matches!(
                mapping_verdict(platform, browser),
                MappingVerdict::Unproven(_)
            ));
        }
    }
    assert!(!REAL_MACHINE_EVIDENCE.is_empty());
    assert!(REAL_MACHINE_EVIDENCE
        .iter()
        .all(|(_, status)| *status == "not_run"));
}

#[test]
fn unique_session_stays_l0_and_similarity_is_not_proof() {
    let mut gate = chrome_linux();
    gate.share(document()).unwrap();
    let only = session("engine-a", "window-1", true);
    assert_eq!(
        select_session("engine-a", 1, std::slice::from_ref(&only))
            .unwrap()
            .session_id,
        only.session_id
    );
    let outcome = gate.evaluate(&claim(), &[only]);
    assert_eq!(reason(&outcome), L2Refusal::WindowUnproven);
    assert_eq!(claim().similarity_discarded(), (100, 100));
    assert!(outcome.uncertain.is_empty());
    assert!(outcome.refused_session.is_some());
}

#[test]
fn ambiguous_sessions_and_foreign_engines_are_refused() {
    let mut gate = chrome_linux();
    gate.share(document()).unwrap();
    let two = [
        session("engine-a", "window-1", true),
        session("engine-a", "window-2", true),
    ];
    assert_eq!(
        select_session("engine-a", 1, &two),
        Err(L2Refusal::AmbiguousSession)
    );
    assert_eq!(
        reason(&gate.evaluate(&claim(), &two)),
        L2Refusal::AmbiguousSession
    );
    let foreign = [
        session("engine-a", "window-1", true),
        session("engine-b", "window-9", true),
    ];
    assert_eq!(
        reason(&gate.evaluate(&claim(), &foreign)),
        L2Refusal::AmbiguousEngine
    );
    assert_eq!(
        reason(&gate.evaluate(&claim(), &[session("engine-a", "window-1", false)])),
        L2Refusal::UnauthorizedSession
    );
    assert_eq!(
        reason(&gate.evaluate(&claim(), &[])),
        L2Refusal::UnauthorizedSession
    );
    let mut other_window = claim();
    other_window.core_window_id = "window-2".into();
    assert_eq!(
        reason(&gate.evaluate(&other_window, &[session("engine-a", "window-1", true)])),
        L2Refusal::WrongWindow
    );
}

#[test]
fn unshared_wrong_tab_window_and_document_are_refused() {
    let mut gate = chrome_linux();
    assert_eq!(
        reason(&gate.evaluate(&claim(), &[session("engine-a", "window-1", true)])),
        L2Refusal::Unshared
    );
    let mut child = document();
    child.frame_id = 4;
    assert_eq!(gate.share(child), Err(L2Refusal::WrongDocument));
    gate.share(document()).unwrap();
    assert_eq!(gate.share(document()), Err(L2Refusal::AmbiguousSession));
    let sessions = [session("engine-a", "window-1", true)];
    let mut tab = claim();
    tab.tab_id = 8;
    assert_eq!(reason(&gate.evaluate(&tab, &sessions)), L2Refusal::WrongTab);
    let mut document_id = claim();
    document_id.document_id = "document-2".into();
    assert_eq!(
        reason(&gate.evaluate(&document_id, &sessions)),
        L2Refusal::WrongDocument
    );
    let mut window = claim();
    window.browser_window_id = 9;
    assert_eq!(
        reason(&gate.evaluate(&window, &sessions)),
        L2Refusal::WrongWindow
    );
    let mut xid = claim();
    xid.os_window = Some(OsWindow::X11 { xid: 0 });
    assert_eq!(
        reason(&gate.evaluate(&xid, &sessions)),
        L2Refusal::WrongWindow
    );
    let mut other_share = claim();
    other_share.share_id = "share-2".into();
    assert_eq!(
        reason(&gate.evaluate(&other_share, &sessions)),
        L2Refusal::Unshared
    );
}

#[test]
fn stale_epoch_late_response_disconnect_and_revoke_do_not_replay() {
    let mut gate = chrome_linux();
    gate.share(document()).unwrap();
    let mut stale = claim();
    stale.epoch = 0;
    assert_eq!(
        reason(&gate.evaluate(&stale, &[session("engine-a", "window-1", true)])),
        L2Refusal::StaleEpoch
    );
    let late = gate.late_response(1, "native-9");
    assert_eq!(reason(&late), L2Refusal::LateResponse);
    assert!(late.uncertain.is_empty());
    let stale_reply = gate.late_response(2, "native-9");
    assert_eq!(reason(&stale_reply), L2Refusal::StaleEpoch);

    let mut connection = consented_connection();
    connection.bind("share-1", "session-a").unwrap();
    let request = connection
        .request(
            "session-a",
            "share-1",
            json!({"type":"click","snapshotId":"snapshot-1","nodeId":"node-1"}),
            &Allow,
        )
        .unwrap();
    let request_id = request["request"]["requestId"].as_str().unwrap().to_owned();
    let cancelled = connection.disconnect();
    let ended = gate.absorb_cancel(CancelKind::Disconnect, &cancelled);
    assert_eq!(reason(&ended), L2Refusal::Disconnected);
    assert_eq!(ended.uncertain.len(), 1);
    assert_eq!(ended.uncertain[0].request_id, request_id);
    assert_eq!(ended.uncertain[0].session_id, "session-a");
    let again = gate.late_response(1, &request_id);
    assert_eq!(reason(&again), L2Refusal::Disconnected);
    assert!(again.uncertain.is_empty());
    assert!(connection
        .request("session-a", "share-1", json!({"type":"observe"}), &gate)
        .is_err());

    let mut gate = chrome_linux();
    gate.share(document()).unwrap();
    let mut connection = consented_connection();
    connection.bind("share-1", "session-a").unwrap();
    let _ = connection
        .request(
            "session-a",
            "share-1",
            json!({"type":"set_value","snapshotId":"snapshot-1","nodeId":"node-1","value":"x"}),
            &Allow,
        )
        .unwrap();
    let revoked = connection.revoke("share-1");
    let outcome = gate.revoke("share-1", &revoked);
    assert_eq!(reason(&outcome), L2Refusal::Revoked);
    assert_eq!(outcome.uncertain.len(), 1);
    assert_eq!(
        reason(&gate.evaluate(&claim(), &[session("engine-a", "window-1", true)])),
        L2Refusal::Revoked
    );
    assert!(gate.request_denied_for_share());
    let after = gate.late_response(gate.epoch(), "native-1");
    assert_eq!(reason(&after), L2Refusal::LateResponse);
    assert!(after.uncertain.is_empty());
}

struct Allow;
impl RouterGate for Allow {
    fn authorize(&self, _: &str, _: &Scope, _: &Value) -> bool {
        true
    }
}

fn consented_connection() -> Connection {
    let identity = embedded();
    let mut connection = Connection::new(&identity, &identity.origin()).unwrap();
    connection
        .receive(json!({
            "kind": "ready",
            "protocol": 1,
            "connectionId": connection.id()
        }))
        .unwrap();
    connection
        .receive(json!({
            "kind": "shared",
            "connectionId": connection.id(),
            "scope": {
                "tabId": 7,
                "frameId": 0,
                "documentId": "document-1",
                "shareId": "share-1"
            }
        }))
        .unwrap();
    connection
}

trait DeniedProbe {
    fn request_denied_for_share(&self) -> bool;
}
impl DeniedProbe for BrokerGate {
    fn request_denied_for_share(&self) -> bool {
        let identity = embedded();
        let mut connection = Connection::new(&identity, &identity.origin()).unwrap();
        connection
            .receive(json!({
                "kind": "ready",
                "protocol": 1,
                "connectionId": connection.id()
            }))
            .unwrap();
        connection
            .receive(json!({
                "kind": "shared",
                "connectionId": connection.id(),
                "scope": {
                    "tabId": 7,
                    "frameId": 0,
                    "documentId": "document-1",
                    "shareId": "share-1"
                }
            }))
            .unwrap();
        connection.bind("share-1", "session-a").unwrap();
        matches!(
            connection.request("session-a", "share-1", json!({"type": "observe"}), self),
            Err(gcu_cli::native_host::Error::Denied)
        )
    }
}

#[test]
fn refused_proof_does_not_authorize_a_dom_request() {
    let mut gate = chrome_linux();
    gate.share(document()).unwrap();
    let outcome = gate.evaluate(&claim(), &[session("engine-a", "window-1", true)]);
    assert_eq!(reason(&outcome), L2Refusal::WindowUnproven);
    assert!(gate.request_denied_for_share());
}

#[test]
fn recorded_live_process_is_not_a_browser_launch() {
    match record_current() {
        Ok(launch) => {
            assert_eq!(launch.host_pid, std::process::id());
            assert_ne!(launch.host_instance, 0);
            assert_eq!(launch.host_build_id, build_id());
            assert!(launch.host_exe.starts_with('/'));
            let refusal = open_with(
                ProofPlatform::LinuxX11,
                ProofBrowser::Chrome,
                &launch,
                &one_engine(),
            )
            .expect_err("this process is not a Chrome native-messaging launch");
            assert!(matches!(
                refusal,
                L2Refusal::FakeRelay | L2Refusal::Identity | L2Refusal::BuildMismatch
            ));
        }
        Err(RecordFault::Unavailable) => {
            // Non-Linux targets have no reviewed reader. That is not a pass.
        }
        Err(RecordFault::Unreadable) => panic!("procfs identity was unreadable"),
    }
}
