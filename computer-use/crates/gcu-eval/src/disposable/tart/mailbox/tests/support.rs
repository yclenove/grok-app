#![cfg(test)]
//! Shared mailbox fixtures. No filesystem and no clock sleep.

use crate::disposable::tart::manifest::Digest;
use crate::disposable::{Check, Probe};

use super::super::codec::encode_frame;
use super::super::ids::{AsciiId, AttemptNonce};
use super::super::ops::{
    Activity, Envelope, InventoryConsume, InventorySnapshot, LeaseRequest, LeaseTerminal,
    MailboxReply, MsgStatus, Op, Outcome, Payload, ReplyState, RevokeOutcome, RevokeSlot, SideOp,
    SideResult, SnapStatus, ValueOutcome, LEASE_WINDOW_MS,
};
use super::super::seq::{AttemptBinding, Observe, SeqTracker};

pub(super) fn need<T, E: core::fmt::Debug>(result: Result<T, E>) -> T {
    match result {
        Ok(value) => value,
        Err(err) => panic!("unexpected error: {err:?}"),
    }
}

pub(super) fn id(text: &str) -> AsciiId {
    need(AsciiId::new(text))
}

pub(super) fn dig() -> Digest {
    need(Digest::try_from_bytes([0x11; 32]))
}

pub(super) fn binding() -> AttemptBinding {
    AttemptBinding {
        run_id: id("run1"),
        attempt_nonce: AttemptNonce::from_bytes([0x22; 16]),
        vm_config_digest: dig(),
    }
}

pub(super) fn envelope(
    op: Op,
    seq: u64,
    boot: Option<&str>,
    status: MsgStatus,
    payload: Payload,
) -> Envelope {
    Envelope {
        v: 1,
        run_id: id("run1"),
        attempt_nonce: AttemptNonce::from_bytes([0x22; 16]),
        vm_config_digest: dig(),
        boot_id: boot.map(id),
        op,
        seq,
        request_seq: seq,
        status,
        payload,
    }
}

pub(super) fn encoded(env: &Envelope) -> Vec<u8> {
    need(encode_frame(env))
}

pub(super) fn inv_request(seq: u64, boot: Option<&str>) -> Vec<u8> {
    encoded(&envelope(
        Op::Inventory,
        seq,
        boot,
        MsgStatus::Request,
        Payload::InventoryRequest(need(InventoryConsume::new(None, 0, 0))),
    ))
}

pub(super) fn inv_terminal(seq: u64, boot: Option<&str>) -> Vec<u8> {
    let payload = Payload::InventoryTerminal(InventorySnapshot {
        uid: 501,
        desktop: id("desk1"),
        config: id("cfg1"),
        sample_ms: 10,
        active: need(Activity::new(SnapStatus::None, None, 0, None)),
        revoke: need(RevokeSlot::new(SnapStatus::None, 0, None)),
    });
    encoded(&envelope(
        Op::Inventory,
        seq,
        boot,
        MsgStatus::Terminal,
        payload,
    ))
}

pub(super) fn bind(tracker: &mut SeqTracker) {
    need(tracker.note_request(&inv_request(1, None)));
    match need(tracker.observe(Op::Inventory, &inv_terminal(1, Some("boot1")))) {
        Observe::Reply(reply) if reply.state == ReplyState::Terminal => {}
        other => panic!("expected inventory terminal, got {other:?}"),
    }
}

pub(super) fn probe_request(seq: u64) -> Vec<u8> {
    encoded(&envelope(
        Op::Probe,
        seq,
        Some("boot1"),
        MsgStatus::Request,
        Payload::ProbeRequest(Probe::Sandbox),
    ))
}

pub(super) fn probe_terminal(seq: u64, check: Check) -> Vec<u8> {
    encoded(&envelope(
        Op::Probe,
        seq,
        Some("boot1"),
        MsgStatus::Terminal,
        Payload::ProbeTerminal(check),
    ))
}

pub(super) fn lease_request(seq: u64, sample_ms: u64) -> Vec<u8> {
    encoded(&envelope(
        Op::Lease,
        seq,
        Some("boot1"),
        MsgStatus::Request,
        Payload::LeaseRequest(LeaseRequest {
            inventory_request_seq: 1,
            sample_ms,
        }),
    ))
}

pub(super) fn lease_terminal(seq: u64, sample_ms: u64, until_ms: u64) -> Vec<u8> {
    encoded(&envelope(
        Op::Lease,
        seq,
        Some("boot1"),
        MsgStatus::Terminal,
        Payload::LeaseTerminal(LeaseTerminal {
            inventory_request_seq: 1,
            sample_ms,
            until_ms,
        }),
    ))
}

pub(super) fn until_ms(sample_ms: u64) -> u64 {
    need(sample_ms.checked_add(LEASE_WINDOW_MS).ok_or("overflow"))
}

pub(super) fn json_of(frame: &[u8]) -> &str {
    let bytes = frame.get(4..).unwrap_or(&[]);
    match std::str::from_utf8(bytes) {
        Ok(text) => text,
        Err(err) => panic!("frame json: {err:?}"),
    }
}

pub(super) fn boot_ok(generation: u64) -> SideResult {
    SideResult::Bootstrap(need(ValueOutcome::new(Outcome::Ok, Some(generation))))
}

pub(super) fn revoke_ok() -> RevokeOutcome {
    need(RevokeOutcome::new(Outcome::Ok, Some([true, true, true])))
}

pub(super) fn pending_reply(op: Op, request_seq: u64) -> MailboxReply {
    MailboxReply::pending(op, request_seq)
}

pub(super) fn activity(
    status: SnapStatus,
    op: Option<SideOp>,
    request_seq: u64,
    result: Option<SideResult>,
) -> Activity {
    need(Activity::new(status, op, request_seq, result))
}

pub(super) fn revoke_row(
    status: SnapStatus,
    request_seq: u64,
    result: Option<RevokeOutcome>,
) -> RevokeSlot {
    need(RevokeSlot::new(status, request_seq, result))
}

pub(super) fn snapshot(active: Activity, revoke: RevokeSlot) -> InventorySnapshot {
    InventorySnapshot {
        uid: 501,
        desktop: id("desk1"),
        config: id("cfg1"),
        sample_ms: 10,
        active,
        revoke,
    }
}

pub(super) fn inv_with(
    seq: u64,
    boot: Option<&str>,
    active: Activity,
    revoke: RevokeSlot,
) -> Vec<u8> {
    encoded(&envelope(
        Op::Inventory,
        seq,
        boot,
        MsgStatus::Terminal,
        Payload::InventoryTerminal(snapshot(active, revoke)),
    ))
}
