#![cfg(test)]
//! Codec rejects. Each case is one closed failure.

use super::super::codec::{decode_frame, decode_reply, encode_reply};
use super::super::ops::{
    Activity, Envelope, InventorySnapshot, LoginState, MailboxReply, MsgStatus, Op, Outcome,
    Payload, ReplyResult, ReplyState, RevokeOutcome, RevokeSlot, SideOp, SideResult, SnapStatus,
    ValueOutcome, FRAME_MAX,
};
use super::super::{CodecFault, MailboxError};
use super::support::{
    activity, boot_ok, encoded, envelope, id, inv_terminal, inv_with, json_of, need, pending_reply,
    probe_request, revoke_ok, revoke_row, snapshot,
};
use crate::disposable::{Check, Probe};

#[test]
fn frame_roundtrip_keeps_closed_fields() {
    let probe = envelope(
        Op::Probe,
        1,
        Some("boot1"),
        MsgStatus::Request,
        Payload::ProbeRequest(Probe::Sandbox),
    );
    let frame = encoded(&probe);
    let text = json_of(&frame);
    if text
        != concat!(
            r#"{"v":1,"run_id":"run1","attempt_nonce":"22222222222222222222222222222222","#,
            r#""vm_config_digest":"1111111111111111111111111111111111111111111111111111111111111111","#,
            r#""boot_id":"boot1","op":"probe","seq":1,"request_seq":1,"status":"request","payload":{"probe":"sandbox"}}"#
        )
    {
        panic!("probe request json: {text}");
    }
    round(&envelope(
        Op::Probe,
        4,
        Some("boot1"),
        MsgStatus::Terminal,
        Payload::ProbeTerminal(Check::Passed),
    ));
    round(&envelope(
        Op::Bootstrap,
        1,
        Some("boot1"),
        MsgStatus::Accepted,
        Payload::Empty,
    ));
    // Side-slot terminals are inventory rows and normalized replies, not outer envelopes.
    let boot_ok_row = activity(
        SnapStatus::Terminal,
        Some(SideOp::Bootstrap),
        1,
        Some(boot_ok(7)),
    );
    let boot_fail = activity(
        SnapStatus::Terminal,
        Some(SideOp::Bootstrap),
        1,
        Some(SideResult::Bootstrap(need(ValueOutcome::new(
            Outcome::Failed,
            None,
        )))),
    );
    let login_ok = activity(
        SnapStatus::Terminal,
        Some(SideOp::Login),
        1,
        Some(SideResult::Login(need(ValueOutcome::new(
            Outcome::Ok,
            Some(LoginState::Ready),
        )))),
    );
    let run_cancel = activity(
        SnapStatus::Terminal,
        Some(SideOp::Run),
        1,
        Some(SideResult::Run(need(ValueOutcome::new(
            Outcome::Cancelled,
            None,
        )))),
    );
    let revoke_fail = revoke_row(
        SnapStatus::Terminal,
        1,
        Some(need(RevokeOutcome::new(Outcome::Failed, None))),
    );
    round_inv(
        1,
        boot_ok_row,
        revoke_row(SnapStatus::Terminal, 1, Some(revoke_ok())),
    );
    round_inv(1, boot_fail, revoke_fail);
    round_inv(1, login_ok, revoke_row(SnapStatus::None, 0, None));
    round_inv(1, run_cancel, revoke_row(SnapStatus::None, 0, None));
    reply_round(&terminal_reply(
        Op::Bootstrap,
        ReplyResult::Bootstrap(need(ValueOutcome::new(Outcome::Ok, Some(7)))),
    ));
    reply_round(&terminal_reply(
        Op::Bootstrap,
        ReplyResult::Bootstrap(need(ValueOutcome::new(Outcome::Failed, None))),
    ));
    reply_round(&terminal_reply(
        Op::Login,
        ReplyResult::Login(need(ValueOutcome::new(
            Outcome::Ok,
            Some(LoginState::Ready),
        ))),
    ));
    reply_round(&terminal_reply(
        Op::Run,
        ReplyResult::Run(need(ValueOutcome::new(Outcome::Cancelled, None))),
    ));
    reply_round(&terminal_reply(
        Op::Revoke,
        ReplyResult::Revoke(revoke_ok()),
    ));
    reply_round(&terminal_reply(
        Op::Revoke,
        ReplyResult::Revoke(need(RevokeOutcome::new(Outcome::Failed, None))),
    ));
    round(&envelope(
        Op::Lease,
        2,
        Some("boot1"),
        MsgStatus::Request,
        Payload::LeaseRequest(super::super::ops::LeaseRequest {
            inventory_request_seq: 1,
            sample_ms: 50,
        }),
    ));
    round(&envelope(
        Op::Lease,
        2,
        Some("boot1"),
        MsgStatus::Terminal,
        Payload::LeaseTerminal(super::super::ops::LeaseTerminal {
            inventory_request_seq: 1,
            sample_ms: 50,
            until_ms: 2050,
        }),
    ));
    round(&envelope(
        Op::Inventory,
        1,
        None,
        MsgStatus::Request,
        Payload::InventoryRequest(need(super::super::ops::InventoryConsume::new(None, 0, 0))),
    ));
    let snap = InventorySnapshot {
        uid: 7,
        desktop: id("desk1"),
        config: id("cfg1"),
        sample_ms: 9,
        active: need(Activity::new(
            SnapStatus::Terminal,
            Some(SideOp::Bootstrap),
            1,
            Some(boot_ok(3)),
        )),
        revoke: need(RevokeSlot::new(SnapStatus::Terminal, 1, Some(revoke_ok()))),
    };
    round(&envelope(
        Op::Inventory,
        3,
        Some("boot1"),
        MsgStatus::Terminal,
        Payload::InventoryTerminal(snap.clone()),
    ));
    let pending = pending_reply(Op::Probe, 0);
    let pending_frame = need(encode_reply(&pending));
    if json_of(pending_frame.as_bytes())
        != r#"{"state":"pending","op":"probe","request_seq":0,"result":null}"#
    {
        panic!("pending reply: {}", json_of(pending_frame.as_bytes()));
    }
    let accepted = MailboxReply {
        state: ReplyState::Accepted,
        op: Op::Login,
        request_seq: 1,
        result: ReplyResult::Empty,
    };
    reply_round(&accepted);
    let terminal = MailboxReply {
        state: ReplyState::Terminal,
        op: Op::Inventory,
        request_seq: 3,
        result: ReplyResult::Inventory(snap),
    };
    reply_round(&terminal);
    let decoded = need(decode_frame(&inv_terminal(1, Some("boot1"))));
    if decoded.op != Op::Inventory || decoded.boot_id != Some(id("boot1")) {
        panic!("inventory terminal: {decoded:?}");
    }
}

