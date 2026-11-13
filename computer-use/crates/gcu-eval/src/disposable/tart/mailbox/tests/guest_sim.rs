#![cfg(test)]
//! Synchronous fake guest. It reads host slots and publishes replies with
//! tmp then rename. Time does not pass here; the caller owns the clock.
//!
//! No thread, no sleep, and no process.

#![cfg(unix)]

use std::cell::{Cell, RefCell};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::fd::{AsRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use std::ffi::CString;
use std::num::NonZeroU64;

use super::super::super::deadline::{
    FakeClock, HostClock, LeaseDeadline, LoginDeadline, ProbeDeadline, QueryDeadline,
    RevokeDeadline, RunDeadline, StageDeadline,
};
use super::super::super::guest::{ConsumeNote, GuestAccounting, GuestChannel};
use super::super::super::mailbox_guest::MailboxGuestChannel;
use super::super::super::manifest::Digest;
use super::super::super::output::{BoundedOutput, CallError};
use super::super::super::schedule::{
    attempt_window_ms, prove, ScheduleError, ACCEPT_AGE_MS, EXTRA_SAMPLES, GRANT_GAP_MS,
    ONE_WAY_MS, POLL_BYTE_BUDGET, RENEW_MS, ROUND_TRIP_MAX_MS, STEP_MS,
};
use super::super::codec::{decode_frame, decode_reply};
use super::super::ops::{
    InventoryConsume, InventorySnapshot, LeaseRequest, LeaseTerminal, LoginState, MsgStatus, Op,
    Outcome, Payload, ReplyResult, ReplyState, SideOp, SideResult, SnapStatus, ValueOutcome,
    LEASE_WINDOW_MS, MAX_POLLS,
};
use super::super::slots::{
    arm, prepare_slots, IoPoint, GUEST_WRITE_DIR, HOST_WRITE_DIR, MAILBOX_DIR,
};
use super::super::sys;
use super::support::{
    activity, binding, boot_ok, encoded, envelope, id, inv_terminal, probe_terminal, revoke_ok,
    revoke_row,
};
use crate::disposable::{Check, Probe};

#[test]
fn repeat_poll_does_not_resend() {
    let mut rig = Rig::open();
    let first = ok(rig.inventory());
    assert_pending(&first, Op::Inventory);
    let host = rig.host(Op::Inventory);
    let env = decode_host(&host);
    if env.seq != 1 || env.boot_id.is_some() || env.status != MsgStatus::Request {
        panic!("first inventory request: {env:?}");
    }
    let second = ok(rig.inventory());
    assert_pending(&second, Op::Inventory);
    if rig.host(Op::Inventory) != host {
        panic!("repeat poll published a new inventory");
    }
}

#[test]
fn delayed_reply_stays_pending_for_n_polls() {
    let mut rig = Rig::open();
    let host = publish_pending(&mut rig, Op::Inventory);
    let _ = ok(rig.inventory());
    let _ = ok(rig.inventory());
    if rig.host(Op::Inventory) != host {
        panic!("delayed polls republished");
    }
    rig.publish_guest(Op::Inventory, &inv_terminal(1, Some("boot1")));
    assert_terminal_op(&ok(rig.inventory()), Op::Inventory);
    if rig.host(Op::Inventory) != host {
        panic!("the completing poll republished");
    }
}

#[test]
fn enoent_once_is_pending_not_sticky() {
    let mut rig = Rig::open();
    let _ = ok(rig.inventory());
    rig.remove_guest(Op::Inventory);
    assert_pending(&ok(rig.inventory()), Op::Inventory);
    assert_pending(&ok(rig.inventory()), Op::Inventory);
    rig.publish_guest(Op::Inventory, &inv_terminal(1, Some("boot1")));
    assert_terminal_op(&ok(rig.inventory()), Op::Inventory);
    rig.clear_guest(Op::Inventory);
    assert_pending(&ok(rig.inventory()), Op::Inventory);
}

#[test]
fn repeated_same_bytes_do_not_complete_the_next_request() {
    let mut rig = Rig::open();
    bind(&mut rig);
    let replay = rig.guest_bytes(Op::Inventory);
    let _ = ok(rig.inventory());
    let host = rig.host(Op::Inventory);
    let env = decode_host(&host);
    if env.seq != 2 {
        panic!("expected a new inventory, got seq {}", env.seq);
    }
    assert_pending(&ok(rig.inventory()), Op::Inventory);
    if rig.host(Op::Inventory) != host || rig.guest_bytes(Op::Inventory) != replay {
        panic!("same-byte replay republished or replaced the guest file");
    }
}

#[test]
fn changed_bytes_fail_closed() {
    let mut rig = Rig::open();
    bind(&mut rig);
    rig.publish_guest(Op::Inventory, &plain_terminal(1, "boot1", 11));
    let err = err_of(rig.inventory());
    assert_failed(&err, b"mailbox");
    let again = err_of(rig.inventory());
    if again != err {
        panic!("changed-byte breach was not sticky: {again:?}");
    }
}

#[test]
fn boot_change_revokes() {
    let mut rig = Rig::open();
    bind(&mut rig);
    let _ = ok(rig.inventory());
    let host = rig.host(Op::Inventory);
    let env = decode_host(&host);
    if env.boot_id.as_ref().map(|boot| boot.as_str()) != Some("boot1") || env.seq != 2 {
        panic!("bound request: {env:?}");
    }
    rig.publish_guest(Op::Inventory, &plain_terminal(2, "boot2", 10));
    let err = err_of(rig.inventory());
    assert_failed(&err, b"boot-changed");
    if err_of(rig.inventory()) != err {
        panic!("boot change was not sticky");
    }
    if rig.channel.bytes_read() == 0 {
        panic!("boot change recorded no bytes");
    }
}

#[test]
fn mismatched_lease_ack_fails_closed() {
    let mut rig = Rig::open();
    bind(&mut rig);
    let seq = seq_of(1);
    assert_pending(&ok(rig.lease(seq)), Op::Lease);
    let sample = 10;
    rig.publish_guest(
        Op::Lease,
        &lease_frame(1, 1, sample, sample + LEASE_WINDOW_MS),
    );
    let good = decode_reply_of(&ok(rig.lease(seq)));
    if good.state != ReplyState::Terminal || good.request_seq != 1 {
        panic!("matching lease was not accepted: {good:?}");
    }
    let next = seq_of(2);
    assert_pending(&ok(rig.lease(next)), Op::Lease);
    let host = rig.host(Op::Lease);
    rig.publish_guest(Op::Lease, &lease_frame(2, 1, sample, sample));
    let err = err_of(rig.lease(next));
    assert_failed(&err, b"mailbox");
    if rig.host(Op::Lease) != host || err_of(rig.lease(next)) != err {
        panic!("mismatched lease ack published again or did not stick");
    }
}

#[test]
fn lease_before_sample_and_wrong_seq_stick() {
    let mut early = Rig::open();
    let seq = seq_of(1);
    let err = err_of(early.lease(seq));
    assert_failed(&err, b"lease-unsampled");
    if err_of(early.lease(seq)) != err {
        panic!("unsampled lease was not sticky");
    }

    let mut rig = Rig::open();
    bind(&mut rig);
    let wrong = seq_of(5);
    let err = err_of(rig.lease(wrong));
    assert_failed(&err, b"lease-seq");
    if err_of(rig.lease(seq)) != err {
        panic!("lease seq breach was not sticky");
    }
}

#[test]
fn accepted_side_effect_completes_through_inventory() {
    let mut rig = Rig::open();
    bind(&mut rig);
    assert_pending(&ok(rig.probe(Probe::Sandbox)), Op::Probe);
    let probe_host = rig.host(Op::Probe);
    assert_pending(&ok(rig.probe(Probe::Sandbox)), Op::Probe);
    if rig.host(Op::Probe) != probe_host {
        panic!("probe poll republished");
    }
    rig.publish_guest(Op::Probe, &probe_terminal(1, Check::Passed));
    let probe = decode_reply_of(&ok(rig.probe(Probe::Sandbox)));
    if probe.state != ReplyState::Terminal {
        panic!("probe did not finish: {probe:?}");
    }

    assert_pending(&ok(rig.bootstrap()), Op::Bootstrap);
    let boot_host = rig.host(Op::Bootstrap);
    rig.publish_guest(Op::Bootstrap, &accepted_frame(Op::Bootstrap, 1));
    assert_accepted(&ok(rig.bootstrap()));
    if rig.host(Op::Bootstrap) != boot_host {
        panic!("accepted bootstrap republished");
    }
    assert_failed(&err_of(rig.login()), b"effect-busy");
    assert_failed(&err_of(rig.login()), b"effect-busy");
    assert_pending(&ok(rig.revoke()), Op::Revoke);
    let revoke_host = rig.host(Op::Revoke);
    rig.publish_guest(Op::Revoke, &accepted_frame(Op::Revoke, 1));
    assert_accepted(&ok(rig.revoke()));
    if rig.host(Op::Revoke) != revoke_host {
        panic!("revoke republished");
    }

    report_bootstrap(&mut rig, 1);
    assert_accepted_login(&mut rig);
}

#[test]
fn run_after_the_ordinary_row_is_terminal() {
    let mut rig = Rig::open();
    bind(&mut rig);
    accept_bootstrap(&mut rig);
    report_bootstrap(&mut rig, 1);
    assert_pending(&ok(rig.run()), Op::Run);
    let host = rig.host(Op::Run);
    rig.publish_guest(Op::Run, &accepted_frame(Op::Run, 1));
    assert_accepted(&ok(rig.run()));
    if rig.host(Op::Run) != host {
        panic!("accepted run republished");
    }
}

#[test]
fn consume_confirm_is_exact_idempotent_and_rejecting() {
    let mut rig = Rig::open();
    bind(&mut rig);
    accept_bootstrap(&mut rig);
    report_bootstrap(&mut rig, 1);
    note_bootstrap(&mut rig, 1);
    let seq = next_inventory(&mut rig);
    assert_consume(
        &decode_host(&rig.host(Op::Inventory)),
        Some(SideOp::Bootstrap),
        1,
    );
    rig.publish_guest(Op::Inventory, &bootstrap_terminal(seq, "boot1", 1));
    assert_terminal_op(&ok(rig.inventory()), Op::Inventory);
    let seq = next_inventory(&mut rig);
    assert_consume(
        &decode_host(&rig.host(Op::Inventory)),
        Some(SideOp::Bootstrap),
        1,
    );
    rig.publish_guest(Op::Inventory, &bootstrap_terminal(seq, "boot1", 1));
    assert_terminal_op(&ok(rig.inventory()), Op::Inventory);
    let seq = next_inventory(&mut rig);
    assert_consume(
        &decode_host(&rig.host(Op::Inventory)),
        Some(SideOp::Bootstrap),
        1,
    );
    rig.publish_guest(Op::Inventory, &plain_terminal(seq, "boot1", 10));
    assert_terminal_op(&ok(rig.inventory()), Op::Inventory);
    let seq = next_inventory(&mut rig);
    assert_consume(&decode_host(&rig.host(Op::Inventory)), None, 0);
    if seq < 4 {
        panic!("consume round did not publish follow-up inventories");
    }

    let mut bad = Rig::open();
    bind(&mut bad);
    accept_bootstrap(&mut bad);
    report_bootstrap(&mut bad, 1);
    note_bootstrap(&mut bad, 1);
    let seq = next_inventory(&mut bad);
    bad.publish_guest(Op::Inventory, &bootstrap_terminal(seq, "boot1", 2));
    let err = err_of(bad.inventory());
    assert_failed(&err, b"consume");
    let bytes = bad.channel.bytes_read();
    if err_of(bad.inventory()) != err || bad.channel.bytes_read() != bytes {
        panic!("wrong consume confirm was not sticky");
    }
}

#[test]
fn poll_byte_budget_fails_closed() {
    let mut rig = Rig::open();
    assert_pending(&ok(rig.inventory()), Op::Inventory);
    let dest = rig.root.message(GUEST_WRITE_DIR, Op::Inventory.file_name());
    let dir = rig.root.path().to_path_buf();
    if let Err(err) = fs::write(&dest, vec![b'a'; 4_000]) {
        panic!("seed: {err}");
    }
    let n = Rc::new(Cell::new(0u32));
    let counter = Rc::clone(&n);
    let hook_dest = dest.clone();
    let _guard = arm(IoPoint::AfterRead, move || {
        let i = counter.get();
        counter.set(i.saturating_add(1));
        let spare = dir.join(format!("spare-{i}"));
        let mut bytes = vec![b'b'; 4_000];
        bytes[0] = u8::try_from(i % 200).unwrap_or(1);
        if let Err(err) = fs::write(&spare, &bytes) {
            panic!("spare: {err}");
        }
        if let Err(err) = fs::rename(&spare, &hook_dest) {
            panic!("rename spare: {err}");
        }
    });
    let err = err_of(rig.inventory());
    assert_failed(&err, b"poll-budget");
    let charged = 32 * 4_000;
    if n.get() != 32 || rig.channel.bytes_read() != charged {
        panic!(
            "replaced 4000-byte frames were read past the remainder: hooks={} bytes={}",
            n.get(),
            rig.channel.bytes_read()
        );
    }
    if err_of(rig.inventory()) != err || rig.channel.bytes_read() != charged {
        panic!("byte budget was not sticky");
    }
}

#[test]
fn poll_cap_2048_fails_closed() {
    let mut rig = Rig::open();
    assert_pending(&ok(rig.inventory()), Op::Inventory);
    rig.publish_guest(Op::Inventory, &inv_terminal(1, Some("boot1")));
    assert_terminal_op(&ok(rig.inventory()), Op::Inventory);
    for seq in 2..=MAX_POLLS {
        rig.clear_guest(Op::Inventory);
        assert_pending(&ok(rig.inventory()), Op::Inventory);
        let env = decode_host(&rig.host(Op::Inventory));
        if env.seq != seq || env.boot_id.as_ref().map(|boot| boot.as_str()) != Some("boot1") {
            panic!("request {seq}: {env:?}");
        }
        rig.publish_guest(Op::Inventory, &inv_terminal(seq, Some("boot1")));
        assert_terminal_op(&ok(rig.inventory()), Op::Inventory);
    }
    let bytes = rig.channel.bytes_read();
    if bytes <= POLL_BYTE_BUDGET {
        panic!("completed schedule stayed inside one read budget: {bytes}");
    }
    let frozen = rig.host(Op::Inventory);
    let err = err_of(rig.inventory());
    assert_failed(&err, b"poll-cap");
    if rig.host(Op::Inventory) != frozen || decode_host(&frozen).seq != MAX_POLLS {
        panic!(
            "poll cap published request {}",
            decode_host(&rig.host(Op::Inventory)).seq
        );
    }
    if err_of(rig.inventory()) != err || rig.channel.bytes_read() != bytes {
        panic!("poll cap was not sticky");
    }
}

#[test]
fn foreign_run_nonce_and_digest_are_rejected() {
    let mut run = envelope(
        Op::Inventory,
        2,
        Some("boot1"),
        MsgStatus::Terminal,
        Payload::InventoryTerminal(empty_snap(10)),
    );
    run.run_id = id("run2");
    reject_binding(run);
    let mut nonce = envelope(
        Op::Inventory,
        2,
        Some("boot1"),
        MsgStatus::Terminal,
        Payload::InventoryTerminal(empty_snap(10)),
    );
    nonce.attempt_nonce = super::super::ids::AttemptNonce::from_bytes([0x99; 16]);
    reject_binding(nonce);
    let mut digest = envelope(
        Op::Inventory,
        2,
        Some("boot1"),
        MsgStatus::Terminal,
        Payload::InventoryTerminal(empty_snap(10)),
    );
    digest.vm_config_digest = need_digest([0x44; 32]);
    reject_binding(digest);
}

#[test]
fn unissued_rows_and_simultaneous_consume() {
    let mut rig = Rig::open();
    bind(&mut rig);
    let seq = next_inventory(&mut rig);
    rig.publish_guest(Op::Inventory, &bootstrap_terminal(seq, "boot1", 1));
    let err = err_of(rig.inventory());
    assert_failed(&err, b"unissued");
    if err_of(rig.inventory()) != err {
        panic!("unissued active row was not sticky");
    }

    let mut both = Rig::open();
    bind(&mut both);
    accept_bootstrap(&mut both);
    let _ = ok(both.revoke());
    both.publish_guest(Op::Revoke, &accepted_frame(Op::Revoke, 1));
    assert_accepted(&ok(both.revoke()));
    report_both(&mut both);
    if let Err(err) = both.channel.note_consumed(ConsumeNote {
        active_kind: 1,
        active_seq: 1,
        revoke_seq: 1,
    }) {
        panic!("note both: {err:?}");
    }
    let seq = next_inventory(&mut both);
    match decode_host(&both.host(Op::Inventory)).payload {
        Payload::InventoryRequest(consume)
            if consume.op == Some(SideOp::Bootstrap)
                && consume.request_seq == 1
                && consume.revoke_seq == 1 => {}
        other => panic!("simultaneous consume: {other:?}"),
    }
    both.publish_guest(Op::Inventory, &plain_terminal(seq, "boot1", 10));
    assert_terminal_op(&ok(both.inventory()), Op::Inventory);
    let _ = seq;
}

#[test]
fn sync_guest_samples_expire_and_rejects_bad_consume() {
    let mut rig = Rig::open();
    let guest = SyncGuest::new(&rig, 50_000);
    assert_pending(&ok(rig.inventory()), Op::Inventory);
    guest.hold_inventory(1_000);
    if let Err(err) = guest.step() {
        panic!("hold: {err}");
    }
    if rig.guest_bytes(Op::Inventory).is_empty() {
        // The pre-created slot is empty until the guest publishes.
    } else {
        panic!("guest published before visibility");
    }
    if rig
        .clock
        .advance(std::time::Duration::from_millis(1_000))
        .is_err()
    {
        panic!("visibility");
    }
    if let Err(err) = guest.step() {
        panic!("publish sample: {err}");
    }
    let body = ok(rig.inventory());
    let reply = decode_reply_of(&body);
    let ReplyResult::Inventory(snap) = reply.result else {
        panic!("sample reply: {reply:?}");
    };
    if snap.sample_ms != 50_000 || guest.generated_host_ms() != 0 {
        panic!(
            "sample {} generated at {}",
            snap.sample_ms,
            guest.generated_host_ms()
        );
    }
    let seq = seq_of(1);
    assert_pending(&ok(rig.lease(seq)), Op::Lease);
    if rig
        .clock
        .advance(std::time::Duration::from_millis(100))
        .is_err()
    {
        panic!("service");
    }
    if let Err(err) = guest.step() {
        panic!("ack: {err}");
    }
    let acked = decode_reply_of(&ok(rig.lease(seq)));
    let ReplyResult::Lease(term) = acked.result else {
        panic!("ack: {acked:?}");
    };
    let age = ms(&rig.clock) - guest.generated_host_ms();
    if age != 1_100 || term.until_ms != snap.sample_ms + LEASE_WINDOW_MS || guest.acks() != 1 {
        panic!("age {age} until {} acks {}", term.until_ms, guest.acks());
    }
    if rig
        .clock
        .advance(std::time::Duration::from_millis(LEASE_WINDOW_MS))
        .is_err()
    {
        panic!("expire");
    }
    let next = seq_of(2);
    assert_pending(&ok(rig.lease(next)), Op::Lease);
    if let Err(err) = guest.step() {
        panic!("expired: {err}");
    }
    if guest.acks() != 1 || !guest.expired() {
        panic!("lease expiry was reversible");
    }
    let tmp = rig.root.message(GUEST_WRITE_DIR, Op::Inventory.tmp_name());
    if let Err(err) = fs::write(&tmp, b"kept") {
        panic!("plant tmp: {err}");
    }
    let blocked = guest.step();
    let still = fs::read(&tmp).ok();
    if blocked.is_ok() || still.as_deref() != Some(b"kept".as_slice()) {
        panic!("tmp existed and was not left in place: {blocked:?} {still:?}");
    }
    let _ = fs::remove_file(&tmp);
    let bad = encode_consume_request(3, SideOp::Bootstrap, 1, 0);
    if let Err(err) = fs::write(
        rig.root.message(HOST_WRITE_DIR, Op::Inventory.file_name()),
        &bad,
    ) {
        panic!("bad consume: {err}");
    }
    if guest.step().is_ok() {
        panic!("bad consume was accepted");
    }
}

fn step(guest: &SyncGuest) {
    if let Err(err) = guest.step() {
        panic!("guest step: {err}");
    }
}

fn advance_to(rig: &Rig, target: u64) {
    let now = ms(&rig.clock);
    if target < now {
        panic!("clock is {now}, cannot reach {target}");
    }
    if target == now {
        return;
    }
    if rig
        .clock
        .advance(Duration::from_millis(target - now))
        .is_err()
    {
        panic!("advance to {target}");
    }
}

fn inventory_snap(body: &BoundedOutput) -> InventorySnapshot {
    match decode_reply_of(body).result {
        ReplyResult::Inventory(snap) => snap,
        other => panic!("inventory snap: {other:?}"),
    }
}

fn lease_request_of(bytes: &[u8]) -> LeaseRequest {
    match decode_host(bytes).payload {
        Payload::LeaseRequest(req) => req,
        other => panic!("lease request: {other:?}"),
    }
}

#[test]
fn asymmetric_900_100_records_age_and_stops_the_tree() {
    let mut rig = Rig::open();
    let guest = SyncGuest::new(&rig, 0);
    guest.schedule();

    advance_to(&rig, 0);
    assert_pending(&ok(rig.inventory()), Op::Inventory);
    if decode_host(&rig.host(Op::Inventory)).seq != 1 {
        panic!("inventory 1 was not published");
    }
    advance_to(&rig, 900);
    step(&guest);
    advance_to(&rig, 1_000);
    let first = inventory_snap(&ok(rig.inventory()));
    if first.sample_ms != 900 {
        panic!("first sample {}", first.sample_ms);
    }

    advance_to(&rig, 1_100);
    rig.clear_guest(Op::Inventory);
    assert_pending(&ok(rig.inventory()), Op::Inventory);
    if decode_host(&rig.host(Op::Inventory)).seq != 2 {
        panic!("inventory 2 was not published at the service step");
    }
    assert_pending(&ok(rig.lease(seq_of(1))), Op::Lease);

    advance_to(&rig, 2_000);
    step(&guest);
    let samples = guest.samples();
    let accepts = guest.accepts();
    if samples.len() != 2
        || samples.first().copied()
            != Some(SampleSeen {
                request_seq: 1,
                sample_ms: 900,
                host_ms: 900,
            })
        || samples.get(1).copied()
            != Some(SampleSeen {
                request_seq: 2,
                sample_ms: 2_000,
                host_ms: 2_000,
            })
    {
        panic!("samples {samples:?}");
    }
    let Some(accept) = accepts.first().copied() else {
        panic!("lease 1 was not accepted");
    };
    let age = accept.host_ms.saturating_sub(900);
    if accepts.len() != 1 || accept.host_ms != 2_000 || age != 1_100 || accept.until_ms != 2_900 {
        panic!("lease 1 age {age} accepts {accepts:?}");
    }
    if guest.stop_deadline() != Some(2_900) || guest.stopped() {
        panic!(
            "stop {:?} stopped {}",
            guest.stop_deadline(),
            guest.stopped()
        );
    }

    advance_to(&rig, 2_100);
    let second = inventory_snap(&ok(rig.inventory()));
    if second.sample_ms != 2_000 {
        panic!("inventory 2 reached the host as {}", second.sample_ms);
    }
    let acked = decode_reply_of(&ok(rig.lease(seq_of(1))));
    let ReplyResult::Lease(term) = acked.result else {
        panic!("lease 1 ack: {acked:?}");
    };
    if term.until_ms != 2_900 || term.sample_ms != 900 || term.inventory_request_seq != 1 {
        panic!("host observed {term:?}");
    }

    advance_to(&rig, 2_200);
    assert_pending(&ok(rig.lease(seq_of(2))), Op::Lease);
    if decode_host(&rig.host(Op::Lease)).seq != 2 {
        panic!("lease 2 was not published");
    }

    advance_to(&rig, 2_900);
    step(&guest);
    if !guest.stopped() || guest.acks() != 1 || guest.stop_deadline() != Some(2_900) {
        panic!(
            "tree did not stop at the first grant: stopped {} acks {} deadline {:?}",
            guest.stopped(),
            guest.acks(),
            guest.stop_deadline()
        );
    }
    advance_to(&rig, 3_100);
    step(&guest);
    if guest.acks() != 1 || guest.accepts().len() != 1 || guest.samples().len() != 2 {
        panic!(
            "late grant changed the trace: acks {} accepts {} samples {}",
            guest.acks(),
            guest.accepts().len(),
            guest.samples().len()
        );
    }
}

#[test]
fn observed_1023_misses_the_guest_deadline() {
    let mut rig = Rig::open();
    let guest = SyncGuest::new(&rig, 0);
    guest.schedule();
    guest.see_lease_after(2_146);

    advance_to(&rig, 0);
    assert_pending(&ok(rig.inventory()), Op::Inventory);
    step(&guest);
    if guest.samples().first().copied()
        != Some(SampleSeen {
            request_seq: 1,
            sample_ms: 0,
            host_ms: 0,
        })
    {
        panic!("sample {:?}", guest.samples());
    }
    advance_to(&rig, 1_023);
    let snap = inventory_snap(&ok(rig.inventory()));
    if snap.sample_ms != 0 {
        panic!("host read sample {}", snap.sample_ms);
    }
    advance_to(&rig, 1_123);
    assert_pending(&ok(rig.lease(seq_of(1))), Op::Lease);
    if lease_request_of(&rig.host(Op::Lease)).sample_ms != 0 {
        panic!("grant did not use the observed sample");
    }

    advance_to(&rig, 1_999);
    step(&guest);
    if guest.stopped() || guest.acks() != 0 {
        panic!("grant was treated as visible before 2146 ms");
    }
    advance_to(&rig, 2_000);
    step(&guest);
    if !guest.stopped() || guest.acks() != 0 || guest.stop_deadline() != Some(2_000) {
        panic!(
            "1023 ms legs: stopped {} acks {} deadline {:?}",
            guest.stopped(),
            guest.acks(),
            guest.stop_deadline()
        );
    }
    advance_to(&rig, 2_146);
    step(&guest);
    if guest.acks() != 0 || !guest.accepts().is_empty() {
        panic!("grant visible at 2146 ms was accepted after the stop");
    }
}

#[test]
fn jitter_slips_accept_age_by_one_step() {
    let mut rig = Rig::open();
    let guest = SyncGuest::new(&rig, 0);
    guest.schedule();
    advance_to(&rig, 0);
    assert_pending(&ok(rig.inventory()), Op::Inventory);
    advance_to(&rig, 900);
    step(&guest);
    advance_to(&rig, 1_000);
    let snap = inventory_snap(&ok(rig.inventory()));
    if snap.sample_ms != 900 {
        panic!("sample {}", snap.sample_ms);
    }
    advance_to(&rig, 1_200);
    assert_pending(&ok(rig.lease(seq_of(1))), Op::Lease);
    advance_to(&rig, 2_100);
    step(&guest);
    let Some(accept) = guest.accepts().first().copied() else {
        panic!("jitter did not accept");
    };
    let age = accept.host_ms.saturating_sub(900);
    if guest.accepts().len() != 1
        || accept.host_ms != 2_100
        || age != 1_200
        || accept.until_ms != 2_900
    {
        panic!("jitter age {age} {:?}", guest.accepts());
    }
    if guest.stop_deadline() != Some(2_900) {
        panic!("jitter moved the stop {:?}", guest.stop_deadline());
    }
}

#[test]
fn short_delay_window_keeps_grant_continuity() {
    const CYCLES: u64 = 40;
    const HG: u64 = 100;
    const GH: u64 = 100;
    const SERVICE: u64 = 100;
    const PERIOD: u64 = 800;
    const SKIP_K: u64 = 6;
    const ENOENT_K: u64 = 3;

    let mut rig = Rig::open();
    let guest = SyncGuest::new(&rig, 0);
    guest.schedule();
    guest.skip_lease(SKIP_K + 1);

    let mut prev_accept: Option<u64> = None;
    for k in 0..CYCLES {
        let base = k * PERIOD;
        advance_to(&rig, base);
        rig.clear_guest(Op::Inventory);
        assert_pending(&ok(rig.inventory()), Op::Inventory);
        let inv_seq = decode_host(&rig.host(Op::Inventory)).seq;
        if inv_seq != k + 1 {
            panic!("inventory seq {inv_seq} at cycle {k}");
        }

        advance_to(&rig, base + HG);
        step(&guest);
        if k == ENOENT_K {
            rig.remove_guest(Op::Inventory);
            assert_pending(&ok(rig.inventory()), Op::Inventory);
            step(&guest);
            if guest.enoent() < 1 {
                panic!("enoent was not counted");
            }
        }
        advance_to(&rig, base + HG + GH);
        let snap = inventory_snap(&ok(rig.inventory()));
        let sample_ms = base + HG;
        if snap.sample_ms != sample_ms {
            panic!("cycle {k} sample {} want {sample_ms}", snap.sample_ms);
        }

        advance_to(&rig, base + HG + GH + SERVICE);
        let lease_seq = k + 1;
        assert_pending(&ok(rig.lease(seq_of(lease_seq))), Op::Lease);
        let req = lease_request_of(&rig.host(Op::Lease));
        if req.inventory_request_seq != inv_seq || req.sample_ms != sample_ms {
            panic!("cycle {k} grant {req:?}");
        }
        if rig.channel.host_grant_end_ms() != Some(sample_ms + LEASE_WINDOW_MS) {
            panic!("cycle {k} host end {:?}", rig.channel.host_grant_end_ms());
        }

        let accept_at = base + HG + GH + SERVICE + HG;
        advance_to(&rig, accept_at);
        step(&guest);
        if k == SKIP_K {
            if guest.accepts().iter().any(|row| row.lease_seq == lease_seq) {
                panic!("skipped lease {lease_seq} was accepted");
            }
            assert_pending(&ok(rig.lease(seq_of(lease_seq))), Op::Lease);
            continue;
        }
        assert_terminal_op(&ok(rig.lease(seq_of(lease_seq))), Op::Lease);
        let Some(accept) = guest
            .accepts()
            .iter()
            .copied()
            .find(|row| row.lease_seq == lease_seq)
        else {
            panic!("missing accept {lease_seq}");
        };
        let age = accept.host_ms.saturating_sub(sample_ms);
        if accept.host_ms != accept_at
            || age != GH + SERVICE + HG
            || accept.until_ms != sample_ms + LEASE_WINDOW_MS
        {
            panic!("cycle {k} age {age} {accept:?}");
        }
        if let Some(prev) = prev_accept {
            let gap = accept.host_ms.saturating_sub(prev);
            let want = if k == SKIP_K + 1 { PERIOD * 2 } else { PERIOD };
            if gap != want {
                panic!("cycle {k} gap {gap} want {want}");
            }
        }
        prev_accept = Some(accept.host_ms);
    }

    if guest.samples().len() != usize::try_from(CYCLES).unwrap_or(usize::MAX)
        || guest.accepts().len() != usize::try_from(CYCLES - 1).unwrap_or(usize::MAX)
    {
        panic!(
            "requests {} accepts {}",
            guest.samples().len(),
            guest.accepts().len()
        );
    }
    if guest.enoent() < 1
        || guest
            .accepts()
            .iter()
            .any(|row| row.lease_seq == SKIP_K + 1)
    {
        panic!("enoent {} or skipped lease was recorded", guest.enoent());
    }
    let last_sample = (CYCLES - 1) * PERIOD + HG;
    if guest.stop_deadline() != Some(last_sample + LEASE_WINDOW_MS) || guest.stopped() {
        panic!(
            "stop {:?} stopped {}",
            guest.stop_deadline(),
            guest.stopped()
        );
    }
    if rig.channel.host_grant_end_ms() != Some(last_sample + LEASE_WINDOW_MS) {
        panic!("host end {:?}", rig.channel.host_grant_end_ms());
    }

    // Forty filesystem cycles stand in for the attempt. The closed form is the
    // full 1_560_000 ms window. Short 100 ms legs fit inside 2000 ms. The
    // measured 1023 ms one-way bound, counted in each direction, does not.
    let window = attempt_window_ms();
    let periodic = window / RENEW_MS + 1;
    let count = periodic + EXTRA_SAMPLES;
    if window != 1_560_000 || periodic != 1_951 || count != 1_959 || count > MAX_POLLS {
        panic!("count formula window {window} periodic {periodic} count {count}");
    }
    let age = ONE_WAY_MS + STEP_MS + ONE_WAY_MS;
    let gap = RENEW_MS + STEP_MS + ONE_WAY_MS;
    if ACCEPT_AGE_MS != 2_146
        || age != ACCEPT_AGE_MS
        || GRANT_GAP_MS != 1_923
        || gap != GRANT_GAP_MS
    {
        panic!("measured age {ACCEPT_AGE_MS} gap {GRANT_GAP_MS}");
    }
    let single = ROUND_TRIP_MAX_MS + STEP_MS + RENEW_MS + STEP_MS;
    if single != 2_023 || single <= LEASE_WINDOW_MS || age + gap < LEASE_WINDOW_MS {
        panic!("measured cycle fits: single {single} sum {}", age + gap);
    }
    if HG + SERVICE + GH + PERIOD >= LEASE_WINDOW_MS {
        panic!("representative delays do not fit inside the grant");
    }
    match prove(window) {
        Err(ScheduleError::Infeasible {
            window_ms,
            count: got,
        }) if window_ms == window && got == count => {}
        other => panic!("full window returned a proof: {other:?}"),
    }
    match prove(1) {
        Err(ScheduleError::Infeasible { .. }) => {}
        other => panic!("short window returned a proof: {other:?}"),
    }
}

#[test]
fn repeated_sample_timestamp_pins_grant_request() {
    let mut rig = Rig::open();
    bind(&mut rig);
    let seq = next_inventory(&mut rig);
    if seq != 2 {
        panic!("seq {seq}");
    }
    rig.publish_guest(Op::Inventory, &plain_terminal(seq, "boot1", 5_000));
    assert_terminal_op(&ok(rig.inventory()), Op::Inventory);
    let Some(pin) = rig.channel.pinned_grant() else {
        panic!("sample 5000 was not pinned");
    };
    if pin
        != (LeaseRequest {
            inventory_request_seq: seq,
            sample_ms: 5_000,
        })
    {
        panic!("pin {pin:?}");
    }

    let again = next_inventory(&mut rig);
    rig.publish_guest(Op::Inventory, &plain_terminal(again, "boot1", 5_000));
    assert_terminal_op(&ok(rig.inventory()), Op::Inventory);
    if rig.channel.pinned_grant() != Some(pin) {
        panic!(
            "repeated sample moved the pin to {:?}",
            rig.channel.pinned_grant()
        );
    }

    assert_pending(&ok(rig.lease(seq_of(1))), Op::Lease);
    let published = lease_request_of(&rig.host(Op::Lease));
    if published != pin || rig.channel.host_grant_end_ms() != Some(7_000) {
        panic!(
            "published {published:?} end {:?}",
            rig.channel.host_grant_end_ms()
        );
    }
    rig.publish_guest(
        Op::Lease,
        &lease_frame(1, pin.inventory_request_seq, pin.sample_ms, 7_000),
    );
    let acked = decode_reply_of(&ok(rig.lease(seq_of(1))));
    let ReplyResult::Lease(term) = acked.result else {
        panic!("ack {acked:?}");
    };
    if term.inventory_request_seq != pin.inventory_request_seq
        || term.sample_ms != pin.sample_ms
        || term.until_ms != 7_000
        || rig.channel.host_grant_end_ms() != Some(7_000)
        || rig.channel.pinned_grant() != Some(pin)
    {
        panic!(
            "ack moved the grant: {term:?} pin {:?}",
            rig.channel.pinned_grant()
        );
    }

    assert_pending(&ok(rig.lease(seq_of(2))), Op::Lease);
    let second = lease_request_of(&rig.host(Op::Lease));
    if second != pin || rig.channel.host_grant_end_ms() != Some(7_000) {
        panic!(
            "second grant {second:?} end {:?}",
            rig.channel.host_grant_end_ms()
        );
    }

    let lower = next_inventory(&mut rig);
    rig.publish_guest(Op::Inventory, &plain_terminal(lower, "boot1", 4_999));
    let err = err_of(rig.inventory());
    assert_failed(&err, b"sample");
    if rig.channel.pinned_grant() != Some(pin) || rig.channel.host_grant_end_ms() != Some(7_000) {
        panic!("lower sample moved the pin or the host end");
    }
}

fn reject_note(rig: &mut Rig, note: ConsumeNote) {
    let before = rig.host(Op::Inventory);
    let err = match rig.channel.note_consumed(note) {
        Err(err) => err,
        Ok(()) => panic!("note was accepted"),
    };
    assert_failed(&err, b"consume");
    if err_of(rig.inventory()) != err || rig.host(Op::Inventory) != before {
        panic!("rejected consume was published or was not sticky");
    }
}

fn boot_outcome(outcome: Outcome) -> SideResult {
    match ValueOutcome::new(outcome, None) {
        Ok(value) => SideResult::Bootstrap(value),
        Err(err) => panic!("outcome: {err:?}"),
    }
}

fn revoke_outcome(outcome: Outcome) -> super::super::ops::RevokeOutcome {
    match super::super::ops::RevokeOutcome::new(outcome, None) {
        Ok(value) => value,
        Err(err) => panic!("revoke outcome: {err:?}"),
    }
}

#[test]
fn note_consumed_rejects_until_successful_terminal() {
    let mut accepted = Rig::open();
    bind(&mut accepted);
    accept_bootstrap(&mut accepted);
    reject_note(
        &mut accepted,
        ConsumeNote {
            active_kind: 1,
            active_seq: 1,
            revoke_seq: 0,
        },
    );

    let mut pending = Rig::open();
    bind(&mut pending);
    let _ = ok(pending.bootstrap());
    let seq = next_inventory(&mut pending);
    pending.publish_guest(
        Op::Inventory,
        &snapshot_frame(
            seq,
            "boot1",
            10,
            "desk1",
            "cfg1",
            activity(SnapStatus::Pending, Some(SideOp::Bootstrap), 1, None),
            revoke_row(SnapStatus::None, 0, None),
        ),
    );
    assert_terminal_op(&ok(pending.inventory()), Op::Inventory);
    reject_note(
        &mut pending,
        ConsumeNote {
            active_kind: 1,
            active_seq: 1,
            revoke_seq: 0,
        },
    );

    for outcome in [Outcome::Failed, Outcome::Unknown, Outcome::Cancelled] {
        let mut rig = Rig::open();
        bind(&mut rig);
        let _ = ok(rig.bootstrap());
        let seq = next_inventory(&mut rig);
        rig.publish_guest(
            Op::Inventory,
            &snapshot_frame(
                seq,
                "boot1",
                10,
                "desk1",
                "cfg1",
                activity(
                    SnapStatus::Terminal,
                    Some(SideOp::Bootstrap),
                    1,
                    Some(boot_outcome(outcome)),
                ),
                revoke_row(SnapStatus::None, 0, None),
            ),
        );
        assert_terminal_op(&ok(rig.inventory()), Op::Inventory);
        reject_note(
            &mut rig,
            ConsumeNote {
                active_kind: 1,
                active_seq: 1,
                revoke_seq: 0,
            },
        );
    }

    let mut unissued = Rig::open();
    bind(&mut unissued);
    reject_note(
        &mut unissued,
        ConsumeNote {
            active_kind: 1,
            active_seq: 1,
            revoke_seq: 0,
        },
    );

    let mut mismatch = Rig::open();
    bind(&mut mismatch);
    accept_bootstrap(&mut mismatch);
    report_bootstrap(&mut mismatch, 1);
    reject_note(
        &mut mismatch,
        ConsumeNote {
            active_kind: 2,
            active_seq: 1,
            revoke_seq: 0,
        },
    );
    let mut other_seq = Rig::open();
    bind(&mut other_seq);
    accept_bootstrap(&mut other_seq);
    report_bootstrap(&mut other_seq, 1);
    reject_note(
        &mut other_seq,
        ConsumeNote {
            active_kind: 1,
            active_seq: 2,
            revoke_seq: 0,
        },
    );

    let mut mixed = Rig::open();
    bind(&mut mixed);
    accept_bootstrap(&mut mixed);
    report_bootstrap(&mut mixed, 1);
    let before = mixed.host(Op::Inventory);
    reject_note(
        &mut mixed,
        ConsumeNote {
            active_kind: 1,
            active_seq: 1,
            revoke_seq: 9,
        },
    );
    if mixed.host(Op::Inventory) != before {
        panic!("a bad revoke queued the good bootstrap row");
    }

    let mut empty = Rig::open();
    bind(&mut empty);
    reject_note(
        &mut empty,
        ConsumeNote {
            active_kind: 0,
            active_seq: 0,
            revoke_seq: 0,
        },
    );

    let mut revoke_bad = Rig::open();
    bind(&mut revoke_bad);
    let _ = ok(revoke_bad.revoke());
    let seq = next_inventory(&mut revoke_bad);
    revoke_bad.publish_guest(
        Op::Inventory,
        &snapshot_frame(
            seq,
            "boot1",
            10,
            "desk1",
            "cfg1",
            activity(SnapStatus::None, None, 0, None),
            revoke_row(
                SnapStatus::Terminal,
                1,
                Some(revoke_outcome(Outcome::Failed)),
            ),
        ),
    );
    assert_terminal_op(&ok(revoke_bad.inventory()), Op::Inventory);
    reject_note(
        &mut revoke_bad,
        ConsumeNote {
            active_kind: 0,
            active_seq: 0,
            revoke_seq: 1,
        },
    );

    let mut login_missing = Rig::open();
    bind(&mut login_missing);
    assert_pending(&ok(login_missing.login()), Op::Login);
    let seq = next_inventory(&mut login_missing);
    let missing = match ValueOutcome::new(Outcome::Ok, Some(LoginState::Missing)) {
        Ok(value) => SideResult::Login(value),
        Err(err) => panic!("missing: {err:?}"),
    };
    login_missing.publish_guest(
        Op::Inventory,
        &snapshot_frame(
            seq,
            "boot1",
            10,
            "desk1",
            "cfg1",
            activity(SnapStatus::Terminal, Some(SideOp::Login), 1, Some(missing)),
            revoke_row(SnapStatus::None, 0, None),
        ),
    );
    assert_terminal_op(&ok(login_missing.inventory()), Op::Inventory);
    if let Err(err) = login_missing.channel.note_consumed(ConsumeNote {
        active_kind: 2,
        active_seq: 1,
        revoke_seq: 0,
    }) {
        panic!("login missing is outcome ok: {err:?}");
    }
    let seq = next_inventory(&mut login_missing);
    assert_consume(
        &decode_host(&login_missing.host(Op::Inventory)),
        Some(SideOp::Login),
        1,
    );
    login_missing.publish_guest(Op::Inventory, &plain_terminal(seq, "boot1", 10));
    assert_terminal_op(&ok(login_missing.inventory()), Op::Inventory);

    let mut revoke_ok_row = Rig::open();
    bind(&mut revoke_ok_row);
    let _ = ok(revoke_ok_row.revoke());
    let seq = next_inventory(&mut revoke_ok_row);
    revoke_ok_row.publish_guest(
        Op::Inventory,
        &snapshot_frame(
            seq,
            "boot1",
            10,
            "desk1",
            "cfg1",
            activity(SnapStatus::None, None, 0, None),
            revoke_row(SnapStatus::Terminal, 1, Some(revoke_ok())),
        ),
    );
    assert_terminal_op(&ok(revoke_ok_row.inventory()), Op::Inventory);
    if let Err(err) = revoke_ok_row.channel.note_consumed(ConsumeNote {
        active_kind: 0,
        active_seq: 0,
        revoke_seq: 1,
    }) {
        panic!("revoke ok: {err:?}");
    }
    let _ = next_inventory(&mut revoke_ok_row);
    match decode_host(&revoke_ok_row.host(Op::Inventory)).payload {
        Payload::InventoryRequest(consume)
            if consume.op.is_none() && consume.request_seq == 0 && consume.revoke_seq == 1 => {}
        other => panic!("revoke consume: {other:?}"),
    }
}

#[test]
fn exact_128_kib_boundary_does_not_read_the_stable_frame() {
    let mut rig = Rig::open();
    assert_pending(&ok(rig.inventory()), Op::Inventory);
    let dest = rig.root.message(GUEST_WRITE_DIR, Op::Inventory.file_name());
    let legal = inv_terminal(1, Some("boot1"));
    if let Err(err) = fs::write(&dest, vec![b'a'; 4_096]) {
        panic!("seed: {err}");
    }
    let n = Rc::new(Cell::new(0u32));
    let counter = Rc::clone(&n);
    let hook_dest = dest.clone();
    let legal_hook = legal.clone();
    let dir = rig.root.path().to_path_buf();
    let _guard = arm(IoPoint::AfterRead, move || {
        let i = counter.get();
        counter.set(i.saturating_add(1));
        let spare = dir.join(format!("edge-{i}"));
        let bytes = if i == 31 {
            legal_hook.clone()
        } else {
            let mut body = vec![b'b'; 4_096];
            body[0] = u8::try_from(i % 200).unwrap_or(1);
            body
        };
        if let Err(err) = fs::write(&spare, bytes) {
            panic!("spare: {err}");
        }
        if let Err(err) = fs::rename(&spare, &hook_dest) {
            panic!("rename: {err}");
        }
    });
    let err = err_of(rig.inventory());
    assert_failed(&err, b"poll-budget");
    if n.get() != 32 || rig.channel.bytes_read() != POLL_BYTE_BUDGET {
        panic!(
            "boundary hooks {} bytes {}",
            n.get(),
            rig.channel.bytes_read()
        );
    }
    match fs::read(&dest) {
        Ok(bytes) if bytes == legal => {}
        other => panic!("stable frame was read or replaced: {other:?}"),
    }
    if err_of(rig.inventory()) != err || rig.channel.bytes_read() != POLL_BYTE_BUDGET {
        panic!("boundary was not sticky");
    }
}

#[test]
fn shared_read_deadline_expires_without_sticking() {
    let mut rig = Rig::open();
    assert_pending(&ok(rig.inventory()), Op::Inventory);
    let dest = rig.root.message(GUEST_WRITE_DIR, Op::Inventory.file_name());
    let first = vec![b'x'; 50];
    if let Err(err) = fs::write(&dest, &first) {
        panic!("seed: {err}");
    }
    let legal = inv_terminal(1, Some("boot1"));
    let legal_len = legal.len();
    let clock = rig.clock.clone();
    let once = Rc::new(Cell::new(true));
    let flag = Rc::clone(&once);
    let hook_dest = dest.clone();
    let dir = rig.root.path().to_path_buf();
    let _guard = arm(IoPoint::AfterRead, move || {
        if !flag.replace(false) {
            return;
        }
        if clock.advance(QueryDeadline::CAP).is_err() {
            panic!("advance");
        }
        let spare = dir.join("deadline-spare");
        if let Err(err) = fs::write(&spare, &legal) {
            panic!("spare: {err}");
        }
        if let Err(err) = fs::rename(&spare, &hook_dest) {
            panic!("rename: {err}");
        }
    });
    let err = err_of(rig.inventory());
    if err != CallError::Expired || rig.channel.bytes_read() != first.len() {
        panic!("deadline {err:?} bytes {}", rig.channel.bytes_read());
    }
    assert_terminal_op(&ok(rig.inventory()), Op::Inventory);
    if rig.channel.bytes_read() != first.len() + legal_len {
        panic!(
            "deadline retry bytes {} want {}",
            rig.channel.bytes_read(),
            first.len() + legal_len
        );
    }
}

#[test]
fn repeated_polls_do_not_issue_requests() {
    let mut rig = Rig::open();
    assert_pending(&ok(rig.inventory()), Op::Inventory);
    let host = rig.host(Op::Inventory);
    for _ in 0..MAX_POLLS {
        assert_pending(&ok(rig.inventory()), Op::Inventory);
        if rig.host(Op::Inventory) != host {
            panic!("an open poll published a new inventory");
        }
    }
    if decode_host(&host).seq != 1 {
        panic!("open polls moved the inventory sequence");
    }
    rig.publish_guest(Op::Inventory, &inv_terminal(1, Some("boot1")));
    assert_terminal_op(&ok(rig.inventory()), Op::Inventory);
    rig.clear_guest(Op::Inventory);
    assert_pending(&ok(rig.inventory()), Op::Inventory);
    if decode_host(&rig.host(Op::Inventory)).seq != 2 {
        panic!("the next issue was not seq 2");
    }
}

struct Rig {
    channel: MailboxGuestChannel<FakeClock>,
    clock: FakeClock,
    root: Root,
}

impl Rig {
    fn open() -> Self {
        let clock = FakeClock::new();
        let root = scratch();
        let layout = match prepare_slots(root.raw()) {
            Ok(layout) => layout,
            Err(err) => panic!("prepare: {err:?}"),
        };
        let channel = MailboxGuestChannel::new(clock.clone(), root.raw(), layout, binding());
        Self {
            channel,
            clock,
            root,
        }
    }

    fn inventory(&mut self) -> Result<BoundedOutput, CallError> {
        let deadline = match QueryDeadline::after(self.clock.now(), QueryDeadline::CAP) {
            Ok(deadline) => deadline,
            Err(err) => panic!("query deadline: {err:?}"),
        };
        self.channel.inventory(deadline)
    }

    fn probe(&mut self, probe: Probe) -> Result<BoundedOutput, CallError> {
        let deadline = match ProbeDeadline::after(self.clock.now(), ProbeDeadline::CAP) {
            Ok(deadline) => deadline,
            Err(err) => panic!("probe deadline: {err:?}"),
        };
        self.channel.probe(probe, deadline)
    }

    fn bootstrap(&mut self) -> Result<BoundedOutput, CallError> {
        let deadline = match StageDeadline::after(self.clock.now(), StageDeadline::CAP) {
            Ok(deadline) => deadline,
            Err(err) => panic!("stage deadline: {err:?}"),
        };
        self.channel.bootstrap(deadline)
    }

    fn login(&mut self) -> Result<BoundedOutput, CallError> {
        let deadline = match LoginDeadline::after(self.clock.now(), LoginDeadline::CAP) {
            Ok(deadline) => deadline,
            Err(err) => panic!("login deadline: {err:?}"),
        };
        self.channel.login(deadline)
    }

    fn run(&mut self) -> Result<BoundedOutput, CallError> {
        let deadline = match RunDeadline::after(self.clock.now(), RunDeadline::CAP) {
            Ok(deadline) => deadline,
            Err(err) => panic!("run deadline: {err:?}"),
        };
        self.channel.run(deadline)
    }

    fn lease(&mut self, seq: NonZeroU64) -> Result<BoundedOutput, CallError> {
        let deadline = match LeaseDeadline::after(self.clock.now(), LeaseDeadline::CAP) {
            Ok(deadline) => deadline,
            Err(err) => panic!("lease deadline: {err:?}"),
        };
        self.channel.lease(seq, deadline)
    }

    fn revoke(&mut self) -> Result<BoundedOutput, CallError> {
        let deadline = match RevokeDeadline::after(self.clock.now(), RevokeDeadline::CAP) {
            Ok(deadline) => deadline,
            Err(err) => panic!("revoke deadline: {err:?}"),
        };
        self.channel.revoke(deadline)
    }

    fn host(&self, op: Op) -> Vec<u8> {
        read_file(&self.root.message(HOST_WRITE_DIR, op.file_name()))
    }

    fn guest_bytes(&self, op: Op) -> Vec<u8> {
        read_file(&self.root.message(GUEST_WRITE_DIR, op.file_name()))
    }

    fn publish_guest(&self, op: Op, bytes: &[u8]) {
        let tmp = self.root.message(GUEST_WRITE_DIR, op.tmp_name());
        let dest = self.root.message(GUEST_WRITE_DIR, op.file_name());
        if tmp.exists() {
            panic!("guest tmp already exists: {}", tmp.display());
        }
        {
            let mut file = match OpenOptions::new().write(true).create_new(true).open(&tmp) {
                Ok(file) => file,
                Err(err) => panic!("create tmp: {err}"),
            };
            if let Err(err) = file.write_all(bytes) {
                panic!("write tmp: {err}");
            }
        }
        if let Err(err) = fs::rename(&tmp, &dest) {
            panic!("rename guest reply: {err}");
        }
    }

    fn clear_guest(&self, op: Op) {
        let dest = self.root.message(GUEST_WRITE_DIR, op.file_name());
        if let Err(err) = fs::write(&dest, []) {
            panic!("clear guest: {err}");
        }
    }

    fn remove_guest(&self, op: Op) {
        let dest = self.root.message(GUEST_WRITE_DIR, op.file_name());
        if let Err(err) = fs::remove_file(&dest) {
            panic!("remove guest: {err}");
        }
    }
}

fn bind(rig: &mut Rig) {
    assert_pending(&ok(rig.inventory()), Op::Inventory);
    rig.publish_guest(Op::Inventory, &inv_terminal(1, Some("boot1")));
    assert_terminal_op(&ok(rig.inventory()), Op::Inventory);
}

fn accept_bootstrap(rig: &mut Rig) {
    let _ = ok(rig.bootstrap());
    rig.publish_guest(Op::Bootstrap, &accepted_frame(Op::Bootstrap, 1));
    assert_accepted(&ok(rig.bootstrap()));
}

fn reject_binding(env: super::super::ops::Envelope) {
    let mut rig = Rig::open();
    bind(&mut rig);
    let seq = next_inventory(&mut rig);
    if seq != env.seq {
        panic!("foreign frame seq {} but host issued {seq}", env.seq);
    }
    rig.publish_guest(Op::Inventory, &encoded(&env));
    let err = err_of(rig.inventory());
    assert_failed(&err, b"mailbox");
    if err_of(rig.inventory()) != err {
        panic!("foreign binding was not sticky: {err:?}");
    }
}

fn empty_snap(sample_ms: u64) -> InventorySnapshot {
    InventorySnapshot {
        uid: 501,
        desktop: id("desk1"),
        config: id("cfg1"),
        sample_ms,
        active: activity(SnapStatus::None, None, 0, None),
        revoke: revoke_row(SnapStatus::None, 0, None),
    }
}

fn need_digest(bytes: [u8; 32]) -> Digest {
    match Digest::try_from_bytes(bytes) {
        Ok(digest) => digest,
        Err(err) => panic!("digest: {err:?}"),
    }
}

fn report_both(rig: &mut Rig) {
    rig.clear_guest(Op::Inventory);
    assert_pending(&ok(rig.inventory()), Op::Inventory);
    let seq = decode_host(&rig.host(Op::Inventory)).seq;
    rig.publish_guest(
        Op::Inventory,
        &snapshot_frame(
            seq,
            "boot1",
            10,
            "desk1",
            "cfg1",
            activity(
                SnapStatus::Terminal,
                Some(SideOp::Bootstrap),
                1,
                Some(boot_ok(1)),
            ),
            revoke_row(SnapStatus::Terminal, 1, Some(revoke_ok())),
        ),
    );
    assert_terminal_op(&ok(rig.inventory()), Op::Inventory);
}

fn encode_consume_request(seq: u64, op: SideOp, request_seq: u64, revoke_seq: u64) -> Vec<u8> {
    let consume = match InventoryConsume::new(Some(op), request_seq, revoke_seq) {
        Ok(consume) => consume,
        Err(err) => panic!("consume: {err:?}"),
    };
    encoded(&envelope(
        Op::Inventory,
        seq,
        Some("boot1"),
        MsgStatus::Request,
        Payload::InventoryRequest(consume),
    ))
}

fn ms(clock: &FakeClock) -> u64 {
    clock.now().nanos() / 1_000_000
}

/// Clock-driven guest. It reads host slots and publishes with exclusive tmp→rename.
///
/// The guest epoch is not the host epoch. Lease expiry is sticky. An existing
/// tmp is a failure and is left in place. No thread and no sleep.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SampleSeen {
    request_seq: u64,
    sample_ms: u64,
    host_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct GrantSeen {
    lease_seq: u64,
    inventory_seq: u64,
    sample_ms: u64,
    until_ms: u64,
    host_ms: u64,
}

struct SyncGuest {
    root: PathBuf,
    clock: FakeClock,
    epoch: u64,
    hold_until: Cell<u64>,
    holding: Cell<bool>,
    generated: Cell<Option<u64>>,
    sample: Cell<Option<u64>>,
    ack_count: Cell<u32>,
    expired_flag: Cell<bool>,
    acked_seqs: RefCell<Vec<u64>>,
    active_done: RefCell<Vec<(SideOp, u64)>>,
    revoke_done: RefCell<Vec<u64>>,
    schedule: Cell<bool>,
    samples: RefCell<Vec<SampleSeen>>,
    accepts: RefCell<Vec<GrantSeen>>,
    answered: RefCell<Vec<u64>>,
    last_frame: RefCell<Option<Vec<u8>>>,
    stop_at: Cell<Option<u64>>,
    stopped: Cell<bool>,
    enoent_count: Cell<u32>,
    skip: RefCell<Vec<u64>>,
    lease_not_before: Cell<u64>,
}

impl SyncGuest {
    fn new(rig: &Rig, epoch: u64) -> Self {
        Self {
            root: rig.root.path().to_path_buf(),
            clock: rig.clock.clone(),
            epoch,
            hold_until: Cell::new(0),
            holding: Cell::new(false),
            generated: Cell::new(None),
            sample: Cell::new(None),
            ack_count: Cell::new(0),
            expired_flag: Cell::new(false),
            acked_seqs: RefCell::new(Vec::new()),
            active_done: RefCell::new(Vec::new()),
            revoke_done: RefCell::new(Vec::new()),
            schedule: Cell::new(false),
            samples: RefCell::new(Vec::new()),
            accepts: RefCell::new(Vec::new()),
            answered: RefCell::new(Vec::new()),
            last_frame: RefCell::new(None),
            stop_at: Cell::new(None),
            stopped: Cell::new(false),
            enoent_count: Cell::new(0),
            skip: RefCell::new(Vec::new()),
            lease_not_before: Cell::new(0),
        }
    }

    fn schedule(&self) {
        self.schedule.set(true);
    }

    fn skip_lease(&self, seq: u64) {
        self.skip.borrow_mut().push(seq);
    }

    /// Host milliseconds before which a lease file on disk is not visible.
    fn see_lease_after(&self, host_ms: u64) {
        self.lease_not_before.set(host_ms);
    }

    fn samples(&self) -> Vec<SampleSeen> {
        self.samples.borrow().clone()
    }

    fn accepts(&self) -> Vec<GrantSeen> {
        self.accepts.borrow().clone()
    }

    fn stop_deadline(&self) -> Option<u64> {
        self.stop_at.get()
    }

    fn stopped(&self) -> bool {
        self.stopped.get()
    }

    fn enoent(&self) -> u32 {
        self.enoent_count.get()
    }

    fn hold_inventory(&self, until_host_ms: u64) {
        self.holding.set(true);
        self.hold_until.set(until_host_ms);
    }

    fn generated_host_ms(&self) -> u64 {
        self.generated.get().unwrap_or(0)
    }

    fn acks(&self) -> u32 {
        self.ack_count.get()
    }

    fn expired(&self) -> bool {
        self.expired_flag.get()
    }

    fn step(&self) -> Result<(), String> {
        let tmp = self.guest_path(Op::Inventory.tmp_name());
        if tmp.exists() {
            return Err("guest tmp already exists".to_owned());
        }
        if self.schedule.get() {
            self.consider_stop();
            if !self.stopped.get() {
                self.serve_inventory_scheduled()?;
                self.serve_lease_scheduled()?;
            }
            self.consider_stop();
            return Ok(());
        }
        self.serve_inventory()?;
        self.serve_lease()?;
        Ok(())
    }

    fn consider_stop(&self) {
        let Some(deadline) = self.stop_at.get() else {
            return;
        };
        let guest_now = self.epoch.saturating_add(self.host_ms());
        if guest_now >= deadline {
            self.stopped.set(true);
            self.expired_flag.set(true);
        }
    }

    fn serve_inventory(&self) -> Result<(), String> {
        let bytes = self.read_host(Op::Inventory);
        if bytes.is_empty() {
            return Ok(());
        }
        let env = decode_frame(&bytes).map_err(|err| format!("inventory host: {err:?}"))?;
        let Payload::InventoryRequest(consume) = env.payload else {
            return Ok(());
        };
        if !self.consume_ok(&consume) {
            return Err("consume names a row this guest has not retained".to_owned());
        }
        if self.generated.get().is_none() {
            let host_ms = self.host_ms();
            self.generated.set(Some(host_ms));
            self.sample.set(Some(self.epoch.saturating_add(host_ms)));
        }
        if self.holding.get() && self.host_ms() < self.hold_until.get() {
            return Ok(());
        }
        let sample = self.sample.get().ok_or("sample was not generated")?;
        let frame = snapshot_frame(
            env.seq,
            "boot1",
            sample,
            "desk1",
            "cfg1",
            activity(SnapStatus::None, None, 0, None),
            revoke_row(SnapStatus::None, 0, None),
        );
        self.publish(Op::Inventory, &frame)
    }

    fn serve_lease(&self) -> Result<(), String> {
        let bytes = self.read_host(Op::Lease);
        if bytes.is_empty() {
            return Ok(());
        }
        let env = decode_frame(&bytes).map_err(|err| format!("lease host: {err:?}"))?;
        let Payload::LeaseRequest(req) = env.payload else {
            return Ok(());
        };
        if self.expired_flag.get() {
            return Ok(());
        }
        let Some(sample) = self.sample.get() else {
            return Ok(());
        };
        let guest_now = self.epoch.saturating_add(self.host_ms());
        let guest_deadline = sample.saturating_add(LEASE_WINDOW_MS);
        if guest_now >= guest_deadline {
            self.expired_flag.set(true);
            return Ok(());
        }
        if self.acked_seqs.borrow().contains(&env.seq) {
            return Ok(());
        }
        let until = req
            .sample_ms
            .checked_add(LEASE_WINDOW_MS)
            .ok_or("lease until overflow")?;
        let frame = lease_frame(env.seq, req.inventory_request_seq, req.sample_ms, until);
        self.publish(Op::Lease, &frame)?;
        self.acked_seqs.borrow_mut().push(env.seq);
        self.ack_count.set(self.ack_count.get().saturating_add(1));
        Ok(())
    }

    fn serve_inventory_scheduled(&self) -> Result<(), String> {
        let bytes = self.read_host(Op::Inventory);
        if bytes.is_empty() {
            return Ok(());
        }
        let env = decode_frame(&bytes).map_err(|err| format!("inventory host: {err:?}"))?;
        let Payload::InventoryRequest(consume) = env.payload else {
            return Ok(());
        };
        if !self.consume_ok(&consume) {
            return Err("consume names a row this guest has not retained".to_owned());
        }
        if self.answered.borrow().contains(&env.seq) {
            let dest = self.guest_path(Op::Inventory.file_name());
            if !dest.exists() {
                self.enoent_count
                    .set(self.enoent_count.get().saturating_add(1));
                let frame = self
                    .last_frame
                    .borrow()
                    .clone()
                    .ok_or("missing inventory frame")?;
                self.publish(Op::Inventory, &frame)?;
            }
            return Ok(());
        }
        let host_ms = self.host_ms();
        let sample = self.epoch.saturating_add(host_ms);
        self.samples.borrow_mut().push(SampleSeen {
            request_seq: env.seq,
            sample_ms: sample,
            host_ms,
        });
        if self.stop_at.get().is_none() {
            self.stop_at
                .set(Some(sample.saturating_add(LEASE_WINDOW_MS)));
        }
        if self.generated.get().is_none() {
            self.generated.set(Some(host_ms));
            self.sample.set(Some(sample));
        }
        let frame = snapshot_frame(
            env.seq,
            "boot1",
            sample,
            "desk1",
            "cfg1",
            activity(SnapStatus::None, None, 0, None),
            revoke_row(SnapStatus::None, 0, None),
        );
        self.last_frame.replace(Some(frame.clone()));
        self.answered.borrow_mut().push(env.seq);
        self.publish(Op::Inventory, &frame)
    }

    fn serve_lease_scheduled(&self) -> Result<(), String> {
        if self.stopped.get() || self.expired_flag.get() {
            return Ok(());
        }
        let bytes = self.read_host(Op::Lease);
        if bytes.is_empty() {
            return Ok(());
        }
        let env = decode_frame(&bytes).map_err(|err| format!("lease host: {err:?}"))?;
        let Payload::LeaseRequest(req) = env.payload else {
            return Ok(());
        };
        if self.skip.borrow().contains(&env.seq) {
            return Ok(());
        }
        if self.host_ms() < self.lease_not_before.get() {
            return Ok(());
        }
        let known = self.samples.borrow().iter().any(|row| {
            row.request_seq == req.inventory_request_seq && row.sample_ms == req.sample_ms
        });
        if !known {
            return Ok(());
        }
        let guest_now = self.epoch.saturating_add(self.host_ms());
        let deadline = req.sample_ms.saturating_add(LEASE_WINDOW_MS);
        if guest_now >= deadline {
            self.expired_flag.set(true);
            self.stopped.set(true);
            self.stop_at.set(Some(deadline));
            return Ok(());
        }
        if self.acked_seqs.borrow().contains(&env.seq) {
            return Ok(());
        }
        let until = req
            .sample_ms
            .checked_add(LEASE_WINDOW_MS)
            .ok_or("lease until overflow")?;
        let frame = lease_frame(env.seq, req.inventory_request_seq, req.sample_ms, until);
        self.publish(Op::Lease, &frame)?;
        self.acked_seqs.borrow_mut().push(env.seq);
        self.ack_count.set(self.ack_count.get().saturating_add(1));
        self.accepts.borrow_mut().push(GrantSeen {
            lease_seq: env.seq,
            inventory_seq: req.inventory_request_seq,
            sample_ms: req.sample_ms,
            until_ms: until,
            host_ms: self.host_ms(),
        });
        if until >= self.stop_at.get().unwrap_or(0) {
            self.stop_at.set(Some(until));
        }
        Ok(())
    }

    fn consume_ok(&self, consume: &InventoryConsume) -> bool {
        if let Some(op) = consume.op {
            let kept = self
                .active_done
                .borrow()
                .iter()
                .any(|(row_op, seq)| *row_op == op && *seq == consume.request_seq);
            if !kept {
                return false;
            }
        }
        if consume.revoke_seq >= 1 && !self.revoke_done.borrow().contains(&consume.revoke_seq) {
            return false;
        }
        true
    }

    fn host_ms(&self) -> u64 {
        ms(&self.clock)
    }

    fn read_host(&self, op: Op) -> Vec<u8> {
        let path = self
            .root
            .join(MAILBOX_DIR)
            .join(HOST_WRITE_DIR)
            .join(op.file_name());
        fs::read(&path).unwrap_or_default()
    }

    fn guest_path(&self, name: &str) -> PathBuf {
        self.root.join(MAILBOX_DIR).join(GUEST_WRITE_DIR).join(name)
    }

    fn publish(&self, op: Op, bytes: &[u8]) -> Result<(), String> {
        let tmp = self.guest_path(op.tmp_name());
        let dest = self.guest_path(op.file_name());
        if tmp.exists() {
            return Err(format!("tmp exists: {}", tmp.display()));
        }
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)
            .map_err(|err| format!("create tmp: {err}"))?;
        file.write_all(bytes)
            .map_err(|err| format!("write tmp: {err}"))?;
        drop(file);
        fs::rename(&tmp, &dest).map_err(|err| format!("rename: {err}"))?;
        Ok(())
    }
}

