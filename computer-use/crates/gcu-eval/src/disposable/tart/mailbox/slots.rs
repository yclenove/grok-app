//! Pre-created mailbox slots. Publish renames onto a placeholder.
//!
//! A zero-length file is an empty slot ([`SlotBody::Pending`]), not an error.
//! `ENOENT` and an identity change during the read are transient and counted.
//! Malformed bytes and an over-cap object stick; the caller stops.

#[cfg(test)]
use std::cell::RefCell;

use super::codec::decode_frame;
use super::ops::{Envelope, MailboxReply, Op, FRAME_MAX};
use super::seq::{Observe, SeqTracker};
use super::{Breach, FailedRead, MailboxError};

#[cfg(unix)]
use super::sys::{self, FileId, Kind, SysErr};
#[cfg(unix)]
use std::ffi::CStr;
#[cfg(unix)]
use std::os::fd::OwnedFd;

/// Exported host directory. Created by [`prepare_slots`].
pub const HOST_WRITE_DIR: &str = "host-write";

/// Exported guest directory. Created by [`prepare_slots`].
pub const GUEST_WRITE_DIR: &str = "guest-write";

/// Host staging directory. Not part of the exported guest tree.
#[cfg_attr(not(test), allow(dead_code))]
pub(super) const HOST_STAGE_DIR: &str = "host-stage";

/// One host-write name. `binding.msg` is published at most once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostSlot {
    /// Raw binding bytes. Not an operation envelope.
    Binding,
    /// One of the seven operation slots.
    Op(Op),
}

/// A host or guest message name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotRef {
    /// `host-write`.
    Host(HostSlot),
    /// `guest-write` operation slot. The guest tmp names are not read here.
    Guest(Op),
}

/// Recorded directory and placeholder inodes. Holds no descriptors.
#[derive(Debug, Clone)]
pub struct Layout {
    #[cfg(unix)]
    inner: Inner,
    #[cfg(not(unix))]
    #[allow(dead_code)]
    _unsupported: (),
}

/// Counters for one slot. A sticky failure stays set.
#[derive(Debug, Clone, Default)]
pub struct SlotWatch {
    /// Transient `ENOENT` results so far.
    pub retry_absent: u64,
    /// Reads discarded because the object changed.
    pub retry_replaced: u64,
    #[cfg_attr(not(unix), allow(dead_code))]
    stuck: bool,
}

/// Bytes after the structural checks, or an empty slot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlotBody {
    /// Zero-length, missing, or replaced during the read.
    Pending,
    /// Stable bytes, at most 4096.
    Frame(Vec<u8>),
}

/// One structural read, including the counters after this call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotRead {
    /// Pending or a frame.
    pub body: SlotBody,
    /// [`SlotWatch::retry_absent`] after this call.
    pub retry_absent: u64,
    /// [`SlotWatch::retry_replaced`] after this call.
    pub retry_replaced: u64,
}

/// A read that also decoded an operation frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckedRead {
    /// Nothing new to decode.
    Pending {
        /// Absent retries so far.
        retry_absent: u64,
        /// Replaced retries so far.
        retry_replaced: u64,
    },
    /// A closed envelope.
    Message {
        /// Decoded frame. Boxed so this enum stays smaller than the envelope.
        envelope: Box<Envelope>,
        /// Absent retries so far.
        retry_absent: u64,
        /// Replaced retries so far.
        retry_replaced: u64,
    },
    /// Raw `binding.msg` bytes. They are not an envelope.
    Binding {
        /// Published bytes.
        bytes: Vec<u8>,
        /// Absent retries so far.
        retry_absent: u64,
        /// Replaced retries so far.
        retry_replaced: u64,
    },
}

/// A guest poll joined with the sequence tracker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuestPoll {
    /// Still waiting.
    Pending {
        /// Slot that was read.
        op: Op,
        /// Request still outstanding. Zero if none was sent.
        request_seq: u64,
        /// Absent retries so far.
        retry_absent: u64,
        /// Replaced retries so far.
        retry_replaced: u64,
    },
    /// Old lease ACK. The deadline was not moved.
    DiscardedLease {
        /// Discarded lease sequence.
        seq: u64,
        /// Absent retries so far.
        retry_absent: u64,
        /// Replaced retries so far.
        retry_replaced: u64,
    },
    /// New accepted or terminal reply.
    Ready {
        /// Normalized reply.
        reply: MailboxReply,
        /// Absent retries so far.
        retry_absent: u64,
        /// Replaced retries so far.
        retry_replaced: u64,
    },
}

