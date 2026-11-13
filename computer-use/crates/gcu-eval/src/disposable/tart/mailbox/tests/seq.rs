//! Sequence, boot, and lease rules. No filesystem.

use super::super::ops::{
    MsgStatus, Op, Outcome, Payload, ReplyResult, ReplyState, ValueOutcome, LEASE_WINDOW_MS,
    MAX_POLLS,
};
use super::super::seq::{succ, Observe, SeqTracker};
use super::super::{MailboxError, SeqFault};
use super::support::{
    bind, binding, encoded, envelope, lease_request, lease_terminal, need, probe_request,
    probe_terminal, until_ms,
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
fn accepted_then_terminal_is_completion() {
    let mut tracker = SeqTracker::new(binding());
    bind(&mut tracker);
    need(tracker.note_request(&bootstrap_request(1)));
    let accepted = observe_reply(&mut tracker, Op::Bootstrap, &bootstrap_accepted(1));
    if accepted.state != ReplyState::Accepted || accepted.result != ReplyResult::Empty {
        panic!("accepted: {accepted:?}");
    }
    let done = bootstrap_terminal(1, Outcome::Ok, Some(4));
    let terminal = observe_reply(&mut tracker, Op::Bootstrap, &done);
    if terminal.state != ReplyState::Terminal || terminal.request_seq != 1 {
        panic!("terminal: {terminal:?}");
    }
    match need(tracker.observe(Op::Bootstrap, &done)) {
        Observe::Pending { op, request_seq } if op == Op::Bootstrap && request_seq == 1 => {}
        other => panic!("repeat terminal should stay pending, got {other:?}"),
    }
    expect_fault(
        tracker.observe(Op::Bootstrap, &bootstrap_terminal(1, Outcome::Failed, None)),
        SeqFault::Changed,
    );
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
        Observe::Pending { op, request_seq } if op == Op::Probe && request_seq == 1 => {}
        other => panic!("stale exact bytes should stay pending, got {other:?}"),
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

fn bootstrap_request(seq: u64) -> Vec<u8> {
    encoded(&envelope(
        Op::Bootstrap,
        seq,
        Some("boot1"),
        MsgStatus::Request,
        Payload::Empty,
    ))
}

fn bootstrap_accepted(seq: u64) -> Vec<u8> {
    encoded(&envelope(
        Op::Bootstrap,
        seq,
        Some("boot1"),
        MsgStatus::Accepted,
        Payload::Empty,
    ))
}

fn bootstrap_terminal(seq: u64, outcome: Outcome, value: Option<u64>) -> Vec<u8> {
    encoded(&envelope(
        Op::Bootstrap,
        seq,
        Some("boot1"),
        MsgStatus::Terminal,
        Payload::BootstrapTerminal(need(ValueOutcome::new(outcome, value))),
    ))
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