fn note_bootstrap(rig: &mut Rig, seq: u64) {
    if let Err(err) = rig.channel.note_consumed(ConsumeNote {
        active_kind: 1,
        active_seq: seq,
        revoke_seq: 0,
    }) {
        panic!("note bootstrap: {err:?}");
    }
}

fn report_bootstrap(rig: &mut Rig, generation: u64) {
    rig.clear_guest(Op::Inventory);
    assert_pending(&ok(rig.inventory()), Op::Inventory);
    let seq = decode_host(&rig.host(Op::Inventory)).seq;
    rig.publish_guest(Op::Inventory, &bootstrap_terminal(seq, "boot1", generation));
    assert_terminal_op(&ok(rig.inventory()), Op::Inventory);
}

fn assert_accepted_login(rig: &mut Rig) {
    assert_pending(&ok(rig.login()), Op::Login);
    rig.publish_guest(Op::Login, &accepted_frame(Op::Login, 1));
    assert_accepted(&ok(rig.login()));
}

fn next_inventory(rig: &mut Rig) -> u64 {
    rig.clear_guest(Op::Inventory);
    assert_pending(&ok(rig.inventory()), Op::Inventory);
    decode_host(&rig.host(Op::Inventory)).seq
}

fn publish_pending(rig: &mut Rig, op: Op) -> Vec<u8> {
    let body = match op {
        Op::Inventory => ok(rig.inventory()),
        Op::Probe => ok(rig.probe(Probe::Sandbox)),
        Op::Bootstrap => ok(rig.bootstrap()),
        Op::Login => ok(rig.login()),
        Op::Run => ok(rig.run()),
        Op::Lease => ok(rig.lease(seq_of(1))),
        Op::Revoke => ok(rig.revoke()),
    };
    assert_pending(&body, op);
    rig.host(op)
}