#[test]
fn pending_inventory_activity_roundtrips() {
    for side in [SideOp::Bootstrap, SideOp::Login, SideOp::Run] {
        let row = activity(SnapStatus::Pending, Some(side), 1, None);
        if row.result.is_some() || row.request_seq != 1 {
            panic!("pending activity: {row:?}");
        }
        let revoke = revoke_row(SnapStatus::None, 0, None);
        round(&envelope(
            Op::Inventory,
            1,
            Some("boot1"),
            MsgStatus::Terminal,
            Payload::InventoryTerminal(snapshot(row, revoke)),
        ));
        reply_round(&MailboxReply {
            state: ReplyState::Terminal,
            op: Op::Inventory,
            request_seq: 1,
            result: ReplyResult::Inventory(snapshot(
                activity(SnapStatus::Pending, Some(side), 1, None),
                revoke_row(SnapStatus::None, 0, None),
            )),
        });
    }
    match Activity::new(SnapStatus::Terminal, Some(SideOp::Bootstrap), 1, None) {
        Err(CodecFault::Shape) => {}
        other => panic!("terminal without a result: {other:?}"),
    }
    match Activity::new(SnapStatus::Pending, Some(SideOp::Login), 1, None) {
        Ok(row) if row.op == Some(SideOp::Login) && row.result.is_none() => {}
        other => panic!("pending login: {other:?}"),
    }
    let pending = inv_with(
        1,
        Some("boot1"),
        activity(SnapStatus::Pending, Some(SideOp::Bootstrap), 1, None),
        revoke_row(SnapStatus::None, 0, None),
    );
    let terminal_without_result = mutate_json(&pending, |value| {
        value["payload"]["active_status"] = serde_json::Value::String("terminal".to_string());
    });
    expect_codec(&terminal_without_result, |err| {
        matches!(err, MailboxError::Malformed(CodecFault::Shape))
    });
    let paired = inv_with(
        1,
        Some("boot1"),
        activity(
            SnapStatus::Terminal,
            Some(SideOp::Bootstrap),
            1,
            Some(boot_ok(3)),
        ),
        revoke_row(SnapStatus::None, 0, None),
    );
    let result_without_op = mutate_json(&paired, |value| {
        value["payload"]["active_op"] = serde_json::Value::Null;
    });
    expect_codec(&result_without_op, |err| {
        matches!(err, MailboxError::Malformed(CodecFault::Shape))
    });
}

