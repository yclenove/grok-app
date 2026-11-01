//! Fake scoped-helper channel. No socket, TCC prompt, or inherited descriptor.
use super::{
    accept_helper_ack, classify, Class, CodeWire, HelperGate, PeerWire, ScopeHello,
    ScopedIdentities,
};
use crate::helper::identity::{
    identities_are_distinct, ordinary_socket, scoped_socket, StoreLedger, StoreRole,
};
use crate::scope::DataPlane;
use gcu_driver_macos::{CodeIdentity, ScriptedFacts};
use gcu_host_protocol::{
    codec,
    scope::{Ack, Control, Scope, LEASE, VERSION},
    Error,
};
use std::future::Future;
use std::pin::Pin;
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc, Mutex,
};
use std::task::{Context, Poll};
use std::time::Duration;
#[cfg(unix)]
use tokio::io::AsyncReadExt;
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt, ReadBuf};

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

fn observed(wire: &CodeWire) -> CodeIdentity {
    CodeIdentity {
        identifier: wire.identifier.clone(),
        team_id: wire.team_id.clone(),
        cdhash: wire.cdhash.clone(),
    }
}

fn primed() -> Arc<ScriptedFacts> {
    let facts = Arc::new(ScriptedFacts::new());
    facts.set_boot(BOOT);
    facts.set_process(11, 1_000);
    facts.set_process(22, 2_000);
    let ids = identities();
    facts.set_code(ids.supervisor.pid, observed(&ids.supervisor_code));
    facts.set_code(ids.helper.pid, observed(&ids.helper_code));
    facts
}

#[derive(Clone, Copy)]
enum LiveFault {
    Cdhash,
    Team,
    Identifier,
    Missing,
}

fn apply_live_fault(facts: &ScriptedFacts, pid: u32, fault: LiveFault) {
    let ids = identities();
    let mut code = if pid == ids.helper.pid {
        observed(&ids.helper_code)
    } else {
        observed(&ids.supervisor_code)
    };
    match fault {
        LiveFault::Cdhash => code.cdhash = "b".repeat(40),
        LiveFault::Team => code.team_id = Some("TEAM123456".into()),
        LiveFault::Identifier => code.identifier = "other.identity".into(),
        LiveFault::Missing => {
            facts.drop_code(pid);
            return;
        }
    }
    facts.set_code(pid, code);
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
    lease: Arc<Mutex<Option<gcu_core::scope::ScopeLease>>>,
}

/// Parks the helper ACK flush so a test can change live code while the write
/// is still outstanding. The first `allow` flushes pass. Later flushes wait
/// until [`FlushHold::release`]. [`FlushHold::fail`] makes the next flush
/// return an IO error instead of parking.
struct FlushHold {
    allow: AtomicUsize,
    held: AtomicBool,
    fail: AtomicBool,
    release: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
}

impl FlushHold {
    fn new(allow_flushes: usize) -> Self {
        Self {
            allow: AtomicUsize::new(allow_flushes),
            held: AtomicBool::new(false),
            fail: AtomicBool::new(false),
            release: Mutex::new(None),
        }
    }

    fn release(&self) {
        if let Some(tx) = self.release.lock().unwrap().take() {
            let _ = tx.send(());
        }
    }

    fn fail(&self) {
        self.fail.store(true, Ordering::SeqCst);
    }
}

enum FlushMode {
    Decide,
    Forward,
    Hold(tokio::sync::oneshot::Receiver<()>),
}

struct HeldFlush<T> {
    inner: T,
    hold: Arc<FlushHold>,
    mode: FlushMode,
}

impl<T: AsyncRead + Unpin> AsyncRead for HeldFlush<T> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_read(cx, buf)
    }
}

impl<T: AsyncWrite + Unpin> AsyncWrite for HeldFlush<T> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.get_mut().inner).poll_write(cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        let this = self.get_mut();
        loop {
            match std::mem::replace(&mut this.mode, FlushMode::Decide) {
                FlushMode::Decide => {
                    if this.hold.fail.load(Ordering::SeqCst) {
                        return Poll::Ready(Err(std::io::Error::new(
                            std::io::ErrorKind::BrokenPipe,
                            "ack flush failed",
                        )));
                    }
                    let parked = this
                        .hold
                        .allow
                        .try_update(Ordering::SeqCst, Ordering::SeqCst, |left| {
                            left.checked_sub(1)
                        })
                        .is_err();
                    if parked {
                        let (tx, rx) = tokio::sync::oneshot::channel();
                        *this.hold.release.lock().unwrap() = Some(tx);
                        this.hold.held.store(true, Ordering::SeqCst);
                        this.mode = FlushMode::Hold(rx);
                    } else {
                        this.mode = FlushMode::Forward;
                    }
                }
                FlushMode::Forward => {
                    let poll = Pin::new(&mut this.inner).poll_flush(cx);
                    this.mode = if poll.is_ready() {
                        FlushMode::Decide
                    } else {
                        FlushMode::Forward
                    };
                    return poll;
                }
                FlushMode::Hold(mut rx) => match Pin::new(&mut rx).poll(cx) {
                    Poll::Pending => {
                        this.mode = FlushMode::Hold(rx);
                        return Poll::Pending;
                    }
                    Poll::Ready(_) => this.mode = FlushMode::Forward,
                },
            }
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_shutdown(cx)
    }
}

