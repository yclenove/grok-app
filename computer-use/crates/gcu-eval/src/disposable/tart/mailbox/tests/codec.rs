//! Codec rejects. Each case is one closed failure.

use super::super::codec::{decode_frame, decode_reply, encode_reply};
use super::super::ops::{
    Activity, Envelope, InventorySnapshot, LoginState, MailboxReply, MsgStatus, Op, Outcome,
    Payload, ReplyResult, ReplyState, RevokeSlot, SideOp, SnapStatus, ValueOutcome, FRAME_MAX,
};
use super::super::MailboxError;
use super::support::{
    boot_ok, encoded, envelope, id, inv_terminal, json_of, need, pending_reply, probe_request,
    revoke_ok,
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
    round(&envelope(
        Op::Bootstrap,
        1,
        Some("boot1"),
        MsgStatus::Terminal,
        Payload::BootstrapTerminal(need(ValueOutcome::new(Outcome::Ok, Some(7)))),
    ));
    round(&envelope(
        Op::Bootstrap,
        1,
        Some("boot1"),
        MsgStatus::Terminal,
        Payload::BootstrapTerminal(need(ValueOutcome::new(Outcome::Failed, None))),
    ));
    round(&envelope(
        Op::Login,
        1,
        Some("boot1"),
        MsgStatus::Terminal,
        Payload::LoginTerminal(need(ValueOutcome::new(
            Outcome::Ok,
            Some(LoginState::Ready),
        ))),
    ));
    round(&envelope(
        Op::Run,
        1,
        Some("boot1"),
        MsgStatus::Terminal,
        Payload::RunTerminal(need(ValueOutcome::new(Outcome::Cancelled, None))),
    ));
    round(&envelope(
        Op::Revoke,
        1,
        Some("boot1"),
        MsgStatus::Terminal,
        Payload::RevokeTerminal(revoke_ok()),
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

fn expect_seq_text(literal: &str, fault: super::super::CodecFault) {
    let json = json_of(&probe_request(1)).replacen("\"seq\":1", &format!("\"seq\":{literal}"), 1);
    expect_codec(&raw_frame(json.as_bytes()), |err| {
        err == &MailboxError::Malformed(fault)
    });
}