#[cfg(unix)]
#[derive(Debug, Clone, Copy)]
struct Inner {
    root_dev: u64,
    root_ino: u64,
    fsid: [u8; 8],
    host_write: DirId,
    guest_write: DirId,
    host_stage: DirId,
    placeholder: [u64; 8],
    published: [Option<u64>; 8],
    binding_published: bool,
}

#[cfg(unix)]
#[derive(Debug, Clone, Copy)]
struct DirId {
    dev: u64,
    ino: u64,
}

/// Where a test replaces a file. One hook at a time. Not a channel flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum IoPoint {
    /// After the source `fsync`, before the source identity check.
    AfterFsync,
    /// After a non-empty read, before the second `fstat`.
    AfterRead,
}

#[cfg(test)]
type HookFn = Box<dyn FnMut()>;
#[cfg(test)]
type ArmedHook = Option<(IoPoint, HookFn)>;

#[cfg(test)]
thread_local! {
    static HOOK: RefCell<ArmedHook> = const { RefCell::new(None) };
}

/// Clears the hook on drop.
#[cfg(test)]
#[derive(Debug)]
pub(super) struct Interrupt;

/// Arm `hook` for the next [`IoPoint`]. The hook must not re-enter.
#[cfg(test)]
pub(super) fn arm(point: IoPoint, hook: impl FnMut() + 'static) -> Interrupt {
    HOOK.with(|cell| *cell.borrow_mut() = Some((point, Box::new(hook))));
    Interrupt
}

#[cfg(test)]
impl Drop for Interrupt {
    fn drop(&mut self) {
        HOOK.with(|cell| *cell.borrow_mut() = None);
    }
}

fn hit(point: IoPoint) {
    #[cfg(test)]
    HOOK.with(|cell| {
        let mut slot = cell.borrow_mut();
        if let Some((armed, hook)) = slot.as_mut() {
            if *armed == point {
                hook();
            }
        }
    });
    #[cfg(not(test))]
    {
        let _ = point;
    }
}

/// Create the three directories and every zero-length placeholder.
///
/// `root` is an already-checked directory descriptor. It is not closed.
pub fn prepare_slots(root: i32) -> Result<Layout, MailboxError> {
    #[cfg(not(unix))]
    {
        let _ = root;
        return Err(MailboxError::Unsupported);
    }
    #[cfg(unix)]
    {
        prepare_unix(root)
    }
}

/// Replace one host placeholder with `frame`. The source FD dies with this call.
pub fn publish(
    root: i32,
    layout: &mut Layout,
    slot: HostSlot,
    frame: &[u8],
) -> Result<(), MailboxError> {
    #[cfg(not(unix))]
    {
        let _ = (root, layout, slot, frame);
        return Err(MailboxError::Unsupported);
    }
    #[cfg(unix)]
    {
        publish_unix(root, &mut layout.inner, slot, frame)
    }
}

/// Read one pre-created slot. Pending is `Ok`. A breach is [`FailedRead`].
pub fn read_slot(
    root: i32,
    layout: &Layout,
    slot: SlotRef,
    watch: &mut SlotWatch,
) -> Result<SlotRead, FailedRead> {
    #[cfg(not(unix))]
    {
        let _ = (root, layout, slot);
        return Err(failed(MailboxError::Unsupported, watch));
    }
    #[cfg(unix)]
    {
        if watch.stuck {
            return Err(failed(MailboxError::Sticky, watch));
        }
        match read_body(root, &layout.inner, slot, watch) {
            Ok(body) => Ok(finished(body, watch)),
            Err(error) => Err(failed(error, watch)),
        }
    }
}