#[test]
fn omitted_nullable_field_is_rejected() {
    let probe = probe_request(1);
    expect_frame_omitted(&probe, &["boot_id"]);
    let first_inventory = super::support::inv_request(1, None);
    expect_frame_omitted(&first_inventory, &["boot_id"]);
    expect_frame_omitted(&first_inventory, &["payload", "consume_op"]);
    let none = inv_terminal(1, Some("boot1"));
    expect_frame_omitted(&none, &["payload", "active_op"]);
    expect_frame_omitted(&none, &["payload", "active_result"]);
    expect_frame_omitted(&none, &["payload", "revoke_result"]);
    let failed = inv_with(
        1,
        Some("boot1"),
        activity(
            SnapStatus::Terminal,
            Some(SideOp::Bootstrap),
            1,
            Some(SideResult::Bootstrap(need(ValueOutcome::new(
                Outcome::Failed,
                None,
            )))),
        ),
        revoke_row(
            SnapStatus::Terminal,
            1,
            Some(need(RevokeOutcome::new(Outcome::Failed, None))),
        ),
    );
    expect_frame_omitted(
        &failed,
        &["payload", "active_result", "native_ack_generation"],
    );
    expect_frame_omitted(&failed, &["payload", "revoke_result", "scope_revoked"]);
    expect_frame_omitted(&failed, &["payload", "revoke_result", "input_released"]);
    expect_frame_omitted(&failed, &["payload", "revoke_result", "tree_exited"]);
    let login = inv_with(
        1,
        Some("boot1"),
        activity(
            SnapStatus::Terminal,
            Some(SideOp::Login),
            1,
            Some(SideResult::Login(need(ValueOutcome::new(
                Outcome::Failed,
                None,
            )))),
        ),
        revoke_row(SnapStatus::None, 0, None),
    );
    expect_frame_omitted(&login, &["payload", "active_result", "login"]);
    let run = inv_with(
        1,
        Some("boot1"),
        activity(
            SnapStatus::Terminal,
            Some(SideOp::Run),
            1,
            Some(SideResult::Run(need(ValueOutcome::new(
                Outcome::Failed,
                None,
            )))),
        ),
        revoke_row(SnapStatus::None, 0, None),
    );
    expect_frame_omitted(&run, &["payload", "active_result", "score"]);
    expect_reply_omitted(
        &terminal_reply(
            Op::Bootstrap,
            ReplyResult::Bootstrap(need(ValueOutcome::new(Outcome::Failed, None))),
        ),
        &["result", "native_ack_generation"],
    );
    expect_reply_omitted(
        &terminal_reply(
            Op::Login,
            ReplyResult::Login(need(ValueOutcome::new(Outcome::Failed, None))),
        ),
        &["result", "login"],
    );
    expect_reply_omitted(
        &terminal_reply(
            Op::Run,
            ReplyResult::Run(need(ValueOutcome::new(Outcome::Failed, None))),
        ),
        &["result", "score"],
    );
    let revoke = terminal_reply(
        Op::Revoke,
        ReplyResult::Revoke(need(RevokeOutcome::new(Outcome::Failed, None))),
    );
    expect_reply_omitted(&revoke, &["result", "scope_revoked"]);
    expect_reply_omitted(&revoke, &["result", "input_released"]);
    expect_reply_omitted(&revoke, &["result", "tree_exited"]);
}

