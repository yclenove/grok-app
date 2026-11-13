#![cfg(test)]
//! Sequence, boot, and lease rules. No filesystem.

use super::super::codec::encode_frame;
use super::super::ops::{
    LoginState, MsgStatus, Op, Outcome, Payload, ReplyResult, ReplyState, RunScore, SideOp,
    SnapStatus, ValueOutcome, LEASE_WINDOW_MS, MAX_POLLS,
};
use super::super::seq::{succ, Observe, SeqTracker};
use super::super::{CodecFault, MailboxError, SeqFault};
use super::support::{
    activity, bind, binding, boot_ok, encoded, envelope, inv_request, inv_with, json_of,
    lease_request, lease_terminal, need, probe_request, probe_terminal, revoke_ok, revoke_row,
    until_ms,
};
use crate::disposable::Check;

#[test]
fn seq_starts_at_one_and_is_consecutive() {
    let mut tracker = SeqTracker::new(binding());
    bind(&mut tracker);
    if tracker.outstanding(Op::Inventory) != 1 {
        panic!("inventory did not start at 1");
    }
    if need(tracker.note_request(&probe_request(1))) != 1 {
        panic!("probe did not accept 1");
    }
    if need(tracker.note_request(&probe_request(2))) != 2 {
        panic!("probe did not accept 2");
    }
    if tracker.outstanding(Op::Probe) != 2 {
        panic!("probe outstanding: {}", tracker.outstanding(Op::Probe));
    }
}

#[test]
fn seq_rejects_skip_and_zero() {
    let mut tracker = SeqTracker::new(binding());
    bind(&mut tracker);
    need(tracker.note_request(&probe_request(1)));
    expect_fault(
        tracker.note_request(&probe_request(3)),
        SeqFault::NotConsecutive,
    );
    expect_fault(
        tracker.note_request(&probe_request(0)),
        SeqFault::NotConsecutive,
    );
    expect_fault(
        tracker.note_request(&probe_request(1)),
        SeqFault::NotConsecutive,
    );
    if need(tracker.note_request(&probe_request(2))) != 2 {
        panic!("probe 2 was rejected after a skip");
    }
}

#[test]
fn seq_does_not_wrap() {
    match succ(u64::MAX) {
        Err(SeqFault::Wrapped) => {}
        other => panic!("expected wrap, got {other:?}"),
    }
    match succ(u64::MAX - 1) {
        Ok(value) if value == u64::MAX => {}
        other => panic!("expected max, got {other:?}"),
    }
}

#[test]
fn reply_echoes_request_and_binding() {
    let mut tracker = SeqTracker::new(binding());
    bind(&mut tracker);
    need(tracker.note_request(&probe_request(1)));
    let reply = observe_reply(&mut tracker, Op::Probe, &probe_terminal(1, Check::Passed));
    if reply.state != ReplyState::Terminal
        || reply.op != Op::Probe
        || reply.request_seq != 1
        || reply.result != ReplyResult::Probe(Check::Passed)
    {
        panic!("probe reply: {reply:?}");
    }
    expect_fault(
        tracker.observe(Op::Probe, &probe_request(1)),
        SeqFault::Status,
    );
}

#[test]
fn side_slot_terminal_is_not_completion() {
    for op in [Op::Bootstrap, Op::Login, Op::Run, Op::Revoke] {
        match encode_frame(&envelope(
            op,
            1,
            Some("boot1"),
            MsgStatus::Terminal,
            side_payload(op),
        )) {
            Err(MailboxError::Malformed(CodecFault::Shape)) => {}
            other => panic!("encode {op:?} terminal: {other:?}"),
        }
        let mut open = SeqTracker::new(binding());
        bind(&mut open);
        need(open.note_request(&side_request(op, 1)));
        expect_shape(open.observe(op, &raw_terminal(op)));
        expect_shape(open.observe(op, &raw_terminal(op)));
        let accepted = observe_reply(&mut open, op, &side_accepted(op, 1));
        if accepted.state != ReplyState::Accepted || accepted.result != ReplyResult::Empty {
            panic!("accepted after a rejected terminal: {accepted:?}");
        }

        let mut tracker = SeqTracker::new(binding());
        bind(&mut tracker);
        need(tracker.note_request(&side_request(op, 1)));
        let bytes = side_accepted(op, 1);
        let accepted = observe_reply(&mut tracker, op, &bytes);
        if accepted.state != ReplyState::Accepted {
            panic!("accepted: {accepted:?}");
        }
        expect_shape(tracker.observe(op, &raw_terminal(op)));
        expect_shape(tracker.observe(op, &raw_terminal(op)));
        expect_fault(tracker.observe(op, &widen(&bytes)), SeqFault::Changed);
        match need(tracker.observe(op, &bytes)) {
            Observe::Pending {
                op: pending,
                request_seq,
            } if pending == op && request_seq == 1 => {}
            other => panic!("exact accepted should stay pending, got {other:?}"),
        }
        if op == Op::Bootstrap {
            inventory_row_does_not_complete_bootstrap(&mut tracker, &bytes);
        }
    }
}