/// [`read_slot`] plus a decode. A codec failure sticks.
pub fn read_checked(
    root: i32,
    layout: &Layout,
    slot: SlotRef,
    watch: &mut SlotWatch,
) -> Result<CheckedRead, FailedRead> {
    let read = read_slot(root, layout, slot, watch)?;
    match read.body {
        SlotBody::Pending => Ok(CheckedRead::Pending {
            retry_absent: read.retry_absent,
            retry_replaced: read.retry_replaced,
        }),
        SlotBody::Frame(bytes) if matches!(slot, SlotRef::Host(HostSlot::Binding)) => {
            Ok(CheckedRead::Binding {
                bytes,
                retry_absent: read.retry_absent,
                retry_replaced: read.retry_replaced,
            })
        }
        SlotBody::Frame(bytes) => match decode_frame(&bytes) {
            Ok(envelope) => Ok(CheckedRead::Message {
                envelope: Box::new(envelope),
                retry_absent: read.retry_absent,
                retry_replaced: read.retry_replaced,
            }),
            Err(error) => {
                watch.stuck = true;
                Err(failed(error, watch))
            }
        },
    }
}

/// Read a guest slot and apply [`SeqTracker::observe`]. Codec and sequence errors stick.
pub fn poll_guest(
    root: i32,
    layout: &Layout,
    op: Op,
    watch: &mut SlotWatch,
    tracker: &mut SeqTracker,
) -> Result<GuestPoll, FailedRead> {
    let read = read_slot(root, layout, SlotRef::Guest(op), watch)?;
    match read.body {
        SlotBody::Pending => Ok(GuestPoll::Pending {
            op,
            request_seq: tracker.outstanding(op),
            retry_absent: read.retry_absent,
            retry_replaced: read.retry_replaced,
        }),
        SlotBody::Frame(bytes) => match tracker.observe(op, &bytes) {
            Ok(Observe::Pending { op, request_seq }) => Ok(GuestPoll::Pending {
                op,
                request_seq,
                retry_absent: read.retry_absent,
                retry_replaced: read.retry_replaced,
            }),
            Ok(Observe::DiscardedLease { seq }) => Ok(GuestPoll::DiscardedLease {
                seq,
                retry_absent: read.retry_absent,
                retry_replaced: read.retry_replaced,
            }),
            Ok(Observe::Reply(reply)) => Ok(GuestPoll::Ready {
                reply,
                retry_absent: read.retry_absent,
                retry_replaced: read.retry_replaced,
            }),
            Err(error) => {
                watch.stuck = true;
                Err(failed(error, watch))
            }
        },
    }
}

fn finished(body: SlotBody, watch: &SlotWatch) -> SlotRead {
    SlotRead {
        body,
        retry_absent: watch.retry_absent,
        retry_replaced: watch.retry_replaced,
    }
}

fn failed(error: MailboxError, watch: &SlotWatch) -> FailedRead {
    FailedRead {
        error,
        retry_absent: watch.retry_absent,
        retry_replaced: watch.retry_replaced,
    }
}

#[cfg(unix)]
fn prepare_unix(root: i32) -> Result<Layout, MailboxError> {
    let root_id = sys::fstat(root).map_err(map_sys)?;
    if root_id.kind != Kind::Dir {
        return Err(Breach::NotDirectory.into());
    }
    let fsid = sys::fsid(root).map_err(map_sys)?;
    let (host_write, host_fd) = make_dir(root, c"host-write", root_id.dev, fsid)?;
    let (guest_write, guest_fd) = make_dir(root, c"guest-write", root_id.dev, fsid)?;
    let (host_stage, stage_fd) = make_dir(root, c"host-stage", root_id.dev, fsid)?;
    let mut placeholder = [0u64; 8];
    for (index, name) in host_names().iter().enumerate() {
        let ino = make_file(sys::raw_fd(&host_fd), name, root_id.dev, fsid)?;
        *placeholder
            .get_mut(index)
            .ok_or(MailboxError::Breach(Breach::Identity))? = ino;
    }
    for name in guest_names() {
        let _ino = make_file(sys::raw_fd(&guest_fd), name, root_id.dev, fsid)?;
    }
    sys::fsync_fd(sys::raw_fd(&host_fd)).map_err(map_sys)?;
    sys::fsync_fd(sys::raw_fd(&guest_fd)).map_err(map_sys)?;
    sys::fsync_fd(sys::raw_fd(&stage_fd)).map_err(map_sys)?;
    Ok(Layout {
        inner: Inner {
            root_dev: root_id.dev,
            root_ino: root_id.ino,
            fsid,
            host_write,
            guest_write,
            host_stage,
            placeholder,
            published: [None; 8],
            binding_published: false,
        },
    })
}

