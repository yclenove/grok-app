//! Closed JSON objects. Field order is the struct order. No extra keys.

use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;

use super::super::guest::GUEST_CHANNEL_VERSION;
use super::ids::{digest_hex, parse_digest, AsciiId, AttemptNonce};
use super::ops::{
    Activity, Envelope, InventoryConsume, InventorySnapshot, LeaseRequest, LeaseTerminal,
    LoginState, MailboxReply, MsgStatus, Op, Outcome, Payload, ReplyResult, ReplyState,
    RevokeOutcome, RevokeSlot, RunScore, SideOp, SideResult, SnapStatus, ValueOutcome,
};
use super::CodecFault;
use crate::disposable::{Check, Probe};

pub(super) fn encode_envelope(env: &Envelope) -> Result<Vec<u8>, CodecFault> {
    env.validate()?;
    let wire = OutEnv {
        v: env.v,
        run_id: env.run_id.as_str(),
        attempt_nonce: env.attempt_nonce.to_hex(),
        vm_config_digest: digest_hex(env.vm_config_digest),
        boot_id: env.boot_id.as_ref().map(AsciiId::as_str),
        op: env.op,
        seq: env.seq,
        request_seq: env.request_seq,
        status: env.status,
        payload: payload_to(env)?,
    };
    serde_json::to_vec(&wire).map_err(|_| CodecFault::Schema)
}

pub(super) fn decode_envelope(json: &str) -> Result<Envelope, CodecFault> {
    let raw: RawEnv = serde_json::from_str(json).map_err(map_serde)?;
    if raw.v != u64::from(GUEST_CHANNEL_VERSION) {
        return Err(CodecFault::Version);
    }
    let payload = payload_from(raw.op, raw.status, raw.payload)?;
    let env = Envelope {
        v: GUEST_CHANNEL_VERSION,
        run_id: AsciiId::new(&raw.run_id)?,
        attempt_nonce: AttemptNonce::from_hex(&raw.attempt_nonce)?,
        vm_config_digest: parse_digest(&raw.vm_config_digest)?,
        boot_id: raw.boot_id.as_deref().map(AsciiId::new).transpose()?,
        op: raw.op,
        seq: raw.seq,
        request_seq: raw.request_seq,
        status: raw.status,
        payload,
    };
    env.validate()?;
    Ok(env)
}

pub(super) fn encode_reply(reply: &MailboxReply) -> Result<Vec<u8>, CodecFault> {
    reply.validate()?;
    let wire = ReplyOut {
        state: reply.state,
        op: reply.op,
        request_seq: reply.request_seq,
        result: reply_body(reply)?,
    };
    serde_json::to_vec(&wire).map_err(|_| CodecFault::Schema)
}

pub(super) fn decode_reply(json: &str) -> Result<MailboxReply, CodecFault> {
    let raw: ReplyIn = serde_json::from_str(json).map_err(map_serde)?;
    let result = reply_from(raw.state, raw.op, raw.result)?;
    let reply = MailboxReply {
        state: raw.state,
        op: raw.op,
        request_seq: raw.request_seq,
        result,
    };
    reply.validate()?;
    Ok(reply)
}

fn map_serde(err: serde_json::Error) -> CodecFault {
    if err.to_string().contains("unknown field") {
        CodecFault::Unknown
    } else {
        CodecFault::Schema
    }
}

