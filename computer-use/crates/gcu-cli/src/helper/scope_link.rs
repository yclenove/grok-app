//! Scoped helper admission. Authority is one private bootstrap, not the
//! ordinary helper socket.
//!
//! A real Tart launch, TCC grant, and inherited file descriptor are not_run.
//! The in-process entry still refuses an ordinary frame and a missing frame,
//! and it never calls the ordinary helper.

use super::auth::CodeSignature;
use super::identity;
use crate::exit::Failure;
use crate::scope::{release_unbound, run_ready, AckEncoder, DataPlane, ReadyLink};
use gcu_driver_macos::{ProcessFacts, ScriptedFacts};
use gcu_host_protocol::{
    codec,
    scope::{Ack, Control, Scope, MAX_CONTROL_FRAME, VERSION},
    transport::BoxStream,
    Error,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;

/// Process instance carried in the scoped handshake. A path is not included.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PeerWire {
    pid: u32,
    os_start_time: u64,
    boot_identity: String,
}

/// Code identity compared with [`CodeSignature`]. No path is stored.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CodeWire {
    identifier: String,
    team_id: Option<String>,
    cdhash: String,
}

/// First frame on the scoped control stream. Later frames are [`Control`].
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ScopeHello {
    version: u32,
    peer: PeerWire,
    code: CodeWire,
    run_id: String,
    generation: u64,
    scope_digest: String,
    scope: Scope,
}

/// ACK the helper writes. It is not the bare S3 record.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ScopedAck {
    run_id: String,
    generation: u64,
    scope_digest: String,
    peer: PeerWire,
    code: CodeWire,
}

/// Identities the two sides already agree on, then re-read from facts.
#[derive(Clone)]
struct ScopedIdentities {
    supervisor: PeerWire,
    helper: PeerWire,
    supervisor_code: CodeWire,
    helper_code: CodeWire,
}

/// Facts, expected identities, and the data latch for one helper session.
struct HelperGate {
    facts: Arc<dyn ProcessFacts>,
    expected: ScopedIdentities,
    plane: DataPlane,
}

enum Class {
    Ordinary,
    Scoped(Box<ScopeHello>),
    Unknown,
}

/// `gcu scoped-agent`. Real FD handoff is not_run. A missing or ordinary frame
/// is [`Failure`] `ScopeDenied`. This does not start the ordinary helper.
///
/// The closed stream below is not an inherited descriptor. Blocking waits stay
/// inside the helper's five-second read. Dropping the future revokes.
pub(crate) async fn run_scoped_agent() -> Result<u8, Failure> {
    if !identity::identities_are_distinct(&std::env::temp_dir(), identity::local_uid())
        || !rejects_ordinary_frame()
    {
        return Err(Failure::arguments("ScopeDenied"));
    }
    let driver = gcu_driver_fake::FakeDriver::from_scene("form")
        .map_err(|_| Failure::arguments("ScopeDenied"))?;
    let (local, remote) = tokio::io::duplex(64);
    drop(remote);
    run_helper(
        Box::new(local),
        driver,
        HelperGate {
            facts: Arc::new(ScriptedFacts::new()),
            expected: closed_identities(),
            plane: DataPlane::new(),
        },
        |_lease, _children| async { Err(Failure::arguments("ScopeDenied")) },
    )
    .await
}