#[cfg(unix)]
fn publish_unix(
    root: i32,
    layout: &mut Inner,
    slot: HostSlot,
    frame: &[u8],
) -> Result<(), MailboxError> {
    let len = u64::try_from(frame.len()).map_err(|_| MailboxError::Breach(Breach::FrameSize))?;
    if len == 0 {
        return Err(Breach::Empty.into());
    }
    if frame.len() > FRAME_MAX {
        return Err(Breach::FrameSize.into());
    }
    let index = host_index(slot);
    if matches!(slot, HostSlot::Binding) && layout.binding_published {
        return Err(Breach::BindingFrozen.into());
    }
    check_root(root, layout)?;
    let stage = open_recorded(root, c"host-stage", layout.host_stage, layout.fsid)?;
    let dest = open_recorded(root, c"host-write", layout.host_write, layout.fsid)?;
    let file_name = host_names()
        .get(index)
        .copied()
        .ok_or(MailboxError::Breach(Breach::Identity))?;
    let tmp_name = host_tmps()
        .get(index)
        .copied()
        .ok_or(MailboxError::Breach(Breach::Identity))?;
    let existing = stat_name(sys::raw_fd(&dest), file_name)?;
    accept_target(&existing, layout, index)?;
    let tmp = create_tmp(sys::raw_fd(&stage), tmp_name)?;
    let born = require_fresh(&tmp, layout)?;
    sys::write_all(sys::raw_fd(&tmp), frame).map_err(map_sys)?;
    sys::fsync_fd(sys::raw_fd(&tmp)).map_err(map_sys)?;
    hit(IoPoint::AfterFsync);
    if !source_intact(&stage, &tmp, tmp_name, &born, len, layout)? {
        return Err(Breach::Identity.into());
    }
    sys::rename_at(sys::raw_fd(&stage), tmp_name, sys::raw_fd(&dest), file_name)
        .map_err(map_sys)?;
    sys::fsync_fd(sys::raw_fd(&dest)).map_err(map_sys)?;
    sys::fsync_fd(sys::raw_fd(&stage)).map_err(map_sys)?;
    if !published_intact(&stage, &dest, &tmp, file_name, &born, len, layout)? {
        return Err(Breach::Identity.into());
    }
    let ino = sys::fstat(sys::raw_fd(&tmp)).map_err(map_sys)?.ino;
    let published = layout
        .published
        .get_mut(index)
        .ok_or(MailboxError::Breach(Breach::Identity))?;
    *published = Some(ino);
    if matches!(slot, HostSlot::Binding) {
        layout.binding_published = true;
    }
    Ok(())
}

#[cfg(unix)]
fn read_body(
    root: i32,
    layout: &Inner,
    slot: SlotRef,
    watch: &mut SlotWatch,
) -> Result<SlotBody, MailboxError> {
    check_root(root, layout)?;
    let (dir_name, expect, file_name) = locate(slot, layout)?;
    let dir = open_recorded(root, dir_name, expect, layout.fsid)?;
    let pre = match sys::fstat_at(sys::raw_fd(&dir), file_name) {
        Ok(id) => id,
        Err(SysErr::NoEnt) => return absent(&dir, expect, layout, watch),
        Err(SysErr::Loop) => return Err(Breach::Link.into()),
        Err(err) => return Err(map_sys(err)),
    };
    if pre.kind == Kind::Link {
        return Err(Breach::Link.into());
    }
    let fd = match sys::open_read(sys::raw_fd(&dir), file_name) {
        Ok(fd) => fd,
        Err(SysErr::NoEnt) => return absent(&dir, expect, layout, watch),
        Err(SysErr::Loop) => return Err(Breach::Link.into()),
        Err(SysErr::Special) => return Err(Breach::NotRegular.into()),
        Err(err) => return Err(map_sys(err)),
    };
    let opened = checked_file(&fd, layout)?;
    if opened.size == 0 {
        return if stable(&dir, &fd, file_name, &opened, expect, layout)? {
            Ok(SlotBody::Pending)
        } else {
            replaced(watch)
        };
    }
    let cap = u64::try_from(FRAME_MAX).map_err(|_| MailboxError::Breach(Breach::FrameSize))?;
    if opened.size > cap {
        watch.stuck = true;
        return Err(Breach::Size.into());
    }
    let bytes = sys::read_capped(sys::raw_fd(&fd), FRAME_MAX.saturating_add(1)).map_err(map_sys)?;
    if !bytes.is_empty() {
        hit(IoPoint::AfterRead);
    }
    if !stable(&dir, &fd, file_name, &opened, expect, layout)? {
        return replaced(watch);
    }
    if u64::try_from(bytes.len()).ok() != Some(opened.size) {
        return replaced(watch);
    }
    Ok(SlotBody::Frame(bytes))
}