fn seq_of(value: u64) -> NonZeroU64 {
    match NonZeroU64::new(value) {
        Some(seq) => seq,
        None => panic!("seq 0"),
    }
}

fn ok(result: Result<BoundedOutput, CallError>) -> BoundedOutput {
    match result {
        Ok(body) => body,
        Err(err) => panic!("unexpected call error: {err:?}"),
    }
}

fn err_of(result: Result<BoundedOutput, CallError>) -> CallError {
    match result {
        Err(err) => err,
        Ok(_) => panic!("expected a call error"),
    }
}

fn assert_failed(err: &CallError, msg: &[u8]) {
    match err {
        CallError::Failed(body) if body.as_bytes() == msg => {}
        other => panic!("expected {msg:?}, got {other:?}"),
    }
}

fn assert_pending(body: &BoundedOutput, op: Op) {
    let reply = decode_reply_of(body);
    if reply.state != ReplyState::Pending || reply.op != op {
        panic!("expected pending {op:?}, got {reply:?}");
    }
}

fn assert_terminal_op(body: &BoundedOutput, op: Op) {
    let reply = decode_reply_of(body);
    if reply.state != ReplyState::Terminal || reply.op != op {
        panic!("expected terminal {op:?}, got {reply:?}");
    }
}

fn assert_accepted(body: &BoundedOutput) {
    let reply = decode_reply_of(body);
    if reply.state != ReplyState::Accepted {
        panic!("expected accepted, got {reply:?}");
    }
}

