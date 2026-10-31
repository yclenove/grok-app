//! Fake scoped-helper channel. No socket, TCC prompt, or inherited descriptor.
use super::{
    accept_helper_ack, classify, Class, CodeWire, HelperGate, PeerWire, ScopeHello,
    ScopedIdentities,
};
use crate::helper::identity::{
    identities_are_distinct, ordinary_socket, scoped_socket, StoreLedger, StoreRole,
};
use crate::scope::DataPlane;
use gcu_driver_macos::ScriptedFacts;
use gcu_host_protocol::{
    codec,
    scope::{Ack, Control, Scope, LEASE, VERSION},
    Error,
};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const BOOT: &str = "boot-session";
const CDHASH: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn code(identifier: &str) -> CodeWire {
    CodeWire {
        identifier: identifier.to_string(),
        team_id: None,
        cdhash: CDHASH.to_string(),
    }
}

fn peer(pid: u32, start: u64) -> PeerWire {
    PeerWire {
        pid,
        os_start_time: start,
        boot_identity: BOOT.to_string(),
    }
}

fn identities() -> ScopedIdentities {
    ScopedIdentities {
        supervisor: peer(11, 1_000),
        helper: peer(22, 2_000),
        supervisor_code: code("ai.grok.computer-use.scope-supervisor"),
        helper_code: code("ai.grok.computer-use.scoped-helper.dev"),
    }
}

fn primed() -> Arc<ScriptedFacts> {
    let facts = Arc::new(ScriptedFacts::new());
    facts.set_boot(BOOT);
    facts.set_process(11, 1_000);
    facts.set_process(22, 2_000);
    facts
}

fn scope_record() -> Scope {
    Scope {
        engine_instance: "engine".into(),
        desktop_session: "desktop".into(),
        control_lease: "lease".into(),
        targets: vec![],
    }
}

fn hello(generation: u64, digest: String) -> ScopeHello {
    let ids = identities();
    ScopeHello {
        version: VERSION,
        peer: ids.supervisor.clone(),
        code: ids.supervisor_code.clone(),
        run_id: "run".into(),
        generation,
        scope_digest: digest,
        scope: scope_record(),
    }
}

fn good_hello(generation: u64) -> ScopeHello {
    let digest = scope_record().digest("run", generation).unwrap();
    hello(generation, digest)
}

struct Ends {
    supervisor: tokio::io::DuplexStream,
    task: tokio::task::JoinHandle<Result<u8, crate::exit::Failure>>,
    plane: DataPlane,
    driver: gcu_driver_fake::FakeDriver,
    facts: Arc<ScriptedFacts>,
    hits: Arc<AtomicUsize>,
    stores: Arc<StoreLedger>,
}

fn start() -> Ends {
    let (supervisor, helper) = tokio::io::duplex(64 * 1024);
    let facts = primed();
    let plane = DataPlane::new();
    let hits = Arc::new(AtomicUsize::new(0));
    let stores = Arc::new(StoreLedger::new());
    let driver = gcu_driver_fake::FakeDriver::from_scene("form").expect("form scene");
    let gate = HelperGate {
        facts: Arc::clone(&facts) as Arc<dyn gcu_driver_macos::ProcessFacts>,
        expected: identities(),
        plane: plane.clone(),
    };
    let serve_plane = plane.clone();
    let serve_hits = Arc::clone(&hits);
    let task = tokio::spawn(super::run_helper(
        Box::new(helper),
        driver.clone(),
        Arc::clone(&stores),
        gate,
        move |_lease, _children| {
            let serve_plane = serve_plane.clone();
            let serve_hits = Arc::clone(&serve_hits);
            async move {
                if serve_plane.request().is_ok() {
                    serve_hits.fetch_add(1, Ordering::SeqCst);
                }
                std::future::pending().await
            }
        },
    ));
    Ends {
        supervisor,
        task,
        plane,
        driver,
        facts,
        hits,
        stores,
    }
}

async fn finish(ends: Ends, never_opened: bool) {
    let result = tokio::time::timeout(Duration::from_secs(2), ends.task)
        .await
        .expect("helper finished")
        .expect("helper task");
    let failure = result.expect_err("a refused session must not succeed");
    assert_eq!(failure.message, "ScopeDenied");
    assert!(!ends.plane.is_open());
    assert!(ends.plane.request().is_err());
    if never_opened {
        assert_eq!(ends.plane.effects(), 0);
        assert_eq!(ends.hits.load(Ordering::SeqCst), 0);
    }
    assert!(ends.driver.release_calls() > 0);
    assert_eq!(ends.stores.ordinary_total(), 0);
    assert!(ends.stores.scoped_total() >= 3);
}