#[cfg(unix)]
fn make_dir(
    parent: i32,
    name: &CStr,
    dev: u64,
    fsid: [u8; 8],
) -> Result<(DirId, OwnedFd), MailboxError> {
    match sys::mkdir_excl(parent, name) {
        Ok(()) => {}
        Err(SysErr::Exist) => return Err(Breach::Exists.into()),
        Err(err) => return Err(map_sys(err)),
    }
    let fd = sys::open_dir(parent, name).map_err(map_sys)?;
    let id = sys::fstat(sys::raw_fd(&fd)).map_err(map_sys)?;
    if id.kind != Kind::Dir || id.dev != dev {
        return Err(Breach::Identity.into());
    }
    same_fs(sys::raw_fd(&fd), fsid)?;
    Ok((
        DirId {
            dev: id.dev,
            ino: id.ino,
        },
        fd,
    ))
}

#[cfg(unix)]
fn make_file(dir: i32, name: &CStr, dev: u64, fsid: [u8; 8]) -> Result<u64, MailboxError> {
    let fd = match sys::open_create(dir, name) {
        Ok(fd) => fd,
        Err(SysErr::Exist) => return Err(Breach::Exists.into()),
        Err(err) => return Err(map_sys(err)),
    };
    let id = sys::fstat(sys::raw_fd(&fd)).map_err(map_sys)?;
    if id.kind != Kind::File || id.nlink != 1 || id.size != 0 || id.dev != dev {
        return Err(Breach::Identity.into());
    }
    same_fs(sys::raw_fd(&fd), fsid)?;
    Ok(id.ino)
}

#[cfg(unix)]
fn check_root(root: i32, layout: &Inner) -> Result<(), MailboxError> {
    let id = sys::fstat(root).map_err(map_sys)?;
    if id.kind != Kind::Dir || id.dev != layout.root_dev || id.ino != layout.root_ino {
        return Err(Breach::Identity.into());
    }
    same_fs(root, layout.fsid)
}

#[cfg(unix)]
fn open_recorded(
    root: i32,
    name: &CStr,
    expect: DirId,
    fsid: [u8; 8],
) -> Result<OwnedFd, MailboxError> {
    let fd = match sys::open_dir(root, name) {
        Ok(fd) => fd,
        Err(SysErr::Loop) => return Err(Breach::Link.into()),
        Err(SysErr::NotDir) => return Err(Breach::NotDirectory.into()),
        Err(err) => return Err(map_sys(err)),
    };
    dir_ok(&fd, expect, fsid)?;
    Ok(fd)
}

#[cfg(unix)]
fn dir_ok(fd: &OwnedFd, expect: DirId, fsid: [u8; 8]) -> Result<(), MailboxError> {
    let id = sys::fstat(sys::raw_fd(fd)).map_err(map_sys)?;
    if id.kind != Kind::Dir || id.dev != expect.dev || id.ino != expect.ino {
        return Err(Breach::Identity.into());
    }
    same_fs(sys::raw_fd(fd), fsid)
}

#[cfg(unix)]
fn same_fs(fd: i32, expect: [u8; 8]) -> Result<(), MailboxError> {
    let got = sys::fsid(fd).map_err(map_sys)?;
    if got == expect {
        Ok(())
    } else {
        Err(Breach::Fsid.into())
    }
}

#[cfg(unix)]
fn stat_name(dir: i32, name: &CStr) -> Result<FileId, MailboxError> {
    match sys::fstat_at(dir, name) {
        Ok(id) => Ok(id),
        Err(SysErr::NoEnt) => Err(Breach::Identity.into()),
        Err(SysErr::Loop) => Err(Breach::Link.into()),
        Err(err) => Err(map_sys(err)),
    }
}