fn assert_consume(env: &super::super::ops::Envelope, op: Option<SideOp>, request_seq: u64) {
    match &env.payload {
        Payload::InventoryRequest(consume)
            if consume.op == op
                && consume.request_seq == request_seq
                && consume.revoke_seq == 0 => {}
        other => panic!("consume {op:?}/{request_seq}, got {other:?}"),
    }
}

fn decode_reply_of(body: &BoundedOutput) -> super::super::ops::MailboxReply {
    match decode_reply(body.as_bytes()) {
        Ok(reply) => reply,
        Err(err) => panic!("decode reply: {err:?}"),
    }
}

fn decode_host(bytes: &[u8]) -> super::super::ops::Envelope {
    match decode_frame(bytes) {
        Ok(env) => env,
        Err(err) => panic!("decode host: {err:?}"),
    }
}

fn accepted_frame(op: Op, seq: u64) -> Vec<u8> {
    encoded(&envelope(
        op,
        seq,
        Some("boot1"),
        MsgStatus::Accepted,
        Payload::Empty,
    ))
}

fn lease_frame(seq: u64, inventory_seq: u64, sample_ms: u64, until_ms: u64) -> Vec<u8> {
    encoded(&envelope(
        Op::Lease,
        seq,
        Some("boot1"),
        MsgStatus::Terminal,
        Payload::LeaseTerminal(LeaseTerminal {
            inventory_request_seq: inventory_seq,
            sample_ms,
            until_ms,
        }),
    ))
}

