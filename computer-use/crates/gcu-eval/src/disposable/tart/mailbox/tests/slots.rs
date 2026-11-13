//! Host publish and guest read against a private temporary directory.
//!
//! Nothing here calls `hdiutil`, Tart, or ssh. The directory is removed on drop.

#![cfg(unix)]

use std::ffi::CString;
use std::fs::{self, File};
use std::io::Write;
use std::os::fd::{AsRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use super::super::ids::AttemptNonce;
use super::super::ops::{
    MsgStatus, Op, Payload, ReplyResult, ReplyState, SideOp, SnapStatus, FRAME_MAX,
};
use super::super::seq::SeqTracker;
use super::super::slots::{
    arm, poll_guest, prepare_slots, publish, read_checked, read_slot, CheckedRead, GuestPoll,
    HostSlot, IoPoint, SlotBody, SlotRef, GUEST_WRITE_DIR, HOST_STAGE_DIR, HOST_WRITE_DIR,
    MAILBOX_DIR,
};
use super::super::sys;
use super::super::{Breach, FailedRead, MailboxError, SeqFault};
use super::support::{
    activity, bind, binding, encoded, envelope, inv_request, inv_with, need, probe_request,
    probe_terminal, revoke_row,
};
use crate::disposable::Check;

#[test]
fn zero_length_slot_reads_pending() {
    let root = scratch();
    let mut layout = need(prepare_slots(root.raw()));
    for slot in [
        SlotRef::Host(HostSlot::Op(Op::Probe)),
        SlotRef::Guest(Op::Probe),
    ] {
        let read = need(read_slot(root.raw(), &mut layout, slot));
        if read.body != SlotBody::Pending || read.retry_absent != 0 || read.retry_replaced != 0 {
            panic!("empty slot: {read:?}");
        }
    }
}

#[test]
fn publish_read_roundtrip() {
    let root = scratch();
    let mut layout = need(prepare_slots(root.raw()));
    let frame = probe_request(1);
    need(publish(
        root.raw(),
        &mut layout,
        HostSlot::Op(Op::Probe),
        &frame,
    ));
    match need(read_slot(
        root.raw(),
        &mut layout,
        SlotRef::Host(HostSlot::Op(Op::Probe)),
    )) {
        read if read.body == SlotBody::Frame(frame.clone()) => {}
        other => panic!("published frame: {other:?}"),
    }
    match need(read_checked(
        root.raw(),
        &mut layout,
        SlotRef::Host(HostSlot::Op(Op::Probe)),
    )) {
        CheckedRead::Message { envelope, .. } if envelope.op == Op::Probe && envelope.seq == 1 => {}
        other => panic!("checked publish: {other:?}"),
    }
    need(publish(
        root.raw(),
        &mut layout,
        HostSlot::Binding,
        b"binding-v1",
    ));
    match need(read_checked(
        root.raw(),
        &mut layout,
        SlotRef::Host(HostSlot::Binding),
    )) {
        CheckedRead::Binding { bytes, .. } if bytes == b"binding-v1" => {}
        other => panic!("binding: {other:?}"),
    }
    match publish(root.raw(), &mut layout, HostSlot::Binding, b"again") {
        Err(MailboxError::Breach(Breach::BindingFrozen)) => {}
        other => panic!("second binding: {other:?}"),
    }
}

#[test]
fn publish_fails_when_stage_tmp_exists() {
    let root = scratch();
    let mut layout = need(prepare_slots(root.raw()));
    let slot = root.message(HOST_WRITE_DIR, Op::Probe.file_name());
    let before = stamp(&slot);
    write_bytes(&root.stage(Op::Probe.tmp_name()), b"already");
    match publish(
        root.raw(),
        &mut layout,
        HostSlot::Op(Op::Probe),
        &probe_request(1),
    ) {
        Err(MailboxError::Breach(Breach::TmpExists)) => {}
        other => panic!("existing tmp: {other:?}"),
    }
    let after = stamp(&slot);
    if after != before || after.len != 0 {
        panic!("tmp collision changed the slot: {before:?} -> {after:?}");
    }
}

#[test]
fn symlink_is_breach_and_not_followed() {
    let root = scratch();
    let mut layout = need(prepare_slots(root.raw()));
    let canary = root.path().join("canary");
    write_bytes(&canary, b"secret-canary");
    let slot = root.message(HOST_WRITE_DIR, Op::Probe.file_name());
    remove_file(&slot);
    if let Err(err) = std::os::unix::fs::symlink(&canary, &slot) {
        panic!("symlink: {err}");
    }
    expect_read_breach(
        read_slot(
            root.raw(),
            &mut layout,
            SlotRef::Host(HostSlot::Op(Op::Probe)),
        ),
        Breach::Link,
    );
    expect_publish_breach(
        publish(
            root.raw(),
            &mut layout,
            HostSlot::Op(Op::Probe),
            &probe_request(1),
        ),
        Breach::Link,
    );
    match fs::read(&canary) {
        Ok(bytes) if bytes == b"secret-canary" => {}
        other => panic!("canary changed: {other:?}"),
    }
    match fs::symlink_metadata(&slot) {
        Ok(meta) if meta.file_type().is_symlink() => {}
        other => panic!("slot is no longer a symlink: {other:?}"),
    }
}

#[test]
fn fifo_is_breach_and_does_not_block() {
    let root = scratch();
    let mut layout = need(prepare_slots(root.raw()));
    let slot = root.message(HOST_WRITE_DIR, Op::Probe.file_name());
    remove_file(&slot);
    make_fifo(&slot);
    let started = Instant::now();
    expect_read_breach(
        read_slot(
            root.raw(),
            &mut layout,
            SlotRef::Host(HostSlot::Op(Op::Probe)),
        ),
        Breach::NotRegular,
    );
    expect_publish_breach(
        publish(
            root.raw(),
            &mut layout,
            HostSlot::Op(Op::Probe),
            &probe_request(1),
        ),
        Breach::NotRegular,
    );
    if started.elapsed().as_secs() >= 2 {
        panic!("fifo open blocked");
    }
}

#[test]
fn directory_slot_is_breach() {
    let root = scratch();
    let mut layout = need(prepare_slots(root.raw()));
    let slot = root.message(GUEST_WRITE_DIR, Op::Probe.file_name());
    remove_file(&slot);
    if let Err(err) = fs::create_dir(&slot) {
        panic!("mkdir slot: {err}");
    }
    expect_read_breach(
        read_slot(root.raw(), &mut layout, SlotRef::Guest(Op::Probe)),
        Breach::NotRegular,
    );
}

#[test]
fn hardlink_nlink_two_is_breach() {
    let root = scratch();
    let mut layout = need(prepare_slots(root.raw()));
    let slot = root.message(HOST_WRITE_DIR, Op::Probe.file_name());
    let extra = root.path().join("extra-link");
    if let Err(err) = fs::hard_link(&slot, &extra) {
        panic!("hard link: {err}");
    }
    expect_read_breach(
        read_slot(
            root.raw(),
            &mut layout,
            SlotRef::Host(HostSlot::Op(Op::Probe)),
        ),
        Breach::Nlink,
    );
}

#[test]
fn oversized_slot_is_breach() {
    let root = scratch();
    let mut layout = need(prepare_slots(root.raw()));
    let slot = root.message(GUEST_WRITE_DIR, Op::Probe.file_name());
    let big = vec![0u8; 4097];
    write_bytes(&slot, &big);
    expect_read_breach(
        read_slot(root.raw(), &mut layout, SlotRef::Guest(Op::Probe)),
        Breach::Size,
    );
    write_bytes(&slot, b"short");
    match read_slot(root.raw(), &mut layout, SlotRef::Guest(Op::Probe)) {
        Err(err) if err.error == MailboxError::Sticky => {}
        other => panic!("oversize should stick, got {other:?}"),
    }
}

#[test]
fn replace_during_read_is_pending() {
    let root = scratch();
    let mut layout = need(prepare_slots(root.raw()));
    let frame = probe_request(1);
    need(publish(
        root.raw(),
        &mut layout,
        HostSlot::Op(Op::Probe),
        &frame,
    ));
    let spare = root.message(HOST_WRITE_DIR, Op::Login.file_name());
    write_bytes(&spare, b"replacement-object");
    let dest = root.message(HOST_WRITE_DIR, Op::Probe.file_name());
    let hook_spare = spare.clone();
    let hook_dest = dest.clone();
    let _guard = arm(IoPoint::AfterRead, move || {
        if let Err(err) = fs::rename(&hook_spare, &hook_dest) {
            panic!("rename over slot: {err}");
        }
    });
    let read = need(read_slot(
        root.raw(),
        &mut layout,
        SlotRef::Host(HostSlot::Op(Op::Probe)),
    ));
    if read.body != SlotBody::Pending || read.retry_replaced != 1 || read.retry_absent != 0 {
        panic!("replaced read: {read:?}");
    }
}

#[test]
fn absent_slot_is_pending() {
    let root = scratch();
    let mut layout = need(prepare_slots(root.raw()));
    let slot = root.message(GUEST_WRITE_DIR, Op::Probe.file_name());
    remove_file(&slot);
    let first = need(read_slot(
        root.raw(),
        &mut layout,
        SlotRef::Guest(Op::Probe),
    ));
    let second = need(read_slot(
        root.raw(),
        &mut layout,
        SlotRef::Guest(Op::Probe),
    ));
    if first.body != SlotBody::Pending
        || first.retry_absent != 1
        || second.retry_absent != 2
        || second.retry_replaced != 0
    {
        panic!("absent counts: {first:?} {second:?}");
    }
    let frame = probe_terminal(1, Check::Passed);
    write_bytes(&slot, &frame);
    let third = need(read_slot(
        root.raw(),
        &mut layout,
        SlotRef::Guest(Op::Probe),
    ));
    if third.body != SlotBody::Frame(frame) || third.retry_absent != 2 {
        panic!("restored slot: {third:?}");
    }
}

#[test]
fn malformed_bytes_stick() {
    let root = scratch();
    let mut layout = need(prepare_slots(root.raw()));
    let slot = root.message(GUEST_WRITE_DIR, Op::Probe.file_name());
    write_bytes(&slot, b"nope");
    match read_checked(root.raw(), &mut layout, SlotRef::Guest(Op::Probe)) {
        Err(err) if matches!(err.error, MailboxError::Malformed(_)) => {}
        other => panic!("malformed: {other:?}"),
    }
    write_bytes(&slot, &probe_terminal(1, Check::Passed));
    match read_checked(root.raw(), &mut layout, SlotRef::Guest(Op::Probe)) {
        Err(err) if err.error == MailboxError::Sticky => {}
        other => panic!("malformed should stick, got {other:?}"),
    }
}

#[test]
fn source_replaced_after_fsync_does_not_rename() {
    let root = scratch();
    let mut layout = need(prepare_slots(root.raw()));
    let slot = root.message(HOST_WRITE_DIR, Op::Probe.file_name());
    let before = stamp(&slot);
    let tmp = root.stage(Op::Probe.tmp_name());
    let away = root.stage("moved-aside");
    let hook_tmp = tmp.clone();
    let hook_away = away.clone();
    let _guard = arm(IoPoint::AfterFsync, move || {
        if let Err(err) = fs::rename(&hook_tmp, &hook_away) {
            panic!("move source: {err}");
        }
        write_bytes(&hook_tmp, b"replaced-source");
    });
    match publish(
        root.raw(),
        &mut layout,
        HostSlot::Op(Op::Probe),
        &probe_request(1),
    ) {
        Err(MailboxError::Breach(Breach::Identity)) => {}
        other => panic!("replaced source: {other:?}"),
    }
    let after = stamp(&slot);
    if after != before || after.len != 0 {
        panic!("source swap renamed the slot: {before:?} -> {after:?}");
    }
}

#[test]
fn export_layout_keeps_stage_out_of_mailbox() {
    let root = scratch();
    let _layout = need(prepare_slots(root.raw()));
    let mailbox = root.path().join(MAILBOX_DIR);
    let stage = root.path().join(HOST_STAGE_DIR);
    if !mailbox.is_dir() || !stage.is_dir() {
        panic!("mailbox or stage missing");
    }
    if !mailbox.join(HOST_WRITE_DIR).is_dir() || !mailbox.join(GUEST_WRITE_DIR).is_dir() {
        panic!("message directories are not under mailbox");
    }
    if root.path().join(HOST_WRITE_DIR).exists() || root.path().join(GUEST_WRITE_DIR).exists() {
        panic!("message directories are siblings of mailbox");
    }
    if stage.parent() != Some(root.path()) || mailbox.parent() != Some(root.path()) {
        panic!("stage is not a sibling of mailbox");
    }
    assert_no_stage(&mailbox);
}

#[test]
fn guest_publication_source_is_created_exclusively() {
    let root = scratch();
    let mut layout = need(prepare_slots(root.raw()));
    for op in OPS {
        let msg = root.message(GUEST_WRITE_DIR, op.file_name());
        let tmp = root.message(GUEST_WRITE_DIR, op.tmp_name());
        let meta = meta_of(&msg);
        if !meta.is_file() || meta.len() != 0 {
            panic!("{} is not an empty read slot", msg.display());
        }
        if tmp.exists() {
            panic!("{} was pre-created", tmp.display());
        }
        let host_tmp = root.stage(op.tmp_name());
        if host_tmp.exists() {
            panic!("host stage pre-created {}", host_tmp.display());
        }
    }
    let guest = open_root(&root.path().join(MAILBOX_DIR).join(GUEST_WRITE_DIR));
    let tmp_name = match CString::new("inventory.msg.tmp") {
        Ok(name) => name,
        Err(err) => panic!("tmp name: {err}"),
    };
    let fd = match sys::open_create(guest.as_raw_fd(), &tmp_name) {
        Ok(fd) => fd,
        Err(err) => panic!("exclusive guest tmp: {err:?}"),
    };
    let bytes = b"guest-inventory-v1";
    if let Err(err) = sys::write_all(fd.as_raw_fd(), bytes) {
        panic!("write guest tmp: {err:?}");
    }
    drop(fd);
    let from = root.message(GUEST_WRITE_DIR, Op::Inventory.tmp_name());
    let to = root.message(GUEST_WRITE_DIR, Op::Inventory.file_name());
    if let Err(err) = fs::rename(&from, &to) {
        panic!("guest rename: {err}");
    }
    match need(read_slot(
        root.raw(),
        &mut layout,
        SlotRef::Guest(Op::Inventory),
    )) {
        read if read.body == SlotBody::Frame(bytes.to_vec()) => {}
        other => panic!("guest publication: {other:?}"),
    }
}

#[test]
fn destination_swap_after_fsync_does_not_rename() {
    for kind in [
        DestSwap::Remove,
        DestSwap::Replace,
        DestSwap::Symlink,
        DestSwap::Fifo,
        DestSwap::Hardlink,
    ] {
        let root = scratch();
        let mut layout = need(prepare_slots(root.raw()));
        let slot = root.message(HOST_WRITE_DIR, Op::Probe.file_name());
        let tmp = root.stage(Op::Probe.tmp_name());
        let frame = probe_request(1);
        let hook_slot = slot.clone();
        let hook_root = root.path().to_path_buf();
        let _guard = arm(IoPoint::AfterFsync, move || {
            alter_dest(kind, &hook_root, &hook_slot)
        });
        match publish(root.raw(), &mut layout, HostSlot::Op(Op::Probe), &frame) {
            Err(MailboxError::Breach(got)) if got == kind.expect() => {}
            other => panic!("{kind:?} publish: {other:?}"),
        }
        match fs::read(&tmp) {
            Ok(bytes) if bytes == frame => {}
            other => panic!("{kind:?} renamed or dropped the source: {other:?}"),
        }
        assert_dest_kept(kind, &root, &slot);
    }
}

#[test]
fn replaced_directory_fails_closed() {
    publish_dir_swap(true);
    publish_dir_swap(false);
    read_dir_swap(true);
    read_dir_swap(false);
}

#[test]
fn pending_inventory_activity_roundtrips() {
    for side in [SideOp::Bootstrap, SideOp::Login, SideOp::Run] {
        let root = scratch();
        let mut layout = need(prepare_slots(root.raw()));
        let mut tracker = SeqTracker::new(binding());
        need(tracker.note_request(&inv_request(1, None)));
        let frame = inv_with(
            1,
            Some("boot1"),
            activity(SnapStatus::Pending, Some(side), 1, None),
            revoke_row(SnapStatus::None, 0, None),
        );
        write_bytes(
            &root.message(GUEST_WRITE_DIR, Op::Inventory.file_name()),
            &frame,
        );
        match poll_guest(root.raw(), &mut layout, Op::Inventory, &mut tracker) {
            Ok(GuestPoll::Ready { reply, .. }) => match reply.result {
                ReplyResult::Inventory(snap)
                    if snap.active.status == SnapStatus::Pending
                        && snap.active.op == Some(side)
                        && snap.active.request_seq == 1
                        && snap.active.result.is_none() => {}
                other => panic!("pending poll: {other:?}"),
            },
            other => panic!("pending poll: {other:?}"),
        }
    }
}

#[test]
fn torn_byte_count_sticks() {
    expect_torn(&[9u8; 16], vec![1, 2, 3, 4]);
    expect_torn(b"abcd", vec![0u8; FRAME_MAX + 1]);
}

#[test]
fn slot_failure_stays_on_that_slot() {
    let root = scratch();
    let mut layout = need(prepare_slots(root.raw()));
    let path = root.message(GUEST_WRITE_DIR, Op::Probe.file_name());
    remove_file(&path);
    let probe = SlotRef::Guest(Op::Probe);
    let first = need(read_slot(root.raw(), &mut layout, probe));
    let second = need(read_slot(root.raw(), &mut layout, probe));
    if first.retry_absent != 1 || second.retry_absent != 2 || second.retry_replaced != 0 {
        panic!("absent before failure: {first:?} {second:?}");
    }
    write_bytes(&path, b"nope");
    match read_checked(root.raw(), &mut layout, probe) {
        Err(err)
            if matches!(err.error, MailboxError::Malformed(_))
                && err.retry_absent == 2
                && err.retry_replaced == 0 => {}
        other => panic!("malformed: {other:?}"),
    }
    expect_sticky(&mut layout, root.raw(), probe, 2);
    match read_checked(root.raw(), &mut layout, probe) {
        Err(err) if err.error == MailboxError::Sticky && err.retry_absent == 2 => {}
        other => panic!("checked after failure: {other:?}"),
    }
    let mut tracker = SeqTracker::new(binding());
    bind(&mut tracker);
    match poll_guest(root.raw(), &mut layout, Op::Probe, &mut tracker) {
        Err(err) if err.error == MailboxError::Sticky && err.retry_absent == 2 => {}
        other => panic!("poll after failure: {other:?}"),
    }
    let other = SlotRef::Guest(Op::Login);
    let read = need(read_slot(root.raw(), &mut layout, other));
    if read.body != SlotBody::Pending || read.retry_absent != 0 || read.retry_replaced != 0 {
        panic!("other slot: {read:?}");
    }
    if layout.retry_absent(probe) != 2 || layout.retry_replaced(probe) != 0 {
        panic!("other slot changed probe counters");
    }
    expect_sticky(&mut layout, root.raw(), probe, 2);
}

#[test]
fn directory_whitelist_stops_at_allowance() {
    let root = scratch();
    let mut layout = need(prepare_slots(root.raw()));
    if root.message(GUEST_WRITE_DIR, Op::Probe.tmp_name()).exists() {
        panic!("tmp required before a clean read");
    }
    let pending = need(read_slot(
        root.raw(),
        &mut layout,
        SlotRef::Guest(Op::Probe),
    ));
    if pending.body != SlotBody::Pending {
        panic!("missing tmp should stay pending: {pending:?}");
    }

    let unknown = scratch();
    let mut layout = need(prepare_slots(unknown.raw()));
    write_bytes(&unknown.message(GUEST_WRITE_DIR, "spare.dat"), b"extra");
    expect_read_breach(
        read_slot(unknown.raw(), &mut layout, SlotRef::Guest(Op::Probe)),
        Breach::Identity,
    );

    let raw = scratch();
    let mut layout = need(prepare_slots(raw.raw()));
    if create_raw(&raw.path().join(MAILBOX_DIR).join(GUEST_WRITE_DIR), &[0xff]) {
        expect_read_breach(
            read_slot(raw.raw(), &mut layout, SlotRef::Guest(Op::Probe)),
            Breach::Identity,
        );
    }

    let linked = scratch();
    let mut layout = need(prepare_slots(linked.raw()));
    let canary = linked.path().join("canary");
    write_bytes(&canary, b"secret-canary");
    if let Err(err) = std::os::unix::fs::symlink(
        &canary,
        linked.message(GUEST_WRITE_DIR, Op::Inventory.tmp_name()),
    ) {
        panic!("symlink: {err}");
    }
    expect_read_breach(
        read_slot(linked.raw(), &mut layout, SlotRef::Guest(Op::Probe)),
        Breach::Link,
    );
    match fs::read(&canary) {
        Ok(bytes) if bytes == b"secret-canary" => {}
        other => panic!("link was followed: {other:?}"),
    }

    let fifo = scratch();
    let mut layout = need(prepare_slots(fifo.raw()));
    make_fifo(&fifo.message(GUEST_WRITE_DIR, Op::Login.tmp_name()));
    let started = Instant::now();
    expect_read_breach(
        read_slot(fifo.raw(), &mut layout, SlotRef::Guest(Op::Probe)),
        Breach::NotRegular,
    );
    if started.elapsed().as_secs() >= 2 {
        panic!("special entry blocked the preflight");
    }

    let export = scratch();
    let mut layout = need(prepare_slots(export.raw()));
    if let Err(err) = fs::create_dir(export.path().join(MAILBOX_DIR).join("extra")) {
        panic!("extra export child: {err}");
    }
    expect_read_breach(
        read_slot(export.raw(), &mut layout, SlotRef::Guest(Op::Probe)),
        Breach::Identity,
    );

    let host = scratch();
    let mut layout = need(prepare_slots(host.raw()));
    write_bytes(&host.message(HOST_WRITE_DIR, "extra.dat"), b"ninth");
    expect_read_breach(
        read_slot(
            host.raw(),
            &mut layout,
            SlotRef::Host(HostSlot::Op(Op::Probe)),
        ),
        Breach::Identity,
    );

    let guest = scratch();
    let mut layout = need(prepare_slots(guest.raw()));
    for index in 0..8 {
        write_bytes(
            &guest.message(GUEST_WRITE_DIR, &format!("extra{index}.dat")),
            b"x",
        );
    }
    expect_read_breach(
        read_slot(guest.raw(), &mut layout, SlotRef::Guest(Op::Probe)),
        Breach::Identity,
    );
}

#[test]
fn stale_exact_guest_file_names_the_current_request() {
    let root = scratch();
    let mut layout = need(prepare_slots(root.raw()));
    let mut tracker = SeqTracker::new(binding());
    bind(&mut tracker);
    need(tracker.note_request(&probe_request(1)));
    let frame = probe_terminal(1, Check::Passed);
    write_bytes(
        &root.message(GUEST_WRITE_DIR, Op::Probe.file_name()),
        &frame,
    );
    match poll_guest(root.raw(), &mut layout, Op::Probe, &mut tracker) {
        Ok(GuestPoll::Ready { reply, .. })
            if reply.state == ReplyState::Terminal && reply.request_seq == 1 => {}
        other => panic!("first guest poll: {other:?}"),
    }
    need(tracker.note_request(&probe_request(2)));
    match poll_guest(root.raw(), &mut layout, Op::Probe, &mut tracker) {
        Ok(GuestPoll::Pending {
            op, request_seq, ..
        }) if op == Op::Probe && request_seq == 2 => {}
        other => panic!("stale guest file: {other:?}"),
    }
    if tracker.outstanding(Op::Probe) != 2 {
        panic!("stale file moved the outstanding probe");
    }
}

#[test]
fn poll_guest_binding_mismatch_stays_failed() {
    let root = scratch();
    let mut layout = need(prepare_slots(root.raw()));
    let mut tracker = SeqTracker::new(binding());
    bind(&mut tracker);
    need(tracker.note_request(&probe_request(1)));
    let mut env = envelope(
        Op::Probe,
        1,
        Some("boot1"),
        MsgStatus::Terminal,
        Payload::ProbeTerminal(Check::Passed),
    );
    env.attempt_nonce = AttemptNonce::from_bytes([0x99; 16]);
    write_bytes(
        &root.message(GUEST_WRITE_DIR, Op::Probe.file_name()),
        &encoded(&env),
    );
    match poll_guest(root.raw(), &mut layout, Op::Probe, &mut tracker) {
        Err(err) if err.error == MailboxError::Sequence(SeqFault::Binding) => {}
        other => panic!("binding mismatch: {other:?}"),
    }
    write_bytes(
        &root.message(GUEST_WRITE_DIR, Op::Probe.file_name()),
        &probe_terminal(1, Check::Passed),
    );
    match poll_guest(root.raw(), &mut layout, Op::Probe, &mut tracker) {
        Err(err) if err.error == MailboxError::Sticky => {}
        other => panic!("corrected bytes: {other:?}"),
    }
}

#[test]
fn replaced_slot_then_stable_reply() {
    let root = scratch();
    let mut layout = need(prepare_slots(root.raw()));
    let mut tracker = SeqTracker::new(binding());
    bind(&mut tracker);
    need(tracker.note_request(&probe_request(1)));
    let probe = root.message(GUEST_WRITE_DIR, Op::Probe.file_name());
    let login = root.message(GUEST_WRITE_DIR, Op::Login.file_name());
    write_bytes(&probe, &probe_terminal(1, Check::Passed));
    write_bytes(&login, &probe_terminal(1, Check::Failed));
    let hook_login = login.clone();
    let hook_probe = probe.clone();
    let guard = arm(IoPoint::AfterRead, move || {
        if let Err(err) = fs::rename(&hook_login, &hook_probe) {
            panic!("rename over probe: {err}");
        }
    });
    match poll_guest(root.raw(), &mut layout, Op::Probe, &mut tracker) {
        Ok(GuestPoll::Pending {
            request_seq: 1,
            retry_replaced: 1,
            retry_absent: 0,
            ..
        }) => {}
        other => panic!("replaced poll: {other:?}"),
    }
    drop(guard);
    match poll_guest(root.raw(), &mut layout, Op::Probe, &mut tracker) {
        Ok(GuestPoll::Ready {
            reply,
            retry_replaced: 1,
            ..
        }) if reply.result == ReplyResult::Probe(Check::Failed) && reply.request_seq == 1 => {}
        other => panic!("stable replacement: {other:?}"),
    }
}

fn expect_read_breach(result: Result<super::super::slots::SlotRead, FailedRead>, kind: Breach) {
    match result {
        Err(err) if err.error == MailboxError::Breach(kind) => {}
        other => panic!("expected {kind:?}, got {other:?}"),
    }
}

fn expect_publish_breach(result: Result<(), MailboxError>, kind: Breach) {
    match result {
        Err(MailboxError::Breach(got)) if got == kind => {}
        other => panic!("expected publish {kind:?}, got {other:?}"),
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

    fn stage(&self, name: &str) -> PathBuf {
        self.path().join(HOST_STAGE_DIR).join(name)
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
    path.push(format!("gcu-mailbox-{nanos}-{n}"));
    if let Err(err) = fs::create_dir(&path) {
        panic!("mkdir {}: {err}", path.display());
    }
    if let Err(err) = fs::set_permissions(&path, fs::Permissions::from_mode(0o700)) {
        panic!("chmod {}: {err}", path.display());
    }
    let fd = open_root(&path);
    Root {
        fd,
        dir: TempDir { path },
    }
}

fn open_root(path: &Path) -> OwnedFd {
    let name = c_path(path);
    match sys::open_root(&name) {
        Ok(fd) => fd,
        Err(err) => panic!("open root: {err:?}"),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Stamp {
    ino: u64,
    len: u64,
}

fn stamp(path: &Path) -> Stamp {
    match fs::symlink_metadata(path) {
        Ok(meta) => Stamp {
            ino: meta.ino(),
            len: meta.len(),
        },
        Err(err) => panic!("stat {}: {err}", path.display()),
    }
}

fn write_bytes(path: &Path, bytes: &[u8]) {
    let mut file = match File::create(path) {
        Ok(file) => file,
        Err(err) => panic!("create {}: {err}", path.display()),
    };
    if let Err(err) = file.write_all(bytes) {
        panic!("write {}: {err}", path.display());
    }
}

fn remove_file(path: &Path) {
    if let Err(err) = fs::remove_file(path) {
        panic!("remove {}: {err}", path.display());
    }
}

fn make_fifo(path: &Path) {
    if let Err(err) = sys::make_fifo(&c_path(path)) {
        panic!("mkfifo: {err:?}");
    }
}

fn c_path(path: &Path) -> CString {
    match CString::new(path.as_os_str().as_bytes()) {
        Ok(name) => name,
        Err(err) => panic!("path: {err}"),
    }
}

const OPS: [Op; 7] = [
    Op::Inventory,
    Op::Probe,
    Op::Bootstrap,
    Op::Login,
    Op::Run,
    Op::Lease,
    Op::Revoke,
];

#[derive(Clone, Copy, Debug)]
enum DestSwap {
    Remove,
    Replace,
    Symlink,
    Fifo,
    Hardlink,
}

impl DestSwap {
    fn expect(self) -> Breach {
        match self {
            Self::Remove | Self::Replace => Breach::Identity,
            Self::Symlink => Breach::Link,
            Self::Fifo => Breach::NotRegular,
            Self::Hardlink => Breach::Nlink,
        }
    }
}

fn alter_dest(kind: DestSwap, root: &Path, slot: &Path) {
    match kind {
        DestSwap::Remove => remove_file(slot),
        DestSwap::Replace => {
            remove_file(slot);
            write_bytes(slot, b"intruder");
        }
        DestSwap::Symlink => {
            let canary = root.join("canary");
            write_bytes(&canary, b"secret-canary");
            remove_file(slot);
            if let Err(err) = std::os::unix::fs::symlink(&canary, slot) {
                panic!("symlink dest: {err}");
            }
        }
        DestSwap::Fifo => {
            remove_file(slot);
            make_fifo(slot);
        }
        DestSwap::Hardlink => {
            let extra = root.join("extra-link");
            if let Err(err) = fs::hard_link(slot, &extra) {
                panic!("hard link dest: {err}");
            }
        }
    }
}

fn assert_dest_kept(kind: DestSwap, root: &Root, slot: &Path) {
    match kind {
        DestSwap::Remove => match fs::symlink_metadata(slot) {
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            other => panic!("removed destination came back: {other:?}"),
        },
        DestSwap::Replace => match fs::read(slot) {
            Ok(bytes) if bytes == b"intruder" => {}
            other => panic!("replacement was overwritten: {other:?}"),
        },
        DestSwap::Symlink => {
            let meta = meta_of(slot);
            if !meta.file_type().is_symlink() {
                panic!("symlink destination was replaced");
            }
            match fs::read(root.path().join("canary")) {
                Ok(bytes) if bytes == b"secret-canary" => {}
                other => panic!("canary changed: {other:?}"),
            }
        }
        DestSwap::Fifo => {
            if !meta_of(slot).file_type().is_fifo() {
                panic!("fifo destination was replaced");
            }
        }
        DestSwap::Hardlink => {
            let meta = meta_of(slot);
            if meta.nlink() != 2 || meta.len() != 0 {
                panic!(
                    "hardlink destination changed: nlink {} len {}",
                    meta.nlink(),
                    meta.len()
                );
            }
        }
    }
}

fn publish_dir_swap(mailbox_level: bool) {
    let root = scratch();
    let mut layout = need(prepare_slots(root.raw()));
    let frame = probe_request(1);
    let mailbox = root.path().join(MAILBOX_DIR);
    let host = mailbox.join(HOST_WRITE_DIR);
    let (from, aside) = if mailbox_level {
        (mailbox.clone(), root.path().join("mailbox-aside"))
    } else {
        (host.clone(), mailbox.join("host-write-aside"))
    };
    let hook_from = from.clone();
    let hook_aside = aside.clone();
    let _guard = arm(IoPoint::AfterFsync, move || {
        swap_dir(&hook_from, &hook_aside)
    });
    expect_publish_breach(
        publish(root.raw(), &mut layout, HostSlot::Op(Op::Probe), &frame),
        Breach::Identity,
    );
    match fs::read(root.stage(Op::Probe.tmp_name())) {
        Ok(bytes) if bytes == frame => {}
        other => panic!("staged source: {other:?}"),
    }
    let kept = aside.join(if mailbox_level {
        format!("{HOST_WRITE_DIR}/{}", Op::Probe.file_name())
    } else {
        Op::Probe.file_name().to_string()
    });
    let kept_meta = meta_of(&kept);
    if kept_meta.len() != 0 {
        panic!("aside slot was published: {}", kept.display());
    }
    let live = mailbox.join(HOST_WRITE_DIR).join(Op::Probe.file_name());
    if live.exists() {
        panic!("new directory gained {}", live.display());
    }
}

fn read_dir_swap(mailbox_level: bool) {
    let root = scratch();
    let mut layout = need(prepare_slots(root.raw()));
    let frame = probe_terminal(1, Check::Passed);
    let mailbox = root.path().join(MAILBOX_DIR);
    let guest = mailbox.join(GUEST_WRITE_DIR);
    write_bytes(&guest.join(Op::Probe.file_name()), &frame);
    let (from, aside) = if mailbox_level {
        (mailbox.clone(), root.path().join("mailbox-aside"))
    } else {
        (guest.clone(), mailbox.join("guest-write-aside"))
    };
    let hook_from = from.clone();
    let hook_aside = aside.clone();
    let planted = guest.join(Op::Probe.file_name());
    let planted_frame = frame.clone();
    let _guard = arm(IoPoint::AfterRead, move || {
        swap_dir(&hook_from, &hook_aside);
        if !mailbox_level {
            write_bytes(&planted, &planted_frame);
        }
    });
    expect_read_breach(
        read_slot(root.raw(), &mut layout, SlotRef::Guest(Op::Probe)),
        Breach::Identity,
    );
    let kept = if mailbox_level {
        aside.join(GUEST_WRITE_DIR).join(Op::Probe.file_name())
    } else {
        aside.join(Op::Probe.file_name())
    };
    match fs::read(&kept) {
        Ok(bytes) if bytes == frame => {}
        other => panic!("aside file: {other:?}"),
    }
}

fn swap_dir(from: &Path, aside: &Path) {
    if let Err(err) = fs::rename(from, aside) {
        panic!("rename {}: {err}", from.display());
    }
    if let Err(err) = fs::create_dir(from) {
        panic!("mkdir {}: {err}", from.display());
    }
}

fn expect_torn(file: &[u8], forced: Vec<u8>) {
    let root = scratch();
    let mut layout = need(prepare_slots(root.raw()));
    write_bytes(&root.message(GUEST_WRITE_DIR, Op::Probe.file_name()), file);
    let slot = SlotRef::Guest(Op::Probe);
    let hold = sys::force_read(forced);
    expect_read_breach(read_slot(root.raw(), &mut layout, slot), Breach::Torn);
    if layout.retry_replaced(slot) != 0 || layout.retry_absent(slot) != 0 {
        panic!(
            "torn counted as a retry: absent {} replaced {}",
            layout.retry_absent(slot),
            layout.retry_replaced(slot)
        );
    }
    drop(hold);
    expect_sticky(&mut layout, root.raw(), slot, 0);
}

fn expect_sticky(layout: &mut super::super::slots::Layout, root: i32, slot: SlotRef, absent: u64) {
    match read_slot(root, layout, slot) {
        Err(err)
            if err.error == MailboxError::Sticky
                && err.retry_absent == absent
                && err.retry_replaced == 0 => {}
        other => panic!("sticky read: {other:?}"),
    }
}

fn assert_no_stage(dir: &Path) {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) => panic!("read {}: {err}", dir.display()),
    };
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(err) => panic!("entry: {err}"),
        };
        if entry.file_name() == "host-stage" {
            panic!("export contains staging at {}", entry.path().display());
        }
        let kind = match entry.file_type() {
            Ok(kind) => kind,
            Err(err) => panic!("type: {err}"),
        };
        if kind.is_dir() {
            assert_no_stage(&entry.path());
        }
    }
}

fn meta_of(path: &Path) -> fs::Metadata {
    match fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(err) => panic!("stat {}: {err}", path.display()),
    }
}

fn create_raw(dir: &Path, name: &[u8]) -> bool {
    let parent = open_root(dir);
    let owned = match CString::new(name.to_vec()) {
        Ok(name) => name,
        Err(err) => panic!("raw name: {err}"),
    };
    match sys::open_create(parent.as_raw_fd(), &owned) {
        Ok(fd) => {
            drop(fd);
            true
        }
        Err(err) => {
            // APFS rejects a filename that is not UTF-8. The byte compare still
            // runs on filesystems that store the name.
            let code = std::io::Error::last_os_error().raw_os_error();
            if code == Some(libc::EILSEQ) || code == Some(libc::EINVAL) {
                false
            } else {
                panic!("create raw name: {err:?} errno {code:?}");
            }
        }
    }
}