#[test]
fn frame_rejects_length_mismatch() {
    let mut bytes = encoded(&envelope(
        Op::Probe,
        1,
        Some("boot1"),
        MsgStatus::Request,
        Payload::ProbeRequest(Probe::Sandbox),
    ));
    if let Some(slot) = bytes.first_mut() {
        *slot = slot.wrapping_add(1);
    }
    expect_codec(&bytes, |err| {
        matches!(
            err,
            MailboxError::Malformed(super::super::CodecFault::Length)
        )
    });
}

#[test]
fn frame_rejects_over_4096() {
    let bytes = vec![0u8; FRAME_MAX.saturating_add(1)];
    expect_codec(&bytes, |err| {
        matches!(
            err,
            MailboxError::Malformed(super::super::CodecFault::TooLong)
        )
    });
}

#[test]
fn frame_rejects_non_utf8() {
    expect_codec(&raw_frame(&[0xff, 0xfe]), |err| {
        matches!(err, MailboxError::Malformed(super::super::CodecFault::Utf8))
    });
}

#[test]
fn frame_rejects_trailing_bytes() {
    expect_codec(&raw_frame(br#"{"a":1} "#), |err| {
        matches!(
            err,
            MailboxError::Malformed(super::super::CodecFault::Trailing)
        )
    });
}