#[cfg(unix)]
fn accept_target(id: &FileId, layout: &Inner, index: usize) -> Result<(), MailboxError> {
    if id.kind == Kind::Link {
        return Err(Breach::Link.into());
    }
    if id.kind != Kind::File {
        return Err(Breach::NotRegular.into());
    }
    if id.nlink != 1 {
        return Err(Breach::Nlink.into());
    }
    if id.dev != layout.root_dev {
        return Err(Breach::Dev.into());
    }
    let place = layout
        .placeholder
        .get(index)
        .copied()
        .ok_or(MailboxError::Breach(Breach::Identity))?;
    let expect = layout
        .published
        .get(index)
        .copied()
        .ok_or(MailboxError::Breach(Breach::Identity))?
        .unwrap_or(place);
    if id.ino == expect {
        Ok(())
    } else {
        Err(Breach::Identity.into())
    }
}

#[cfg(unix)]
fn create_tmp(dir: i32, name: &CStr) -> Result<OwnedFd, MailboxError> {
    match sys::open_create(dir, name) {
        Ok(fd) => Ok(fd),
        Err(SysErr::Exist) => Err(Breach::TmpExists.into()),
        Err(SysErr::Loop) => Err(Breach::Link.into()),
        Err(err) => Err(map_sys(err)),
    }
}

#[cfg(unix)]
fn require_fresh(fd: &OwnedFd, layout: &Inner) -> Result<FileId, MailboxError> {
    let id = sys::fstat(sys::raw_fd(fd)).map_err(map_sys)?;
    if id.kind != Kind::File || id.nlink != 1 || id.size != 0 || id.dev != layout.root_dev {
        return Err(Breach::Identity.into());
    }
    same_fs(sys::raw_fd(fd), layout.fsid)?;
    Ok(id)
}

#[cfg(unix)]
fn source_intact(
    stage: &OwnedFd,
    tmp: &OwnedFd,
    name: &CStr,
    born: &FileId,
    len: u64,
    layout: &Inner,
) -> Result<bool, MailboxError> {
    dir_ok(stage, layout.host_stage, layout.fsid)?;
    let after = sys::fstat(sys::raw_fd(tmp)).map_err(map_sys)?;
    let named = match sys::fstat_at(sys::raw_fd(stage), name) {
        Ok(id) => id,
        Err(_) => return Ok(false),
    };
    same_fs(sys::raw_fd(tmp), layout.fsid)?;
    Ok(same_object(&after, born)
        && after.nlink == 1
        && after.kind == Kind::File
        && after.size == len
        && same_file(&named, &after))
}

#[cfg(unix)]
fn published_intact(
    stage: &OwnedFd,
    dest: &OwnedFd,
    tmp: &OwnedFd,
    name: &CStr,
    born: &FileId,
    len: u64,
    layout: &Inner,
) -> Result<bool, MailboxError> {
    dir_ok(stage, layout.host_stage, layout.fsid)?;
    dir_ok(dest, layout.host_write, layout.fsid)?;
    let named = match sys::fstat_at(sys::raw_fd(dest), name) {
        Ok(id) => id,
        Err(_) => return Ok(false),
    };
    let still = sys::fstat(sys::raw_fd(tmp)).map_err(map_sys)?;
    Ok(same_object(&still, born)
        && still.nlink == 1
        && still.size == len
        && same_file(&named, &still))
}

#[cfg(unix)]
fn checked_file(fd: &OwnedFd, layout: &Inner) -> Result<FileId, MailboxError> {
    let id = sys::fstat(sys::raw_fd(fd)).map_err(map_sys)?;
    if id.kind == Kind::Link {
        return Err(Breach::Link.into());
    }
    if id.kind != Kind::File {
        return Err(Breach::NotRegular.into());
    }
    if id.nlink != 1 {
        return Err(Breach::Nlink.into());
    }
    if id.dev != layout.root_dev {
        return Err(Breach::Dev.into());
    }
    same_fs(sys::raw_fd(fd), layout.fsid)?;
    Ok(id)
}

#[cfg(unix)]
fn stable(
    dir: &OwnedFd,
    fd: &OwnedFd,
    name: &CStr,
    before: &FileId,
    expect: DirId,
    layout: &Inner,
) -> Result<bool, MailboxError> {
    dir_ok(dir, expect, layout.fsid)?;
    let after = sys::fstat(sys::raw_fd(fd)).map_err(map_sys)?;
    let named = match sys::fstat_at(sys::raw_fd(dir), name) {
        Ok(id) => id,
        Err(SysErr::NoEnt) => return Ok(false),
        Err(err) => return Err(map_sys(err)),
    };
    Ok(same_file(before, &after) && named.dev == after.dev && named.ino == after.ino)
}