fn payload_from(op: Op, status: MsgStatus, value: Value) -> Result<Payload, CodecFault> {
    let parsed = match (op, status) {
        (Op::Inventory, MsgStatus::Request) => {
            let wire: InvReq = take(value)?;
            Payload::InventoryRequest(InventoryConsume::new(
                wire.consume_op,
                wire.consume_request_seq,
                wire.consume_revoke_seq,
            )?)
        }
        (Op::Inventory, MsgStatus::Terminal) => Payload::InventoryTerminal(snapshot_from(value)?),
        (Op::Probe, MsgStatus::Request) => Payload::ProbeRequest(take::<ProbeReq>(value)?.probe),
        (Op::Probe, MsgStatus::Terminal) => Payload::ProbeTerminal(take::<ProbeTerm>(value)?.check),
        (
            Op::Bootstrap | Op::Login | Op::Run | Op::Revoke,
            MsgStatus::Request | MsgStatus::Accepted,
        ) => {
            let _empty: Empty = take(value)?;
            Payload::Empty
        }
        (Op::Bootstrap, MsgStatus::Terminal) => Payload::BootstrapTerminal(boot_in(take(value)?)?),
        (Op::Login, MsgStatus::Terminal) => Payload::LoginTerminal(login_in(take(value)?)?),
        (Op::Run, MsgStatus::Terminal) => Payload::RunTerminal(run_in(take(value)?)?),
        (Op::Lease, MsgStatus::Request) => {
            let wire: LeaseReq = take(value)?;
            Payload::LeaseRequest(LeaseRequest {
                inventory_request_seq: wire.inventory_request_seq,
                sample_ms: wire.sample_ms,
            })
        }
        (Op::Lease, MsgStatus::Terminal) => {
            let wire: LeaseTerm = take(value)?;
            Payload::LeaseTerminal(LeaseTerminal {
                inventory_request_seq: wire.inventory_request_seq,
                sample_ms: wire.sample_ms,
                until_ms: wire.until_ms,
            })
        }
        (Op::Revoke, MsgStatus::Terminal) => Payload::RevokeTerminal(revoke_in(take(value)?)?),
        _ => return Err(CodecFault::Shape),
    };
    Ok(parsed)
}

fn payload_to(env: &Envelope) -> Result<OutPayload<'_>, CodecFault> {
    Ok(match (&env.op, env.status, &env.payload) {
        (Op::Inventory, MsgStatus::Request, Payload::InventoryRequest(body)) => {
            OutPayload::InvReq(InvReq {
                consume_op: body.op,
                consume_request_seq: body.request_seq,
                consume_revoke_seq: body.revoke_seq,
            })
        }
        (Op::Inventory, MsgStatus::Terminal, Payload::InventoryTerminal(body)) => {
            OutPayload::InvTerm(snap_out(body)?)
        }
        (Op::Probe, MsgStatus::Request, Payload::ProbeRequest(probe)) => {
            OutPayload::ProbeReq(ProbeReq { probe: *probe })
        }
        (Op::Probe, MsgStatus::Terminal, Payload::ProbeTerminal(check)) => {
            OutPayload::ProbeTerm(ProbeTerm { check: *check })
        }
        (
            Op::Bootstrap | Op::Login | Op::Run | Op::Revoke,
            MsgStatus::Request | MsgStatus::Accepted,
            Payload::Empty,
        ) => OutPayload::Empty(Empty {}),
        (Op::Bootstrap, MsgStatus::Terminal, Payload::BootstrapTerminal(body)) => {
            OutPayload::Boot(boot_out(body)?)
        }
        (Op::Login, MsgStatus::Terminal, Payload::LoginTerminal(body)) => {
            OutPayload::Login(login_out(body)?)
        }
        (Op::Run, MsgStatus::Terminal, Payload::RunTerminal(body)) => {
            OutPayload::Run(run_out(body)?)
        }
        (Op::Lease, MsgStatus::Request, Payload::LeaseRequest(body)) => {
            OutPayload::LeaseReq(LeaseReq {
                inventory_request_seq: body.inventory_request_seq,
                sample_ms: body.sample_ms,
            })
        }
        (Op::Lease, MsgStatus::Terminal, Payload::LeaseTerminal(body)) => {
            OutPayload::LeaseTerm(LeaseTerm {
                inventory_request_seq: body.inventory_request_seq,
                sample_ms: body.sample_ms,
                until_ms: body.until_ms,
            })
        }
        (Op::Revoke, MsgStatus::Terminal, Payload::RevokeTerminal(body)) => {
            OutPayload::Revoke(revoke_out(body)?)
        }
        _ => return Err(CodecFault::Shape),
    })
}