/// Admit one scoped session on `control`. The first frame is classified here.
/// Later frames use the S3 state machine. The data latch opens only after a
/// flushed ACK. A rejected hello releases input and does not serve.
async fn run_helper<F, D>(
    mut control: BoxStream,
    driver: gcu_driver_fake::FakeDriver,
    gate: HelperGate,
    serve: F,
) -> Result<u8, Failure>
where
    F: FnOnce(gcu_core::scope::ScopeLease, crate::script_children::Children) -> D,
    D: std::future::Future<Output = Result<(), Failure>>,
{
    let plane = gate.plane.clone();
    let value = match tokio::time::timeout(
        std::time::Duration::from_secs(5),
        codec::read_frame_limited::<_, Value>(&mut control, MAX_CONTROL_FRAME),
    )
    .await
    {
        Ok(Ok(Some(value))) => value,
        _ => return release_unbound(driver, plane).await,
    };
    let hello = match classify(&value) {
        Class::Scoped(hello) => *hello,
        Class::Ordinary | Class::Unknown => return release_unbound(driver, plane).await,
    };
    if hello.version != VERSION || admit(&hello, &gate.expected, gate.facts.as_ref()).is_err() {
        return release_unbound(driver, plane).await;
    }
    let facts = gate.facts;
    let expected = gate.expected;
    let peer_facts = Arc::clone(&facts);
    let peer_expected = expected.clone();
    let peer = move || recheck(&peer_expected, peer_facts.as_ref());
    let encoder = ack_encoder(facts, expected.clone());
    run_ready(
        control,
        driver,
        serve,
        ReadyLink {
            install: install_from_hello(&hello),
            peer,
            encoder,
            plane,
        },
    )
    .await
}

/// True when `bytes` is a scoped handshake. The ordinary helper must not
/// serve it. Memory only. No timeout or cancellation.
#[cfg(any(unix, test))]
pub(crate) fn ordinary_must_refuse(bytes: &[u8]) -> bool {
    let Ok(value) = serde_json::from_slice::<Value>(bytes) else {
        return false;
    };
    matches!(classify(&value), Class::Scoped(_))
}

fn rejects_ordinary_frame() -> bool {
    let value = serde_json::json!({
        "kind": "hello",
        "version": "0",
        "role": "cli",
        "profile": "default",
        "turn_idle_secs": 45
    });
    matches!(classify(&value), Class::Ordinary)
}

fn classify(value: &Value) -> Class {
    if value.get("kind").is_some() {
        return Class::Ordinary;
    }
    match serde_json::from_value::<ScopeHello>(value.clone()) {
        Ok(hello) => Class::Scoped(Box::new(hello)),
        Err(_) => Class::Unknown,
    }
}

fn admit(
    hello: &ScopeHello,
    expected: &ScopedIdentities,
    facts: &dyn ProcessFacts,
) -> Result<(), Error> {
    if hello.peer != expected.supervisor {
        return Err(Error::Authentication);
    }
    verify_peer(&hello.peer, &hello.code, facts, &expected.supervisor_code)?;
    verify_peer(
        &expected.helper,
        &expected.helper_code,
        facts,
        &expected.helper_code,
    )
}

fn recheck(expected: &ScopedIdentities, facts: &dyn ProcessFacts) -> Result<(), Error> {
    verify_peer(
        &expected.supervisor,
        &expected.supervisor_code,
        facts,
        &expected.supervisor_code,
    )?;
    verify_peer(
        &expected.helper,
        &expected.helper_code,
        facts,
        &expected.helper_code,
    )
}

fn verify_peer(
    claimed: &PeerWire,
    claimed_code: &CodeWire,
    facts: &dyn ProcessFacts,
    expected_code: &CodeWire,
) -> Result<(), Error> {
    verify_instance(claimed, facts)?;
    verify_code(claimed_code, expected_code)?;
    // The wire claim is not the code that is running. Read it again.
    verify_observed_code(claimed.pid, expected_code, facts)
}

fn verify_instance(claimed: &PeerWire, facts: &dyn ProcessFacts) -> Result<(), Error> {
    if claimed.pid == 0 || claimed.os_start_time == 0 || claimed.boot_identity.is_empty() {
        return Err(Error::Authentication);
    }
    let boot = facts.boot_identity().ok_or(Error::Authentication)?;
    if boot != claimed.boot_identity {
        return Err(Error::Authentication);
    }
    let start = facts
        .process_start_us(claimed.pid)
        .ok_or(Error::Authentication)?;
    if start != claimed.os_start_time {
        Err(Error::Authentication)
    } else {
        Ok(())
    }
}

fn verify_code(claimed: &CodeWire, expected: &CodeWire) -> Result<(), Error> {
    codes_match(
        &claimed.identifier,
        claimed.team_id.as_deref(),
        &claimed.cdhash,
        expected,
    )
}