#[test]
fn reply_rejects_wrong_echo_and_binding() {
    let mut early = SeqTracker::new(binding());
    expect_fault(early.note_request(&inv_with_boot()), SeqFault::Boot);
    need(early.note_request(&super::support::inv_request(1, None)));
    expect_fault(
        early.observe(Op::Inventory, &super::support::inv_terminal(1, None)),
        SeqFault::Boot,
    );

    let mut tracker = SeqTracker::new(binding());
    bind(&mut tracker);
    need(tracker.note_request(&probe_request(1)));
    expect_fault(tracker.observe(Op::Probe, &echo_mismatch()), SeqFault::Echo);
    expect_fault(
        tracker.observe(Op::Probe, &nonce_mismatch()),
        SeqFault::Binding,
    );
    expect_fault(
        tracker.observe(Op::Probe, &boot_text("other")),
        SeqFault::Boot,
    );
    expect_fault(
        tracker.observe(Op::Probe, &boot_text_none()),
        SeqFault::Boot,
    );
    need(tracker.note_request(&probe_request(2)));
    expect_fault(
        tracker.observe(Op::Probe, &probe_terminal(1, Check::Unknown)),
        SeqFault::WrongRequest,
    );
}

#[test]
fn identical_bytes_stay_pending() {
    let mut tracker = SeqTracker::new(binding());
    bind(&mut tracker);
    need(tracker.note_request(&probe_request(1)));
    let bytes = probe_terminal(1, Check::Passed);
    observe_reply(&mut tracker, Op::Probe, &bytes);
    match need(tracker.observe(Op::Probe, &bytes)) {
        Observe::Pending { op, request_seq } if op == Op::Probe && request_seq == 1 => {}
        other => panic!("same bytes should stay pending, got {other:?}"),
    }
    need(tracker.note_request(&probe_request(2)));
    match need(tracker.observe(Op::Probe, &bytes)) {
        Observe::Pending { op, request_seq } if op == Op::Probe && request_seq == 2 => {}
        other => panic!("stale exact bytes should name the current request, got {other:?}"),
    }
    if tracker.outstanding(Op::Probe) != 2 || tracker.lease_deadline_ms().is_some() {
        panic!("pending replay moved the attempt");
    }
}

#[test]
fn changed_bytes_same_seq_are_breach() {
    let mut tracker = SeqTracker::new(binding());
    bind(&mut tracker);
    need(tracker.note_request(&probe_request(1)));
    observe_reply(&mut tracker, Op::Probe, &probe_terminal(1, Check::Passed));
    expect_fault(
        tracker.observe(Op::Probe, &probe_terminal(1, Check::Failed)),
        SeqFault::Changed,
    );
}

#[test]
fn wrong_op_is_breach() {
    let mut tracker = SeqTracker::new(binding());
    bind(&mut tracker);
    need(tracker.note_request(&probe_request(1)));
    expect_fault(
        tracker.observe(Op::Login, &probe_terminal(1, Check::Passed)),
        SeqFault::WrongOp,
    );
}

#[test]
fn future_and_unsent_seq_are_breach() {
    let mut tracker = SeqTracker::new(binding());
    bind(&mut tracker);
    need(tracker.note_request(&probe_request(1)));
    expect_fault(
        tracker.observe(Op::Probe, &probe_terminal(2, Check::Passed)),
        SeqFault::Future,
    );
    expect_fault(
        tracker.observe(Op::Probe, &probe_at(0, 0)),
        SeqFault::Unsent,
    );
}