fn snapshot_from(value: Value) -> Result<InventorySnapshot, CodecFault> {
    let wire: SnapIn = take(value)?;
    // A pending row is an op with an explicit null result. `Activity::new`
    // still rejects a missing result on terminal, and a result with no op.
    let active_result = wire.active_result;
    let result = match (wire.active_op, active_result) {
        (None, None) => None,
        (None, Some(_)) => return Err(CodecFault::Shape),
        (Some(_), None) => None,
        (Some(op), Some(body)) => Some(side_from(op, body)?),
    };
    let revoke_result = match wire.revoke_result {
        None => None,
        Some(body) => Some(revoke_in(body)?),
    };
    let snap = InventorySnapshot {
        uid: wire.uid,
        desktop: AsciiId::new(&wire.desktop)?,
        config: AsciiId::new(&wire.config)?,
        sample_ms: wire.sample_ms,
        active: Activity::new(
            wire.active_status,
            wire.active_op,
            wire.active_request_seq,
            result,
        )?,
        revoke: RevokeSlot::new(wire.revoke_status, wire.revoke_request_seq, revoke_result)?,
    };
    snap.validate()?;
    Ok(snap)
}

fn snap_out(snap: &InventorySnapshot) -> Result<SnapOut<'_>, CodecFault> {
    snap.validate()?;
    Ok(SnapOut {
        uid: snap.uid,
        desktop: snap.desktop.as_str(),
        config: snap.config.as_str(),
        sample_ms: snap.sample_ms,
        active_op: snap.active.op,
        active_request_seq: snap.active.request_seq,
        active_status: snap.active.status,
        active_result: side_out(&snap.active.result)?,
        revoke_request_seq: snap.revoke.request_seq,
        revoke_status: snap.revoke.status,
        revoke_result: snap.revoke.result.as_ref().map(revoke_out).transpose()?,
    })
}

fn side_from(op: SideOp, value: Value) -> Result<SideResult, CodecFault> {
    Ok(match op {
        SideOp::Bootstrap => SideResult::Bootstrap(boot_in(take(value)?)?),
        SideOp::Login => SideResult::Login(login_in(take(value)?)?),
        SideOp::Run => SideResult::Run(run_in(take(value)?)?),
    })
}

fn side_out(result: &Option<SideResult>) -> Result<Option<SideOut>, CodecFault> {
    Ok(match result {
        None => None,
        Some(SideResult::Bootstrap(body)) => Some(SideOut::Boot(boot_out(body)?)),
        Some(SideResult::Login(body)) => Some(SideOut::Login(login_out(body)?)),
        Some(SideResult::Run(body)) => Some(SideOut::Run(run_out(body)?)),
    })
}

fn boot_in(wire: BootWire) -> Result<ValueOutcome<u64>, CodecFault> {
    ValueOutcome::new(wire.outcome, wire.native_ack_generation)
}

fn boot_out(body: &ValueOutcome<u64>) -> Result<BootWire, CodecFault> {
    body.validate()?;
    Ok(BootWire {
        outcome: body.outcome,
        native_ack_generation: body.value,
    })
}

fn login_in(wire: LoginWire) -> Result<ValueOutcome<LoginState>, CodecFault> {
    ValueOutcome::new(wire.outcome, wire.login)
}

fn login_out(body: &ValueOutcome<LoginState>) -> Result<LoginWire, CodecFault> {
    body.validate()?;
    Ok(LoginWire {
        outcome: body.outcome,
        login: body.value,
    })
}

fn run_in(wire: RunWire) -> Result<ValueOutcome<RunScore>, CodecFault> {
    ValueOutcome::new(wire.outcome, wire.score)
}

fn run_out(body: &ValueOutcome<RunScore>) -> Result<RunWire, CodecFault> {
    body.validate()?;
    Ok(RunWire {
        outcome: body.outcome,
        score: body.value,
    })
}