fn plain_terminal(seq: u64, boot: &str, sample_ms: u64) -> Vec<u8> {
    snapshot_frame(
        seq,
        boot,
        sample_ms,
        "desk1",
        "cfg1",
        activity(SnapStatus::None, None, 0, None),
        revoke_row(SnapStatus::None, 0, None),
    )
}

fn bootstrap_terminal(seq: u64, boot: &str, generation: u64) -> Vec<u8> {
    snapshot_frame(
        seq,
        boot,
        10,
        "desk1",
        "cfg1",
        activity(
            SnapStatus::Terminal,
            Some(SideOp::Bootstrap),
            1,
            Some(boot_ok(generation)),
        ),
        revoke_row(SnapStatus::None, 0, None),
    )
}

fn snapshot_frame(
    seq: u64,
    boot: &str,
    sample_ms: u64,
    desktop: &str,
    config: &str,
    active: super::super::ops::Activity,
    revoke: super::super::ops::RevokeSlot,
) -> Vec<u8> {
    encoded(&envelope(
        Op::Inventory,
        seq,
        Some(boot),
        MsgStatus::Terminal,
        Payload::InventoryTerminal(InventorySnapshot {
            uid: 501,
            desktop: id(desktop),
            config: id(config),
            sample_ms,
            active,
            revoke,
        }),
    ))
}