#[cfg(unix)]
fn absent(
    dir: &OwnedFd,
    expect: DirId,
    layout: &Inner,
    watch: &mut SlotWatch,
) -> Result<SlotBody, MailboxError> {
    dir_ok(dir, expect, layout.fsid)?;
    bump(&mut watch.retry_absent)?;
    Ok(SlotBody::Pending)
}

#[cfg(unix)]
fn replaced(watch: &mut SlotWatch) -> Result<SlotBody, MailboxError> {
    bump(&mut watch.retry_replaced)?;
    Ok(SlotBody::Pending)
}

#[cfg(unix)]
fn bump(counter: &mut u64) -> Result<(), MailboxError> {
    *counter = counter.checked_add(1).ok_or(MailboxError::Io)?;
    Ok(())
}

#[cfg(unix)]
fn locate(
    slot: SlotRef,
    layout: &Inner,
) -> Result<(&'static CStr, DirId, &'static CStr), MailboxError> {
    match slot {
        SlotRef::Host(host) => {
            let index = host_index(host);
            let name = host_names()
                .get(index)
                .copied()
                .ok_or(MailboxError::Breach(Breach::Identity))?;
            Ok((c"host-write", layout.host_write, name))
        }
        SlotRef::Guest(op) => {
            let name = guest_msgs()
                .get(op.index())
                .copied()
                .ok_or(MailboxError::Breach(Breach::Identity))?;
            Ok((c"guest-write", layout.guest_write, name))
        }
    }
}

#[cfg(unix)]
fn same_object(id: &FileId, born: &FileId) -> bool {
    id.dev == born.dev && id.ino == born.ino
}

#[cfg(unix)]
fn same_file(left: &FileId, right: &FileId) -> bool {
    left.dev == right.dev
        && left.ino == right.ino
        && left.nlink == right.nlink
        && left.size == right.size
        && left.mtime_sec == right.mtime_sec
        && left.mtime_nsec == right.mtime_nsec
        && left.ctime_sec == right.ctime_sec
        && left.ctime_nsec == right.ctime_nsec
        && left.kind == right.kind
}

#[cfg(unix)]
fn host_index(slot: HostSlot) -> usize {
    match slot {
        HostSlot::Binding => 0,
        HostSlot::Op(op) => op.index().saturating_add(1),
    }
}

#[cfg(unix)]
fn map_sys(err: SysErr) -> MailboxError {
    match err {
        SysErr::NoEnt => Breach::Identity.into(),
        SysErr::Exist => Breach::Exists.into(),
        SysErr::Loop => Breach::Link.into(),
        SysErr::NotDir => Breach::NotDirectory.into(),
        SysErr::Special => Breach::NotRegular.into(),
        SysErr::Io => MailboxError::Io,
    }
}

#[cfg(unix)]
fn host_names() -> [&'static CStr; 8] {
    [
        c"binding.msg",
        c"inventory.msg",
        c"probe.msg",
        c"bootstrap.msg",
        c"login.msg",
        c"run.msg",
        c"lease.msg",
        c"revoke.msg",
    ]
}

#[cfg(unix)]
fn host_tmps() -> [&'static CStr; 8] {
    [
        c"binding.msg.tmp",
        c"inventory.msg.tmp",
        c"probe.msg.tmp",
        c"bootstrap.msg.tmp",
        c"login.msg.tmp",
        c"run.msg.tmp",
        c"lease.msg.tmp",
        c"revoke.msg.tmp",
    ]
}

#[cfg(unix)]
fn guest_msgs() -> [&'static CStr; 7] {
    [
        c"inventory.msg",
        c"probe.msg",
        c"bootstrap.msg",
        c"login.msg",
        c"run.msg",
        c"lease.msg",
        c"revoke.msg",
    ]
}

#[cfg(unix)]
fn guest_names() -> impl Iterator<Item = &'static CStr> {
    guest_msgs().into_iter().chain(guest_tmps())
}

#[cfg(unix)]
fn guest_tmps() -> [&'static CStr; 7] {
    [
        c"inventory.msg.tmp",
        c"probe.msg.tmp",
        c"bootstrap.msg.tmp",
        c"login.msg.tmp",
        c"run.msg.tmp",
        c"lease.msg.tmp",
        c"revoke.msg.tmp",
    ]
}