fn revoke_in(wire: RevokeWire) -> Result<RevokeOutcome, CodecFault> {
    let flags = match (wire.scope_revoked, wire.input_released, wire.tree_exited) {
        (None, None, None) => None,
        (Some(scope), Some(input), Some(tree)) => Some([scope, input, tree]),
        _ => return Err(CodecFault::Shape),
    };
    RevokeOutcome::new(wire.outcome, flags)
}

fn revoke_out(body: &RevokeOutcome) -> Result<RevokeWire, CodecFault> {
    let flags = if body.done {
        Some([true, true, true])
    } else {
        None
    };
    let checked = RevokeOutcome::new(body.outcome, flags)?;
    let flag = if checked.done { Some(true) } else { None };
    Ok(RevokeWire {
        outcome: checked.outcome,
        scope_revoked: flag,
        input_released: flag,
        tree_exited: flag,
    })
}

fn reply_body(reply: &MailboxReply) -> Result<ReplyBody<'_>, CodecFault> {
    Ok(match &reply.result {
        ReplyResult::Pending => ReplyBody::Pending(()),
        ReplyResult::Empty => ReplyBody::Empty(Empty {}),
        ReplyResult::Inventory(body) => ReplyBody::Inventory(snap_out(body)?),
        ReplyResult::Probe(check) => ReplyBody::Probe(ProbeTerm { check: *check }),
        ReplyResult::Bootstrap(body) => ReplyBody::Boot(boot_out(body)?),
        ReplyResult::Login(body) => ReplyBody::Login(login_out(body)?),
        ReplyResult::Run(body) => ReplyBody::Run(run_out(body)?),
        ReplyResult::Lease(body) => ReplyBody::Lease(LeaseTerm {
            inventory_request_seq: body.inventory_request_seq,
            sample_ms: body.sample_ms,
            until_ms: body.until_ms,
        }),
        ReplyResult::Revoke(body) => ReplyBody::Revoke(revoke_out(body)?),
    })
}

fn reply_from(state: ReplyState, op: Op, value: Value) -> Result<ReplyResult, CodecFault> {
    let result = match state {
        ReplyState::Pending => {
            if !value.is_null() {
                return Err(CodecFault::Shape);
            }
            ReplyResult::Pending
        }
        ReplyState::Accepted => {
            let _empty: Empty = take(value)?;
            ReplyResult::Empty
        }
        ReplyState::Terminal => match op {
            Op::Inventory => ReplyResult::Inventory(snapshot_from(value)?),
            Op::Probe => ReplyResult::Probe(take::<ProbeTerm>(value)?.check),
            Op::Bootstrap => ReplyResult::Bootstrap(boot_in(take(value)?)?),
            Op::Login => ReplyResult::Login(login_in(take(value)?)?),
            Op::Run => ReplyResult::Run(run_in(take(value)?)?),
            Op::Lease => {
                let wire: LeaseTerm = take(value)?;
                ReplyResult::Lease(LeaseTerminal {
                    inventory_request_seq: wire.inventory_request_seq,
                    sample_ms: wire.sample_ms,
                    until_ms: wire.until_ms,
                })
            }
            Op::Revoke => ReplyResult::Revoke(revoke_in(take(value)?)?),
        },
    };
    Ok(result)
}

fn take<T: serde::de::DeserializeOwned>(value: Value) -> Result<T, CodecFault> {
    serde_json::from_value(value).map_err(map_serde)
}

/// JSON null is accepted. A missing key is not: serde would otherwise turn
/// `Option` into `None` for both, and the schema requires an explicit null.
fn required_null<'de, T, D>(deserializer: D) -> Result<Option<T>, D::Error>
where
    T: Deserialize<'de>,
    D: serde::Deserializer<'de>,
{
    Option::<T>::deserialize(deserializer)
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct RawEnv {
    v: u64,
    run_id: String,
    attempt_nonce: String,
    vm_config_digest: String,
    #[serde(deserialize_with = "required_null")]
    boot_id: Option<String>,
    op: Op,
    seq: u64,
    request_seq: u64,
    status: MsgStatus,
    payload: Value,
}

#[derive(Serialize)]
struct OutEnv<'a> {
    v: u16,
    run_id: &'a str,
    attempt_nonce: String,
    vm_config_digest: String,
    boot_id: Option<&'a str>,
    op: Op,
    seq: u64,
    request_seq: u64,
    status: MsgStatus,
    payload: OutPayload<'a>,
}