fn read_file(path: &Path) -> Vec<u8> {
    match fs::read(path) {
        Ok(bytes) => bytes,
        Err(err) => panic!("read {}: {err}", path.display()),
    }
}

struct TempDir {
    path: PathBuf,
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

struct Root {
    fd: OwnedFd,
    dir: TempDir,
}

impl Root {
    fn raw(&self) -> i32 {
        self.fd.as_raw_fd()
    }

    fn path(&self) -> &Path {
        &self.dir.path
    }

    fn message(&self, dir: &str, name: &str) -> PathBuf {
        self.path().join(MAILBOX_DIR).join(dir).join(name)
    }
}

static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

fn scratch() -> Root {
    let n = NEXT_DIR.fetch_add(1, Ordering::Relaxed);
    let nanos = match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(elapsed) => elapsed.as_nanos(),
        Err(_) => 0,
    };
    let mut path = std::env::temp_dir();
    path.push(format!("gcu-m4-guest-{nanos}-{n}"));
    if let Err(err) = fs::create_dir(&path) {
        panic!("mkdir {}: {err}", path.display());
    }
    if let Err(err) = fs::set_permissions(&path, fs::Permissions::from_mode(0o700)) {
        panic!("chmod {}: {err}", path.display());
    }
    let name = match CString::new(path.as_os_str().as_bytes()) {
        Ok(name) => name,
        Err(err) => panic!("root path: {err}"),
    };
    let fd = match sys::open_root(&name) {
        Ok(fd) => fd,
        Err(err) => panic!("open root: {err:?}"),
    };
    Root {
        fd,
        dir: TempDir { path },
    }
}