#[test]
fn stale_lease_ack_does_not_extend_deadline() {
    let mut tracker = SeqTracker::new(binding());
    bind(&mut tracker);
    let overflow_at = u64::MAX - (LEASE_WINDOW_MS - 1);
    expect_fault(
        tracker.note_request(&lease_request(1, overflow_at)),
        SeqFault::Overflow,
    );
    if tracker.outstanding(Op::Lease) != 0 || tracker.lease_deadline_ms().is_some() {
        panic!("overflowing lease was recorded");
    }
    need(tracker.note_request(&lease_request(1, 100)));
    if tracker.lease_deadline_ms() != Some(until_ms(100)) {
        panic!("first deadline: {:?}", tracker.lease_deadline_ms());
    }
    need(tracker.note_request(&lease_request(2, 500)));
    let latest = until_ms(500);
    if tracker.lease_deadline_ms() != Some(latest) {
        panic!("second deadline: {:?}", tracker.lease_deadline_ms());
    }
    let old = lease_terminal(1, 100, until_ms(100));
    match need(tracker.observe(Op::Lease, &old)) {
        Observe::DiscardedLease { seq: 1 } => {}
        other => panic!("stale ack: {other:?}"),
    }
    match need(tracker.observe(Op::Lease, &old)) {
        Observe::DiscardedLease { seq: 1 } => {}
        other => panic!("repeat stale ack: {other:?}"),
    }
    if tracker.lease_deadline_ms() != Some(latest) {
        panic!("stale ack moved the deadline");
    }
    let current = lease_terminal(2, 500, latest);
    let reply = observe_reply(&mut tracker, Op::Lease, &current);
    if reply.state != ReplyState::Terminal || reply.request_seq != 2 {
        panic!("current lease: {reply:?}");
    }
    if tracker.lease_deadline_ms() != Some(latest) {
        panic!("current ack moved the deadline");
    }
    match need(tracker.observe(Op::Lease, &current)) {
        Observe::Pending { op, request_seq } if op == Op::Lease && request_seq == 2 => {}
        other => panic!("repeat current ack: {other:?}"),
    }
    if tracker.lease_deadline_ms() != Some(latest) {
        panic!("repeat ack moved the deadline");
    }
}

#[test]
fn lease_content_mismatch_fails() {
    let mut tracker = SeqTracker::new(binding());
    bind(&mut tracker);
    need(tracker.note_request(&lease_request(1, 100)));
    need(tracker.note_request(&lease_request(2, 500)));
    let deadline = tracker.lease_deadline_ms();
    expect_fault(
        tracker.observe(Op::Lease, &lease_terminal(1, 100, until_ms(100) + 1)),
        SeqFault::LeaseMismatch,
    );
    expect_fault(
        tracker.observe(Op::Lease, &lease_terminal(2, 501, until_ms(500))),
        SeqFault::LeaseMismatch,
    );
    expect_fault(
        tracker.observe(Op::Lease, &lease_wrong_inventory()),
        SeqFault::LeaseMismatch,
    );
    if tracker.lease_deadline_ms() != deadline || tracker.outstanding(Op::Lease) != 2 {
        panic!("mismatch changed lease state");
    }
}

#[test]
fn inventory_and_lease_stop_at_2048() {
    let mut tracker = SeqTracker::new(binding());
    bind(&mut tracker);
    for seq in 2..=MAX_POLLS {
        let noted = need(tracker.note_request(&super::support::inv_request(seq, Some("boot1"))));
        if noted != seq {
            panic!("inventory {seq} recorded as {noted}");
        }
    }
    expect_fault(
        tracker.note_request(&super::support::inv_request(MAX_POLLS + 1, Some("boot1"))),
        SeqFault::TooMany,
    );
    if tracker.outstanding(Op::Inventory) != MAX_POLLS {
        panic!("inventory advanced past the cap");
    }
    for seq in 1..=MAX_POLLS {
        let noted = need(tracker.note_request(&lease_request(seq, seq)));
        if noted != seq {
            panic!("lease {seq} recorded as {noted}");
        }
    }
    let deadline = Some(until_ms(MAX_POLLS));
    if tracker.lease_deadline_ms() != deadline {
        panic!("last lease deadline: {:?}", tracker.lease_deadline_ms());
    }
    expect_fault(
        tracker.note_request(&lease_request(MAX_POLLS + 1, MAX_POLLS + 1)),
        SeqFault::TooMany,
    );
    if tracker.outstanding(Op::Lease) != MAX_POLLS || tracker.lease_deadline_ms() != deadline {
        panic!("lease advanced past the cap");
    }
}

#[test]
fn stale_exact_reply_names_the_current_request() {
    let mut tracker = SeqTracker::new(binding());
    bind(&mut tracker);
    need(tracker.note_request(&probe_request(1)));
    let bytes = probe_terminal(1, Check::Passed);
    observe_reply(&mut tracker, Op::Probe, &bytes);
    need(tracker.note_request(&probe_request(2)));
    match need(tracker.observe(Op::Probe, &bytes)) {
        Observe::Pending { op, request_seq } if op == Op::Probe && request_seq == 2 => {}
        other => panic!("stale reply: {other:?}"),
    }
    if tracker.outstanding(Op::Probe) != 2 {
        panic!("stale reply changed the outstanding probe");
    }
}