/// Compare the process's current code, as read from `facts`, with `expected`.
///
/// `None` or an unusable observation is authentication failure. Memory only.
/// No timeout or cancellation. A pathname is not consulted.
fn verify_observed_code(
    pid: u32,
    expected: &CodeWire,
    facts: &dyn ProcessFacts,
) -> Result<(), Error> {
    let observed = facts.code_identity(pid).ok_or(Error::Authentication)?;
    codes_match(
        &observed.identifier,
        observed.team_id.as_deref(),
        &observed.cdhash,
        expected,
    )
}

fn codes_match(
    identifier: &str,
    team_id: Option<&str>,
    cdhash: &str,
    expected: &CodeWire,
) -> Result<(), Error> {
    let claimed_sig =
        CodeSignature::new(identifier, team_id, cdhash).ok_or(Error::Authentication)?;
    let expected_sig = CodeSignature::new(
        &expected.identifier,
        expected.team_id.as_deref(),
        &expected.cdhash,
    )
    .ok_or(Error::Authentication)?;
    if claimed_sig.same_identity(&expected_sig) && claimed_sig.same_cdhash(&expected_sig) {
        Ok(())
    } else {
        Err(Error::Authentication)
    }
}

fn install_from_hello(hello: &ScopeHello) -> Control {
    Control::Install {
        version: hello.version,
        run_id: hello.run_id.clone(),
        generation: hello.generation,
        scope_digest: hello.scope_digest.clone(),
        scope: hello.scope.clone(),
    }
}

fn ack_encoder(facts: Arc<dyn ProcessFacts>, expected: ScopedIdentities) -> AckEncoder {
    Arc::new(move |ack: &Ack| {
        // Encode only after both peers' live code still matches. The body
        // carries that observed helper code, not a copy checked against itself.
        verify_peer(
            &expected.supervisor,
            &expected.supervisor_code,
            facts.as_ref(),
            &expected.supervisor_code,
        )?;
        verify_peer(
            &expected.helper,
            &expected.helper_code,
            facts.as_ref(),
            &expected.helper_code,
        )?;
        let observed = facts
            .code_identity(expected.helper.pid)
            .ok_or(Error::Authentication)?;
        let code = CodeWire {
            identifier: observed.identifier,
            team_id: observed.team_id,
            cdhash: observed.cdhash,
        };
        let body = ScopedAck {
            run_id: ack.run_id.clone(),
            generation: ack.generation,
            scope_digest: ack.scope_digest.clone(),
            peer: expected.helper.clone(),
            code,
        };
        accept_helper_ack(
            &body,
            ack,
            &expected.helper,
            &expected.helper_code,
            facts.as_ref(),
        )?;
        serde_json::to_vec(&body).map_err(|_| Error::Invalid)
    })
}

/// Supervisor check of one scoped ACK. A mismatch is authentication failure.
/// Memory only. No timeout or cancellation.
fn accept_helper_ack(
    body: &ScopedAck,
    expected: &Ack,
    helper: &PeerWire,
    code: &CodeWire,
    facts: &dyn ProcessFacts,
) -> Result<(), Error> {
    if body.run_id != expected.run_id
        || body.generation != expected.generation
        || body.scope_digest != expected.scope_digest
        || body.peer != *helper
    {
        return Err(Error::Authentication);
    }
    verify_peer(&body.peer, &body.code, facts, code)
}

fn closed_identities() -> ScopedIdentities {
    let code = CodeWire {
        identifier: identity::scoped_bundle(cfg!(debug_assertions)).to_string(),
        team_id: None,
        cdhash: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
    };
    ScopedIdentities {
        supervisor: PeerWire {
            pid: 1,
            os_start_time: 1,
            boot_identity: "closed".to_string(),
        },
        helper: PeerWire {
            pid: 2,
            os_start_time: 1,
            boot_identity: "closed".to_string(),
        },
        supervisor_code: code.clone(),
        helper_code: code,
    }
}

#[cfg(test)]
#[path = "scope_link_tests.rs"]
mod link_tests;