fn form_driver() -> gcu_driver_fake::FakeDriver {
    gcu_driver_fake::FakeDriver::from_scene("form").expect("form scene")
}

fn start() -> Ends {
    let (supervisor, helper) = tokio::io::duplex(64 * 1024);
    launch(supervisor, Box::new(helper), form_driver())
}

fn start_held(allow_flushes: usize) -> (Ends, Arc<FlushHold>) {
    start_with(form_driver(), allow_flushes)
}

fn start_with(driver: gcu_driver_fake::FakeDriver, allow_flushes: usize) -> (Ends, Arc<FlushHold>) {
    let (supervisor, helper) = tokio::io::duplex(64 * 1024);
    let hold = Arc::new(FlushHold::new(allow_flushes));
    let stream = HeldFlush {
        inner: helper,
        hold: Arc::clone(&hold),
        mode: FlushMode::Decide,
    };
    (launch(supervisor, Box::new(stream), driver), hold)
}

fn launch(
    supervisor: tokio::io::DuplexStream,
    helper: gcu_host_protocol::transport::BoxStream,
    driver: gcu_driver_fake::FakeDriver,
) -> Ends {
    let facts = primed();
    let plane = DataPlane::new();
    let hits = Arc::new(AtomicUsize::new(0));
    let stores = Arc::new(StoreLedger::new());
    let lease = Arc::new(Mutex::new(None));
    let gate = HelperGate {
        facts: Arc::clone(&facts) as Arc<dyn gcu_driver_macos::ProcessFacts>,
        expected: identities(),
        plane: plane.clone(),
    };
    let serve_plane = plane.clone();
    let serve_hits = Arc::clone(&hits);
    let serve_lease = Arc::clone(&lease);
    let task = tokio::spawn(super::run_helper(
        helper,
        driver.clone(),
        Arc::clone(&stores),
        gate,
        move |scope_lease, _children| {
            *serve_lease.lock().unwrap() = Some(scope_lease);
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
        lease,
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

async fn wait_held(hold: &FlushHold) {
    tokio::time::timeout(Duration::from_secs(2), async {
        while !hold.held.load(Ordering::SeqCst) {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("ack flush parked");
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

#[test]
fn unchanged_ack_wire_is_refused_when_live_code_changes() {
    let faults = [
        LiveFault::Cdhash,
        LiveFault::Team,
        LiveFault::Identifier,
        LiveFault::Missing,
    ];
    for pid in [11, 22] {
        for fault in faults {
            let facts = primed();
            let ids = identities();
            apply_live_fault(&facts, pid, fault);
            let expected = Ack {
                run_id: "run".into(),
                generation: 1,
                scope_digest: scope_record().digest("run", 1).unwrap(),
            };
            let body = super::ScopedAck {
                run_id: expected.run_id.clone(),
                generation: expected.generation,
                scope_digest: expected.scope_digest.clone(),
                peer: ids.helper.clone(),
                code: ids.helper_code.clone(),
            };
            // The supervisor check reads the helper identity named by the body.
            if pid == ids.helper.pid {
                assert!(
                    matches!(
                        accept_helper_ack(
                            &body,
                            &expected,
                            &ids.helper,
                            &ids.helper_code,
                            facts.as_ref()
                        ),
                        Err(Error::Authentication)
                    ),
                    "ack check must read the helper"
                );
            }
            let encoder = super::ack_encoder(
                Arc::clone(&facts) as Arc<dyn gcu_driver_macos::ProcessFacts>,
                ids,
            );
            assert!(
                encoder(&expected).is_err(),
                "ack encode must read pid {pid}"
            );
        }
    }
}

#[tokio::test]
async fn unchanged_wire_live_code_is_refused_at_admit_ack_and_recheck() {
    tokio::time::timeout(Duration::from_secs(20), async {
        let faults = [
            LiveFault::Cdhash,
            LiveFault::Team,
            LiveFault::Identifier,
            LiveFault::Missing,
        ];
        for pid in [11, 22] {
            for fault in faults {
                let mut admitted = start();
                apply_live_fault(&admitted.facts, pid, fault);
                codec::write_frame(&mut admitted.supervisor, &good_hello(1))
                    .await
                    .unwrap();
                finish_never_opened(admitted).await;

                let (mut pending, hold) = start_held(0);
                codec::write_frame(&mut pending.supervisor, &good_hello(1))
                    .await
                    .unwrap();
                wait_held(&hold).await;
                apply_live_fault(&pending.facts, pid, fault);
                tokio::time::sleep(Duration::from_millis(80)).await;
                hold.release();
                finish_never_opened(pending).await;

                let mut live = start();
                codec::write_frame(&mut live.supervisor, &good_hello(1))
                    .await
                    .unwrap();
                let _ = read_ack(&mut live).await;
                apply_live_fault(&live.facts, pid, fault);
                finish_revoked(live).await;
            }
        }
    })
    .await
    .unwrap();
}

fn rebind(generation: u64) -> Control {
    Control::Rebind {
        run_id: "run".into(),
        generation,
        scope: scope_record(),
    }
}

#[test]
fn fresh_accept_uses_the_admitted_generation_gate() {
    let source = include_str!("../scope/data.rs");
    assert!(
        source.contains("admit_data(&plane, &lease)"),
        "a fresh connection must pass the admitted-generation gate"
    );
    assert_eq!(
        source.matches("lease.arm_attempt()").count(),
        2,
        "only the factory startup and admit_data may arm"
    );
}

#[tokio::test]
async fn rebind_data_stays_closed_until_the_ack_flushes() {
    tokio::time::timeout(Duration::from_secs(5), async {
        let (mut ends, hold) = start_held(1);
        codec::write_frame(&mut ends.supervisor, &good_hello(1))
            .await
            .unwrap();
        let first = read_ack(&mut ends).await;
        assert_eq!(first.generation, 1);
        assert!(ends.plane.is_open());
        codec::write_frame(&mut ends.supervisor, &rebind(2))
            .await
            .unwrap();
        wait_held(&hold).await;
        let lease = ends
            .lease
            .lock()
            .unwrap()
            .clone()
            .expect("data future holds the published lease");
        assert_eq!(lease.current().expect("gen 2 is published").generation(), 2);
        let effects = ends.plane.effects();
        assert!(
            crate::scope::admit_data(&ends.plane, &lease).is_err(),
            "gen 2 is not data authority before its ACK flushes"
        );
        assert_eq!(ends.plane.effects(), effects, "a refusal is not an effect");
        assert!(
            ends.plane.request().is_err(),
            "the previous generation's latch must be suspended"
        );
        assert_eq!(lease.arm_attempt().unwrap().scope().generation(), 2);
        ends.facts.drop_process(11);
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let published = ends.lease.lock().unwrap().clone();
                let gone = published
                    .as_ref()
                    .is_some_and(|item| item.current().is_none());
                if gone && ends.driver.release_calls() > 0 {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("peer loss during the ack write revokes and releases input");
        finish_revoked(ends).await;
    })
    .await
    .unwrap();
}

async fn held_keys() -> gcu_driver_fake::FakeDriver {
    use gcu_core::driver::{Cancel, Driver, DriverAction, KeyRoute};
    let driver = form_driver();
    let win = driver.fixture_window("fake.form").await.unwrap();
    driver
        .inject_action(gcu_driver_fake::faults::Fault::Crash {
            maybe_executed: true,
        })
        .await
        .unwrap();
    let req = DriverAction::PressKeys {
        win,
        chord: gcu_protocol::KeyChord::parse("ctrl+a").unwrap(),
        route: KeyRoute::TargetedOnly,
    };
    assert!(driver.perform(&req, &Cancel::default()).await.is_err());
    assert_ne!(driver.oracle().await.unwrap().held_input, 0);
    driver
}

#[tokio::test]
async fn rebind_ack_flush_failure_releases_held_input() {
    tokio::time::timeout(Duration::from_secs(5), async {
        let driver = held_keys().await;
        let observed = driver.clone();
        let (mut ends, hold) = start_with(driver, 1);
        codec::write_frame(&mut ends.supervisor, &good_hello(1))
            .await
            .unwrap();
        let _ = read_ack(&mut ends).await;
        hold.fail();
        codec::write_frame(&mut ends.supervisor, &rebind(2))
            .await
            .unwrap();
        finish_revoked(ends).await;
        assert_eq!(observed.oracle().await.unwrap().held_input, 0);
    })
    .await
    .unwrap();
}