#[derive(Serialize)]
#[serde(untagged)]
enum OutPayload<'a> {
    InvReq(InvReq),
    InvTerm(SnapOut<'a>),
    ProbeReq(ProbeReq),
    ProbeTerm(ProbeTerm),
    Empty(Empty),
    Boot(BootWire),
    Login(LoginWire),
    Run(RunWire),
    LeaseReq(LeaseReq),
    LeaseTerm(LeaseTerm),
    Revoke(RevokeWire),
}

#[derive(Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct InvReq {
    #[serde(deserialize_with = "required_null")]
    consume_op: Option<SideOp>,
    consume_request_seq: u64,
    consume_revoke_seq: u64,
}

#[derive(Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ProbeReq {
    probe: Probe,
}

#[derive(Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ProbeTerm {
    check: Check,
}

#[derive(Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Empty {}

#[derive(Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct BootWire {
    outcome: Outcome,
    #[serde(deserialize_with = "required_null")]
    native_ack_generation: Option<u64>,
}

#[derive(Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct LoginWire {
    outcome: Outcome,
    #[serde(deserialize_with = "required_null")]
    login: Option<LoginState>,
}

#[derive(Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct RunWire {
    outcome: Outcome,
    #[serde(deserialize_with = "required_null")]
    score: Option<RunScore>,
}

#[derive(Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct LeaseReq {
    inventory_request_seq: u64,
    sample_ms: u64,
}

#[derive(Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct LeaseTerm {
    inventory_request_seq: u64,
    sample_ms: u64,
    until_ms: u64,
}

#[derive(Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct RevokeWire {
    outcome: Outcome,
    #[serde(deserialize_with = "required_null")]
    scope_revoked: Option<bool>,
    #[serde(deserialize_with = "required_null")]
    input_released: Option<bool>,
    #[serde(deserialize_with = "required_null")]
    tree_exited: Option<bool>,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct SnapIn {
    uid: u32,
    desktop: String,
    config: String,
    sample_ms: u64,
    #[serde(deserialize_with = "required_null")]
    active_op: Option<SideOp>,
    active_request_seq: u64,
    active_status: SnapStatus,
    #[serde(deserialize_with = "required_null")]
    active_result: Option<Value>,
    revoke_request_seq: u64,
    revoke_status: SnapStatus,
    #[serde(deserialize_with = "required_null")]
    revoke_result: Option<RevokeWire>,
}

#[derive(Serialize)]
struct SnapOut<'a> {
    uid: u32,
    desktop: &'a str,
    config: &'a str,
    sample_ms: u64,
    active_op: Option<SideOp>,
    active_request_seq: u64,
    active_status: SnapStatus,
    active_result: Option<SideOut>,
    revoke_request_seq: u64,
    revoke_status: SnapStatus,
    revoke_result: Option<RevokeWire>,
}

#[derive(Serialize)]
#[serde(untagged)]
enum SideOut {
    Boot(BootWire),
    Login(LoginWire),
    Run(RunWire),
}

#[derive(Serialize)]
struct ReplyOut<'a> {
    state: ReplyState,
    op: Op,
    request_seq: u64,
    result: ReplyBody<'a>,
}

#[derive(Serialize)]
#[serde(untagged)]
enum ReplyBody<'a> {
    Pending(()),
    Empty(Empty),
    Inventory(SnapOut<'a>),
    Probe(ProbeTerm),
    Boot(BootWire),
    Login(LoginWire),
    Run(RunWire),
    Lease(LeaseTerm),
    Revoke(RevokeWire),
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ReplyIn {
    state: ReplyState,
    op: Op,
    request_seq: u64,
    result: Value,
}
