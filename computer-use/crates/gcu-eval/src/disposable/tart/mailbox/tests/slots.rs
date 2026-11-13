//! Host publish and guest read against a private temporary directory.
//!
//! Nothing here calls `hdiutil`, Tart, or ssh. The directory is removed on drop.

#![cfg(unix)]

use std::ffi::CString;
use std::fs::{self, File};
use std::io::Write;
use std::os::fd::{AsRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use super::super::ops::Op;
use super::super::slots::{
    arm, prepare_slots, publish, read_checked, read_slot, CheckedRead, HostSlot, IoPoint, SlotBody,
    SlotRef, SlotWatch, GUEST_WRITE_DIR, HOST_STAGE_DIR, HOST_WRITE_DIR,
};
use super::super::sys;
use super::super::{Breach, FailedRead, MailboxError};
use super::support::{need, probe_request, probe_terminal};
use crate::disposable::Check;

#[test]
fn zero_length_slot_reads_pending() {
    let root = scratch();
    let layout = need(prepare_slots(root.raw()));
    for slot in [
        SlotRef::Host(HostSlot::Op(Op::Probe)),
        SlotRef::Guest(Op::Probe),
    ] {
        let mut watch = SlotWatch::default();
        let read = need(read_slot(root.raw(), &layout, slot, &mut watch));
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
    let mut watch = SlotWatch::default();
    match need(read_slot(
        root.raw(),
        &layout,
        SlotRef::Host(HostSlot::Op(Op::Probe)),
        &mut watch,
    )) {
        read if read.body == SlotBody::Frame(frame.clone()) => {}
        other => panic!("published frame: {other:?}"),
    }
    match need(read_checked(
        root.raw(),
        &layout,
        SlotRef::Host(HostSlot::Op(Op::Probe)),
        &mut watch,
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
        &layout,
        SlotRef::Host(HostSlot::Binding),
        &mut watch,
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
    let slot = root.child(HOST_WRITE_DIR, Op::Probe.file_name());
    let before = stamp(&slot);
    write_bytes(
        &root.child(HOST_STAGE_DIR, Op::Probe.tmp_name()),
        b"already",
    );
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
    let slot = root.child(HOST_WRITE_DIR, Op::Probe.file_name());
    remove_file(&slot);
    if let Err(err) = std::os::unix::fs::symlink(&canary, &slot) {
        panic!("symlink: {err}");
    }
    let mut watch = SlotWatch::default();
    expect_read_breach(
        read_slot(
            root.raw(),
            &layout,
            SlotRef::Host(HostSlot::Op(Op::Probe)),
            &mut watch,
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
    let slot = root.child(HOST_WRITE_DIR, Op::Probe.file_name());
    remove_file(&slot);
    make_fifo(&slot);
    let started = Instant::now();
    let mut watch = SlotWatch::default();
    expect_read_breach(
        read_slot(
            root.raw(),
            &layout,
            SlotRef::Host(HostSlot::Op(Op::Probe)),
            &mut watch,
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
    let layout = need(prepare_slots(root.raw()));
    let slot = root.child(GUEST_WRITE_DIR, Op::Probe.file_name());
    remove_file(&slot);
    if let Err(err) = fs::create_dir(&slot) {
        panic!("mkdir slot: {err}");
    }
    let mut watch = SlotWatch::default();
    expect_read_breach(
        read_slot(root.raw(), &layout, SlotRef::Guest(Op::Probe), &mut watch),
        Breach::NotRegular,
    );
}

#[test]
fn hardlink_nlink_two_is_breach() {
    let root = scratch();
    let layout = need(prepare_slots(root.raw()));
    let slot = root.child(HOST_WRITE_DIR, Op::Probe.file_name());
    let extra = root.path().join("extra-link");
    if let Err(err) = fs::hard_link(&slot, &extra) {
        panic!("hard link: {err}");
    }
    let mut watch = SlotWatch::default();
    expect_read_breach(
        read_slot(
            root.raw(),
            &layout,
            SlotRef::Host(HostSlot::Op(Op::Probe)),
            &mut watch,
        ),
        Breach::Nlink,
    );
}

#[test]
fn oversized_slot_is_breach() {
    let root = scratch();
    let layout = need(prepare_slots(root.raw()));
    let slot = root.child(GUEST_WRITE_DIR, Op::Probe.file_name());
    let big = vec![0u8; 4097];
    write_bytes(&slot, &big);
    let mut watch = SlotWatch::default();
    expect_read_breach(
        read_slot(root.raw(), &layout, SlotRef::Guest(Op::Probe), &mut watch),
        Breach::Size,
    );
    write_bytes(&slot, b"short");
    match read_slot(root.raw(), &layout, SlotRef::Guest(Op::Probe), &mut watch) {
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
    let spare = root.child(HOST_WRITE_DIR, "spare.dat");
    write_bytes(&spare, b"replacement-object");
    let dest = root.child(HOST_WRITE_DIR, Op::Probe.file_name());
    let hook_spare = spare.clone();
    let hook_dest = dest.clone();
    let _guard = arm(IoPoint::AfterRead, move || {
        if let Err(err) = fs::rename(&hook_spare, &hook_dest) {
            panic!("rename over slot: {err}");
        }
    });
    let mut watch = SlotWatch::default();
    let read = need(read_slot(
        root.raw(),
        &layout,
        SlotRef::Host(HostSlot::Op(Op::Probe)),
        &mut watch,
    ));
    if read.body != SlotBody::Pending || read.retry_replaced != 1 || read.retry_absent != 0 {
        panic!("replaced read: {read:?}");
    }
}

#[test]
fn absent_slot_is_pending() {
    let root = scratch();
    let layout = need(prepare_slots(root.raw()));
    let slot = root.child(GUEST_WRITE_DIR, Op::Probe.file_name());
    remove_file(&slot);
    let mut watch = SlotWatch::default();
    let first = need(read_slot(
        root.raw(),
        &layout,
        SlotRef::Guest(Op::Probe),
        &mut watch,
    ));
    let second = need(read_slot(
        root.raw(),
        &layout,
        SlotRef::Guest(Op::Probe),
        &mut watch,
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
        &layout,
        SlotRef::Guest(Op::Probe),
        &mut watch,
    ));
    if third.body != SlotBody::Frame(frame) || third.retry_absent != 2 {
        panic!("restored slot: {third:?}");
    }
}

#[test]
fn malformed_bytes_stick() {
    let root = scratch();
    let layout = need(prepare_slots(root.raw()));
    let slot = root.child(GUEST_WRITE_DIR, Op::Probe.file_name());
    write_bytes(&slot, b"nope");
    let mut watch = SlotWatch::default();
    match read_checked(root.raw(), &layout, SlotRef::Guest(Op::Probe), &mut watch) {
        Err(err) if matches!(err.error, MailboxError::Malformed(_)) => {}
        other => panic!("malformed: {other:?}"),
    }
    write_bytes(&slot, &probe_terminal(1, Check::Passed));
    match read_checked(root.raw(), &layout, SlotRef::Guest(Op::Probe), &mut watch) {
        Err(err) if err.error == MailboxError::Sticky => {}
        other => panic!("malformed should stick, got {other:?}"),
    }
}

#[test]
fn source_replaced_after_fsync_does_not_rename() {
    let root = scratch();
    let mut layout = need(prepare_slots(root.raw()));
    let slot = root.child(HOST_WRITE_DIR, Op::Probe.file_name());
    let before = stamp(&slot);
    let tmp = root.child(HOST_STAGE_DIR, Op::Probe.tmp_name());
    let away = root.child(HOST_STAGE_DIR, "moved-aside");
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

    fn child(&self, dir: &str, name: &str) -> PathBuf {
        self.path().join(dir).join(name)
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