#[test]
fn frame_rejects_unknown_field() {
    let json = json_of(&probe_request(1)).replacen(r#""v":1"#, r#""v":1,"extra":1"#, 1);
    expect_codec(&raw_frame(json.as_bytes()), |err| {
        matches!(
            err,
            MailboxError::Malformed(super::super::CodecFault::Unknown)
        )
    });
}

#[test]
fn frame_rejects_duplicate_field() {
    expect_codec(&raw_frame(br#"{"v":1,"v":1}"#), |err| {
        matches!(
            err,
            MailboxError::Malformed(super::super::CodecFault::Duplicate)
        )
    });
    expect_codec(&raw_frame(br#"{"v":1,"\u0076":1}"#), |err| {
        matches!(
            err,
            MailboxError::Malformed(super::super::CodecFault::Duplicate)
        )
    });
}

#[test]
fn frame_rejects_depth_four() {
    expect_codec(&raw_frame(br#"{"a":{"b":{"c":{"d":1}}}}"#), |err| {
        matches!(
            err,
            MailboxError::Malformed(super::super::CodecFault::Depth)
        )
    });
}

#[test]
fn frame_rejects_array_of_seventeen() {
    let mut json = String::from(r#"{"a":["#);
    for index in 0..17 {
        if index > 0 {
            json.push(',');
        }
        json.push_str(&index.to_string());
    }
    json.push_str("]}");
    expect_codec(&raw_frame(json.as_bytes()), |err| {
        matches!(
            err,
            MailboxError::Malformed(super::super::CodecFault::Array)
        )
    });
}

#[test]
fn frame_rejects_negative_seq() {
    expect_seq_text("-1", super::super::CodecFault::Negative);
}

#[test]
fn frame_rejects_fractional_seq() {
    expect_seq_text("1.5", super::super::CodecFault::Fraction);
}

#[test]
fn frame_rejects_exponent_seq() {
    expect_seq_text("1e2", super::super::CodecFault::Exponent);
}

#[test]
fn frame_rejects_overflow_seq() {
    expect_seq_text("18446744073709551616", super::super::CodecFault::Overflow);
}

#[test]
fn frame_rejects_overlong_identity() {
    let mut run = String::from("a");
    run.push_str(&"a".repeat(64));
    let json = json_of(&probe_request(1)).replacen("run1", &run, 1);
    expect_codec(&raw_frame(json.as_bytes()), |err| {
        matches!(
            err,
            MailboxError::Malformed(super::super::CodecFault::Identity)
        )
    });
}

#[test]
fn frame_rejects_bad_digest_length() {
    let full = "1".repeat(64);
    let short = "1".repeat(63);
    let json = json_of(&probe_request(1)).replacen(&full, &short, 1);
    expect_codec(&raw_frame(json.as_bytes()), |err| {
        matches!(
            err,
            MailboxError::Malformed(super::super::CodecFault::DigestLen)
        )
    });
}

fn round_inv(seq: u64, active: super::super::ops::Activity, revoke: super::super::ops::RevokeSlot) {
    round(&envelope(
        Op::Inventory,
        seq,
        Some("boot1"),
        MsgStatus::Terminal,
        Payload::InventoryTerminal(snapshot(active, revoke)),
    ));
}

fn terminal_reply(op: Op, result: ReplyResult) -> MailboxReply {
    MailboxReply {
        state: ReplyState::Terminal,
        op,
        request_seq: 1,
        result,
    }
}

fn round(env: &Envelope) {
    let frame = encoded(env);
    if encoded(env) != frame {
        panic!("unstable frame");
    }
    let decoded = need(decode_frame(&frame));
    if decoded != *env {
        panic!("envelope mismatch");
    }
}

fn reply_round(reply: &MailboxReply) {
    let first = need(encode_reply(reply));
    let second = need(encode_reply(reply));
    if first.as_bytes() != second.as_bytes() {
        panic!("unstable reply");
    }
    let decoded = need(decode_reply(first.as_bytes()));
    if decoded != *reply {
        panic!("reply mismatch");
    }
}

fn raw_frame(json: &[u8]) -> Vec<u8> {
    let len = u32::try_from(json.len()).unwrap_or(u32::MAX);
    let mut out = Vec::with_capacity(json.len().saturating_add(4));
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(json);
    out
}

fn expect_codec(bytes: &[u8], pred: impl Fn(&MailboxError) -> bool) {
    match decode_frame(bytes) {
        Err(err) if pred(&err) => {}
        other => panic!("unexpected decode: {other:?}"),
    }
}

fn expect_frame_omitted(frame: &[u8], path: &[&str]) {
    expect_omitted(frame, path, |bytes| decode_frame(bytes).map(|_| ()));
}

fn expect_reply_omitted(reply: &MailboxReply, path: &[&str]) {
    let frame = need(encode_reply(reply));
    expect_omitted(frame.as_bytes(), path, |bytes| {
        decode_reply(bytes).map(|_| ())
    });
}

fn expect_omitted(frame: &[u8], path: &[&str], decode: impl Fn(&[u8]) -> Result<(), MailboxError>) {
    if let Err(err) = decode(frame) {
        panic!("source frame should decode ({path:?}): {err:?}");
    }
    let omitted = mutate_json(frame, |value| remove_key(value, path));
    match decode(&omitted) {
        Err(MailboxError::Malformed(CodecFault::Schema)) => {}
        other => panic!("omit {path:?}: {other:?}"),
    }
}

fn mutate_json(frame: &[u8], edit: impl FnOnce(&mut serde_json::Value)) -> Vec<u8> {
    let mut value = match serde_json::from_str::<serde_json::Value>(json_of(frame)) {
        Ok(value) => value,
        Err(err) => panic!("parse frame: {err}"),
    };
    edit(&mut value);
    let bytes = match serde_json::to_vec(&value) {
        Ok(bytes) => bytes,
        Err(err) => panic!("reencode: {err}"),
    };
    raw_frame(&bytes)
}

fn remove_key(value: &mut serde_json::Value, path: &[&str]) {
    let (last, parents) = match path.split_last() {
        Some(pair) => pair,
        None => panic!("empty omit path"),
    };
    let mut cursor = value;
    for key in parents {
        cursor = match cursor.get_mut(*key) {
            Some(next) => next,
            None => panic!("missing {key} in {path:?}"),
        };
    }
    let Some(obj) = cursor.as_object_mut() else {
        panic!("not an object at {path:?}");
    };
    if obj.remove(*last).is_none() {
        panic!("key {last} was not present");
    }
}

fn expect_seq_text(literal: &str, fault: super::super::CodecFault) {
    let json = json_of(&probe_request(1)).replacen("\"seq\":1", &format!("\"seq\":{literal}"), 1);
    expect_codec(&raw_frame(json.as_bytes()), |err| {
        err == &MailboxError::Malformed(fault)
    });
}