async fn finish_never_opened(ends: Ends) {
    finish(ends, true).await;
}

async fn finish_revoked(ends: Ends) {
    finish(ends, false).await;
}

async fn read_ack(ends: &mut Ends) -> super::ScopedAck {
    let ack: super::ScopedAck = tokio::time::timeout(
        Duration::from_secs(2),
        codec::read_frame(&mut ends.supervisor),
    )
    .await
    .expect("ack wait")
    .expect("ack read")
    .expect("ack frame");
    let ids = identities();
    let expected = Ack {
        run_id: "run".into(),
        generation: ack.generation,
        scope_digest: ack.scope_digest.clone(),
    };
    accept_helper_ack(
        &ack,
        &expected,
        &ids.helper,
        &ids.helper_code,
        ends.facts.as_ref(),
    )
    .expect("supervisor accepts the helper ack");
    tokio::time::timeout(Duration::from_secs(2), async {
        while !ends.plane.is_open() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("data plane opened after ack");
    ack
}

#[test]
fn identities_do_not_share_socket_or_state_roots() {
    let temp = std::env::temp_dir();
    assert!(identities_are_distinct(&temp, 42));
    assert_ne!(ordinary_socket(&temp, 42), scoped_socket(&temp, 42));
    let ledger = StoreLedger::new();
    ledger.touch(StoreRole::Ordinary, false);
    ledger.touch(StoreRole::Ordinary, true);
    assert_eq!(ledger.ordinary_total(), 2);
    assert_eq!(ledger.scoped_total(), 0);
}

#[tokio::test]
async fn ordinary_and_scoped_handshakes_reject_each_other() {
    tokio::time::timeout(Duration::from_secs(5), async {
        let mut ends = start();
        assert!(ends.plane.request().is_err());
        assert_eq!(ends.plane.effects(), 0);
        let ordinary = serde_json::json!({
            "kind": "hello",
            "version": "0.0.0",
            "role": "cli",
            "profile": "default",
            "turn_idle_secs": 45
        });
        codec::write_frame(&mut ends.supervisor, &ordinary)
            .await
            .unwrap();
        finish_never_opened(ends).await;

        let scoped = serde_json::to_vec(&good_hello(1)).unwrap();
        assert!(super::ordinary_must_refuse(&scoped));
        let ordinary_bytes = serde_json::to_vec(&ordinary).unwrap();
        assert!(!super::ordinary_must_refuse(&ordinary_bytes));
        let value: serde_json::Value = serde_json::from_slice(&ordinary_bytes).unwrap();
        assert!(matches!(classify(&value), Class::Ordinary));
    })
    .await
    .unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn ordinary_frame_reader_rejects_a_scoped_handshake() {
    tokio::time::timeout(Duration::from_secs(5), async {
        let (mut reader, mut writer) = tokio::io::duplex(4096);
        let bytes = serde_json::to_vec(&good_hello(1)).unwrap();
        writer
            .write_all(&(bytes.len() as u32).to_be_bytes())
            .await
            .unwrap();
        writer.write_all(&bytes).await.unwrap();
        writer.flush().await.unwrap();
        drop(writer);
        let error = crate::helper::frame::read_frame(&mut reader, 4096)
            .await
            .expect_err("scoped handshake is not an ordinary frame");
        assert!(matches!(error, crate::helper::HelperError::Frame));
        let mut leftover = [0u8; 1];
        assert_eq!(reader.read(&mut leftover).await.unwrap(), 0);
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn missing_field_unknown_version_and_bad_digest_revoke() {
    tokio::time::timeout(Duration::from_secs(5), async {
        let mut missing = start();
        assert!(missing.plane.request().is_err());
        let bytes = br#"{"version":1,"run_id":"run"}"#;
        missing
            .supervisor
            .write_all(&(bytes.len() as u32).to_be_bytes())
            .await
            .unwrap();
        missing.supervisor.write_all(bytes).await.unwrap();
        missing.supervisor.flush().await.unwrap();
        finish_never_opened(missing).await;

        let mut version = start();
        let mut bad = good_hello(1);
        bad.version = VERSION + 1;
        codec::write_frame(&mut version.supervisor, &bad)
            .await
            .unwrap();
        finish_never_opened(version).await;

        let mut digest = start();
        codec::write_frame(&mut digest.supervisor, &hello(1, "not-the-digest".into()))
            .await
            .unwrap();
        finish_never_opened(digest).await;
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn old_generation_replay_and_code_mismatch_revoke() {
    tokio::time::timeout(Duration::from_secs(5), async {
        let mut old = start();
        codec::write_frame(&mut old.supervisor, &good_hello(0))
            .await
            .unwrap();
        finish_never_opened(old).await;

        let mut replay = start();
        codec::write_frame(&mut replay.supervisor, &good_hello(1))
            .await
            .unwrap();
        let ack = read_ack(&mut replay).await;
        assert_eq!(ack.generation, 1);
        assert!(replay.plane.request().is_ok());
        assert!(replay.plane.effects() >= 1);
        let again = Control::Install {
            version: VERSION,
            run_id: "run".into(),
            generation: 1,
            scope_digest: scope_record().digest("run", 1).unwrap(),
            scope: scope_record(),
        };
        codec::write_frame(&mut replay.supervisor, &again)
            .await
            .unwrap();
        finish_revoked(replay).await;

        let mut code = start();
        let mut forged = good_hello(1);
        forged.code.cdhash = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".into();
        codec::write_frame(&mut code.supervisor, &forged)
            .await
            .unwrap();
        finish_never_opened(code).await;
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn stale_rebind_is_revoked() {
    tokio::time::timeout(Duration::from_secs(5), async {
        let mut ends = start();
        codec::write_frame(&mut ends.supervisor, &good_hello(1))
            .await
            .unwrap();
        let _ = read_ack(&mut ends).await;
        let rebind = Control::Rebind {
            run_id: "run".into(),
            generation: 1,
            scope: scope_record(),
        };
        codec::write_frame(&mut ends.supervisor, &rebind)
            .await
            .unwrap();
        finish_revoked(ends).await;
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn control_eof_revokes_and_releases_input() {
    tokio::time::timeout(Duration::from_secs(5), async {
        let mut ends = start();
        assert!(ends.plane.request().is_err());
        assert_eq!(ends.plane.effects(), 0);
        codec::write_frame(&mut ends.supervisor, &good_hello(1))
            .await
            .unwrap();
        let _ = read_ack(&mut ends).await;
        assert!(ends.hits.load(Ordering::SeqCst) >= 1 || ends.plane.is_open());
        ends.supervisor.shutdown().await.unwrap();
        finish_revoked(ends).await;
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn supervisor_death_and_instance_change_revoke() {
    tokio::time::timeout(Duration::from_secs(5), async {
        let mut dead = start();
        codec::write_frame(&mut dead.supervisor, &good_hello(1))
            .await
            .unwrap();
        let _ = read_ack(&mut dead).await;
        dead.facts.drop_process(11);
        finish_revoked(dead).await;

        let mut changed = start();
        codec::write_frame(&mut changed.supervisor, &good_hello(1))
            .await
            .unwrap();
        let _ = read_ack(&mut changed).await;
        changed.facts.set_process(22, 9_999);
        finish_revoked(changed).await;
    })
    .await
    .unwrap();
}

#[tokio::test(start_paused = true)]
async fn lease_expiry_revokes_and_releases_input() {
    tokio::time::timeout(Duration::from_secs(10), async {
        let mut ends = start();
        assert!(ends.plane.request().is_err());
        codec::write_frame(&mut ends.supervisor, &good_hello(1))
            .await
            .unwrap();
        let _ = read_ack(&mut ends).await;
        tokio::time::advance(LEASE + Duration::from_millis(1)).await;
        finish_revoked(ends).await;
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn scoped_agent_without_an_inherited_descriptor_is_denied() {
    tokio::time::timeout(Duration::from_secs(6), async {
        let failure = super::run_scoped_agent()
            .await
            .expect_err("closed bootstrap is not a session");
        assert_eq!(failure.message, "ScopeDenied");
    })
    .await
    .unwrap();
}

#[test]
fn ack_mismatch_is_authentication_failure() {
    let facts = primed();
    let ids = identities();
    let expected = Ack {
        run_id: "run".into(),
        generation: 1,
        scope_digest: scope_record().digest("run", 1).unwrap(),
    };
    let mut body = super::ScopedAck {
        run_id: expected.run_id.clone(),
        generation: expected.generation,
        scope_digest: expected.scope_digest.clone(),
        peer: ids.helper.clone(),
        code: ids.helper_code.clone(),
    };
    assert!(accept_helper_ack(
        &body,
        &expected,
        &ids.helper,
        &ids.helper_code,
        facts.as_ref()
    )
    .is_ok());
    body.scope_digest = "other".into();
    assert!(matches!(
        accept_helper_ack(
            &body,
            &expected,
            &ids.helper,
            &ids.helper_code,
            facts.as_ref()
        ),
        Err(Error::Authentication)
    ));
}