#[test]
fn stale_lease_ack_rejects_reencoded_bytes() {
    let mut current = SeqTracker::new(binding());
    bind(&mut current);
    need(current.note_request(&lease_request(1, 100)));
    let first = lease_terminal(1, 100, until_ms(100));
    let reply = observe_reply(&mut current, Op::Lease, &first);
    if reply.state != ReplyState::Terminal || reply.request_seq != 1 {
        panic!("current lease 1: {reply:?}");
    }
    need(current.note_request(&lease_request(2, 500)));
    let deadline = until_ms(500);
    if current.lease_deadline_ms() != Some(deadline) {
        panic!(
            "deadline before stale ack: {:?}",
            current.lease_deadline_ms()
        );
    }
    expect_fault(
        current.observe(Op::Lease, &widen(&first)),
        SeqFault::Changed,
    );
    expect_fault(
        current.observe(Op::Lease, &escape_boot(&first)),
        SeqFault::Changed,
    );
    match need(current.observe(Op::Lease, &first)) {
        Observe::DiscardedLease { seq: 1 } => {}
        other => panic!("original stale ack: {other:?}"),
    }
    if current.lease_deadline_ms() != Some(deadline) {
        panic!("changed stale ack moved the deadline");
    }

    let mut stale = SeqTracker::new(binding());
    bind(&mut stale);
    need(stale.note_request(&lease_request(1, 100)));
    need(stale.note_request(&lease_request(2, 500)));
    match need(stale.observe(Op::Lease, &first)) {
        Observe::DiscardedLease { seq: 1 } => {}
        other => panic!("first stale ack: {other:?}"),
    }
    expect_fault(stale.observe(Op::Lease, &widen(&first)), SeqFault::Changed);
    expect_fault(
        stale.observe(Op::Lease, &escape_boot(&first)),
        SeqFault::Changed,
    );
    match need(stale.observe(Op::Lease, &first)) {
        Observe::DiscardedLease { seq: 1 } => {}
        other => panic!("repeat original stale ack: {other:?}"),
    }
    if stale.lease_deadline_ms() != Some(deadline) {
        panic!("first-seen stale ack moved the deadline");
    }
    let second = lease_terminal(2, 500, deadline);
    let current_reply = observe_reply(&mut stale, Op::Lease, &second);
    if current_reply.state != ReplyState::Terminal || current_reply.request_seq != 2 {
        panic!("lease 2 after stale acks: {current_reply:?}");
    }
    if stale.lease_deadline_ms() != Some(deadline) {
        panic!("current ack moved the deadline");
    }
}

fn inventory_row_does_not_complete_bootstrap(tracker: &mut SeqTracker, accepted: &[u8]) {
    // M4 consumes this inventory row. M3 only keeps the side slot at accepted.
    need(tracker.note_request(&inv_request(2, Some("boot1"))));
    let frame = inv_with(
        2,
        Some("boot1"),
        activity(
            SnapStatus::Terminal,
            Some(SideOp::Bootstrap),
            1,
            Some(boot_ok(4)),
        ),
        revoke_row(SnapStatus::None, 0, None),
    );
    let reply = observe_reply(tracker, Op::Inventory, &frame);
    match reply.result {
        ReplyResult::Inventory(snap)
            if snap.active.status == SnapStatus::Terminal
                && snap.active.op == Some(SideOp::Bootstrap)
                && snap.active.request_seq == 1
                && snap.active.result == Some(boot_ok(4)) => {}
        other => panic!("inventory completion row: {other:?}"),
    }
    match need(tracker.observe(Op::Bootstrap, accepted)) {
        Observe::Pending { op, request_seq } if op == Op::Bootstrap && request_seq == 1 => {}
        other => panic!("bootstrap slot moved off accepted: {other:?}"),
    }
}

fn side_request(op: Op, seq: u64) -> Vec<u8> {
    encoded(&envelope(
        op,
        seq,
        Some("boot1"),
        MsgStatus::Request,
        Payload::Empty,
    ))
}

fn side_accepted(op: Op, seq: u64) -> Vec<u8> {
    encoded(&envelope(
        op,
        seq,
        Some("boot1"),
        MsgStatus::Accepted,
        Payload::Empty,
    ))
}

fn side_payload(op: Op) -> Payload {
    match op {
        Op::Bootstrap => Payload::BootstrapTerminal(need(ValueOutcome::new(Outcome::Ok, Some(4)))),
        Op::Login => Payload::LoginTerminal(need(ValueOutcome::new(
            Outcome::Ok,
            Some(LoginState::Ready),
        ))),
        Op::Run => Payload::RunTerminal(need(ValueOutcome::new(Outcome::Ok, Some(RunScore::Pass)))),
        Op::Revoke => Payload::RevokeTerminal(revoke_ok()),
        _ => panic!("not a side op"),
    }
}

fn raw_terminal(op: Op) -> Vec<u8> {
    let accepted = side_accepted(op, 1);
    let json = json_of(&accepted);
    let payload = match op {
        Op::Bootstrap => r#"{"outcome":"ok","native_ack_generation":4}"#,
        Op::Login => r#"{"outcome":"ok","login":"ready"}"#,
        Op::Run => r#"{"outcome":"ok","score":"pass"}"#,
        Op::Revoke => {
            r#"{"outcome":"ok","scope_revoked":true,"input_released":true,"tree_exited":true}"#
        }
        _ => panic!("not a side op"),
    };
    let replaced = json.replacen(
        "\"status\":\"accepted\",\"payload\":{}",
        &format!("\"status\":\"terminal\",\"payload\":{payload}"),
        1,
    );
    if replaced == json {
        panic!("accepted frame had no status marker: {json}");
    }
    reprefix(&replaced)
}

fn widen(frame: &[u8]) -> Vec<u8> {
    let json = json_of(frame);
    let mut widened = String::new();
    let mut done = false;
    for ch in json.chars() {
        widened.push(ch);
        if !done && ch == ':' {
            widened.push(' ');
            done = true;
        }
    }
    reprefix(&widened)
}

fn escape_boot(frame: &[u8]) -> Vec<u8> {
    let json = json_of(frame);
    let replaced = json.replacen("\"boot1\"", "\"\\u0062oot1\"", 1);
    if replaced == json {
        panic!("frame has no boot1");
    }
    reprefix(&replaced)
}

fn reprefix(json: &str) -> Vec<u8> {
    let len = match u32::try_from(json.len()) {
        Ok(len) => len,
        Err(_) => panic!("frame too long"),
    };
    let mut out = Vec::with_capacity(json.len().saturating_add(4));
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(json.as_bytes());
    out
}

fn expect_shape<T: std::fmt::Debug>(result: Result<T, MailboxError>) {
    match result {
        Err(MailboxError::Malformed(CodecFault::Shape)) => {}
        other => panic!("expected shape, got {other:?}"),
    }
}

fn probe_at(seq: u64, request_seq: u64) -> Vec<u8> {
    let mut env = envelope(
        Op::Probe,
        seq,
        Some("boot1"),
        MsgStatus::Terminal,
        Payload::ProbeTerminal(Check::Passed),
    );
    env.request_seq = request_seq;
    encoded(&env)
}

fn echo_mismatch() -> Vec<u8> {
    probe_at(1, 2)
}

fn nonce_mismatch() -> Vec<u8> {
    let mut env = envelope(
        Op::Probe,
        1,
        Some("boot1"),
        MsgStatus::Terminal,
        Payload::ProbeTerminal(Check::Passed),
    );
    env.attempt_nonce = super::super::ids::AttemptNonce::from_bytes([0x33; 16]);
    encoded(&env)
}

fn boot_text(boot: &str) -> Vec<u8> {
    encoded(&envelope(
        Op::Probe,
        1,
        Some(boot),
        MsgStatus::Terminal,
        Payload::ProbeTerminal(Check::Passed),
    ))
}

fn boot_text_none() -> Vec<u8> {
    encoded(&envelope(
        Op::Probe,
        1,
        None,
        MsgStatus::Terminal,
        Payload::ProbeTerminal(Check::Passed),
    ))
}

fn inv_with_boot() -> Vec<u8> {
    super::support::inv_request(1, Some("boot1"))
}

fn lease_wrong_inventory() -> Vec<u8> {
    encoded(&envelope(
        Op::Lease,
        2,
        Some("boot1"),
        MsgStatus::Terminal,
        Payload::LeaseTerminal(super::super::ops::LeaseTerminal {
            inventory_request_seq: 9,
            sample_ms: 500,
            until_ms: until_ms(500),
        }),
    ))
}

fn observe_reply(
    tracker: &mut SeqTracker,
    op: Op,
    bytes: &[u8],
) -> super::super::ops::MailboxReply {
    match need(tracker.observe(op, bytes)) {
        Observe::Reply(reply) => reply,
        other => panic!("expected reply, got {other:?}"),
    }
}

fn expect_fault<T: std::fmt::Debug>(result: Result<T, MailboxError>, fault: SeqFault) {
    match result {
        Err(MailboxError::Sequence(got)) if got == fault => {}
        other => panic!("expected {fault:?}, got {other:?}"),
    }
}
